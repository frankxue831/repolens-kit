//! Staleness assessment for wiki articles against the active pack snapshot.

use repolens_core::model::wiki::{Staleness, WikiArticle, WikiIndex};
use repolens_core::pack_reader::PackIndex;

/// Returns `true` if the anchor looks like a file path or qualified symbol
/// (contains `/` or `.`). Bare names like `AuthService` are not groundable.
fn is_groundable(anchor: &str) -> bool {
    anchor.contains('/') || anchor.contains('.')
}

/// Assess the staleness of a single wiki article against an optional pack index.
///
/// Rules:
/// - If no groundable anchors → `Fresh`
/// - If pack is None → `Unverifiable`
/// - If all groundable anchors are found in the pack → `Fresh`
/// - If any groundable anchor is missing → `Stale`
pub fn assess_staleness(article: &WikiArticle, pack: Option<&PackIndex>) -> Staleness {
    let groundable: Vec<&String> = article
        .anchors
        .iter()
        .filter(|a| is_groundable(a))
        .collect();

    if groundable.is_empty() {
        return Staleness::Fresh;
    }

    let pack = match pack {
        Some(p) => p,
        None => return Staleness::Unverifiable,
    };

    for anchor in &groundable {
        let anchor_str: &str = anchor;
        let matched = pack.entries.iter().any(|entry| {
            if entry.path == anchor_str {
                return true;
            }
            entry
                .symbols
                .iter()
                .any(|s| s.qualified_name.as_deref() == Some(anchor_str))
        });

        if !matched {
            return Staleness::Stale;
        }
    }

    Staleness::Fresh
}

/// Assess staleness for all articles in the index, updating the staleness field
/// in place.
///
/// Returns the IDs of articles that became newly `Stale` (were not `Stale`
/// before, and are `Stale` after assessment).
pub fn flag_stale_articles(index: &mut WikiIndex, pack: Option<&PackIndex>) -> Vec<String> {
    let mut newly_stale = Vec::new();

    for article in &mut index.articles {
        let was_stale = article.staleness == Staleness::Stale;
        let new_staleness = assess_staleness(article, pack);
        let is_now_stale = new_staleness == Staleness::Stale;

        article.staleness = new_staleness;

        if !was_stale && is_now_stale {
            newly_stale.push(article.id.clone());
        }
    }

    newly_stale
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::file_entry::{FileEntry, Symbol};
    use repolens_core::model::manifest::Manifest;
    use repolens_core::model::wiki::{ArticleType, WikiArticle};
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
    fn all_anchors_found_is_fresh() {
        let article = make_article(vec!["src/auth.rs", "com.example.AuthService"]);
        let pack = make_pack(
            vec!["src/auth.rs"],
            vec![("src/AuthService.java", "com.example.AuthService")],
        );
        assert_eq!(assess_staleness(&article, Some(&pack)), Staleness::Fresh);
    }

    #[test]
    fn missing_file_anchor_is_stale() {
        let article = make_article(vec!["src/deleted.rs"]);
        let pack = make_pack(vec!["src/main.rs"], vec![]);
        assert_eq!(assess_staleness(&article, Some(&pack)), Staleness::Stale);
    }

    #[test]
    fn missing_symbol_anchor_is_stale() {
        let article = make_article(vec!["com.example.RemovedService"]);
        let pack = make_pack(vec![], vec![("src/Other.java", "com.example.Other")]);
        assert_eq!(assess_staleness(&article, Some(&pack)), Staleness::Stale);
    }

    #[test]
    fn mixed_found_and_missing_is_stale() {
        let article = make_article(vec!["src/auth.rs", "src/deleted.rs"]);
        let pack = make_pack(vec!["src/auth.rs"], vec![]);
        assert_eq!(assess_staleness(&article, Some(&pack)), Staleness::Stale);
    }

    #[test]
    fn no_pack_is_unverifiable() {
        let article = make_article(vec!["src/auth.rs"]);
        assert_eq!(assess_staleness(&article, None), Staleness::Unverifiable);
    }

    #[test]
    fn no_anchors_is_fresh() {
        let article = make_article(vec![]);
        let pack = make_pack(vec![], vec![]);
        assert_eq!(assess_staleness(&article, Some(&pack)), Staleness::Fresh);
    }

    #[test]
    fn batch_flag_returns_stale_ids() {
        let now = OffsetDateTime::now_utc();

        let mut fresh_article = make_article(vec!["src/missing.rs"]);
        fresh_article.id = "article-fresh-to-stale".to_string();
        fresh_article.staleness = Staleness::Fresh;

        let mut already_stale = make_article(vec!["src/also_missing.rs"]);
        already_stale.id = "article-already-stale".to_string();
        already_stale.staleness = Staleness::Stale;

        let mut will_stay_fresh = make_article(vec!["src/present.rs"]);
        will_stay_fresh.id = "article-stays-fresh".to_string();
        will_stay_fresh.staleness = Staleness::Fresh;

        let mut index = repolens_core::model::wiki::WikiIndex {
            schema_version: "1.0".to_string(),
            updated_at: now,
            articles: vec![fresh_article, already_stale, will_stay_fresh],
        };

        let pack = make_pack(vec!["src/present.rs"], vec![]);
        let newly_stale = flag_stale_articles(&mut index, Some(&pack));

        // Only the article that transitioned from Fresh -> Stale should be returned
        assert_eq!(newly_stale, vec!["article-fresh-to-stale"]);

        // Verify final staleness states
        assert_eq!(index.articles[0].staleness, Staleness::Stale);
        assert_eq!(index.articles[1].staleness, Staleness::Stale);
        assert_eq!(index.articles[2].staleness, Staleness::Fresh);
    }
}
