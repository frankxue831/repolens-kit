use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::graph::{FileGraph, GraphResolutionMetrics};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Returns the path to `fixtures/scenarios/`.
fn scenarios_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repolens-rs root
    p.push("fixtures");
    p.push("scenarios");
    p
}

fn load_graph(scenario: &str) -> FileGraph {
    let path = scenarios_dir()
        .join(scenario)
        .join("pack")
        .join("graph.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse graph.json for {}: {}", scenario, e))
}

/// Load all pack entries for a scenario by walking its `files-*.json`
/// chunks. Duplicated from `pack_contract_test.rs` to avoid a test-crate
/// cross-file dependency; the two files share the same scenario fixtures.
fn load_entries(scenario: &str) -> Vec<FileEntry> {
    let pack_dir = scenarios_dir().join(scenario).join("pack");
    let mut chunk_files: Vec<PathBuf> = fs::read_dir(&pack_dir)
        .unwrap_or_else(|e| panic!("failed to read pack dir for {}: {}", scenario, e))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("files-") && name_str.ends_with(".json") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    chunk_files.sort();
    let mut out = Vec::new();
    for path in chunk_files {
        let json = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
        let chunk: Vec<FileEntry> = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));
        out.extend(chunk);
    }
    out
}

fn load_metrics(scenario: &str) -> GraphResolutionMetrics {
    let path = scenarios_dir()
        .join(scenario)
        .join("pack")
        .join("graph-metrics.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse graph-metrics.json for {}: {}", scenario, e))
}

/// Valid edge type strings as defined by the Java extractor.
const VALID_EDGE_TYPES: &[&str] = &["imports", "extends", "implements", "calls", "type_ref"];

// ---------------------------------------------------------------------------
// Deserialization — all scenarios
// ---------------------------------------------------------------------------

#[test]
fn graph_java_small_deserializes() {
    let g = load_graph("java-small");
    // 4 nodes (README.md, pom.xml, App.java, Helper.java)
    assert_eq!(g.nodes.len(), 4);
    // 3 edges (imports, calls, type_ref)
    assert_eq!(g.edges.len(), 3);
}

#[test]
fn graph_mixed_small_deserializes() {
    let g = load_graph("mixed-small");
    assert!(!g.nodes.is_empty(), "mixed-small graph must have nodes");
    // At least one edge
    assert!(!g.edges.is_empty(), "mixed-small graph must have edges");
}

#[test]
fn graph_excluded_small_deserializes() {
    let g = load_graph("excluded-small");
    assert!(!g.nodes.is_empty(), "excluded-small graph must have nodes");
    // May have zero edges for a tiny excluded repo — just verify it parses
    let _edges_count = g.edges.len();
}

// ---------------------------------------------------------------------------
// Metrics deserialization — all scenarios
// ---------------------------------------------------------------------------

#[test]
fn metrics_java_small_deserializes() {
    let m = load_metrics("java-small");
    assert_eq!(m.total_relationships, 3);
    assert_eq!(m.resolved_edges, 3);
    assert_eq!(m.unresolved_edges, 0);
    let per_lang = m
        .per_language
        .as_ref()
        .expect("java-small must have perLanguage");
    assert!(
        per_lang.contains_key("Java"),
        "java-small perLanguage must have Java entry"
    );
    assert_eq!(per_lang["Java"], vec![3, 0]);
}

#[test]
fn metrics_mixed_small_deserializes() {
    let m = load_metrics("mixed-small");
    assert_eq!(m.total_relationships, 1);
    assert_eq!(m.resolved_edges, 1);
    assert_eq!(m.unresolved_edges, 0);
    let per_lang = m
        .per_language
        .as_ref()
        .expect("mixed-small must have perLanguage");
    assert!(
        per_lang.contains_key("TypeScript"),
        "mixed-small perLanguage must have TypeScript"
    );
}

#[test]
fn metrics_excluded_small_deserializes() {
    let m = load_metrics("excluded-small");
    assert_eq!(m.total_relationships, 0);
    assert_eq!(m.resolved_edges, 0);
    assert_eq!(m.unresolved_edges, 0);
    // perLanguage is an empty object {} in the fixture — deserializes to Some({})
    // (not None, since the key is present)
    let per_lang = m
        .per_language
        .as_ref()
        .expect("excluded-small must have perLanguage key");
    assert!(
        per_lang.is_empty(),
        "excluded-small perLanguage must be empty"
    );
}

// ---------------------------------------------------------------------------
// Edge-type validation — all scenarios
// ---------------------------------------------------------------------------

#[test]
fn all_edge_types_are_valid_in_java_small() {
    let g = load_graph("java-small");
    for edge in &g.edges {
        assert!(
            VALID_EDGE_TYPES.contains(&edge.edge_type.as_str()),
            "unexpected edge type {:?} in java-small (source={}, target={})",
            edge.edge_type,
            edge.source,
            edge.target,
        );
    }
}

#[test]
fn all_edge_types_are_valid_in_mixed_small() {
    let g = load_graph("mixed-small");
    for edge in &g.edges {
        assert!(
            VALID_EDGE_TYPES.contains(&edge.edge_type.as_str()),
            "unexpected edge type {:?} in mixed-small (source={}, target={})",
            edge.edge_type,
            edge.source,
            edge.target,
        );
    }
}

#[test]
fn all_edge_types_are_valid_in_excluded_small() {
    let g = load_graph("excluded-small");
    for edge in &g.edges {
        assert!(
            VALID_EDGE_TYPES.contains(&edge.edge_type.as_str()),
            "unexpected edge type {:?} in excluded-small (source={}, target={})",
            edge.edge_type,
            edge.source,
            edge.target,
        );
    }
}

#[test]
fn every_edge_endpoint_exists_as_a_node_in_all_scenarios() {
    for scenario in ["java-small", "mixed-small", "excluded-small"] {
        let g = load_graph(scenario);
        let node_ids: std::collections::HashSet<&str> =
            g.nodes.iter().map(|n| n.id.as_str()).collect();

        for edge in &g.edges {
            assert!(
                node_ids.contains(edge.source.as_str()),
                "{scenario}: missing source node for edge {:?}",
                edge
            );
            assert!(
                node_ids.contains(edge.target.as_str()),
                "{scenario}: missing target node for edge {:?}",
                edge
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Node content checks
// ---------------------------------------------------------------------------

#[test]
fn java_small_nodes_have_expected_files() {
    let g = load_graph("java-small");
    let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(
        ids.contains(&"src/main/java/com/example/App.java"),
        "App.java must be a node"
    );
    assert!(
        ids.contains(&"src/main/java/com/example/Helper.java"),
        "Helper.java must be a node"
    );
}

#[test]
fn java_small_app_node_has_symbols() {
    let g = load_graph("java-small");
    let app = g
        .nodes
        .iter()
        .find(|n| n.id == "src/main/java/com/example/App.java")
        .expect("App.java node must exist");
    assert!(!app.symbols.is_empty(), "App.java must have symbols");
    assert!(
        app.symbols.contains(&"App".to_string()),
        "App.java must have 'App' symbol"
    );
}

#[test]
fn java_small_readme_node_has_empty_symbols() {
    let g = load_graph("java-small");
    let readme = g
        .nodes
        .iter()
        .find(|n| n.id == "README.md")
        .expect("README.md node must exist");
    assert!(
        readme.symbols.is_empty(),
        "README.md must have empty symbols"
    );
}

// ---------------------------------------------------------------------------
// Edge content checks
// ---------------------------------------------------------------------------

#[test]
fn java_small_edges_have_detail() {
    let g = load_graph("java-small");
    for edge in &g.edges {
        assert!(
            edge.detail.is_some(),
            "all java-small edges should have detail (edge {} -> {})",
            edge.source,
            edge.target
        );
    }
}

#[test]
fn java_small_has_all_three_edge_types() {
    let g = load_graph("java-small");
    let types: Vec<&str> = g.edges.iter().map(|e| e.edge_type.as_str()).collect();
    assert!(
        types.contains(&"imports"),
        "java-small must have an imports edge"
    );
    assert!(
        types.contains(&"calls"),
        "java-small must have a calls edge"
    );
    assert!(
        types.contains(&"type_ref"),
        "java-small must have a type_ref edge"
    );
}

// ---------------------------------------------------------------------------
// Snapshot — re-serialization of java-small graph.json
// ---------------------------------------------------------------------------

#[test]
fn snapshot_java_small_graph_reserialization() {
    let g = load_graph("java-small");
    let json = serde_json::to_string_pretty(&g).expect("re-serialization must succeed");
    insta::assert_snapshot!("java_small_graph", json);
}

#[test]
fn snapshot_java_small_metrics_reserialization() {
    let m = load_metrics("java-small");
    let json = serde_json::to_string_pretty(&m).expect("re-serialization must succeed");
    insta::assert_snapshot!("java_small_metrics", json);
}

// ---------------------------------------------------------------------------
// Observation invariants for the `graph import edges` family.
//
// These tests implement the catalogue defined in
// `docs/superpowers/specs/2026-04-14-graph-import-edges-invariant-catalogue.md`.
// The pre-existing `every_edge_endpoint_exists_as_a_node_in_all_scenarios`
// test above implements `I-GRAPH-EDGE-CLOSURE` from the prior rollout; the
// four tests below add the remaining invariants named in the catalogue.
//
// Scoped to the captured contract scenarios
// (`java-small`, `mixed-small`, `excluded-small`). Declares edges, cycle
// detection, and orphan detection all require Rust fixtures and are deferred
// to a later catalogue.
// ---------------------------------------------------------------------------

const INVARIANT_SCENARIOS: &[&str] = &["java-small", "mixed-small", "excluded-small"];

/// I-GRAPH-NODE-ID-UNIQUE — within a single graph, `nodes[].id` values are
/// unique.
///
/// Guards against duplicate node entries that would let
/// `every_edge_endpoint_exists_as_a_node` pass vacuously while concealing a
/// resolver bug that emits the same file twice.
#[test]
fn i_graph_node_id_unique() {
    for scenario in INVARIANT_SCENARIOS {
        let g = load_graph(scenario);
        let total = g.nodes.len();
        let unique: HashSet<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            unique.len(),
            total,
            "{scenario}: graph node ids must be unique (total={total}, unique={})",
            unique.len()
        );
    }
}

/// I-GRAPH-NODE-MATCHES-PACK-PATH — every `graph.nodes[].id` appears as an
/// included `FileEntry.path` in the same pack.
///
/// Guards against resolver ghost nodes: an import that resolves to a
/// non-existent file should never produce a graph node, because a downstream
/// consumer asking "give me the captured bytes for this node" would fail.
/// An included-entry match is required because the graph describes the
/// scanned set, not the gitignored set.
#[test]
fn i_graph_node_matches_pack_path() {
    for scenario in INVARIANT_SCENARIOS {
        let g = load_graph(scenario);
        let entries = load_entries(scenario);
        let included_paths: HashSet<&str> = entries
            .iter()
            .filter(|e| e.included)
            .map(|e| e.path.as_str())
            .collect();
        for node in &g.nodes {
            assert!(
                included_paths.contains(node.id.as_str()),
                "{scenario}: graph node {:?} is not an included FileEntry in the same pack",
                node.id
            );
        }
    }
}

/// I-GRAPH-NO-SELF-LOOP — no edge has `source == target`.
///
/// A file does not import, call, extend, or type-ref itself at the
/// file-granularity we model. A self-loop would indicate a resolver bug that
/// fell back to the same file when the true target could not be resolved —
/// the exact class of silent corruption Path B is meant to catch.
#[test]
fn i_graph_no_self_loop() {
    for scenario in INVARIANT_SCENARIOS {
        let g = load_graph(scenario);
        for edge in &g.edges {
            assert_ne!(
                edge.source, edge.target,
                "{scenario}: self-loop edge {:?} on file {:?}",
                edge.edge_type, edge.source
            );
        }
    }
}

/// I-GRAPH-IMPORT-EDGE-DETAIL — every `imports`-type edge has non-empty
/// `detail`.
///
/// The `detail` field carries the resolver input (the import spec or
/// qualified name that produced the edge). An imports edge without detail
/// is unresolvable on inspection — a memory consumer cannot tell which
/// import statement the edge came from, which breaks audit. Non-imports
/// edge types (`calls`, `type_ref`, `extends`, `implements`) are permitted
/// to carry `None` because their detail is not used by the resolver.
#[test]
fn i_graph_import_edge_detail() {
    for scenario in INVARIANT_SCENARIOS {
        let g = load_graph(scenario);
        for edge in &g.edges {
            if edge.edge_type != "imports" {
                continue;
            }
            let detail = edge.detail.as_deref().unwrap_or_else(|| {
                panic!(
                    "{scenario}: imports edge {} -> {} must have Some(detail)",
                    edge.source, edge.target
                )
            });
            assert!(
                !detail.is_empty(),
                "{scenario}: imports edge {} -> {} has empty detail",
                edge.source,
                edge.target
            );
        }
    }
}
