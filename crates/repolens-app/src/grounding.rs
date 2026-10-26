use time::OffsetDateTime;

use repolens_core::model::memory::{GroundingStatus, MemoryGraph, MemoryNode};
use repolens_core::pack_reader::PackIndex;
use repolens_extract::{extraction_fidelity, ExtractionFidelity};

/// Result of cross-referencing a memory node's anchors against a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundingAssessment {
    pub grounding_status: GroundingStatus,
    pub missing_anchors: Vec<String>,
    pub found_anchors: Vec<String>,
}

/// Returns `true` if the anchor looks like a file path or qualified symbol
/// (contains `/` or `.`). Bare names like `AuthService` are not groundable.
fn is_groundable(anchor: &str) -> bool {
    anchor.contains('/') || anchor.contains('.')
}

/// True if the pack contains at least one source file in a language RepoLens
/// extracts with reduced (`Heuristic`, e.g. Python via regex) or no
/// (`Unsupported`) fidelity. In such a pack a *missing* qualified symbol may
/// reflect RepoLens's own extraction gap rather than the symbol's absence, so a
/// symbol-name miss is not a reliable contradiction.
///
/// This is a pack-level signal: a bare qualified anchor (e.g. `pkg.Class.method`)
/// carries no language, so it cannot be tied to a specific file's fidelity. The
/// trade-off is conservative — in a mixed repo a missing *high*-fidelity-language
/// symbol is also treated as Unverifiable rather than Contradicted. That errs
/// toward not penalising a possibly-correct memory, which is the safer error for
/// a memory system whose core promise is to retain accurate memory.
fn pack_has_low_fidelity_source(pack: &PackIndex) -> bool {
    pack.entries.iter().any(|e| {
        matches!(
            extraction_fidelity(&e.language),
            ExtractionFidelity::Heuristic | ExtractionFidelity::Unsupported
        )
    })
}

/// Assess grounding of a memory node against an optional pack index.
///
/// Partitions anchors into groundable vs unsupported, then checks groundable
/// anchors against file paths and qualified symbol names in the pack.
pub fn assess_grounding(node: &MemoryNode, pack: Option<&PackIndex>) -> GroundingAssessment {
    let groundable: Vec<&String> = node.anchors.iter().filter(|a| is_groundable(a)).collect();

    if groundable.is_empty() {
        return GroundingAssessment {
            grounding_status: GroundingStatus::Unchecked,
            missing_anchors: vec![],
            found_anchors: vec![],
        };
    }

    let pack = match pack {
        Some(p) => p,
        None => {
            return GroundingAssessment {
                grounding_status: GroundingStatus::Unverifiable,
                missing_anchors: vec![],
                found_anchors: vec![],
            };
        }
    };

    let mut found_anchors = Vec::new();
    let mut missing_anchors = Vec::new();

    for anchor in &groundable {
        let anchor_str: &str = anchor;
        let matched = pack.entries.iter().any(|entry| {
            // File path match
            if entry.path == anchor_str {
                return true;
            }
            // Qualified symbol match
            entry
                .symbols
                .iter()
                .any(|s| s.qualified_name.as_deref() == Some(anchor_str))
        });

        if matched {
            found_anchors.push(anchor_str.to_string());
        } else {
            missing_anchors.push(anchor_str.to_string());
        }
    }

    let grounding_status = if missing_anchors.is_empty() {
        GroundingStatus::Confirmed
    } else if missing_anchors.iter().any(|a| a.contains('/')) {
        // A missing file-path anchor means the file is genuinely absent from the
        // pack — a reliable, extraction-independent contradiction.
        GroundingStatus::Contradicted
    } else if pack_has_low_fidelity_source(pack) {
        // All misses are qualified-symbol anchors AND the pack contains source in
        // a language RepoLens extracts with reduced/no fidelity (e.g. Python via
        // regex). A symbol-name miss there may be an extraction gap, not a real
        // contradiction — so it is Unverifiable (neutral decay), not Contradicted
        // (0.5× accelerated decay). `missing_anchors` is still reported below, so
        // the unresolved anchors stay visible to the agent.
        GroundingStatus::Unverifiable
    } else {
        // Qualified-symbol miss in an all-AST-fidelity pack: extraction is
        // reliable, so the symbol is genuinely gone — a real contradiction.
        GroundingStatus::Contradicted
    };

    GroundingAssessment {
        grounding_status,
        missing_anchors,
        found_anchors,
    }
}

/// Record a recall-time grounding assessment onto the matching node in the
/// graph, for the decay engine to consume on its next run.
///
/// Idempotent: it *overwrites* the snapshot (`last_grounding` +
/// `last_grounded_at`), never accumulates — re-recalling a node re-stamps the
/// same status without distorting importance. Unknown ids are a no-op.
///
/// Every [`GroundingStatus`] value is recorded (including `Unverifiable` /
/// `Unchecked`) as an honest "last grounding" snapshot; the decay engine only
/// *acts* on `Confirmed` / `Contradicted`.
pub fn record_grounding(
    graph: &mut MemoryGraph,
    id: &str,
    status: GroundingStatus,
    now: OffsetDateTime,
) {
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == id) {
        node.last_grounding = Some(status);
        node.last_grounded_at = Some(now);
    }
}

/// Ground every memory node in `graph` against `pack` and persist each node's
/// grounding snapshot (`last_grounding` + `last_grounded_at`). Returns the
/// number of nodes stamped (= all nodes). Each node gets an honest snapshot —
/// `Confirmed` / `Contradicted` / `Unchecked` — never `Unverifiable`, because a
/// pack is always supplied. Idempotent: overwrites the snapshot, never
/// accumulates. No I/O: mutates `graph` only.
///
/// O(n): unlike a loop of `record_grounding` calls (which re-scans nodes by id
/// each call → O(n^2)), this assesses and stamps each node in one pass.
/// `record_grounding` stays for single-node callers (recall, get_summary).
pub fn ground_all_memories(
    graph: &mut MemoryGraph,
    pack: &PackIndex,
    now: OffsetDateTime,
) -> usize {
    for node in graph.nodes.iter_mut() {
        let status = assess_grounding(node, Some(pack)).grounding_status;
        node.last_grounding = Some(status);
        node.last_grounded_at = Some(now);
    }
    graph.nodes.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::{FileEntry, Symbol};
    use repolens_core::model::manifest::Manifest;
    use repolens_core::model::memory::{MemorySource, MemoryType};
    use repolens_core::serde_util::java_epoch_seconds::JavaEpochSeconds;
    use rust_decimal::Decimal;
    use time::OffsetDateTime;

    fn make_node(anchors: Vec<&str>) -> MemoryNode {
        let now = OffsetDateTime::now_utc();
        MemoryNode {
            id: "test-node".to_string(),
            content: "test content".to_string(),
            memory_type: MemoryType::Decision,
            importance: 0.8,
            created: now,
            last_accessed: now,
            access_count: 1,
            anchors: anchors.into_iter().map(String::from).collect(),
            tags: vec![],
            source: MemorySource::Explicit,
            provenance: "test".to_string(),
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256: Default::default(),
        }
    }

    fn make_pack(paths: Vec<&str>, symbols: Vec<(&str, &str)>) -> PackIndex {
        let mut entries: Vec<FileEntry> = paths
            .into_iter()
            .map(|p| FileEntry {
                path: p.to_string(),
                sha256: String::new(),
                language: String::new(),
                loc: 0,
                included: true,
                reason: String::new(),
                content_path: None,
                symbols: vec![],
            })
            .collect();

        for (file_path, qualified_name) in symbols {
            let sym = Symbol {
                symbol_type: "class".to_string(),
                name: qualified_name
                    .rsplit('.')
                    .next()
                    .unwrap_or(qualified_name)
                    .to_string(),
                qualified_name: Some(qualified_name.to_string()),
                visibility: None,
                line: None,
            };
            if let Some(entry) = entries.iter_mut().find(|e| e.path == file_path) {
                entry.symbols.push(sym);
            } else {
                entries.push(FileEntry {
                    path: file_path.to_string(),
                    sha256: String::new(),
                    language: String::new(),
                    loc: 0,
                    included: true,
                    reason: String::new(),
                    content_path: None,
                    symbols: vec![sym],
                });
            }
        }

        PackIndex {
            manifest: Manifest {
                schema_version: "1.0".to_string(),
                schema_url: "".to_string(),
                timestamp: JavaEpochSeconds(Decimal::ZERO),
                chunks: vec![],
                scan_mode: None,
                scan_scope: None,
                base_pack_id: None,
                base_head_commit: None,
                head_commit: None,
                worktree_dirty: None,
                scan_config_hash: None,
                reprocessed_files: None,
                deleted_files: None,
                carried_forward_files: None,
            },
            entries,
        }
    }

    #[test]
    fn file_path_anchor_found() {
        let node = make_node(vec!["src/auth.rs"]);
        let pack = make_pack(vec!["src/auth.rs"], vec![]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Confirmed);
        assert_eq!(result.found_anchors, vec!["src/auth.rs"]);
        assert!(result.missing_anchors.is_empty());
    }

    #[test]
    fn qualified_symbol_anchor_found() {
        let node = make_node(vec!["com.example.AuthService"]);
        let pack = make_pack(
            vec![],
            vec![("src/AuthService.java", "com.example.AuthService")],
        );
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Confirmed);
        assert_eq!(result.found_anchors, vec!["com.example.AuthService"]);
        assert!(result.missing_anchors.is_empty());
    }

    #[test]
    fn missing_file_path_anchor() {
        let node = make_node(vec!["src/deleted.rs"]);
        let pack = make_pack(vec!["src/main.rs"], vec![]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Contradicted);
        assert_eq!(result.missing_anchors, vec!["src/deleted.rs"]);
        assert!(result.found_anchors.is_empty());
    }

    #[test]
    fn missing_qualified_symbol_anchor() {
        let node = make_node(vec!["com.example.RemovedService"]);
        let pack = make_pack(vec![], vec![("src/Other.java", "com.example.Other")]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Contradicted);
        assert_eq!(result.missing_anchors, vec!["com.example.RemovedService"]);
        assert!(result.found_anchors.is_empty());
    }

    #[test]
    fn no_anchors_returns_unchecked() {
        let node = make_node(vec![]);
        let pack = make_pack(vec!["src/main.rs"], vec![]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Unchecked);
        assert!(result.found_anchors.is_empty());
        assert!(result.missing_anchors.is_empty());
    }

    #[test]
    fn unsupported_anchor_format_returns_unchecked() {
        let node = make_node(vec!["AuthService"]);
        let pack = make_pack(vec!["src/auth.rs"], vec![]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Unchecked);
        assert!(result.found_anchors.is_empty());
        assert!(result.missing_anchors.is_empty());
    }

    #[test]
    fn no_pack_returns_unverifiable() {
        let node = make_node(vec!["src/auth.rs"]);
        let result = assess_grounding(&node, None);
        assert_eq!(result.grounding_status, GroundingStatus::Unverifiable);
        assert!(result.found_anchors.is_empty());
        assert!(result.missing_anchors.is_empty());
    }

    #[test]
    fn mixed_found_and_missing_returns_contradicted() {
        let node = make_node(vec!["src/auth.rs", "src/deleted.rs"]);
        let pack = make_pack(vec!["src/auth.rs"], vec![]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Contradicted);
        assert_eq!(result.found_anchors, vec!["src/auth.rs"]);
        assert_eq!(result.missing_anchors, vec!["src/deleted.rs"]);
    }

    // -- Iteration 6: extraction-fidelity-aware grounding --------------------

    /// Pack of `(path, language)` files, no symbols — to drive the pack-level
    /// fidelity check.
    fn pack_with_languages(files: Vec<(&str, &str)>) -> PackIndex {
        let mut p = make_pack(vec![], vec![]);
        p.entries = files
            .into_iter()
            .map(|(path, lang)| FileEntry {
                path: path.to_string(),
                sha256: String::new(),
                language: lang.to_string(),
                loc: 0,
                included: true,
                reason: String::new(),
                content_path: None,
                symbols: vec![],
            })
            .collect();
        p
    }

    /// A correct memory about a Python method whose regex extractor never emitted
    /// a qualified name: the anchor is "missing", but that is RepoLens's
    /// extraction gap (Python = Heuristic fidelity), not a contradiction. Must be
    /// Unverifiable (neutral decay), not Contradicted (0.5× accelerated decay).
    #[test]
    fn qualified_miss_in_low_fidelity_pack_is_unverifiable_not_contradicted() {
        let node = make_node(vec!["worker.Worker.run"]);
        let pack = pack_with_languages(vec![("py/worker.py", "Python")]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Unverifiable);
        // Transparency preserved: the unresolved anchor is still reported.
        assert_eq!(result.missing_anchors, vec!["worker.Worker.run"]);
    }

    /// A genuinely-absent file is a reliable, extraction-independent
    /// contradiction even when the pack also contains low-fidelity source.
    #[test]
    fn missing_file_path_anchor_stays_contradicted_even_with_low_fidelity_pack() {
        let node = make_node(vec!["py/gone.py"]);
        let pack = pack_with_languages(vec![("py/worker.py", "Python")]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Contradicted);
    }

    /// All-AST-fidelity pack: a symbol-name miss is genuine (extraction is
    /// reliable), so it stays Contradicted.
    #[test]
    fn qualified_miss_in_high_fidelity_pack_stays_contradicted() {
        let node = make_node(vec!["foo.Bar"]);
        let pack = pack_with_languages(vec![("src/foo.rs", "Rust")]);
        let result = assess_grounding(&node, Some(&pack));
        assert_eq!(result.grounding_status, GroundingStatus::Contradicted);
    }

    fn graph_with_nodes(nodes: Vec<MemoryNode>) -> MemoryGraph {
        let now = OffsetDateTime::now_utc();
        MemoryGraph {
            schema_version: "1.0".to_string(),
            repo_root: "/tmp/test".to_string(),
            nodes,
            edges: vec![],
            last_decay_run: None,
            updated_at: now,
        }
    }

    #[test]
    fn ground_all_memories_stamps_every_node() {
        let pack = make_pack(vec!["src/keep.rs"], vec![]);
        let now = OffsetDateTime::now_utc();

        let mut resolving = make_node(vec!["src/keep.rs"]);
        resolving.id = "resolving".to_string();
        let mut missing = make_node(vec!["src/gone.rs"]);
        missing.id = "missing".to_string();
        let mut bare = make_node(vec!["BareName"]);
        bare.id = "bare".to_string();
        let mut no_anchor = make_node(vec![]);
        no_anchor.id = "noanchor".to_string();

        let mut graph = graph_with_nodes(vec![resolving, missing, bare, no_anchor]);

        let n = ground_all_memories(&mut graph, &pack, now);
        assert_eq!(n, 4);

        let by_id = |id: &str| graph.nodes.iter().find(|x| x.id == id).unwrap();
        assert_eq!(
            by_id("resolving").last_grounding,
            Some(GroundingStatus::Confirmed)
        );
        assert_eq!(
            by_id("missing").last_grounding,
            Some(GroundingStatus::Contradicted)
        );
        assert_eq!(
            by_id("bare").last_grounding,
            Some(GroundingStatus::Unchecked)
        );
        assert_eq!(
            by_id("noanchor").last_grounding,
            Some(GroundingStatus::Unchecked)
        );
        assert!(by_id("resolving").last_grounded_at.is_some());
    }

    #[test]
    fn ground_all_memories_empty_graph_is_zero() {
        let pack = make_pack(vec!["src/keep.rs"], vec![]);
        let now = OffsetDateTime::now_utc();
        let mut graph = graph_with_nodes(vec![]);
        assert_eq!(ground_all_memories(&mut graph, &pack, now), 0);
    }

    #[test]
    fn ground_all_memories_overwrites_prior_snapshot() {
        let pack = make_pack(vec!["src/keep.rs"], vec![]);
        let now = OffsetDateTime::now_utc();
        let mut node = make_node(vec!["src/keep.rs"]);
        node.id = "n".to_string();
        node.last_grounding = Some(GroundingStatus::Contradicted); // stale
        let mut graph = graph_with_nodes(vec![node]);

        ground_all_memories(&mut graph, &pack, now);
        assert_eq!(
            graph.nodes[0].last_grounding,
            Some(GroundingStatus::Confirmed)
        );
        assert_eq!(graph.nodes[0].last_grounded_at, Some(now));
    }

    fn graph_with_node(node: MemoryNode) -> MemoryGraph {
        graph_with_nodes(vec![node])
    }

    #[test]
    fn record_grounding_sets_snapshot() {
        let mut node = make_node(vec!["src/auth.rs"]);
        node.id = "n1".to_string();
        let mut graph = graph_with_node(node);
        let now = OffsetDateTime::now_utc();

        // Records status + timestamp on the matching node.
        record_grounding(&mut graph, "n1", GroundingStatus::Confirmed, now);
        assert_eq!(
            graph.nodes[0].last_grounding,
            Some(GroundingStatus::Confirmed)
        );
        assert_eq!(graph.nodes[0].last_grounded_at, Some(now));

        // Unknown id is a no-op — the existing snapshot is untouched.
        record_grounding(&mut graph, "missing", GroundingStatus::Contradicted, now);
        assert_eq!(
            graph.nodes[0].last_grounding,
            Some(GroundingStatus::Confirmed)
        );

        // Re-call overwrites (idempotent — never accumulates).
        let later = now + time::Duration::hours(1);
        record_grounding(&mut graph, "n1", GroundingStatus::Contradicted, later);
        assert_eq!(
            graph.nodes[0].last_grounding,
            Some(GroundingStatus::Contradicted)
        );
        assert_eq!(graph.nodes[0].last_grounded_at, Some(later));
    }
}
