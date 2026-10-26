//! Save and recall for the typed memory graph (`.repolens/memory/memory.json`).
//!
//! Shared by the MCP tools `memorize_insight` / `recall_longterm` /
//! `forget_insight` and the CLI commands `repolens memory save` / `recall` /
//! `list` / `delete`, so both front ends store, decay, score, ground and
//! select memories the same way.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;
use tracing::info;
use uuid::Uuid;

use repolens_core::model::memory::{
    GroundingStatus, MemoryEdge, MemoryGraph, MemoryNode, MemoryRelation, MemorySource, MemoryType,
};
use repolens_core::pack_reader::PackIndex;
use repolens_memory::decay::MemoryDecayEngine;
use repolens_memory::store::{self, MemoryStore};

use crate::grounding::{self, GroundingAssessment};

/// Accepted `type` values, in the wire spelling.
pub const MEMORY_TYPES: &str = "PATTERN, DECISION, CONVENTION, TRADE_OFF, or CODE_ANCHOR";

/// Parse a wire memory type (`PATTERN`, `DECISION`, ...).
pub fn parse_memory_type(s: &str) -> Result<MemoryType, String> {
    match s {
        "PATTERN" => Ok(MemoryType::Pattern),
        "DECISION" => Ok(MemoryType::Decision),
        "CONVENTION" => Ok(MemoryType::Convention),
        "TRADE_OFF" => Ok(MemoryType::TradeOff),
        "CODE_ANCHOR" => Ok(MemoryType::CodeAnchor),
        other => Err(format!(
            "Invalid memory type: {other}. Must be {MEMORY_TYPES}"
        )),
    }
}

/// Wire spelling of a memory type.
pub fn memory_type_name(t: &MemoryType) -> &'static str {
    match t {
        MemoryType::Pattern => "PATTERN",
        MemoryType::Decision => "DECISION",
        MemoryType::Convention => "CONVENTION",
        MemoryType::TradeOff => "TRADE_OFF",
        MemoryType::CodeAnchor => "CODE_ANCHOR",
    }
}

#[derive(Debug, Clone, Default)]
pub struct SaveRequest {
    pub content: String,
    /// Wire type; `None` means `PATTERN`.
    pub memory_type: Option<String>,
    /// 0.0-1.0; `None` means 0.5.
    pub importance: Option<f32>,
    pub anchors: Vec<String>,
    pub tags: Vec<String>,
    /// Id of a memory this one replaces (adds a SUPERSEDES edge).
    pub supersedes: Option<String>,
    /// Who wrote it, e.g. `mcp:memorize_insight` or `cli:memory save`.
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Created {
        id: String,
    },
    /// A memory with exactly this content already exists; nothing written.
    Duplicate {
        id: String,
    },
}

/// Validate and store one memory. Errors are user-facing messages.
pub fn save(root: &Path, req: SaveRequest) -> Result<SaveOutcome, String> {
    let content = req.content.trim();
    if content.is_empty() {
        return Err("content must not be empty".into());
    }
    let importance = req.importance.unwrap_or(0.5);
    if !(0.0..=1.0).contains(&importance) {
        return Err("importance must be between 0.0 and 1.0".into());
    }
    let memory_type = match req.memory_type.as_deref() {
        None => MemoryType::Pattern,
        Some(t) => parse_memory_type(t)?,
    };

    let mut guard =
        MemoryStore::lock(root).map_err(|e| format!("Failed to load memory graph: {e}"))?;

    if let Some(existing) = guard.graph().nodes.iter().find(|n| n.content == content) {
        return Ok(SaveOutcome::Duplicate {
            id: existing.id.clone(),
        });
    }

    let tags: Vec<String> = req
        .tags
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    // Remember each file anchor's content hash from the latest scan, so
    // recall can say when the file changed since this note was saved (#14).
    let anchor_sha256: std::collections::BTreeMap<String, String> =
        match crate::discover::PackResolver::new().resolve(None, Some(&root.to_string_lossy())) {
            Ok((_, pack)) => req
                .anchors
                .iter()
                .filter_map(|a| {
                    pack.entries
                        .iter()
                        .find(|e| e.path == *a && !e.sha256.is_empty())
                        .map(|e| (a.clone(), e.sha256.clone()))
                })
                .collect(),
            Err(_) => Default::default(),
        };

    let now = repolens_memory::clock::now();
    let node_id = Uuid::new_v4().to_string();
    store::add_node(
        guard.graph_mut(),
        MemoryNode {
            id: node_id.clone(),
            content: content.to_string(),
            memory_type,
            importance,
            created: now,
            last_accessed: now,
            access_count: 0,
            anchors: req.anchors,
            tags,
            source: MemorySource::Explicit,
            provenance: req.provenance,
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256,
        },
    );

    if let Some(ref supersedes_id) = req.supersedes {
        if guard.graph().nodes.iter().any(|n| n.id == *supersedes_id) {
            store::add_edge(
                guard.graph_mut(),
                MemoryEdge {
                    source_id: node_id.clone(),
                    target_id: supersedes_id.clone(),
                    relation: MemoryRelation::Supersedes,
                },
            );
            info!(new_id = %node_id, supersedes = %supersedes_id, "memory save: added SUPERSEDES edge");
        } else {
            info!(supersedes = %supersedes_id, "memory save: supersedes target not found, skipping edge");
        }
    }

    guard
        .save()
        .map_err(|e| format!("Failed to save memory graph: {e}"))?;
    Ok(SaveOutcome::Created { id: node_id })
}

#[derive(Debug, Clone, Default)]
pub struct RecallRequest {
    pub query: String,
    pub memory_type: Option<String>,
    pub tags: Option<Vec<String>>,
    pub min_importance: Option<f32>,
    /// `None` means 10.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RecallHit {
    pub id: String,
    pub content: String,
    pub memory_type: MemoryType,
    /// Importance after decay.
    pub importance: f32,
    /// Importance after decay but before the CONTRADICTED grounding
    /// penalty; equals `importance` when no penalty applies.
    pub importance_before_grounding: f32,
    /// `importance / importance_before_grounding` when the grounding
    /// penalty lowered the score, `None` otherwise (#14).
    pub grounding_factor: Option<f64>,
    /// File anchors whose content changed since the note was saved, by
    /// the saved hash against the pack used for grounding (#14).
    pub changed_anchors: Vec<String>,
    /// Fraction of query words found in the memory (0-1).
    pub keyword_score: f64,
    /// `keyword_score * importance`.
    pub score: f64,
    pub tags: Vec<String>,
    pub anchors: Vec<String>,
    pub created: OffsetDateTime,
    /// Last access before this recall.
    pub last_accessed: OffsetDateTime,
    pub grounding: GroundingAssessment,
}

#[derive(Debug, Clone)]
pub struct RecallOutcome {
    pub hits: Vec<RecallHit>,
    pub total: usize,
    pub decay_ran: bool,
    /// Memories that would have matched this query and filters but were
    /// deleted by this recall's decay run because they faded (#14).
    pub removed: usize,
    /// Pack used for grounding, if any.
    pub pack_dir: Option<PathBuf>,
    pub now: OffsetDateTime,
}

/// Recall memories matching `req.query`: lazily decay the graph, score,
/// ground against `pack`, then touch and record grounding on the hits.
pub fn recall(
    root: &Path,
    pack: Option<(PathBuf, PackIndex)>,
    req: RecallRequest,
) -> Result<RecallOutcome, String> {
    let limit = req.limit.unwrap_or(10);
    let type_filter = req
        .memory_type
        .as_deref()
        .map(parse_memory_type)
        .transpose()?;

    let mut guard =
        MemoryStore::lock(root).map_err(|e| format!("Failed to load memory graph: {e}"))?;

    let now = repolens_memory::clock::now();
    // With REPOLENS_CLOCK_OVERRIDE set, recall is a read-only "what if":
    // decay and grounding are computed on a copy and nothing is written.
    let read_only = repolens_memory::clock::is_overridden();
    let pack_index_ref = pack.as_ref().map(|(_, idx)| idx);

    let mut working = if read_only {
        let mut g = guard.graph().clone();
        // Ground every node first so decay sees current anchors (the
        // confirmed-anchor floor) without relying on persisted state.
        let assessed: Vec<(String, GroundingStatus)> = g
            .nodes
            .iter()
            .filter(|n| !n.anchors.is_empty())
            .map(|n| {
                (
                    n.id.clone(),
                    grounding::assess_grounding(n, pack_index_ref).grounding_status,
                )
            })
            .collect();
        for (id, status) in assessed {
            grounding::record_grounding(&mut g, &id, status, now);
        }
        Some(g)
    } else {
        None
    };

    let tag_filter: Option<Vec<String>> = req
        .tags
        .map(|t| t.into_iter().map(|s| s.to_lowercase()).collect());
    let query_tokens = tokenize(&req.query);
    let matches_query = |n: &MemoryNode| {
        type_filter.as_ref().is_none_or(|tf| &n.memory_type == tf)
            && tag_filter.as_ref().is_none_or(|required| {
                let node_tags: BTreeSet<String> = n.tags.iter().map(|t| t.to_lowercase()).collect();
                required.iter().all(|rt| node_tags.contains(rt))
            })
            && keyword_score(&query_tokens, n) > 0.0
    };
    let matched_before: BTreeSet<String> = working
        .as_ref()
        .unwrap_or_else(|| guard.graph())
        .nodes
        .iter()
        .filter(|n| matches_query(n))
        .map(|n| n.id.clone())
        .collect();

    // Lazy decay: run if never run or more than an hour old.
    let decay_ran =
        MemoryDecayEngine::should_run(working.as_ref().unwrap_or_else(|| guard.graph()), now);
    if decay_ran {
        if let Some(g) = working.as_mut() {
            *g = MemoryDecayEngine::decay(g, now);
        } else {
            let decayed = MemoryDecayEngine::decay(guard.graph(), now);
            *guard.graph_mut() = decayed;
            if let Err(e) = guard.save() {
                // Non-fatal: continue with the decayed in-memory graph.
                info!(error = %e, "memory recall: failed to save after decay");
            }
        }
    }
    let graph_view: &MemoryGraph = match working.as_ref() {
        Some(g) => g,
        None => guard.graph(),
    };

    let min_importance = req.min_importance.unwrap_or(0.0);
    let removed = matched_before
        .iter()
        .filter(|id| !graph_view.nodes.iter().any(|n| &n.id == *id))
        .count();

    let mut scored: Vec<(&MemoryNode, f64)> = graph_view
        .nodes
        .iter()
        .filter(|n| type_filter.as_ref().is_none_or(|tf| &n.memory_type == tf))
        .filter(|n| {
            tag_filter.as_ref().is_none_or(|required| {
                let node_tags: BTreeSet<String> = n.tags.iter().map(|t| t.to_lowercase()).collect();
                required.iter().all(|rt| node_tags.contains(rt))
            })
        })
        .filter(|n| n.importance >= min_importance)
        .map(|n| (n, keyword_score(&query_tokens, n)))
        .filter(|(n, ks)| ks * n.importance as f64 > 0.0)
        .collect();

    // Sort: score desc, importance desc, id asc.
    scored.sort_by(|(a, ka), (b, kb)| {
        let sa = ka * a.importance as f64;
        let sb = kb * b.importance as f64;
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.id.cmp(&b.id))
    });

    let total = scored.len();
    let pack_index = pack.as_ref().map(|(_, idx)| idx);
    let hits: Vec<RecallHit> = scored
        .into_iter()
        .take(limit)
        .map(|(n, ks)| RecallHit {
            id: n.id.clone(),
            content: n.content.clone(),
            memory_type: n.memory_type.clone(),
            importance: n.importance,
            importance_before_grounding: n.importance_before_grounding.unwrap_or(n.importance),
            grounding_factor: grounding_factor(n),
            changed_anchors: changed_anchors(n, pack_index),
            keyword_score: ks,
            score: ks * n.importance as f64,
            tags: n.tags.clone(),
            anchors: n.anchors.clone(),
            created: n.created,
            last_accessed: n.last_accessed,
            grounding: grounding::assess_grounding(n, pack_index),
        })
        .collect();

    // Touch hits and persist their recall-time grounding so the next decay
    // run can use it (Confirmed: slower, Contradicted: faster).
    if !hits.is_empty() && !read_only {
        for h in &hits {
            let graph = guard.graph_mut();
            store::touch_node(graph, &h.id);
            grounding::record_grounding(graph, &h.id, h.grounding.grounding_status.clone(), now);
        }
        if let Err(e) = guard.save() {
            info!(error = %e, "memory recall: failed to save after touch");
        }
    }

    Ok(RecallOutcome {
        hits,
        total,
        decay_ran,
        removed,
        pack_dir: pack.map(|(p, _)| p),
        now,
    })
}

/// The factor the grounding penalty left on `n`'s importance, when it
/// lowered it visibly (shown to 2 decimals, so 1.00 means none).
fn grounding_factor(n: &MemoryNode) -> Option<f64> {
    let before = n.importance_before_grounding? as f64;
    if before <= 0.0 {
        return None;
    }
    let factor = n.importance as f64 / before;
    (factor < 0.995).then_some(factor)
}

/// File anchors saved with a hash whose current pack entry has a
/// different one. A missing entry is reported by grounding, not here.
fn changed_anchors(n: &MemoryNode, pack: Option<&PackIndex>) -> Vec<String> {
    let Some(pack) = pack else {
        return vec![];
    };
    n.anchors
        .iter()
        .filter(|a| {
            n.anchor_sha256.get(*a).is_some_and(|saved| {
                pack.entries
                    .iter()
                    .find(|e| e.path == **a)
                    .is_some_and(|e| !e.sha256.is_empty() && e.sha256 != *saved)
            })
        })
        .cloned()
        .collect()
}

/// Shortest id prefix `memory forget` and MCP `forget_insight` accept, for
/// a preview as well as a delete. A full id of any length always matches.
pub const MIN_ID_PREFIX: usize = 4;

/// At most this many matches of an ambiguous prefix are listed.
pub const MAX_LISTED_MATCHES: usize = 10;

/// An id prefix that starts more than one memory, so it selects none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousId {
    /// The id as the caller gave it (trimmed).
    pub id: String,
    /// Full ids of every memory it starts, in save order.
    pub matches: Vec<String>,
}

impl AmbiguousId {
    /// The matches to list ([`MAX_LISTED_MATCHES`] at most) and how many
    /// more there are.
    pub fn listed(&self) -> (&[String], usize) {
        let n = self.matches.len().min(MAX_LISTED_MATCHES);
        (&self.matches[..n], self.matches.len() - n)
    }
}

/// Why [`find_by_id`] refused an id. Either way it selects nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdLookupError {
    /// The prefix starts several memories.
    Ambiguous(AmbiguousId),
    /// A prefix shorter than [`MIN_ID_PREFIX`].
    TooShort { id: String },
}

impl IdLookupError {
    /// One-line summary, shared by the CLI and MCP `forget_insight`.
    pub fn message(&self) -> String {
        match self {
            IdLookupError::Ambiguous(a) => format!(
                "{} matches {} memories; use a longer prefix.",
                a.id,
                a.matches.len()
            ),
            IdLookupError::TooShort { .. } => {
                format!("Use at least {MIN_ID_PREFIX} characters of the id.")
            }
        }
    }
}

/// Find the memory `id` names: its full id, or a unique prefix of at least
/// [`MIN_ID_PREFIX`] characters, such as the 8 characters
/// `repolens memory save` prints (#35). Ids are hex, so the match ignores
/// case. A blank id names nothing. A shorter prefix, or one shared by
/// several memories, is refused and selects nothing. The CLI
/// (`memory forget`) and MCP `forget_insight` both select through this.
pub fn find_by_id<'a>(
    graph: &'a MemoryGraph,
    id: &str,
) -> Result<Option<&'a MemoryNode>, IdLookupError> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    let want = id.to_lowercase();
    if let Some(n) = graph.nodes.iter().find(|n| n.id.to_lowercase() == want) {
        return Ok(Some(n));
    }
    if id.chars().count() < MIN_ID_PREFIX {
        return Err(IdLookupError::TooShort { id: id.to_string() });
    }
    let hits: Vec<&MemoryNode> = graph
        .nodes
        .iter()
        .filter(|n| n.id.to_lowercase().starts_with(&want))
        .collect();
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        many => Err(IdLookupError::Ambiguous(AmbiguousId {
            id: id.to_string(),
            matches: many.iter().map(|n| n.id.clone()).collect(),
        })),
    }
}

/// Every saved memory, in save order, exactly as stored (importance as last
/// saved; no decay is applied). Read-only: no lock, file or directory is
/// created or written.
pub fn list(root: &Path) -> Result<Vec<MemoryNode>, String> {
    MemoryStore::load(root)
        .map(|g| g.nodes)
        .map_err(|e| format!("Failed to load memory graph: {e}"))
}

/// Result of [`forget`].
#[derive(Debug, Clone)]
pub enum ForgetOutcome {
    /// No memory has that id or id prefix (or the id was blank).
    NotFound,
    /// The id was refused (too short, or ambiguous). `matches` holds the
    /// listed matches of an ambiguous prefix (see [`AmbiguousId::listed`]).
    Refused {
        error: IdLookupError,
        matches: Vec<MemoryNode>,
    },
    /// Dry run: this memory would be deleted. Nothing was written.
    Preview(MemoryNode),
    /// This memory and its edges were deleted.
    Deleted(MemoryNode),
}

/// Delete the memory `id` names (see [`find_by_id`]), like MCP
/// `forget_insight` with an id. The lookup runs on a read-only load first,
/// so a preview, an unknown id or a refused id writes nothing: no lock file
/// or memory directory is created. Only with `confirm` and a match is the
/// lock taken; the match is re-checked under it before deleting.
pub fn forget(root: &Path, id: &str, confirm: bool) -> Result<ForgetOutcome, String> {
    let load_err = |e: anyhow::Error| format!("Failed to load memory graph: {e}");
    let refused = |graph: &MemoryGraph, error: IdLookupError| {
        let matches = match &error {
            IdLookupError::Ambiguous(a) => a
                .listed()
                .0
                .iter()
                .filter_map(|m| graph.nodes.iter().find(|n| &n.id == m).cloned())
                .collect(),
            IdLookupError::TooShort { .. } => Vec::new(),
        };
        ForgetOutcome::Refused { error, matches }
    };
    let graph = MemoryStore::load(root).map_err(load_err)?;
    let node = match find_by_id(&graph, id) {
        Ok(Some(n)) => n.clone(),
        Ok(None) => return Ok(ForgetOutcome::NotFound),
        Err(e) => return Ok(refused(&graph, e)),
    };
    if !confirm {
        return Ok(ForgetOutcome::Preview(node));
    }
    let mut guard = MemoryStore::lock(root).map_err(load_err)?;
    // Another writer may have changed the graph since the read-only load.
    let node = match find_by_id(guard.graph(), &node.id) {
        Ok(Some(n)) => n.clone(),
        Ok(None) => return Ok(ForgetOutcome::NotFound),
        Err(e) => return Ok(refused(guard.graph(), e)),
    };
    store::remove_node(guard.graph_mut(), &node.id);
    guard
        .save()
        .map_err(|e| format!("Failed to save memory graph: {e}"))?;
    Ok(ForgetOutcome::Deleted(node))
}

/// Wire spelling of a grounding status.
pub fn grounding_name(s: &GroundingStatus) -> &'static str {
    match s {
        GroundingStatus::Confirmed => "CONFIRMED",
        GroundingStatus::Contradicted => "CONTRADICTED",
        GroundingStatus::Unverifiable => "UNVERIFIABLE",
        GroundingStatus::Unchecked => "UNCHECKED",
    }
}

/// Tokenize text into lowercase alphanumeric tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Common English words that carry no search meaning. They are dropped from
/// a recall query, so "the a of" finds nothing instead of every note that
/// says "the" (#35). "not" and "no" stay: they change what a note means.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "do", "for", "from", "has",
    "have", "if", "in", "into", "is", "it", "its", "of", "on", "or", "so", "than", "that", "the",
    "their", "then", "there", "these", "they", "this", "those", "to", "was", "were", "what",
    "when", "where", "which", "who", "will", "with",
];

/// Words whose trailing "s" is part of the word, not a plural (#56):
/// stemming leaves them whole, so "news" doesn't meet "new".
const KEEP_FINAL_S: &[&str] = &["news", "series", "species"];

/// Plurals ending in "is" that do lose their "s" (#56). Other "-is" words
/// ("analysis", "basis") keep it.
const STRIP_FINAL_S: &[&str] = &["apis", "guis", "kpis", "uris"];

/// Light English stemming so plural and simple verb forms meet (#35):
/// image/images, check/checks/checked/checking, box/boxes, policy/policies.
/// Applied to query and note alike, so only consistency matters, not
/// producing real words. Non-ASCII tokens (CJK...) pass through unchanged.
/// A trailing "s" is stripped from words longer than 3 letters
/// (tags/tag, keys/key), except words in [`KEEP_FINAL_S`] such as "news"
/// (#56). Words in [`STRIP_FINAL_S`] (apis/api) lose it despite ending
/// in "is".
pub fn stem(token: &str) -> String {
    if !token.bytes().all(|b| b.is_ascii_lowercase()) {
        return token.to_string();
    }
    let mut t = token.to_string();
    if KEEP_FINAL_S.contains(&token) {
        // Not a plural.
    } else if STRIP_FINAL_S.contains(&token) {
        t.pop();
    } else if t.len() > 4 && t.ends_with("ies") {
        t.truncate(t.len() - 3);
        t.push('i');
    } else if t.len() > 3
        && t.ends_with('s')
        && !t.ends_with("ss")
        && !t.ends_with("us")
        && !t.ends_with("is")
    {
        t.pop();
    }
    if t.len() >= 6 && t.ends_with("ing") {
        t.truncate(t.len() - 3);
    } else if t.len() >= 5 && t.ends_with("ed") {
        t.truncate(t.len() - 2);
    }
    trim_final_e_or_y(t)
}

/// Last step of [`stem`]: "cache" and "cach(ing)" meet, "policy" and
/// "polici(es)" meet.
fn trim_final_e_or_y(mut t: String) -> String {
    if t.len() > 3 && t.ends_with('e') {
        t.pop();
    } else if t.len() > 3 && t.ends_with('y') {
        t.pop();
        t.push('i');
    }
    t
}

/// Shortest extra form [`stem_forms`] adds for an -ed/-ing word: a
/// shorter one would meet unrelated short words (#56).
const MIN_FORM_LEN: usize = 3;

/// Every stem a token matches as: its [`stem`], plus, for an -ed or -ing
/// word, both the bare stem and the stem with "e" put back, so "used" and
/// "using" meet "use" (#56). A form that is a stop word is left out, so
/// "thing" doesn't meet every note that says "the". So is a form shorter
/// than 3 letters: "used" adds "use" but not "us", so it doesn't meet a
/// note that says "ping us" (and need/seed/feed/bring don't meet
/// ne/se/fe/br).
pub fn stem_forms(token: &str) -> Vec<String> {
    let mut out = vec![stem(token)];
    if !token.bytes().all(|b| b.is_ascii_lowercase()) {
        return out;
    }
    for suffix in ["ing", "ed"] {
        let Some(base) = token.strip_suffix(suffix) else {
            continue;
        };
        for form in [base.to_string(), format!("{base}e")] {
            let form = trim_final_e_or_y(form);
            if form.len() >= MIN_FORM_LEN
                && !STOP_WORDS.contains(&form.as_str())
                && !out.contains(&form)
            {
                out.push(form);
            }
        }
    }
    out
}

/// The query tokens to match on: tokens minus stop words. A one-word query
/// that is a stop word keeps that word, so "IT" and "who" are searchable
/// (#56); a query of several words that are all stop words, such as
/// "the a of", has nothing to match on (#35).
fn query_words(tokens: &[String]) -> Vec<&str> {
    if let [only] = tokens {
        return vec![only.as_str()];
    }
    tokens
        .iter()
        .map(String::as_str)
        .filter(|t| !STOP_WORDS.contains(t))
        .collect()
}

/// The query terms recall matches on, stemmed: the words minus stop words,
/// or the word itself for a one-word query (#56). Empty when the query has
/// no words, or only several stop words.
pub fn search_terms(query: &str) -> Vec<String> {
    query_words(&tokenize(query))
        .into_iter()
        .map(stem)
        .collect()
}

/// A text's terms minus stop words, stemmed, with no fallback: empty for a
/// text of only stop words. For matching a task description word by word.
pub fn content_terms(text: &str) -> Vec<String> {
    tokenize(text)
        .into_iter()
        .filter(|t| !STOP_WORDS.contains(&t.as_str()))
        .map(|t| stem(&t))
        .collect()
}

/// Fraction of the query's search terms (see [`search_terms`]) found in the
/// node's content, tags and anchors. Both sides match through
/// [`stem_forms`]: a term counts when any of its forms is a form of a node
/// word. `query_tokens` may be raw [`tokenize`] output; stop words in it are
/// ignored unless the query is that one word.
pub fn keyword_score(query_tokens: &[String], node: &MemoryNode) -> f64 {
    let terms: Vec<Vec<String>> = query_words(query_tokens)
        .into_iter()
        .map(stem_forms)
        .collect();
    if terms.is_empty() {
        return 0.0;
    }
    let mut node_forms: BTreeSet<String> = BTreeSet::new();
    let mut add = |text: &str| {
        for t in tokenize(text) {
            node_forms.extend(stem_forms(&t));
        }
    };
    add(&node.content);
    for tag in &node.tags {
        add(tag);
    }
    for anchor in &node.anchors {
        add(anchor);
    }
    let matches = terms
        .iter()
        .filter(|forms| forms.iter().any(|f| node_forms.contains(f)))
        .count();
    matches as f64 / terms.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_with_ids(ids: &[&str]) -> MemoryGraph {
        let d = tempfile::tempdir().unwrap();
        let mut g = store::empty_graph(d.path());
        let now = OffsetDateTime::UNIX_EPOCH;
        for id in ids {
            g.nodes.push(MemoryNode {
                id: id.to_string(),
                content: format!("note {id}"),
                memory_type: MemoryType::Pattern,
                importance: 0.5,
                created: now,
                last_accessed: now,
                access_count: 0,
                anchors: vec![],
                tags: vec![],
                source: MemorySource::Explicit,
                provenance: String::new(),
                last_grounding: None,
                last_grounded_at: None,
                last_decayed_at: None,
                importance_before_grounding: None,
                anchor_sha256: Default::default(),
            });
        }
        g
    }

    #[test]
    fn find_by_id_takes_full_ids_and_unique_prefixes_in_any_case() {
        let g = graph_with_ids(&["abcd1111-aa", "abcd2222-bb", "ef0"]);
        let id = |q: &str| find_by_id(&g, q).map(|n| n.map(|n| n.id.clone()));
        assert_eq!(id("abcd1111-aa"), Ok(Some("abcd1111-aa".into())));
        assert_eq!(id(" ABCD2 "), Ok(Some("abcd2222-bb".into())));
        assert_eq!(id("ffff"), Ok(None));
        assert_eq!(id("  "), Ok(None));
        // A full id always matches, however short.
        assert_eq!(id("EF0"), Ok(Some("ef0".into())));
        let err = find_by_id(&g, "ABCD").unwrap_err();
        let IdLookupError::Ambiguous(ref amb) = err else {
            panic!("{err:?}")
        };
        assert_eq!(amb.matches, vec!["abcd1111-aa", "abcd2222-bb"]);
        assert_eq!(
            err.message(),
            "ABCD matches 2 memories; use a longer prefix."
        );
    }

    #[test]
    fn prefixes_under_four_characters_are_refused() {
        let g = graph_with_ids(&["abcd1111-aa"]);
        for q in ["a", "abc", "zz"] {
            let err = find_by_id(&g, q).unwrap_err();
            assert_eq!(err, IdLookupError::TooShort { id: q.into() });
            assert_eq!(err.message(), "Use at least 4 characters of the id.");
        }
    }

    #[test]
    fn ambiguous_lists_at_most_ten_matches() {
        let ids: Vec<String> = (0..12).map(|i| format!("abcd{i:02}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let g = graph_with_ids(&refs);
        let IdLookupError::Ambiguous(amb) = find_by_id(&g, "abcd").unwrap_err() else {
            panic!()
        };
        let (shown, more) = amb.listed();
        assert_eq!(shown.len(), 10);
        assert_eq!(more, 2);
    }

    #[test]
    fn forget_writes_nothing_unless_it_deletes() {
        let d = tempfile::tempdir().unwrap();
        for confirm in [false, true] {
            assert!(matches!(
                forget(d.path(), "abcd", confirm),
                Ok(ForgetOutcome::NotFound)
            ));
            assert!(matches!(
                forget(d.path(), "ab", confirm),
                Ok(ForgetOutcome::Refused { .. })
            ));
        }
        assert!(list(d.path()).unwrap().is_empty());
        assert!(!d.path().join(".repolens").exists());
    }

    #[test]
    fn save_validates_like_the_mcp_tool() {
        let d = tempfile::tempdir().unwrap();
        let bad = |r: SaveRequest| save(d.path(), r).unwrap_err();
        assert_eq!(
            bad(SaveRequest {
                content: " ".into(),
                ..Default::default()
            }),
            "content must not be empty"
        );
        assert_eq!(
            bad(SaveRequest {
                content: "x".into(),
                importance: Some(2.0),
                ..Default::default()
            }),
            "importance must be between 0.0 and 1.0"
        );
        assert!(bad(SaveRequest {
            content: "x".into(),
            memory_type: Some("NOPE".into()),
            ..Default::default()
        })
        .starts_with("Invalid memory type: NOPE"));
    }

    #[test]
    fn save_dedups_and_recall_scores() {
        let d = tempfile::tempdir().unwrap();
        let req = SaveRequest {
            content: "Use anyhow for errors".into(),
            tags: vec!["Errors".into(), "errors".into()],
            provenance: "test".into(),
            ..Default::default()
        };
        let SaveOutcome::Created { id } = save(d.path(), req.clone()).unwrap() else {
            panic!("expected created");
        };
        assert_eq!(
            save(d.path(), req).unwrap(),
            SaveOutcome::Duplicate { id: id.clone() }
        );

        let out = recall(
            d.path(),
            None,
            RecallRequest {
                query: "anyhow banana".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.total, 1);
        let h = &out.hits[0];
        assert_eq!(h.id, id);
        assert_eq!(h.tags, vec!["errors".to_string()]);
        assert!((h.keyword_score - 0.5).abs() < 1e-9);
        assert!((h.score - 0.25).abs() < 1e-6);
        assert_eq!(h.grounding.grounding_status, GroundingStatus::Unchecked);
    }

    fn recall_hits(dir: &Path, query: &str) -> Vec<RecallHit> {
        recall(
            dir,
            None,
            RecallRequest {
                query: query.into(),
                ..Default::default()
            },
        )
        .unwrap()
        .hits
    }

    #[test]
    fn plurals_and_verb_forms_match_both_ways() {
        let pairs = [
            ("image", "images"),
            ("check", "checks"),
            ("check", "checked"),
            ("check", "checking"),
            ("box", "boxes"),
            ("policy", "policies"),
            ("cache", "caches"),
            ("class", "classes"),
        ];
        for (a, b) in pairs {
            assert_eq!(stem(a), stem(b), "{a} / {b}");
        }
        // Words that only look plural keep their "s".
        assert_eq!(stem("status"), "status");
        assert_eq!(stem("this"), "this");
        assert_eq!(stem("图片"), "图片");

        let d = tempfile::tempdir().unwrap();
        for content in [
            "keep the image check",
            "Project images live under assets/img",
        ] {
            save(
                d.path(),
                SaveRequest {
                    content: content.into(),
                    provenance: "test".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let found = |q: &str| -> Vec<String> {
            let mut v: Vec<String> = recall_hits(d.path(), q)
                .into_iter()
                .map(|h| h.content)
                .collect();
            v.sort();
            v
        };
        assert_eq!(found("image").len(), 2);
        assert_eq!(found("images").len(), 2);
        let hits = recall_hits(d.path(), "image checks");
        assert_eq!(hits[0].content, "keep the image check");
        assert!((hits[0].keyword_score - 1.0).abs() < 1e-9);
    }

    fn recall_contents(root: &Path, q: &str) -> Vec<String> {
        let mut v: Vec<String> = recall_hits(root, q)
            .into_iter()
            .map(|h| h.content)
            .collect();
        v.sort();
        v
    }

    fn save_notes(root: &Path, notes: &[&str]) {
        for content in notes {
            save(
                root,
                SaveRequest {
                    content: content.to_string(),
                    provenance: "test".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
    }

    /// #56 item 3: a one-word query that is a stop word searches that word,
    /// so "IT" and "who" find notes. A query of several words that are all
    /// stop words still finds nothing (#35), and stop words don't dilute a
    /// real query.
    #[test]
    fn single_stop_word_queries_search_the_word() {
        let d = tempfile::tempdir().unwrap();
        save_notes(
            d.path(),
            &[
                "keep the image check of a build",
                "IT owns the VPN certificates",
                "ask who reviewed the schema first",
            ],
        );
        assert_eq!(search_terms("IT"), vec!["it"]);
        assert_eq!(search_terms("Who?"), vec!["who"]);
        assert_eq!(search_terms("the image"), vec![stem("image")]);
        assert_eq!(content_terms("the a of"), Vec::<String>::new());
        assert_eq!(search_terms("the a of"), Vec::<String>::new());
        assert_eq!(search_terms("The, A; OF"), Vec::<String>::new());
        assert!(recall_hits(d.path(), "the a of").is_empty());
        assert!(recall_hits(d.path(), "The, A; OF").is_empty());
        assert!(recall_hits(d.path(), "who it").is_empty());
        assert_eq!(
            recall_contents(d.path(), "IT"),
            vec!["IT owns the VPN certificates"]
        );
        assert_eq!(
            recall_contents(d.path(), "who"),
            vec!["ask who reviewed the schema first"]
        );
        // Stop words don't dilute a real query.
        let hits = recall_hits(d.path(), "the image");
        assert_eq!(hits.len(), 1);
        assert!((hits[0].keyword_score - 1.0).abs() < 1e-9);
        // Punctuation alone still has nothing to search for.
        assert!(search_terms("?!").is_empty());
        assert!(recall_hits(d.path(), "?!").is_empty());
    }

    /// #56 item 1: a few listed words keep their trailing "s", so "new" and
    /// "news" no longer meet. Short plurals still meet their singular.
    #[test]
    fn listed_words_keep_their_s_and_short_plurals_still_meet() {
        assert_eq!(stem("news"), "news");
        assert_ne!(stem("new"), stem("news"));
        assert_eq!(stem("series"), "series");
        assert_eq!(stem("species"), "species");
        assert_eq!(stem("analysis"), "analysis");
        for (plural, singular) in [
            ("tags", "tag"),
            ("logs", "log"),
            ("keys", "key"),
            ("bugs", "bug"),
            ("apis", "api"),
            ("uris", "uri"),
            ("uses", "use"),
            ("images", "image"),
            ("caches", "cache"),
        ] {
            assert_eq!(stem(plural), stem(singular), "{plural}/{singular}");
        }
        let d = tempfile::tempdir().unwrap();
        save_notes(
            d.path(),
            &[
                "news feed is cached",
                "new parser lands in v2",
                "tags are sorted by name",
                "rotate the logs weekly",
                "one key per tenant",
                "triage bugs on Monday",
                "the public api is frozen",
            ],
        );
        assert_eq!(
            recall_contents(d.path(), "new"),
            vec!["new parser lands in v2"]
        );
        assert_eq!(
            recall_contents(d.path(), "news"),
            vec!["news feed is cached"]
        );
        for (q, want) in [
            ("tag", "tags are sorted by name"),
            ("log", "rotate the logs weekly"),
            ("keys", "one key per tenant"),
            ("bug", "triage bugs on Monday"),
            ("apis", "the public api is frozen"),
        ] {
            assert_eq!(recall_contents(d.path(), q), vec![want], "{q}");
        }
    }

    /// #56 item 2: -ed and -ing words try both stems, so "used" and "using"
    /// meet "use" (and each other).
    #[test]
    fn ed_and_ing_words_try_both_stems() {
        assert!(stem_forms("used").contains(&"use".to_string()));
        assert!(stem_forms("using").contains(&"use".to_string()));
        // No form shorter than 3 letters: "used" doesn't add "us".
        assert!(!stem_forms("used").contains(&"us".to_string()));
        for w in ["used", "using", "need", "seed", "feed", "bring", "needed"] {
            assert!(
                stem_forms(w).iter().all(|f| f.len() >= 3),
                "{w}: {:?}",
                stem_forms(w)
            );
        }
        // A form that is a stop word is never added: "thing" must not
        // match every note that says "the".
        assert!(!stem_forms("thing").contains(&"the".to_string()));
        assert_eq!(stem_forms("use"), vec!["use"]);
        let d = tempfile::tempdir().unwrap();
        save_notes(
            d.path(),
            &[
                "we use rustls for TLS",
                "the thing about caching",
                "parser checked inputs",
            ],
        );
        for q in ["used", "using", "use", "Used rustls"] {
            assert_eq!(
                recall_contents(d.path(), q),
                vec!["we use rustls for TLS"],
                "{q}"
            );
        }
        let hits = recall_hits(d.path(), "using rustls");
        assert!((hits[0].keyword_score - 1.0).abs() < 1e-9);
        assert_eq!(
            recall_contents(d.path(), "checking"),
            vec!["parser checked inputs"]
        );
        assert_eq!(
            recall_contents(d.path(), "thing"),
            vec!["the thing about caching"]
        );
    }

    /// #56 QA F1: an -ed/-ing word's extra forms are at least 3 letters, so
    /// "used" doesn't meet "us" and need/seed/feed/bring don't meet
    /// ne/se/fe/br, as on main. 3-letter stems still work: used/using/use
    /// and apis/api.
    #[test]
    fn short_extra_forms_dont_cross_match() {
        let d = tempfile::tempdir().unwrap();
        save_notes(d.path(), &["Ping us before the release freeze"]);
        for q in ["used", "using", "need", "seed", "feed", "bring"] {
            assert!(recall_hits(d.path(), q).is_empty(), "{q}");
        }
        // The word itself still finds it.
        assert_eq!(
            recall_contents(d.path(), "us"),
            vec!["Ping us before the release freeze"]
        );
        save_notes(
            d.path(),
            &["we use rustls for TLS", "the public api is frozen"],
        );
        for q in ["used", "using"] {
            assert_eq!(
                recall_contents(d.path(), q),
                vec!["we use rustls for TLS"],
                "{q}"
            );
        }
        assert_eq!(
            recall_contents(d.path(), "apis"),
            vec!["the public api is frozen"]
        );
        for (plural, singular) in [("guis", "gui"), ("kpis", "kpi"), ("uris", "uri")] {
            assert_eq!(stem(plural), singular);
        }
    }

    /// #14: a live recall whose decay run deletes a faded match says so
    /// once (counting only matches for this query and type), then can't.
    #[test]
    fn live_recall_reports_matches_its_decay_run_removed() {
        let d = tempfile::tempdir().unwrap();
        for (content, t) in [
            ("ghost decision note", "DECISION"),
            ("ghost pattern note", "PATTERN"),
            ("unrelated words", "DECISION"),
        ] {
            save(
                d.path(),
                SaveRequest {
                    content: content.into(),
                    memory_type: Some(t.into()),
                    provenance: "test".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        {
            let mut guard = MemoryStore::lock(d.path()).unwrap();
            let long_ago = OffsetDateTime::now_utc() - time::Duration::days(400);
            for n in guard.graph_mut().nodes.iter_mut() {
                n.created = long_ago;
                n.last_accessed = long_ago;
                n.last_decayed_at = None;
            }
            guard.graph_mut().last_decay_run = None;
            guard.save().unwrap();
        }
        let ask = |t: Option<&str>| {
            recall(
                d.path(),
                None,
                RecallRequest {
                    query: "ghost".into(),
                    memory_type: t.map(str::to_string),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let first = ask(Some("DECISION"));
        assert!(first.decay_ran);
        assert!(first.hits.is_empty());
        assert_eq!(first.removed, 1, "only the DECISION match counts");
        // Deleted for real: nothing left to report on the next run.
        assert!(MemoryStore::load(d.path()).unwrap().nodes.is_empty());
        assert_eq!(ask(None).removed, 0);
    }

    /// #14: the score terms come from the decayed state: importance
    /// before the penalty, times the factor, equals the importance used.
    #[test]
    fn recall_hit_exposes_the_grounding_factor() {
        let d = tempfile::tempdir().unwrap();
        save(
            d.path(),
            SaveRequest {
                content: "ghost anchor note".into(),
                provenance: "test".into(),
                ..Default::default()
            },
        )
        .unwrap();
        {
            let mut guard = MemoryStore::lock(d.path()).unwrap();
            let n = &mut guard.graph_mut().nodes[0];
            n.importance = 0.125;
            n.importance_before_grounding = Some(0.25);
            guard.graph_mut().last_decay_run = Some(OffsetDateTime::now_utc());
            guard.save().unwrap();
        }
        let h = &recall_hits(d.path(), "ghost")[0];
        assert!((h.importance_before_grounding - 0.25).abs() < 1e-6);
        let g = h.grounding_factor.expect("penalty shown");
        assert!((g - 0.5).abs() < 1e-6);
        let product = h.importance_before_grounding as f64 * g * h.keyword_score;
        assert!((product - h.score).abs() < 1e-6);

        // No penalty: no factor.
        let mut n = MemoryNode {
            importance_before_grounding: None,
            ..MemoryStore::load(d.path()).unwrap().nodes[0].clone()
        };
        assert_eq!(grounding_factor(&n), None);
        n.importance_before_grounding = Some(n.importance);
        assert_eq!(grounding_factor(&n), None);
    }

    #[test]
    fn memory_type_round_trips() {
        for t in [
            "PATTERN",
            "DECISION",
            "CONVENTION",
            "TRADE_OFF",
            "CODE_ANCHOR",
        ] {
            assert_eq!(memory_type_name(&parse_memory_type(t).unwrap()), t);
        }
    }
}
