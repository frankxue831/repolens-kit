//! MCP tool handlers for persistent memory: memorize, recall, forget.

use std::path::Path;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use rmcp::{model::CallToolResult, ErrorData as McpError};

use repolens_app::discover::PackResolver;
use repolens_app::memory_ops::{self, keyword_score, tokenize};
use repolens_core::model::memory::{GroundingStatus, MemoryGraph, MemoryNode, MemoryType};
use repolens_memory::store::{self, MemoryStore};

use crate::epistemic::*;
use crate::err_json;

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorizeInsightParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "The memory content to store")]
    pub content: String,

    #[serde(rename = "type")]
    #[schemars(description = "Memory type: PATTERN, DECISION, CONVENTION, TRADE_OFF, CODE_ANCHOR")]
    pub memory_type: Option<String>,

    #[schemars(description = "Importance score 0.0-1.0 (default 0.5)")]
    pub importance: Option<f32>,

    #[schemars(description = "File path anchors for this memory")]
    pub anchors: Option<Vec<String>>,

    #[schemars(description = "Tags (auto-lowercased, deduped)")]
    pub tags: Option<Vec<String>>,

    #[schemars(description = "UUID of memory this replaces (creates SUPERSEDES edge)")]
    pub supersedes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallLongtermParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "Search query string")]
    pub query: String,

    #[serde(rename = "type")]
    #[schemars(
        description = "Filter by memory type: PATTERN, DECISION, CONVENTION, TRADE_OFF, CODE_ANCHOR"
    )]
    pub memory_type: Option<String>,

    #[schemars(description = "Filter by tags (AND match)")]
    pub tags: Option<Vec<String>>,

    #[serde(rename = "minImportance")]
    #[schemars(description = "Minimum importance threshold")]
    pub min_importance: Option<f32>,

    #[schemars(description = "Maximum results to return (default 10)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForgetInsightParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(
        description = "UUID of a memory to delete, or a unique prefix of at least 4 characters (any case, such as the 8 characters `repolens memory save` prints)"
    )]
    pub id: Option<String>,

    #[schemars(description = "Keyword query to find memories to delete")]
    pub query: Option<String>,

    #[schemars(description = "Set true to actually delete (default false = dry-run)")]
    pub confirm: Option<bool>,
}

// ---------------------------------------------------------------------------
// Response structs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallResult {
    id: String,
    content: String,
    #[serde(rename = "type")]
    memory_type: MemoryType,
    importance: f32,
    score: f64,
    tags: Vec<String>,
    anchors: Vec<String>,
    grounding_status: GroundingStatus,
    missing_anchors: Vec<String>,
    found_anchors: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallResponse {
    results: Vec<RecallResult>,
    total: usize,
    decay_ran: bool,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub fn handle_memorize_insight(params: MemorizeInsightParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, "memorize_insight: entry");

    let req = memory_ops::SaveRequest {
        content: params.content,
        memory_type: params.memory_type,
        importance: params.importance,
        anchors: params.anchors.unwrap_or_default(),
        tags: params.tags.unwrap_or_default(),
        supersedes: params.supersedes,
        provenance: "mcp:memorize_insight".to_string(),
    };
    let outcome = match memory_ops::save(Path::new(&params.root), req) {
        Ok(o) => o,
        Err(msg) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %msg, "memorize_insight: error");
            return err_json(&msg);
        }
    };

    let (node_id, body) = match outcome {
        memory_ops::SaveOutcome::Duplicate { id } => {
            info!(elapsed_ms = %start.elapsed().as_millis(), duplicate_of = %id, "memorize_insight: duplicate detected");
            let body = serde_json::json!({ "status": "duplicate", "duplicateOf": id });
            (id, body)
        }
        memory_ops::SaveOutcome::Created { id } => {
            info!(elapsed_ms = %start.elapsed().as_millis(), id = %id, "memorize_insight: exit");
            let body = serde_json::json!({ "status": "created", "id": id });
            (id, body)
        }
    };
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::MemoryNode {
            node_id,
            decay_importance: None,
            grounding_status: None,
        }),
        body,
    )
}

pub fn handle_recall_longterm(
    resolver: &PackResolver,
    params: RecallLongtermParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, query = %params.query, "recall_longterm: entry");

    // Validate the type filter before touching the store or the pack.
    if let Some(t) = params.memory_type.as_deref() {
        if let Err(msg) = memory_ops::parse_memory_type(t) {
            return err_json(&msg);
        }
    }
    let resolved_pack = resolver.resolve(None, Some(&params.root)).ok();
    let req = memory_ops::RecallRequest {
        query: params.query,
        memory_type: params.memory_type,
        tags: params.tags,
        min_importance: params.min_importance,
        limit: params.limit,
    };
    let outcome = match memory_ops::recall(Path::new(&params.root), resolved_pack.clone(), req) {
        Ok(o) => o,
        Err(msg) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %msg, "recall_longterm: error");
            return err_json(&msg);
        }
    };

    let results: Vec<RecallResult> = outcome
        .hits
        .into_iter()
        .map(|h| RecallResult {
            id: h.id,
            content: h.content,
            memory_type: h.memory_type,
            importance: h.importance,
            score: h.score,
            tags: h.tags,
            anchors: h.anchors,
            grounding_status: h.grounding.grounding_status,
            missing_anchors: h.grounding.missing_anchors,
            found_anchors: h.grounding.found_anchors,
        })
        .collect();
    let total = outcome.total;
    let decay_ran = outcome.decay_ran;
    let response = RecallResponse {
        total,
        results,
        decay_ran,
    };

    let result_ids: Vec<String> = response.results.iter().map(|r| r.id.clone()).collect();

    let (layers, confidence, provenance) = if let Some((ref pack_dir, _)) = resolved_pack {
        (
            KnowledgeLayerField::Multiple(vec![
                KnowledgeLayer::Interpretation,
                KnowledgeLayer::Observation,
            ]),
            EpistemicConfidence::Variable,
            ProvenanceField::Multiple(vec![
                Provenance::MemoryStore {
                    source: "memory.json".into(),
                    node_ids: result_ids,
                },
                Provenance::PackExtraction {
                    pack_id: pack_id_from_dir(pack_dir),
                    source: "grounding".into(),
                },
            ]),
        )
    } else {
        (
            KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
            EpistemicConfidence::Variable,
            ProvenanceField::Single(Provenance::MemoryStore {
                source: "memory.json".into(),
                node_ids: result_ids,
            }),
        )
    };

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        total,
        returned = response.results.len(),
        decay_ran,
        "recall_longterm: exit"
    );
    ok_epistemic(
        layers,
        confidence,
        provenance,
        serde_json::to_value(&response).unwrap_or_default(),
    )
}

/// Nodes `forget_insight` would remove. An `id` is matched by
/// [`memory_ops::find_by_id`], the same lookup `repolens memory forget`
/// uses: a full id, or a unique prefix of at least
/// [`memory_ops::MIN_ID_PREFIX`] characters, in any case. A shorter prefix
/// or one shared by several notes is an error and selects nothing, for a
/// dry run as well as a confirmed delete.
fn forget_matches(
    graph: &MemoryGraph,
    id: Option<&str>,
    query: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    if let Some(id) = id.filter(|s| !s.trim().is_empty()) {
        return match memory_ops::find_by_id(graph, id) {
            Ok(n) => Ok(n
                .into_iter()
                .map(|n| (n.id.clone(), n.content.clone()))
                .collect()),
            Err(memory_ops::IdLookupError::Ambiguous(a)) => {
                let (shown, more) = a.listed();
                let mut list = shown.join(", ");
                if more > 0 {
                    list.push_str(&format!(", …and {more} more"));
                }
                Err(format!(
                    "{} Matches: {list}.",
                    memory_ops::IdLookupError::Ambiguous(a.clone()).message()
                ))
            }
            Err(e) => Err(e.message()),
        };
    }

    let query_tokens = tokenize(query.unwrap_or(""));
    let mut scored: Vec<(&MemoryNode, f64)> = graph
        .nodes
        .iter()
        .map(|n| {
            let ks = keyword_score(&query_tokens, n);
            (n, ks)
        })
        .filter(|(_, score)| *score > 0.1)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored
        .into_iter()
        .map(|(n, _)| (n.id.clone(), n.content.clone()))
        .collect())
}

pub fn handle_forget_insight(params: ForgetInsightParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, "forget_insight: entry");

    // Require either id or query; treat an empty/whitespace query as missing
    // so callers don't silently get a zero-match response.
    let has_id = params.id.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_query = params
        .query
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_id && !has_query {
        info!(elapsed_ms = %start.elapsed().as_millis(), "forget_insight: missing id or query");
        return err_json("Either 'id' or 'query' must be provided and non-empty");
    }

    let root = Path::new(&params.root);
    let confirm = params.confirm.unwrap_or(false);

    // Select on a read-only load first: a dry run, a refused id or no match
    // creates no lock file or memory directory.
    let graph = match MemoryStore::load(root) {
        Ok(g) => g,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "forget_insight: load error");
            return err_json(&format!("Failed to load memory graph: {e}"));
        }
    };
    let matches = match forget_matches(&graph, params.id.as_deref(), params.query.as_deref()) {
        Ok(m) => m,
        Err(e) => return err_json(&e),
    };

    if !confirm {
        // Dry-run
        let match_ids: Vec<String> = matches.iter().map(|(id, _)| id.clone()).collect();
        let match_items: Vec<serde_json::Value> = matches
            .iter()
            .map(|(id, content)| serde_json::json!({ "id": id, "content": content }))
            .collect();
        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            match_count = matches.len(),
            "forget_insight: dry_run exit"
        );
        return ok_epistemic(
            KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
            EpistemicConfidence::Variable,
            ProvenanceField::Single(Provenance::MemoryStore {
                source: "memory.json".into(),
                node_ids: match_ids,
            }),
            serde_json::json!({
                "status": "dry_run",
                "matches": match_items,
            }),
        );
    }

    // Lock and re-select for the delete transaction only when something
    // matched; another writer may have changed the graph since the load.
    let mut guard = None;
    let matches = if matches.is_empty() {
        matches
    } else {
        let g = match MemoryStore::lock(root) {
            Ok(g) => g,
            Err(e) => {
                info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "forget_insight: load error");
                return err_json(&format!("Failed to load memory graph: {e}"));
            }
        };
        let m = match forget_matches(g.graph(), params.id.as_deref(), params.query.as_deref()) {
            Ok(m) => m,
            Err(e) => return err_json(&e),
        };
        guard = Some(g);
        m
    };

    // Actually delete
    let removed_ids: Vec<String> = matches.iter().map(|(id, _)| id.clone()).collect();
    if let Some(guard) = guard.as_mut().filter(|_| !removed_ids.is_empty()) {
        for id in &removed_ids {
            store::remove_node(guard.graph_mut(), id);
        }
        if let Err(e) = guard.save() {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "forget_insight: save error");
            return err_json(&format!("Failed to save memory graph: {e}"));
        }
    }

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        removed_count = removed_ids.len(),
        "forget_insight: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::MemoryStore {
            source: "memory.json".into(),
            node_ids: removed_ids.clone(),
        }),
        serde_json::json!({
            "status": "deleted",
            "removedCount": removed_ids.len(),
            "removedIds": removed_ids,
        }),
    )
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------
