use std::collections::HashSet;

use repolens_core::model::graph::FileGraph;
use repolens_core::model::health::GraphHealthInput;
use repolens_core::model::profile::ProjectProfile;
use repolens_core::pack_reader::PackIndex;

use crate::graph_analysis::GraphAnalyzer;
use crate::summary::compute_test_to_source_ratio_in_pack;

/// Build a fully-populated `GraphHealthInput` from graph analysis.
///
/// Single-sources the cycle, orphan, and test-ratio computation so that
/// `SummaryService` and MCP `analyze_health` use identical logic.
pub fn build_graph_health_input(
    index: &PackIndex,
    profile: &ProjectProfile,
    graph: &FileGraph,
) -> GraphHealthInput {
    build_graph_health_input_in_pack(index, profile, graph, None)
}

/// Pack-aware variant: with `pack_dir`, inline Rust tests count toward the
/// test-to-source ratio.
pub fn build_graph_health_input_in_pack(
    index: &PackIndex,
    profile: &ProjectProfile,
    graph: &FileGraph,
    pack_dir: Option<&std::path::Path>,
) -> GraphHealthInput {
    let cycle_report = GraphAnalyzer::find_cycles(graph);

    let files_in_cycles_count = {
        let mut files: HashSet<&str> = HashSet::new();
        for scc in &cycle_report.cycles {
            for file in &scc.files {
                files.insert(file.as_str());
            }
        }
        files.len()
    };

    let mut entry_point_files: HashSet<String> = profile
        .entry_points
        .iter()
        .map(|ep| ep.path.clone())
        .collect();
    // #12: files loaded by HTML <script> tags or run directly by CI are used.
    if let Some(dir) = pack_dir {
        entry_point_files.extend(crate::references::referenced_files(index, dir));
    }
    let orphan_report = GraphAnalyzer::find_orphans(graph, &entry_point_files);

    GraphHealthInput {
        cycle_count: cycle_report.total_cycles,
        largest_cycle_size: cycle_report.cycles.first().map(|c| c.size).unwrap_or(0),
        files_in_cycles_count,
        orphan_count: orphan_report.total_orphans,
        orphan_files: orphan_report.orphan_files.into_iter().take(10).collect(),
        test_to_source_ratio: compute_test_to_source_ratio_in_pack(index, pack_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::graph::{GraphEdge, GraphNode};
    use repolens_core::model::manifest::Manifest;
    use repolens_core::model::profile::ProjectType;
    use std::collections::BTreeMap;

    fn node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            symbols: vec![],
        }
    }

    fn edge(source: &str, target: &str, edge_type: &str) -> GraphEdge {
        GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            edge_type: edge_type.to_string(),
            detail: None,
        }
    }

    fn make_manifest() -> Manifest {
        let json = r#"{
            "schemaVersion": "1.0",
            "schemaUrl": "",
            "timestamp": 0,
            "chunks": [],
            "scanMode": "full"
        }"#;
        serde_json::from_str(json).expect("test manifest parse")
    }

    fn make_index() -> PackIndex {
        PackIndex {
            manifest: make_manifest(),
            entries: vec![],
        }
    }

    fn make_profile() -> ProjectProfile {
        ProjectProfile {
            name: "test".to_string(),
            build_system: None,
            frameworks: vec![],
            project_type: ProjectType::Unknown,
            entry_points: vec![],
            description: None,
            language_distribution: BTreeMap::new(),
            modules: vec![],
        }
    }

    #[test]
    fn test_build_graph_health_with_cycles() {
        // Two cycles: (a,b) size 2, (c,d,e) size 3 → 5 distinct files in cycles
        let graph = FileGraph {
            nodes: vec![
                node("a.rs"),
                node("b.rs"),
                node("c.rs"),
                node("d.rs"),
                node("e.rs"),
                node("f.rs"),
            ],
            edges: vec![
                edge("a.rs", "b.rs", "imports"),
                edge("b.rs", "a.rs", "imports"),
                edge("c.rs", "d.rs", "imports"),
                edge("d.rs", "e.rs", "extends"),
                edge("e.rs", "c.rs", "implements"),
                edge("f.rs", "a.rs", "imports"), // f is not in a cycle
            ],
        };
        let result = build_graph_health_input(&make_index(), &make_profile(), &graph);
        assert_eq!(result.cycle_count, 2);
        assert_eq!(result.largest_cycle_size, 3);
        assert_eq!(result.files_in_cycles_count, 5);
    }

    #[test]
    fn test_build_graph_health_no_cycles() {
        let graph = FileGraph {
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![edge("a.rs", "b.rs", "imports")],
        };
        let result = build_graph_health_input(&make_index(), &make_profile(), &graph);
        assert_eq!(result.cycle_count, 0);
        assert_eq!(result.largest_cycle_size, 0);
        assert_eq!(result.files_in_cycles_count, 0);
    }
}
