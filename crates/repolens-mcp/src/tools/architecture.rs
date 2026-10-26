use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use schemars::JsonSchema;
use serde::Deserialize;
use tracing::info;

use repolens_app::discover::PackResolver;
use repolens_app::summary::read_graph;
use repolens_core::model::graph::FileGraph;
use rmcp::{model::CallToolResult, ErrorData as McpError};

use crate::epistemic::*;
use crate::err_json;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetArchitectureParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
    #[schemars(
        description = "Directory scope (e.g., 'src/main/java'). Omit for top-level overview."
    )]
    pub scope: Option<String>,
}

pub fn handle_get_architecture(
    resolver: &Arc<PackResolver>,
    params: GetArchitectureParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(
        pack = ?params.pack,
        root = ?params.root,
        scope = ?params.scope,
        "get_architecture: entry"
    );

    let (pack_dir, index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_architecture: resolve error");
            return err_json(&e.to_string());
        }
    };

    let graph: FileGraph = read_graph(&pack_dir).unwrap_or_else(|| {
        info!("get_architecture: no graph.json found, using empty graph");
        FileGraph {
            nodes: vec![],
            edges: vec![],
        }
    });

    let included_entries: Vec<_> = index.entries.iter().filter(|e| e.included).collect();

    let response = if let Some(ref scope) = params.scope {
        let scope_prefix = if scope.ends_with('/') {
            scope.clone()
        } else {
            format!("{}/", scope)
        };

        info!(scope_prefix = %scope_prefix, "get_architecture: computing scoped subgraph");

        let scoped_files: HashSet<String> = included_entries
            .iter()
            .filter(|e| e.path.starts_with(&scope_prefix) || e.path == *scope)
            .map(|e| e.path.clone())
            .collect();

        let nodes: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| scoped_files.contains(&n.id))
            .collect();

        let mut internal_edges = Vec::new();
        let mut external_edges = Vec::new();
        let mut connectivity: HashMap<String, (usize, usize)> = HashMap::new();
        for file in &scoped_files {
            connectivity.insert(file.clone(), (0, 0));
        }

        for edge in &graph.edges {
            let src_in = scoped_files.contains(&edge.source);
            let tgt_in = scoped_files.contains(&edge.target);

            if src_in {
                connectivity
                    .entry(edge.source.clone())
                    .and_modify(|(_, out)| *out += 1);
            }
            if tgt_in {
                connectivity
                    .entry(edge.target.clone())
                    .and_modify(|(in_, _)| *in_ += 1);
            }

            if src_in && tgt_in {
                internal_edges.push(edge);
            } else if src_in || tgt_in {
                external_edges.push(edge);
            }
        }

        let connectivity_json: serde_json::Value = connectivity
            .iter()
            .map(|(path, (in_deg, out_deg))| {
                (
                    path.clone(),
                    serde_json::json!({ "inDegree": in_deg, "outDegree": out_deg }),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into();

        // Total LOC and language counts for scoped entries
        let scoped_entries: Vec<_> = included_entries
            .iter()
            .filter(|e| scoped_files.contains(&e.path))
            .collect();

        let total_loc: i32 = scoped_entries.iter().map(|e| e.loc).sum();
        let mut lang_counts: HashMap<&str, usize> = HashMap::new();
        for entry in &scoped_entries {
            *lang_counts.entry(entry.language.as_str()).or_insert(0) += 1;
        }

        // Top symbols: rank by file connections (in+out degree), limit 20
        let mut symbol_entries: Vec<(String, usize, Vec<String>)> = scoped_entries
            .iter()
            .map(|e| {
                let connections = connectivity.get(&e.path).map(|(i, o)| i + o).unwrap_or(0);
                let symbols: Vec<String> = e.symbols.iter().map(|s| s.name.clone()).collect();
                (e.path.clone(), connections, symbols)
            })
            .collect();
        symbol_entries.sort_by_key(|(_, connections, _)| std::cmp::Reverse(*connections));

        let top_symbols: Vec<serde_json::Value> = symbol_entries
            .iter()
            .flat_map(|(path, _, syms)| {
                syms.iter()
                    .map(move |sym| serde_json::json!({ "file": path, "symbol": sym }))
            })
            .take(20)
            .collect();

        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            file_count = scoped_files.len(),
            internal_edges = internal_edges.len(),
            external_edges = external_edges.len(),
            "get_architecture: scoped exit"
        );

        serde_json::json!({
            "scope": scope,
            "fileCount": scoped_files.len(),
            "internalEdgeCount": internal_edges.len(),
            "externalEdgeCount": external_edges.len(),
            "totalLoc": total_loc,
            "languages": lang_counts,
            "files": nodes,
            "internalEdges": internal_edges,
            "externalEdges": external_edges,
            "connectivity": connectivity_json,
            "topSymbols": top_symbols,
        })
    } else {
        info!("get_architecture: computing directory overview");

        struct DirStats {
            file_count: usize,
            loc: i32,
            lang_loc: HashMap<String, i32>,
        }

        let mut dir_stats: HashMap<String, DirStats> = HashMap::new();

        for entry in &included_entries {
            let dir = if let Some(slash_pos) = entry.path.find('/') {
                entry.path[..slash_pos].to_string()
            } else {
                "(root)".to_string()
            };

            let stats = dir_stats.entry(dir).or_insert(DirStats {
                file_count: 0,
                loc: 0,
                lang_loc: HashMap::new(),
            });
            stats.file_count += 1;
            stats.loc += entry.loc;
            *stats.lang_loc.entry(entry.language.clone()).or_insert(0) += entry.loc;
        }

        let mut dir_internal_edges: HashMap<String, usize> = HashMap::new();
        for edge in &graph.edges {
            let src_dir = if let Some(p) = edge.source.find('/') {
                edge.source[..p].to_string()
            } else {
                "(root)".to_string()
            };
            let tgt_dir = if let Some(p) = edge.target.find('/') {
                edge.target[..p].to_string()
            } else {
                "(root)".to_string()
            };
            if src_dir == tgt_dir {
                *dir_internal_edges.entry(src_dir).or_insert(0) += 1;
            }
        }

        let mut directories: Vec<serde_json::Value> = dir_stats
            .iter()
            .map(|(dir, stats)| {
                let primary_language = stats
                    .lang_loc
                    .iter()
                    .max_by_key(|(_, &v)| v)
                    .map(|(lang, _)| lang.as_str())
                    .unwrap_or("Unknown");
                let internal_edge_count = dir_internal_edges.get(dir).copied().unwrap_or(0);
                serde_json::json!({
                    "directory": dir,
                    "fileCount": stats.file_count,
                    "loc": stats.loc,
                    "primaryLanguage": primary_language,
                    "internalEdgeCount": internal_edge_count,
                })
            })
            .collect();

        directories.sort_by(|a, b| {
            b["fileCount"]
                .as_i64()
                .unwrap_or(0)
                .cmp(&a["fileCount"].as_i64().unwrap_or(0))
        });

        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            total_files = included_entries.len(),
            total_edges = graph.edges.len(),
            dir_count = directories.len(),
            "get_architecture: overview exit"
        );

        serde_json::json!({
            "overview": true,
            "totalFiles": included_entries.len(),
            "totalEdges": graph.edges.len(),
            "directories": directories,
        })
    };

    let pid = pack_id_from_dir(&pack_dir);
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        ProvenanceField::Single(Provenance::PackExtraction {
            pack_id: pid,
            source: "multiple".into(),
        }),
        response,
    )
}
