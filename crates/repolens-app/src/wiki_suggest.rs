//! Suggestion ranking for wiki articles based on file centrality, symbol count,
//! size, and dependency breadth.

use std::collections::HashMap;
use std::path::Path;

use repolens_core::model::graph::FileGraph;
use repolens_core::model::wiki::{ArticleType, WikiIndex};
use repolens_core::pack_reader::PackIndex;
use repolens_wiki::coverage::{compute_coverage, CoverageResult};

/// A ranked suggestion for a wiki article to write.
pub struct Suggestion {
    pub file_path: String,
    pub importance: f64,
    pub reason: String,
    pub suggested_type: ArticleType,
    pub suggested_title: String,
    pub symbols: Vec<String>,
}

/// The result of running suggestion ranking.
pub struct SuggestResult {
    pub suggestions: Vec<Suggestion>,
    pub coverage: CoverageResult,
}

/// File names that indicate an overview/entry-point module.
const OVERVIEW_FILES: &[&str] = &[
    "main.rs",
    "lib.rs",
    "mod.rs",
    "index.ts",
    "index.js",
    "index.tsx",
    "__init__.py",
];

/// Compute ranked wiki-article suggestions for uncovered files.
///
/// 1. Computes coverage gaps via `compute_coverage`.
/// 2. Optionally filters to a directory prefix.
/// 3. Ranks each gap file by a weighted importance score:
///    - centrality (inbound edges): 40%
///    - symbol count: 20%
///    - file size (LOC): 20%
///    - dependency breadth (outbound edges): 20%
/// 4. Filters by `min_importance`, sorts descending, truncates to `limit`.
pub fn suggest(
    wiki_index: &WikiIndex,
    pack: &PackIndex,
    graph: &FileGraph,
    directory: Option<&str>,
    min_importance: Option<f64>,
    limit: usize,
) -> SuggestResult {
    let coverage = compute_coverage(wiki_index, pack);

    // Filter gaps to directory prefix if provided.
    let gaps: Vec<&str> = coverage
        .gaps
        .iter()
        .filter(|g| match directory {
            Some(dir) => g.starts_with(dir),
            None => true,
        })
        .map(String::as_str)
        .collect();

    // Build lookup from path -> FileEntry for included entries.
    let entry_lookup: HashMap<&str, _> = pack
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| (e.path.as_str(), e))
        .collect();

    // Count inbound and outbound edges per file.
    let mut inbound: HashMap<&str, usize> = HashMap::new();
    let mut outbound: HashMap<&str, usize> = HashMap::new();
    for edge in &graph.edges {
        *inbound.entry(edge.target.as_str()).or_default() += 1;
        *outbound.entry(edge.source.as_str()).or_default() += 1;
    }

    // Compute max values across all included entries for normalization.
    let mut max_inbound: usize = 0;
    let mut max_outbound: usize = 0;
    let mut max_sym_count: usize = 0;
    let mut max_loc: i32 = 0;
    for entry in pack.entries.iter().filter(|e| e.included) {
        let path = entry.path.as_str();
        max_inbound = max_inbound.max(*inbound.get(path).unwrap_or(&0));
        max_outbound = max_outbound.max(*outbound.get(path).unwrap_or(&0));
        max_sym_count = max_sym_count.max(entry.symbols.len());
        max_loc = max_loc.max(entry.loc);
    }

    // Build suggestions for each gap file.
    let mut suggestions: Vec<Suggestion> = gaps
        .iter()
        .filter_map(|&gap_path| {
            let entry = entry_lookup.get(gap_path)?;

            let in_count = *inbound.get(gap_path).unwrap_or(&0);
            let out_count = *outbound.get(gap_path).unwrap_or(&0);
            let sym_count = entry.symbols.len();
            let loc = entry.loc;

            // Normalize each metric to 0-1, guarding against div-by-zero.
            let norm_centrality = if max_inbound > 0 {
                in_count as f64 / max_inbound as f64
            } else {
                0.0
            };
            let norm_sym = if max_sym_count > 0 {
                sym_count as f64 / max_sym_count as f64
            } else {
                0.0
            };
            let norm_loc = if max_loc > 0 {
                loc as f64 / max_loc as f64
            } else {
                0.0
            };
            let norm_dep = if max_outbound > 0 {
                out_count as f64 / max_outbound as f64
            } else {
                0.0
            };

            let importance =
                norm_centrality * 0.4 + norm_sym * 0.2 + norm_loc * 0.2 + norm_dep * 0.2;

            // Filter by min_importance early.
            if let Some(min) = min_importance {
                if importance < min {
                    return None;
                }
            }

            let reason = format!(
                "{} importers, {} dependencies, {} symbols, {} LOC",
                in_count, out_count, sym_count, loc
            );

            let file_name = Path::new(gap_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();

            let suggested_type = if OVERVIEW_FILES.contains(&file_name.as_ref()) {
                ArticleType::Overview
            } else {
                ArticleType::Entity
            };

            let suggested_title = title_case(
                &Path::new(gap_path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy(),
            );

            let symbols = entry.symbols.iter().map(|s| s.name.clone()).collect();

            Some(Suggestion {
                file_path: gap_path.to_string(),
                importance,
                reason,
                suggested_type,
                suggested_title,
                symbols,
            })
        })
        .collect();

    // Sort by importance descending.
    suggestions.sort_by(|a, b| {
        b.importance
            .partial_cmp(&a.importance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    suggestions.truncate(limit);

    SuggestResult {
        suggestions,
        coverage,
    }
}

/// Title-case a file stem: replace `_` and `-` with spaces and capitalize each word.
fn title_case(stem: &str) -> String {
    stem.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{}{}", upper, chars.as_str())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::{FileEntry, Symbol};
    use repolens_core::model::graph::{FileGraph, GraphEdge, GraphNode};
    use repolens_core::model::wiki::{ArticleType, Staleness, WikiArticle, WikiIndex};
    use time::OffsetDateTime;

    fn make_entry(path: &str, loc: i32, symbols: Vec<&str>) -> FileEntry {
        let syms = symbols
            .into_iter()
            .map(|name| Symbol {
                symbol_type: "class".to_string(),
                name: name.to_string(),
                qualified_name: None,
                visibility: None,
                line: None,
            })
            .collect();
        FileEntry {
            path: path.to_string(),
            sha256: String::new(),
            language: String::new(),
            loc,
            included: true,
            reason: String::new(),
            content_path: None,
            symbols: syms,
        }
    }

    fn make_wiki(articles: Vec<WikiArticle>) -> WikiIndex {
        WikiIndex {
            schema_version: "1.0".to_string(),
            updated_at: OffsetDateTime::now_utc(),
            articles,
        }
    }

    fn make_article(anchors: Vec<&str>) -> WikiArticle {
        let now = OffsetDateTime::now_utc();
        WikiArticle {
            id: "test-article".to_string(),
            title: "Test Article".to_string(),
            article_type: ArticleType::Entity,
            path: "entity/test-article.md".to_string(),
            anchors: anchors.into_iter().map(String::from).collect(),
            memory_refs: vec![],
            tags: vec![],
            confidence: 0.9,
            staleness: Staleness::Fresh,
            summary: "test".to_string(),
            created_at: now,
            updated_at: now,
            source: None,
        }
    }

    fn make_graph(nodes: Vec<&str>, edges: Vec<(&str, &str)>) -> FileGraph {
        FileGraph {
            nodes: nodes
                .into_iter()
                .map(|id| GraphNode {
                    id: id.to_string(),
                    symbols: vec![],
                })
                .collect(),
            edges: edges
                .into_iter()
                .map(|(src, tgt)| GraphEdge {
                    source: src.to_string(),
                    target: tgt.to_string(),
                    edge_type: "imports".to_string(),
                    detail: None,
                })
                .collect(),
        }
    }

    #[test]
    fn high_centrality_ranks_first() {
        // file_a has 3 importers, file_b has 1.
        let pack = PackIndex::from_entries(vec![
            make_entry("src/file_a.rs", 10, vec!["A"]),
            make_entry("src/file_b.rs", 10, vec!["B"]),
            make_entry("src/c.rs", 10, vec![]),
            make_entry("src/d.rs", 10, vec![]),
            make_entry("src/e.rs", 10, vec![]),
        ]);
        let graph = make_graph(
            vec![
                "src/file_a.rs",
                "src/file_b.rs",
                "src/c.rs",
                "src/d.rs",
                "src/e.rs",
            ],
            vec![
                ("src/c.rs", "src/file_a.rs"),
                ("src/d.rs", "src/file_a.rs"),
                ("src/e.rs", "src/file_a.rs"),
                ("src/c.rs", "src/file_b.rs"),
            ],
        );
        let wiki = make_wiki(vec![]);

        let result = suggest(&wiki, &pack, &graph, None, None, 10);
        assert!(result.suggestions.len() >= 2);
        assert_eq!(result.suggestions[0].file_path, "src/file_a.rs");
    }

    #[test]
    fn covered_files_excluded() {
        let pack = PackIndex::from_entries(vec![
            make_entry("src/covered.rs", 10, vec!["Cov"]),
            make_entry("src/uncovered.rs", 10, vec!["Unc"]),
        ]);
        let graph = make_graph(vec!["src/covered.rs", "src/uncovered.rs"], vec![]);
        let wiki = make_wiki(vec![make_article(vec!["src/covered.rs"])]);

        let result = suggest(&wiki, &pack, &graph, None, None, 10);
        let paths: Vec<&str> = result
            .suggestions
            .iter()
            .map(|s| s.file_path.as_str())
            .collect();
        assert!(!paths.contains(&"src/covered.rs"));
        assert!(paths.contains(&"src/uncovered.rs"));
    }

    #[test]
    fn directory_filter() {
        let pack = PackIndex::from_entries(vec![
            make_entry("src/auth/login.rs", 10, vec![]),
            make_entry("src/db/pool.rs", 10, vec![]),
        ]);
        let graph = make_graph(vec!["src/auth/login.rs", "src/db/pool.rs"], vec![]);
        let wiki = make_wiki(vec![]);

        let result = suggest(&wiki, &pack, &graph, Some("src/auth/"), None, 10);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].file_path, "src/auth/login.rs");
    }

    #[test]
    fn min_importance_filter() {
        let pack = PackIndex::from_entries(vec![
            make_entry("src/big.rs", 100, vec!["A", "B", "C"]),
            make_entry("src/tiny.rs", 1, vec![]),
        ]);
        let graph = make_graph(
            vec!["src/big.rs", "src/tiny.rs"],
            vec![("src/tiny.rs", "src/big.rs")],
        );
        let wiki = make_wiki(vec![]);

        let result = suggest(&wiki, &pack, &graph, None, Some(0.5), 10);
        for s in &result.suggestions {
            assert!(
                s.importance >= 0.5,
                "suggestion {} had importance {} < 0.5",
                s.file_path,
                s.importance
            );
        }
    }

    #[test]
    fn suggest_type_heuristics() {
        let pack = PackIndex::from_entries(vec![
            make_entry("src/lib.rs", 10, vec![]),
            make_entry("src/main.rs", 10, vec![]),
            make_entry("src/auth/mod.rs", 10, vec![]),
            make_entry("src/components/index.ts", 10, vec![]),
            make_entry("src/service.rs", 10, vec![]),
        ]);
        let graph = make_graph(
            vec![
                "src/lib.rs",
                "src/main.rs",
                "src/auth/mod.rs",
                "src/components/index.ts",
                "src/service.rs",
            ],
            vec![],
        );
        let wiki = make_wiki(vec![]);

        let result = suggest(&wiki, &pack, &graph, None, None, 10);
        let by_path: HashMap<&str, &Suggestion> = result
            .suggestions
            .iter()
            .map(|s| (s.file_path.as_str(), s))
            .collect();

        assert_eq!(by_path["src/lib.rs"].suggested_type, ArticleType::Overview);
        assert_eq!(by_path["src/main.rs"].suggested_type, ArticleType::Overview);
        assert_eq!(
            by_path["src/auth/mod.rs"].suggested_type,
            ArticleType::Overview
        );
        assert_eq!(
            by_path["src/components/index.ts"].suggested_type,
            ArticleType::Overview
        );
        assert_eq!(
            by_path["src/service.rs"].suggested_type,
            ArticleType::Entity
        );
    }

    #[test]
    fn suggest_title_formatting() {
        assert_eq!(title_case("auth_service"), "Auth Service");
        assert_eq!(title_case("user-manager"), "User Manager");
        assert_eq!(title_case("lib"), "Lib");
    }
}
