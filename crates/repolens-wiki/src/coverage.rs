//! Coverage analysis: computes which included pack files are referenced by wiki articles.

use std::collections::{HashMap, HashSet};

use repolens_core::model::wiki::WikiIndex;
use repolens_core::pack_reader::PackIndex;

/// Result of a coverage analysis run.
pub struct CoverageResult {
    pub uncovered_files: usize,
    pub total_files: usize,
    pub coverage_percent: f64,
    /// File paths of included pack entries that are not covered by any wiki anchor.
    pub gaps: Vec<String>,
}

/// Compute wiki coverage over the included entries in `pack`.
///
/// An included file is considered *covered* if at least one anchor across all
/// wiki articles resolves to it — either by matching the file path directly or
/// by matching the `qualified_name` of one of its symbols.
pub fn compute_coverage(wiki_index: &WikiIndex, pack: &PackIndex) -> CoverageResult {
    let included_paths: HashSet<String> = pack
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.path.clone())
        .collect();

    let total_files = included_paths.len();

    if total_files == 0 {
        return CoverageResult {
            uncovered_files: 0,
            total_files: 0,
            coverage_percent: 100.0,
            gaps: vec![],
        };
    }

    let mut symbol_to_file: HashMap<String, String> = HashMap::new();
    for entry in pack.entries.iter().filter(|e| e.included) {
        for sym in &entry.symbols {
            if let Some(qn) = &sym.qualified_name {
                symbol_to_file.insert(qn.clone(), entry.path.clone());
            }
        }
    }

    let mut covered: HashSet<String> = HashSet::new();
    for article in &wiki_index.articles {
        for anchor in &article.anchors {
            if included_paths.contains(anchor.as_str()) {
                covered.insert(anchor.clone());
            } else if let Some(file_path) = symbol_to_file.get(anchor.as_str()) {
                covered.insert(file_path.clone());
            }
        }
    }

    let mut gaps: Vec<String> = included_paths.difference(&covered).cloned().collect();
    gaps.sort();

    let uncovered_files = gaps.len();
    let coverage_percent = (total_files - uncovered_files) as f64 / total_files as f64 * 100.0;

    CoverageResult {
        uncovered_files,
        total_files,
        coverage_percent,
        gaps,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::{FileEntry, Symbol};
    use repolens_core::model::manifest::Manifest;
    use repolens_core::model::wiki::{ArticleType, Staleness, WikiArticle, WikiIndex};
    use repolens_core::pack_reader::PackIndex;
    use repolens_core::serde_util::java_epoch_seconds::JavaEpochSeconds;
    use rust_decimal::Decimal;
    use time::OffsetDateTime;

    fn make_article(anchors: Vec<&str>) -> WikiArticle {
        let now = OffsetDateTime::now_utc();
        WikiArticle {
            id: "test-article".to_string(),
            title: "Test Article".to_string(),
            article_type: ArticleType::Concept,
            path: "concept/test-article.md".to_string(),
            anchors: anchors.into_iter().map(String::from).collect(),
            memory_refs: vec![],
            tags: vec![],
            confidence: 0.9,
            staleness: Staleness::Fresh,
            summary: "A test article".to_string(),
            created_at: now,
            updated_at: now,
            source: None,
        }
    }

    fn make_wiki(articles: Vec<WikiArticle>) -> WikiIndex {
        WikiIndex {
            schema_version: "1.0".to_string(),
            updated_at: OffsetDateTime::now_utc(),
            articles,
        }
    }

    fn make_entry(path: &str, included: bool, symbols: Vec<(&str, &str)>) -> FileEntry {
        let syms = symbols
            .into_iter()
            .map(|(name, qn)| Symbol {
                symbol_type: "class".to_string(),
                name: name.to_string(),
                qualified_name: Some(qn.to_string()),
                visibility: None,
                line: None,
            })
            .collect();
        FileEntry {
            path: path.to_string(),
            sha256: String::new(),
            language: String::new(),
            loc: 0,
            included,
            reason: String::new(),
            content_path: None,
            symbols: syms,
        }
    }

    fn make_pack(entries: Vec<FileEntry>) -> PackIndex {
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
    fn full_coverage() {
        let article = make_article(vec!["src/auth.rs"]);
        let wiki = make_wiki(vec![article]);
        let pack = make_pack(vec![make_entry("src/auth.rs", true, vec![])]);

        let result = compute_coverage(&wiki, &pack);
        assert_eq!(result.total_files, 1);
        assert_eq!(result.uncovered_files, 0);
        assert!((result.coverage_percent - 100.0).abs() < f64::EPSILON);
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn no_coverage() {
        let wiki = make_wiki(vec![]);
        let pack = make_pack(vec![
            make_entry("src/a.rs", true, vec![]),
            make_entry("src/b.rs", true, vec![]),
        ]);

        let result = compute_coverage(&wiki, &pack);
        assert_eq!(result.total_files, 2);
        assert_eq!(result.uncovered_files, 2);
        assert!((result.coverage_percent - 0.0).abs() < f64::EPSILON);
        assert_eq!(result.gaps.len(), 2);
    }

    #[test]
    fn symbol_anchor_covers_owning_file() {
        let article = make_article(vec!["com.example.AuthService"]);
        let wiki = make_wiki(vec![article]);
        let pack = make_pack(vec![make_entry(
            "src/AuthService.java",
            true,
            vec![("AuthService", "com.example.AuthService")],
        )]);

        let result = compute_coverage(&wiki, &pack);
        assert_eq!(result.total_files, 1);
        assert_eq!(result.uncovered_files, 0);
        assert!((result.coverage_percent - 100.0).abs() < f64::EPSILON);
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn excluded_files_ignored() {
        let article = make_article(vec!["src/included.rs"]);
        let wiki = make_wiki(vec![article]);
        let pack = make_pack(vec![
            make_entry("src/included.rs", true, vec![]),
            make_entry("src/excluded.rs", false, vec![]),
        ]);

        let result = compute_coverage(&wiki, &pack);
        // Only the included file counts in totals.
        assert_eq!(result.total_files, 1);
        assert_eq!(result.uncovered_files, 0);
        assert!((result.coverage_percent - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_pack() {
        let wiki = make_wiki(vec![]);
        let pack = make_pack(vec![]);

        let result = compute_coverage(&wiki, &pack);
        assert_eq!(result.total_files, 0);
        assert_eq!(result.uncovered_files, 0);
        assert!((result.coverage_percent - 100.0).abs() < f64::EPSILON);
        assert!(result.gaps.is_empty());
    }
}
