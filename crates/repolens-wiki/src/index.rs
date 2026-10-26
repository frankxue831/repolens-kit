//! Tantivy BM25 search index for wiki articles.
//!
//! Provides full-text search with English stemming, field boosts, and a custom
//! path tokenizer for code anchors. The index is stored at
//! `.repolens/wiki/.tantivy/` inside the repo root.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::tokenizer::{
    Language, LowerCaser, RegexTokenizer, RemoveLongFilter, SimpleTokenizer, Stemmer, TextAnalyzer,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};
use tracing::{debug, info, warn};

use repolens_core::model::wiki::{WikiArticle, WikiIndex};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const EN_STEM_TOKENIZER: &str = "en_stem";
const PATH_TOKENIZER: &str = "path_tok";
const RAW_LOWER_TOKENIZER: &str = "raw_lower";
const WRITER_HEAP_SIZE_BULK: usize = 50_000_000;
const WRITER_HEAP_SIZE_SINGLE: usize = 15_000_000;

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// A single search result from the tantivy index.
#[derive(Debug, Clone)]
pub struct IndexSearchResult {
    pub article_id: String,
    pub bm25_score: f32,
}

// ---------------------------------------------------------------------------
// Schema helpers
// ---------------------------------------------------------------------------

/// Field handles returned by [`build_schema`].
struct WikiFields {
    id: Field,
    title: Field,
    summary: Field,
    body: Field,
    tags: Field,
    anchors: Field,
    article_type: Field,
}

fn text_options_stemmed_stored() -> TextOptions {
    TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(EN_STEM_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored()
}

fn text_options_stemmed_not_stored() -> TextOptions {
    TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(EN_STEM_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    )
}

fn text_options_raw_stored() -> TextOptions {
    TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(RAW_LOWER_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored()
}

fn text_options_path_stored() -> TextOptions {
    TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(PATH_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored()
}

fn build_schema() -> (Schema, WikiFields) {
    let mut builder = Schema::builder();

    let id = builder.add_text_field("id", STRING | STORED);
    let title = builder.add_text_field("title", text_options_stemmed_stored());
    let summary = builder.add_text_field("summary", text_options_stemmed_stored());
    let body = builder.add_text_field("body", text_options_stemmed_not_stored());
    let tags = builder.add_text_field("tags", text_options_raw_stored());
    let anchors = builder.add_text_field("anchors", text_options_path_stored());
    let article_type = builder.add_text_field("article_type", STRING | STORED);

    let schema = builder.build();
    let fields = WikiFields {
        id,
        title,
        summary,
        body,
        tags,
        anchors,
        article_type,
    };
    (schema, fields)
}

// ---------------------------------------------------------------------------
// Tokenizer registration
// ---------------------------------------------------------------------------

fn register_tokenizers(index: &Index) {
    // English stemmer tokenizer
    let en_stem = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .filter(Stemmer::new(Language::English))
        .build();
    index.tokenizers().register(EN_STEM_TOKENIZER, en_stem);

    // Raw-lowercase tokenizer for tags: splits on whitespace, lowercases,
    // but no stemming — tag "auth" must not match "authentication".
    let raw_lower = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(RAW_LOWER_TOKENIZER, raw_lower);

    // Path tokenizer: splits on `/`, `::`, `\`, and `.` — keeps path segments
    // as individual tokens (e.g. "src/auth/middleware.rs" -> ["src", "auth", "middleware", "rs"]).
    let path_tok =
        TextAnalyzer::builder(RegexTokenizer::new(r"[a-zA-Z0-9_]+").expect("valid regex"))
            .filter(LowerCaser)
            .build();
    index.tokenizers().register(PATH_TOKENIZER, path_tok);
}

// ---------------------------------------------------------------------------
// Index directory
// ---------------------------------------------------------------------------

fn tantivy_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".repolens").join("wiki").join(".tantivy")
}

// ---------------------------------------------------------------------------
// WikiSearchIndex
// ---------------------------------------------------------------------------

/// Manages a tantivy BM25 search index over wiki articles.
pub struct WikiSearchIndex {
    index: Index,
    reader: IndexReader,
    fields: WikiFields,
}

impl WikiSearchIndex {
    /// Open (or create) the on-disk tantivy index at `.repolens/wiki/.tantivy/`.
    ///
    /// If the existing index is corrupt, it is deleted and recreated.
    pub fn open(repo_root: &Path) -> Result<Self> {
        let dir = tantivy_dir(repo_root);
        let (schema, fields) = build_schema();
        let mut needs_rebuild = false;

        let index = if dir.exists() {
            match Index::open_in_dir(&dir) {
                Ok(idx) => {
                    debug!(path = %dir.display(), "Opened existing tantivy index");
                    idx
                }
                Err(e) => {
                    warn!(
                        path = %dir.display(),
                        error = %e,
                        "Tantivy index corrupt, deleting and recreating"
                    );
                    fs::remove_dir_all(&dir).with_context(|| {
                        format!("Failed to remove corrupt tantivy index: {}", dir.display())
                    })?;
                    fs::create_dir_all(&dir).with_context(|| {
                        format!("Failed to create tantivy directory: {}", dir.display())
                    })?;
                    needs_rebuild = true;
                    Index::create_in_dir(&dir, schema.clone())
                        .context("Failed to create tantivy index")?
                }
            }
        } else {
            fs::create_dir_all(&dir).with_context(|| {
                format!("Failed to create tantivy directory: {}", dir.display())
            })?;
            info!(path = %dir.display(), "Creating new tantivy index");
            needs_rebuild = true;
            Index::create_in_dir(&dir, schema.clone()).context("Failed to create tantivy index")?
        };

        register_tokenizers(&index);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .context("Failed to create tantivy reader")?;

        let search_index = Self {
            index,
            reader,
            fields,
        };

        if needs_rebuild {
            match crate::store::WikiStore::load_index(repo_root) {
                Ok(wiki_index) if !wiki_index.articles.is_empty() => {
                    info!(
                        articles = wiki_index.articles.len(),
                        "Rebuilding tantivy index from existing wiki content"
                    );
                    search_index.rebuild(&wiki_index, repo_root)?;
                }
                Ok(_) => {}
                Err(e) => {
                    debug!(
                        path = %repo_root.display(),
                        error = %e,
                        "Skipping tantivy rebuild because wiki index is unavailable"
                    );
                }
            }
        }

        Ok(search_index)
    }

    /// Create an in-RAM index for testing.
    pub fn open_in_ram() -> Result<Self> {
        let (schema, fields) = build_schema();
        let index = Index::create_in_ram(schema.clone());
        register_tokenizers(&index);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .context("Failed to create tantivy reader")?;

        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    /// Rebuild the entire index from a [`WikiIndex`] by reading article bodies
    /// from disk via [`WikiStore::read_article`].
    pub fn rebuild(&self, wiki_index: &WikiIndex, repo_root: &Path) -> Result<()> {
        let mut writer: IndexWriter<TantivyDocument> = self
            .index
            .writer(WRITER_HEAP_SIZE_BULK)
            .context("Failed to create tantivy writer")?;

        writer.delete_all_documents()?;
        writer.commit()?;

        let mut skipped = 0usize;
        for article in &wiki_index.articles {
            let body = match crate::store::WikiStore::read_article(repo_root, article) {
                Ok(b) => b,
                Err(e) => {
                    warn!(
                        article_id = %article.id,
                        error = %e,
                        "Failed to read article body during rebuild, skipping"
                    );
                    skipped += 1;
                    continue;
                }
            };
            add_article_doc(&mut writer, &self.fields, article, &body)?;
        }

        writer.commit().context("Failed to commit rebuild")?;
        self.reader.reload()?;
        info!(
            articles = wiki_index.articles.len(),
            skipped, "Rebuilt tantivy index"
        );
        Ok(())
    }

    /// Insert or update a single article in the index.
    ///
    /// Deletes any existing document with the same ID first, then adds the new
    /// one.
    pub fn upsert(&self, article: &WikiArticle, body: &str) -> Result<()> {
        let mut writer: IndexWriter<TantivyDocument> =
            self.index
                .writer(WRITER_HEAP_SIZE_SINGLE)
                .context("Failed to create tantivy writer")?;

        let id_term = Term::from_field_text(self.fields.id, &article.id);
        writer.delete_term(id_term);

        add_article_doc(&mut writer, &self.fields, article, body)?;

        writer.commit().context("Failed to commit upsert")?;
        self.reader.reload()?;
        debug!(article_id = %article.id, "Upserted article in tantivy index");
        Ok(())
    }

    /// Remove an article from the index by ID.
    pub fn remove(&self, article_id: &str) -> Result<()> {
        let mut writer: IndexWriter<TantivyDocument> =
            self.index
                .writer(WRITER_HEAP_SIZE_SINGLE)
                .context("Failed to create tantivy writer")?;

        let id_term = Term::from_field_text(self.fields.id, article_id);
        writer.delete_term(id_term);

        writer.commit().context("Failed to commit remove")?;
        self.reader.reload()?;
        debug!(article_id, "Removed article from tantivy index");
        Ok(())
    }

    /// Search the index using BM25 scoring.
    ///
    /// When `phrase` is true, the query string is wrapped in double quotes to
    /// perform a phrase query. Results are returned sorted by BM25 score
    /// descending.
    pub fn search(
        &self,
        query_str: &str,
        phrase: bool,
        limit: usize,
    ) -> Result<Vec<IndexSearchResult>> {
        let searcher = self.reader.searcher();

        let mut parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.title,
                self.fields.summary,
                self.fields.body,
                self.fields.tags,
                self.fields.anchors,
            ],
        );

        // Set field boosts
        parser.set_field_boost(self.fields.title, 3.0);
        parser.set_field_boost(self.fields.summary, 2.0);
        parser.set_field_boost(self.fields.body, 1.0);
        parser.set_field_boost(self.fields.tags, 2.0);
        parser.set_field_boost(self.fields.anchors, 1.5);

        let effective_query = if phrase {
            format!("\"{}\"", query_str)
        } else {
            query_str.to_string()
        };

        let query = parser
            .parse_query(&effective_query)
            .with_context(|| format!("Failed to parse query: {}", effective_query))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .context("Tantivy search failed")?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .context("Failed to retrieve document")?;

            if let Some(id_value) = doc.get_first(self.fields.id) {
                if let Some(id_str) = id_value.as_str() {
                    results.push(IndexSearchResult {
                        article_id: id_str.to_string(),
                        bm25_score: score,
                    });
                }
            }
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn add_article_doc(
    writer: &mut IndexWriter<TantivyDocument>,
    fields: &WikiFields,
    article: &WikiArticle,
    body: &str,
) -> Result<()> {
    let article_type_str = article.article_type.as_str();

    let mut doc = TantivyDocument::default();
    doc.add_text(fields.id, &article.id);
    doc.add_text(fields.title, &article.title);
    doc.add_text(fields.summary, &article.summary);
    doc.add_text(fields.body, body);
    doc.add_text(fields.tags, article.tags.join(" "));
    doc.add_text(fields.anchors, article.anchors.join(" "));
    doc.add_text(fields.article_type, article_type_str);

    writer.add_document(doc)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::wiki::{ArticleType, Staleness, WikiIndex};
    use tempfile::tempdir;
    use time::OffsetDateTime;

    fn make_article(
        id: &str,
        title: &str,
        summary: &str,
        tags: Vec<&str>,
        anchors: Vec<&str>,
        article_type: ArticleType,
    ) -> WikiArticle {
        let now = OffsetDateTime::now_utc();
        WikiArticle {
            id: id.to_string(),
            title: title.to_string(),
            article_type,
            path: format!("articles/concept/{}.md", id),
            anchors: anchors.into_iter().map(String::from).collect(),
            memory_refs: vec![],
            tags: tags.into_iter().map(String::from).collect(),
            confidence: 0.9,
            staleness: Staleness::Fresh,
            summary: summary.to_string(),
            created_at: now,
            updated_at: now,
            source: None,
        }
    }

    #[test]
    fn build_and_query_basic() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        let article = make_article(
            "art-1",
            "Authentication Overview",
            "How the auth system works",
            vec!["auth"],
            vec![],
            ArticleType::Overview,
        );
        idx.upsert(
            &article,
            "Detailed body about authentication flows and tokens.",
        )
        .unwrap();

        let results = idx.search("authentication", false, 10).unwrap();
        assert!(!results.is_empty(), "should find the article");
        assert_eq!(results[0].article_id, "art-1");
        assert!(results[0].bm25_score > 0.0);
    }

    #[test]
    fn stemming_matches() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        let article = make_article(
            "cfg-1",
            "Configuration Guide",
            "How to configure the system",
            vec![],
            vec![],
            ArticleType::Concept,
        );
        idx.upsert(&article, "This guide covers configuring all modules.")
            .unwrap();

        // "configuring" should match "configuration" and "configure" via stemming
        let results = idx.search("configuring", false, 10).unwrap();
        assert!(
            !results.is_empty(),
            "stemming should match 'configuring' to 'configuration'"
        );
        assert_eq!(results[0].article_id, "cfg-1");
    }

    #[test]
    fn title_boosted_over_body() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        // Article A: "authentication" in title
        let art_a = make_article(
            "title-match",
            "Authentication Module",
            "Overview of the module",
            vec![],
            vec![],
            ArticleType::Entity,
        );
        idx.upsert(&art_a, "This module handles logins.").unwrap();

        // Article B: "authentication" only in body
        let art_b = make_article(
            "body-match",
            "Security Module",
            "Overview of security",
            vec![],
            vec![],
            ArticleType::Entity,
        );
        idx.upsert(
            &art_b,
            "This module performs authentication checks on every request.",
        )
        .unwrap();

        let results = idx.search("authentication", false, 10).unwrap();
        assert!(results.len() >= 2, "both articles should match");
        assert_eq!(
            results[0].article_id, "title-match",
            "title match should rank above body-only match"
        );
    }

    #[test]
    fn phrase_query() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        let art_di = make_article(
            "di-article",
            "Dependency Injection Pattern",
            "How DI works in the codebase",
            vec![],
            vec![],
            ArticleType::Concept,
        );
        idx.upsert(
            &art_di,
            "The dependency injection pattern decouples components.",
        )
        .unwrap();

        let art_other = make_article(
            "dep-article",
            "Dependency Management",
            "Managing project dependencies and injection of configs",
            vec![],
            vec![],
            ArticleType::Concept,
        );
        idx.upsert(
            &art_other,
            "This covers dependency resolution. Injection is also discussed.",
        )
        .unwrap();

        let results = idx.search("dependency injection", true, 10).unwrap();
        assert!(!results.is_empty(), "phrase query should find matches");
        assert_eq!(
            results[0].article_id, "di-article",
            "exact phrase match should rank first"
        );
    }

    #[test]
    fn upsert_replaces_existing() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        let article = make_article(
            "art-1",
            "Old Title About Caching",
            "Old summary about caching",
            vec!["caching"],
            vec![],
            ArticleType::Concept,
        );
        idx.upsert(&article, "Old body about caching strategies.")
            .unwrap();

        // Verify old content is found
        let results = idx.search("caching", false, 10).unwrap();
        assert!(!results.is_empty(), "old content should be searchable");

        // Upsert with completely new content
        let updated = make_article(
            "art-1",
            "New Title About Routing",
            "New summary about routing",
            vec!["routing"],
            vec![],
            ArticleType::Concept,
        );
        idx.upsert(&updated, "New body about routing strategies.")
            .unwrap();

        // Old content should be gone
        let old_results = idx.search("caching", false, 10).unwrap();
        assert!(
            old_results.is_empty(),
            "old content should not be found after upsert"
        );

        // New content should be found
        let new_results = idx.search("routing", false, 10).unwrap();
        assert!(!new_results.is_empty(), "new content should be searchable");
        assert_eq!(new_results[0].article_id, "art-1");
    }

    #[test]
    fn remove_deletes_from_index() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        let article = make_article(
            "art-1",
            "Logging Framework",
            "How logging works",
            vec![],
            vec![],
            ArticleType::Entity,
        );
        idx.upsert(&article, "Detailed info about the logging framework.")
            .unwrap();

        // Verify it exists
        let results = idx.search("logging", false, 10).unwrap();
        assert!(!results.is_empty(), "article should be found before remove");

        // Remove it
        idx.remove("art-1").unwrap();

        // Verify it is gone
        let results = idx.search("logging", false, 10).unwrap();
        assert!(
            results.is_empty(),
            "article should not be found after remove"
        );
    }

    #[test]
    fn anchor_path_searchable() {
        let idx = WikiSearchIndex::open_in_ram().unwrap();

        let article = make_article(
            "mw-article",
            "Auth Middleware",
            "Authentication middleware layer",
            vec![],
            vec!["src/auth/middleware.rs", "src/auth/mod.rs"],
            ArticleType::Entity,
        );
        idx.upsert(&article, "The middleware intercepts requests.")
            .unwrap();

        // Search for a path segment
        let results = idx.search("middleware", false, 10).unwrap();
        assert!(
            !results.is_empty(),
            "'middleware' should match anchor 'src/auth/middleware.rs'"
        );
        assert_eq!(results[0].article_id, "mw-article");
    }

    #[test]
    fn open_rebuilds_existing_wiki_when_cache_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let article = make_article(
            "existing-article",
            "Authentication Overview",
            "How authentication works",
            vec!["auth"],
            vec![],
            ArticleType::Overview,
        );

        let wiki_index = WikiIndex {
            schema_version: "1.0".to_string(),
            updated_at: OffsetDateTime::now_utc(),
            articles: vec![article.clone()],
        };
        crate::store::WikiStore::save_index(root, &wiki_index).unwrap();
        crate::store::WikiStore::write_article(
            root,
            &article,
            "Detailed body about authentication flows and tokens.",
        )
        .unwrap();

        let tantivy_dir = root.join(".repolens").join("wiki").join(".tantivy");
        assert!(
            !tantivy_dir.exists(),
            "test setup should start without tantivy cache"
        );

        let idx = WikiSearchIndex::open(root).unwrap();
        let results = idx.search("authentication", false, 10).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].article_id, "existing-article");
    }
}
