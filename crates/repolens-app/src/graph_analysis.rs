use std::collections::{HashMap, HashSet};

use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;

use repolens_core::model::graph::{FileGraph, GraphEdge};

/// Edge types considered "structural" for orphan analysis — a file
/// referenced by any of these has a real reason to exist in the graph.
/// `calls` edges are excluded: they project from method names to files and
/// are too noisy for structural signals.
/// `type_import` (a Python import inside `if TYPE_CHECKING:`) counts: the
/// file is used, even if only by the type checker.
const STRUCTURAL_EDGE_TYPES: &[&str] = &[
    "imports",
    "type_import",
    "extends",
    "implements",
    "type_ref",
    "declares",
];

/// Edge types that represent a real dependency and therefore participate
/// in cycle detection. `declares` is intentionally excluded: in languages
/// like Rust it represents module-tree containment (`mod foo;`), not a
/// dependency, and combining it with a child's `use crate::…` import
/// manufactures false-positive cycles. `type_import` is excluded too: an
/// import under `if TYPE_CHECKING:` never runs, so it can't close a runtime
/// import loop (psf/requests' false 8-file cycle, #22).
const DEPENDENCY_EDGE_TYPES: &[&str] = &["imports", "extends", "implements", "type_ref"];

fn is_structural(edge: &GraphEdge) -> bool {
    STRUCTURAL_EDGE_TYPES.contains(&edge.edge_type.as_str())
}

fn is_dependency(edge: &GraphEdge) -> bool {
    DEPENDENCY_EDGE_TYPES.contains(&edge.edge_type.as_str())
}

/// A strongly connected component (file-level dependency cycle).
#[derive(Debug, Clone, PartialEq)]
pub struct Scc {
    /// Sorted file paths in this SCC.
    pub files: Vec<String>,
    /// All structural edges internal to the SCC, sorted by (source, target, type, detail).
    pub edges: Vec<GraphEdge>,
    /// Number of files in this SCC.
    pub size: usize,
}

/// Result of cycle detection across the full file graph.
#[derive(Debug, Clone, PartialEq)]
pub struct CycleReport {
    pub cycles: Vec<Scc>,
    pub total_cycles: usize,
}

/// Result of orphan file detection.
#[derive(Debug, Clone, PartialEq)]
pub struct OrphanReport {
    /// Files with zero inbound structural edges (excluding entry points).
    pub orphan_files: Vec<String>,
    pub total_orphans: usize,
    pub total_files: usize,
}

pub struct GraphAnalyzer;

impl GraphAnalyzer {
    /// Run Tarjan's SCC on structural edges. Returns only SCCs with size >= 2.
    pub fn find_cycles(graph: &FileGraph) -> CycleReport {
        // 1. Filter to dependency edges (declares is intentionally excluded)
        let structural_edges: Vec<&GraphEdge> =
            graph.edges.iter().filter(|e| is_dependency(e)).collect();

        // 2. Build petgraph DiGraph with node indices mapped by file path
        let mut pg = DiGraph::<&str, ()>::new();
        let mut node_indices: HashMap<&str, petgraph::graph::NodeIndex> = HashMap::new();

        for node in &graph.nodes {
            let idx = pg.add_node(node.id.as_str());
            node_indices.insert(node.id.as_str(), idx);
        }

        // Also ensure edge endpoints that aren't in nodes get added
        for edge in &structural_edges {
            if !node_indices.contains_key(edge.source.as_str()) {
                let idx = pg.add_node(edge.source.as_str());
                node_indices.insert(edge.source.as_str(), idx);
            }
            if !node_indices.contains_key(edge.target.as_str()) {
                let idx = pg.add_node(edge.target.as_str());
                node_indices.insert(edge.target.as_str(), idx);
            }
        }

        for edge in &structural_edges {
            if let (Some(&src), Some(&tgt)) = (
                node_indices.get(edge.source.as_str()),
                node_indices.get(edge.target.as_str()),
            ) {
                pg.add_edge(src, tgt, ());
            }
        }

        // 3. Run Tarjan's SCC
        let sccs = tarjan_scc(&pg);

        // 4. Filter to SCCs with size >= 2, collect edges internal to each SCC
        let mut cycles: Vec<Scc> = Vec::new();

        for scc_indices in &sccs {
            if scc_indices.len() < 2 {
                continue;
            }

            let scc_files: HashSet<&str> = scc_indices.iter().map(|&idx| pg[idx]).collect();

            let mut files: Vec<String> = scc_files.iter().map(|s| s.to_string()).collect();
            files.sort();

            let mut edges: Vec<GraphEdge> = structural_edges
                .iter()
                .filter(|e| {
                    scc_files.contains(e.source.as_str()) && scc_files.contains(e.target.as_str())
                })
                .map(|e| (*e).clone())
                .collect();
            edges.sort_by(|a, b| {
                a.source
                    .cmp(&b.source)
                    .then_with(|| a.target.cmp(&b.target))
                    .then_with(|| a.edge_type.cmp(&b.edge_type))
                    .then_with(|| a.detail.cmp(&b.detail))
            });

            let size = files.len();
            cycles.push(Scc { files, edges, size });
        }

        // 5. Sort by size descending, then by first file alphabetically for ties
        cycles.sort_by(|a, b| {
            b.size
                .cmp(&a.size)
                .then_with(|| a.files.first().cmp(&b.files.first()))
        });

        let total_cycles = cycles.len();
        CycleReport {
            cycles,
            total_cycles,
        }
    }

    /// Find files with zero inbound structural edges, excluding known entry-point files.
    pub fn find_orphans(graph: &FileGraph, entry_point_files: &HashSet<String>) -> OrphanReport {
        // Build set of files with at least one inbound structural edge
        let mut has_inbound: HashSet<&str> = HashSet::new();
        for edge in &graph.edges {
            if is_structural(edge) {
                has_inbound.insert(edge.target.as_str());
            }
        }

        // Importing any module inside a Python package runs the package's
        // `__init__.py` first, so `from app.render.runner import X` uses
        // `app/__init__.py` and `app/render/__init__.py` without naming them.
        let used_python_packages: HashSet<&str> = has_inbound
            .iter()
            .filter(|p| p.ends_with(".py"))
            .flat_map(|p| p.match_indices('/').map(move |(i, _)| &p[..i]))
            .collect();
        // Go has no file-level imports: every file in a package folder is
        // compiled together, and an import names the folder (#24). So when
        // one file of a Go package is imported or is an entry point (a
        // `package main` with `func main()`), the package's other files are
        // in use too.
        let go_dir = |id: &str| -> String {
            id.rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default()
        };
        let used_go_packages: HashSet<String> = graph
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .filter(|id| id.ends_with(".go") && !id.ends_with("_test.go"))
            .filter(|id| has_inbound.contains(id) || entry_point_files.contains(*id))
            .map(go_dir)
            .collect();
        let implicitly_used = |id: &str| -> bool {
            id.strip_suffix("/__init__.py")
                .is_some_and(|pkg| used_python_packages.contains(pkg))
                || (id.ends_with(".go") && used_go_packages.contains(&go_dir(id)))
        };

        let mut orphan_files: Vec<String> = graph
            .nodes
            .iter()
            .filter(|n| {
                !has_inbound.contains(n.id.as_str())
                    && !entry_point_files.contains(&n.id)
                    && !implicitly_used(&n.id)
            })
            .map(|n| n.id.clone())
            .collect();
        orphan_files.sort();

        let total_orphans = orphan_files.len();
        let total_files = graph.nodes.len();

        OrphanReport {
            orphan_files,
            total_orphans,
            total_files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::graph::GraphNode;

    fn edge(source: &str, target: &str, edge_type: &str, detail: Option<&str>) -> GraphEdge {
        GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            edge_type: edge_type.to_string(),
            detail: detail.map(|d| d.to_string()),
        }
    }

    fn node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            symbols: vec![],
        }
    }

    #[test]
    fn test_no_cycles_in_dag() {
        let graph = FileGraph {
            nodes: vec![node("a.rs"), node("b.rs"), node("c.rs")],
            edges: vec![
                edge("a.rs", "b.rs", "imports", None),
                edge("b.rs", "c.rs", "imports", None),
            ],
        };
        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(report.total_cycles, 0);
        assert!(report.cycles.is_empty());
    }

    #[test]
    fn test_simple_two_node_cycle() {
        let graph = FileGraph {
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![
                edge("a.rs", "b.rs", "imports", Some("b::Foo")),
                edge("b.rs", "a.rs", "type_ref", Some("a::Bar")),
            ],
        };
        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(report.total_cycles, 1);
        assert_eq!(report.cycles[0].size, 2);
        assert_eq!(report.cycles[0].files, vec!["a.rs", "b.rs"]);
        assert_eq!(report.cycles[0].edges.len(), 2);
    }

    #[test]
    fn test_multiple_sccs() {
        // Two independent cycles: (a,b) and (c,d,e)
        let graph = FileGraph {
            nodes: vec![
                node("a.rs"),
                node("b.rs"),
                node("c.rs"),
                node("d.rs"),
                node("e.rs"),
            ],
            edges: vec![
                edge("a.rs", "b.rs", "imports", None),
                edge("b.rs", "a.rs", "imports", None),
                edge("c.rs", "d.rs", "imports", None),
                edge("d.rs", "e.rs", "extends", None),
                edge("e.rs", "c.rs", "implements", None),
            ],
        };
        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(report.total_cycles, 2);
        // Sorted by size descending
        assert_eq!(report.cycles[0].size, 3);
        assert_eq!(report.cycles[1].size, 2);
    }

    /// I-GRAPH-CYCLES-EXCLUDE-DECLARES (graph declares + graph cycles families).
    #[test]
    fn i_graph_cycles_exclude_declares() {
        // Rust module tree: lib.rs declares foo (via `mod foo;`), foo.rs uses
        // `use crate::Trait` which resolves back to lib.rs. This is not a real
        // dependency cycle — `declares` is structural containment, not a dependency.
        let graph = FileGraph {
            nodes: vec![node("lib.rs"), node("foo.rs")],
            edges: vec![
                edge("lib.rs", "foo.rs", "declares", Some("foo")),
                edge("foo.rs", "lib.rs", "imports", Some("crate::Trait")),
            ],
        };
        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(
            report.total_cycles, 0,
            "declares+imports between parent and child module should not form a cycle"
        );
    }

    /// I-GRAPH-ORPHANS-RESPECT-DECLARES (graph declares + orphan detection families).
    #[test]
    fn i_graph_orphans_respect_declares() {
        // A file referenced only by a `declares` edge (e.g. a module with
        // no outward dependencies) should NOT be reported as an orphan.
        let graph = FileGraph {
            nodes: vec![node("lib.rs"), node("foo.rs")],
            edges: vec![edge("lib.rs", "foo.rs", "declares", Some("foo"))],
        };
        let entry_points: HashSet<String> = vec!["lib.rs".to_string()].into_iter().collect();
        let report = GraphAnalyzer::find_orphans(&graph, &entry_points);
        assert_eq!(
            report.total_orphans, 0,
            "foo.rs is referenced by a declares edge and should not be an orphan"
        );
    }

    /// I-GRAPH-CYCLES-EXCLUDE-CALLS (graph cycles family).
    #[test]
    fn i_graph_cycles_exclude_calls() {
        // Cycle exists only via "calls" edges — should not be detected
        let graph = FileGraph {
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![
                edge("a.rs", "b.rs", "calls", None),
                edge("b.rs", "a.rs", "calls", None),
            ],
        };
        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(report.total_cycles, 0);
    }

    /// I-GRAPH-CYCLES-EXCLUDE-SELF-LOOP (graph cycles family).
    #[test]
    fn i_graph_cycles_exclude_self_loop() {
        let graph = FileGraph {
            nodes: vec![node("a.rs")],
            edges: vec![edge("a.rs", "a.rs", "imports", None)],
        };
        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(report.total_cycles, 0);
    }

    /// I-GRAPH-ORPHANS-BASELINE (orphan detection family).
    #[test]
    fn i_graph_orphans_baseline() {
        let graph = FileGraph {
            nodes: vec![node("a.rs"), node("b.rs"), node("c.rs")],
            edges: vec![edge("a.rs", "b.rs", "imports", None)],
        };
        let report = GraphAnalyzer::find_orphans(&graph, &HashSet::new());
        // a.rs and c.rs have zero inbound structural edges
        assert_eq!(report.total_orphans, 2);
        assert_eq!(report.orphan_files, vec!["a.rs", "c.rs"]);
        assert_eq!(report.total_files, 3);
    }

    /// #24: one imported or entry-point file marks its whole Go package
    /// folder as used; subfolders and other packages are separate.
    #[test]
    fn go_package_files_share_use() {
        let graph = FileGraph {
            nodes: vec![
                node("main.go"),
                node("flags.go"),
                node("pkg/a.go"),
                node("pkg/b.go"),
                node("pkg/sub/c.go"),
                node("other/d.go"),
                node("tests/e_test.go"),
                node("tests/f.go"),
            ],
            edges: vec![
                edge("main.go", "pkg/a.go", "imports", None),
                edge("tests/e_test.go", "tests/f.go", "calls", None),
            ],
        };
        let entry_points: HashSet<String> = ["main.go".to_string()].into();
        let report = GraphAnalyzer::find_orphans(&graph, &entry_points);
        assert_eq!(
            report.orphan_files,
            vec![
                "other/d.go",
                "pkg/sub/c.go",
                "tests/e_test.go",
                "tests/f.go"
            ]
        );
    }

    /// I-GRAPH-ORPHANS-EXCLUDE-ENTRY-POINTS (orphan detection family).
    #[test]
    fn i_graph_orphans_exclude_entry_points() {
        let graph = FileGraph {
            nodes: vec![node("main.rs"), node("lib.rs"), node("unused.rs")],
            edges: vec![edge("main.rs", "lib.rs", "imports", None)],
        };
        let entry_points: HashSet<String> = vec!["main.rs".to_string()].into_iter().collect();
        let report = GraphAnalyzer::find_orphans(&graph, &entry_points);
        // main.rs excluded (entry point), lib.rs has inbound edge, unused.rs is orphan
        assert_eq!(report.total_orphans, 1);
        assert_eq!(report.orphan_files, vec!["unused.rs"]);
    }

    /// I-GRAPH-ORPHANS-IGNORE-CALLS (orphan detection family).
    #[test]
    fn i_graph_orphans_ignore_calls() {
        let graph = FileGraph {
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![edge("a.rs", "b.rs", "calls", None)],
        };
        let report = GraphAnalyzer::find_orphans(&graph, &HashSet::new());
        // "calls" is not structural, so b.rs still has zero structural inbound
        assert_eq!(report.total_orphans, 2);
    }

    /// I-GRAPH-CYCLE-REPORT-SHAPE and I-GRAPH-ORPHAN-REPORT-SHAPE
    /// (graph cycles + orphan detection families). Asserts cycle + orphan
    /// structure of the synthetic `fixtures/test-data/cyclic-graph/graph.json`.
    #[test]
    fn i_graph_cycle_and_orphan_report_shape_from_fixture() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/test-data/cyclic-graph/graph.json");
        let json = std::fs::read_to_string(&fixture_path).unwrap_or_else(|e| {
            panic!("Failed to read fixture: {}: {}", fixture_path.display(), e)
        });
        let graph: FileGraph = serde_json::from_str(&json).expect("Failed to parse fixture");

        let report = GraphAnalyzer::find_cycles(&graph);
        assert_eq!(
            report.total_cycles, 2,
            "expected 2 cycles, got {:?}",
            report.cycles
        );

        // Largest cycle first (3 files)
        assert_eq!(report.cycles[0].size, 3);
        assert_eq!(
            report.cycles[0].files,
            vec!["src/a.rs", "src/b.rs", "src/c.rs"]
        );
        assert_eq!(report.cycles[0].edges.len(), 3);

        // Second cycle (2 files)
        assert_eq!(report.cycles[1].size, 2);
        assert_eq!(report.cycles[1].files, vec!["src/d.rs", "src/e.rs"]);
        assert_eq!(report.cycles[1].edges.len(), 2);

        // Orphan detection
        let orphan_report = GraphAnalyzer::find_orphans(&graph, &HashSet::new());
        assert!(
            orphan_report
                .orphan_files
                .contains(&"src/orphan.rs".to_string()),
            "expected orphan.rs in orphan list"
        );
    }
}
