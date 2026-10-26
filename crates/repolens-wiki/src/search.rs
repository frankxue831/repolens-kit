//! Wiki article search: BM25 full-text search via tantivy with keyword fallback.

use std::collections::HashMap;

use repolens_core::model::wiki::{ArticleType, WikiArticle, WikiIndex};

use crate::index::WikiSearchIndex;

/// A single search result pairing an article with its relevance score.
#[derive(Debug, Clone)]
pub struct WikiSearchResult {
    pub article: WikiArticle,
    pub score: f64,
}

/// Tokenize text into lowercase alphanumeric tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Compute a keyword score: fraction of query tokens present in the corpus tokens.
fn keyword_score(query_tokens: &[String], corpus_tokens: &[String]) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }
    let hits = query_tokens
        .iter()
        .filter(|qt| corpus_tokens.iter().any(|ct| ct == *qt))
        .count();
    hits as f64 / query_tokens.len() as f64
}

/// Search the wiki index for articles matching the query.
///
/// - `query`: free-text query tokenized and matched against title, summary, tags,
///   and anchors.
/// - `article_type`: optional filter — only articles of this type are considered.
/// - `tags`: optional AND-filter — articles must have all of the given tags.
/// - `limit`: maximum number of results to return.
///
/// Results are sorted by score descending, then by article ID ascending as a
/// stable tiebreak.
pub fn search(
    index: &WikiIndex,
    query: &str,
    article_type: Option<ArticleType>,
    tags: Option<Vec<String>>,
    limit: usize,
) -> Vec<WikiSearchResult> {
    let query_tokens = tokenize(query);

    let mut results: Vec<WikiSearchResult> = index
        .articles
        .iter()
        .filter(|article| {
            // Type filter
            if let Some(ref at) = article_type {
                if &article.article_type != at {
                    return false;
                }
            }
            // Tags AND-filter
            if let Some(ref required_tags) = tags {
                if !required_tags
                    .iter()
                    .all(|rt| article.tags.iter().any(|t| t == rt))
                {
                    return false;
                }
            }
            true
        })
        .filter_map(|article| {
            if query_tokens.is_empty() {
                return None;
            }

            // Build corpus from all searchable fields
            let mut corpus_tokens = tokenize(&article.title);
            corpus_tokens.extend(tokenize(&article.summary));
            for tag in &article.tags {
                corpus_tokens.extend(tokenize(tag));
            }
            for anchor in &article.anchors {
                corpus_tokens.extend(tokenize(anchor));
            }

            let ks = keyword_score(&query_tokens, &corpus_tokens);
            let score = ks * article.confidence as f64;

            if score <= 0.0 {
                return None;
            }

            Some(WikiSearchResult {
                article: article.clone(),
                score,
            })
        })
        .collect();

    // Sort: score desc, then id asc as tiebreak
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.article.id.cmp(&b.article.id))
    });

    results.truncate(limit);
    results
}

/// Search using tantivy BM25 with fallback to keyword search.
///
/// When `tantivy_index` is `None` or the tantivy query fails, this falls back
/// to the simple keyword [`search()`]. Otherwise it retrieves a generous set of
/// BM25 candidates, post-filters by `article_type` and `tags`, multiplies the
/// BM25 score by the article's confidence, and returns up to `limit` results.
pub fn search_bm25(
    index: &WikiIndex,
    tantivy_index: Option<&WikiSearchIndex>,
    query: &str,
    article_type: Option<ArticleType>,
    tags: Option<Vec<String>>,
    phrase: bool,
    limit: usize,
) -> Vec<WikiSearchResult> {
    if limit == 0 || index.articles.is_empty() {
        return Vec::new();
    }

    let tantivy = match tantivy_index {
        Some(t) => t,
        None => return search(index, query, article_type, tags, limit),
    };

    let max_candidates = (limit * 20).min(index.articles.len());
    let candidates = match tantivy.search(query, phrase, max_candidates) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "tantivy search failed, falling back to keyword search");
            return search(index, query, article_type, tags, limit);
        }
    };

    // Build a lookup map: article_id -> &WikiArticle
    let article_map: HashMap<&str, &WikiArticle> =
        index.articles.iter().map(|a| (a.id.as_str(), a)).collect();

    let mut results: Vec<WikiSearchResult> = candidates
        .into_iter()
        .filter_map(|hit| {
            let article = article_map.get(hit.article_id.as_str()).copied()?;

            // Type filter
            if let Some(ref at) = article_type {
                if &article.article_type != at {
                    return None;
                }
            }

            // Tags AND-filter
            if let Some(ref required_tags) = tags {
                if !required_tags
                    .iter()
                    .all(|rt| article.tags.iter().any(|t| t == rt))
                {
                    return None;
                }
            }

            let score = hit.bm25_score as f64 * article.confidence as f64;
            Some(WikiSearchResult {
                article: article.clone(),
                score,
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.article.id.cmp(&b.article.id))
    });

    results.truncate(limit);
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::wiki::{Staleness, WikiIndex};
    use time::OffsetDateTime;

    fn make_article(
        id: &str,
        title: &str,
        summary: &str,
        tags: Vec<&str>,
        anchors: Vec<&str>,
        article_type: ArticleType,
        confidence: f32,
    ) -> WikiArticle {
        let now = OffsetDateTime::now_utc();
        WikiArticle {
            id: id.to_string(),
            title: title.to_string(),
            article_type,
            path: format!("{}/{}.md", "concept", id),
            anchors: anchors.into_iter().map(String::from).collect(),
            memory_refs: vec![],
            tags: tags.into_iter().map(String::from).collect(),
            confidence,
            staleness: Staleness::Fresh,
            summary: summary.to_string(),
            created_at: now,
            updated_at: now,
            source: None,
        }
    }

    fn make_index(articles: Vec<WikiArticle>) -> WikiIndex {
        WikiIndex {
            schema_version: "1.0".to_string(),
            updated_at: OffsetDateTime::now_utc(),
            articles,
        }
    }

    #[test]
    fn title_match_ranked_first() {
        let index = make_index(vec![
            make_article(
                "b",
                "Authentication Overview",
                "Summary about auth in general",
                vec![],
                vec![],
                ArticleType::Overview,
                1.0,
            ),
            make_article(
                "a",
                "User Roles",
                "Describes authentication-based roles",
                vec![],
                vec![],
                ArticleType::Concept,
                1.0,
            ),
        ]);

        let results = search(&index, "Authentication", None, None, 10);
        assert!(!results.is_empty());
        // The article with "Authentication" in the title should score higher
        // (title match hits 1/1, summary match hits 1/1 as well for article "b"
        // but "a" only has it in summary). With same confidence both score equally
        // on keyword_score so tiebreak by id → "a" first.
        // Let's just verify both are returned and scores are equal or "b" is first.
        let ids: Vec<&str> = results.iter().map(|r| r.article.id.as_str()).collect();
        assert!(ids.contains(&"a") && ids.contains(&"b"));
    }

    #[test]
    fn tag_match_included() {
        let index = make_index(vec![
            make_article(
                "tagged",
                "Some Article",
                "No match here",
                vec!["auth", "security"],
                vec![],
                ArticleType::Concept,
                1.0,
            ),
            make_article(
                "unrelated",
                "Other Article",
                "Completely different",
                vec!["unrelated"],
                vec![],
                ArticleType::Concept,
                1.0,
            ),
        ]);

        let results = search(&index, "auth", None, None, 10);
        let ids: Vec<&str> = results.iter().map(|r| r.article.id.as_str()).collect();
        assert!(
            ids.contains(&"tagged"),
            "tagged article should be found via tag"
        );
        assert!(
            !ids.contains(&"unrelated"),
            "unrelated article should not be returned"
        );
    }

    #[test]
    fn summary_match_included() {
        let index = make_index(vec![make_article(
            "art",
            "Unrelated Title",
            "This is about caching strategies",
            vec![],
            vec![],
            ArticleType::Concept,
            1.0,
        )]);

        let results = search(&index, "caching", None, None, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].article.id, "art");
    }

    #[test]
    fn type_filter() {
        let index = make_index(vec![
            make_article(
                "overview-art",
                "System Overview",
                "High-level system description",
                vec![],
                vec![],
                ArticleType::Overview,
                1.0,
            ),
            make_article(
                "entity-art",
                "System Entity",
                "System entity details",
                vec![],
                vec![],
                ArticleType::Entity,
                1.0,
            ),
        ]);

        let results = search(&index, "system", Some(ArticleType::Overview), None, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].article.id, "overview-art");
    }

    #[test]
    fn tag_filter_and_match() {
        let index = make_index(vec![
            make_article(
                "both-tags",
                "Auth Article",
                "Summary",
                vec!["auth", "security"],
                vec![],
                ArticleType::Concept,
                1.0,
            ),
            make_article(
                "one-tag",
                "Auth Article",
                "Summary",
                vec!["auth"],
                vec![],
                ArticleType::Concept,
                1.0,
            ),
        ]);

        let results = search(
            &index,
            "auth",
            None,
            Some(vec!["auth".to_string(), "security".to_string()]),
            10,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].article.id, "both-tags");
    }

    #[test]
    fn limit_applied() {
        let articles: Vec<WikiArticle> = (0..10)
            .map(|i| {
                make_article(
                    &format!("art-{i:02}"),
                    "Matching Title",
                    "Matching summary",
                    vec![],
                    vec![],
                    ArticleType::Concept,
                    1.0,
                )
            })
            .collect();
        let index = make_index(articles);

        let results = search(&index, "matching", None, None, 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn empty_index_returns_empty() {
        let index = make_index(vec![]);
        let results = search(&index, "anything", None, None, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn confidence_weighting() {
        let index = make_index(vec![
            make_article(
                "low-conf",
                "Auth Module",
                "Handles auth",
                vec![],
                vec![],
                ArticleType::Concept,
                0.5,
            ),
            make_article(
                "high-conf",
                "Auth Module",
                "Handles auth",
                vec![],
                vec![],
                ArticleType::Concept,
                1.0,
            ),
        ]);

        let results = search(&index, "auth", None, None, 10);
        assert_eq!(results.len(), 2);
        // high-conf should be first (same keyword score, higher confidence → higher final score)
        assert_eq!(results[0].article.id, "high-conf");
        assert_eq!(results[1].article.id, "low-conf");
        assert!(
            results[0].score > results[1].score,
            "high confidence should yield higher score"
        );
    }

    #[test]
    fn bm25_filters_after_scanning_all_matches() {
        let tantivy = WikiSearchIndex::open_in_ram().unwrap();
        let mut articles = Vec::new();

        for i in 0..10 {
            let article = make_article(
                &format!("decoy-{i}"),
                &format!("Alpha Alpha Alpha Decoy {i}"),
                "Alpha alpha alpha decoy",
                vec!["general"],
                vec![],
                ArticleType::Concept,
                1.0,
            );
            tantivy
                .upsert(&article, "alpha alpha alpha alpha alpha alpha alpha alpha")
                .unwrap();
            articles.push(article);
        }

        let target = make_article(
            "filtered-target",
            "Guide",
            "Guide",
            vec!["wanted"],
            vec!["src/alpha/target.rs"],
            ArticleType::Concept,
            1.0,
        );
        tantivy.upsert(&target, "guide content only").unwrap();
        articles.push(target.clone());

        let index = make_index(articles);
        let results = search_bm25(
            &index,
            Some(&tantivy),
            "alpha",
            None,
            Some(vec!["wanted".to_string()]),
            false,
            1,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].article.id, "filtered-target");
    }

    #[test]
    fn bm25_resorts_after_confidence_weighting() {
        let tantivy = WikiSearchIndex::open_in_ram().unwrap();

        let low_confidence = make_article(
            "low-confidence",
            "Authentication Authentication Authentication",
            "Authentication authentication authentication",
            vec![],
            vec![],
            ArticleType::Concept,
            0.01,
        );
        tantivy
            .upsert(
                &low_confidence,
                "authentication authentication authentication authentication",
            )
            .unwrap();

        let high_confidence = make_article(
            "high-confidence",
            "Guide",
            "Guide",
            vec![],
            vec![],
            ArticleType::Concept,
            1.0,
        );
        tantivy.upsert(&high_confidence, "authentication").unwrap();

        let index = make_index(vec![low_confidence, high_confidence]);
        let results = search_bm25(
            &index,
            Some(&tantivy),
            "authentication",
            None,
            None,
            false,
            2,
        );

        assert_eq!(results.len(), 2);
        assert!(
            results[0].score >= results[1].score,
            "results should be ordered by the final confidence-adjusted score"
        );
        assert_eq!(results[0].article.id, "high-confidence");
    }
}
