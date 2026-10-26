//! MCP tool handlers for the wiki compiled-knowledge layer:
//! compile, read, search, index, lint, delete.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tracing::info;
use uuid::Uuid;

use rmcp::{model::CallToolResult, ErrorData as McpError};

use repolens_app::discover::PackResolver;
use repolens_core::model::graph::FileGraph;
use repolens_core::model::wiki::{
    ArticleType, ExternalSource, Staleness, WikiArticle, WikiEventKind, WikiLogEntry,
};
use repolens_memory::store::MemoryStore;
use repolens_wiki::coverage::compute_coverage;
use repolens_wiki::staleness::{assess_staleness, flag_stale_articles};
use repolens_wiki::store::{compute_article_path, WikiStore};

use crate::epistemic::*;
use crate::err_json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_article_type(s: &str) -> Option<ArticleType> {
    match s {
        "ENTITY" => Some(ArticleType::Entity),
        "CONCEPT" => Some(ArticleType::Concept),
        "OVERVIEW" => Some(ArticleType::Overview),
        "COMPARISON" => Some(ArticleType::Comparison),
        _ => None,
    }
}

fn staleness_str(s: &Staleness) -> &'static str {
    match s {
        Staleness::Fresh => "FRESH",
        Staleness::Stale => "STALE",
        Staleness::Unverifiable => "UNVERIFIABLE",
    }
}

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiCompileParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "Article title")]
    pub title: String,

    #[serde(rename = "articleType")]
    #[schemars(description = "Article type: ENTITY, CONCEPT, OVERVIEW, COMPARISON")]
    pub article_type: String,

    #[schemars(description = "Article body in Markdown")]
    pub body: String,

    #[schemars(description = "One-sentence summary of the article")]
    pub summary: String,

    #[schemars(description = "File-path or qualified-name anchors for staleness checking")]
    pub anchors: Option<Vec<String>>,

    #[serde(rename = "memoryRefs")]
    #[schemars(description = "Memory node IDs referenced by this article")]
    pub memory_refs: Option<Vec<String>>,

    #[schemars(description = "Tags (auto-lowercased, deduped)")]
    pub tags: Option<Vec<String>>,

    #[schemars(description = "Confidence score 0.0-1.0 (default 0.8 for new articles)")]
    pub confidence: Option<f32>,

    #[schemars(description = "UUID of existing article to update (omit to create new)")]
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiReadParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "UUID of the article to read")]
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiSearchParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "Free-text search query")]
    pub query: String,

    #[serde(rename = "articleType")]
    #[schemars(description = "Filter by article type: ENTITY, CONCEPT, OVERVIEW, COMPARISON")]
    pub article_type: Option<String>,

    #[schemars(description = "Filter by tags (AND match, all tags must be present)")]
    pub tags: Option<Vec<String>>,

    #[serde(rename = "phraseQuery")]
    #[schemars(description = "Treat query as exact phrase (default false)")]
    pub phrase_query: Option<bool>,

    #[schemars(description = "Maximum number of results (default 10)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiIndexParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[serde(rename = "articleType")]
    #[schemars(description = "Filter by article type: ENTITY, CONCEPT, OVERVIEW, COMPARISON")]
    pub article_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiLintParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiDeleteParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "UUID of the article to delete")]
    pub id: String,

    #[schemars(description = "Set true to actually delete (default false = dry-run)")]
    pub confirm: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiSuggestParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "Maximum number of suggestions (default 10)")]
    pub limit: Option<usize>,

    #[schemars(description = "Filter to subdirectory (e.g. \"src/auth\")")]
    pub directory: Option<String>,

    #[serde(rename = "minImportance")]
    #[schemars(description = "Minimum importance score 0.0-1.0")]
    pub min_importance: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WikiIngestExternalParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,

    #[schemars(description = "Article title")]
    pub title: String,

    #[serde(rename = "articleType")]
    #[schemars(description = "Article type: ENTITY, CONCEPT, OVERVIEW, COMPARISON")]
    pub article_type: String,

    #[schemars(description = "Article body in Markdown")]
    pub body: String,

    #[schemars(description = "One-sentence summary of the article")]
    pub summary: String,

    #[serde(rename = "sourceOrigin")]
    #[schemars(description = "URL, doc name, or identifier of the external source")]
    pub source_origin: String,

    #[serde(rename = "sourceVersion")]
    #[schemars(description = "Version string or date of the external source")]
    pub source_version: Option<String>,

    #[schemars(description = "File-path or qualified-name anchors to ground to local code")]
    pub anchors: Option<Vec<String>>,

    #[serde(rename = "memoryRefs")]
    #[schemars(description = "Memory node IDs referenced by this article")]
    pub memory_refs: Option<Vec<String>>,

    #[schemars(description = "Tags (auto-lowercased, deduped)")]
    pub tags: Option<Vec<String>>,

    #[schemars(description = "Confidence score 0.0-1.0 (default 0.6 for external articles)")]
    pub confidence: Option<f32>,

    #[schemars(description = "UUID of existing external article to update (omit to create new)")]
    pub id: Option<String>,
}

// ---------------------------------------------------------------------------
// Response structs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArticleMetadata {
    id: String,
    title: String,
    article_type: String,
    path: String,
    summary: String,
    tags: Vec<String>,
    anchors: Vec<String>,
    memory_refs: Vec<String>,
    confidence: f32,
    staleness: String,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<repolens_core::model::wiki::ExternalSource>,
}

impl ArticleMetadata {
    fn from_article(a: &WikiArticle) -> Self {
        ArticleMetadata {
            id: a.id.clone(),
            title: a.title.clone(),
            article_type: a.article_type.as_str().to_string(),
            path: a.path.clone(),
            summary: a.summary.clone(),
            tags: a.tags.clone(),
            anchors: a.anchors.clone(),
            memory_refs: a.memory_refs.clone(),
            confidence: a.confidence,
            staleness: staleness_str(&a.staleness).to_string(),
            created_at: a.created_at.to_string(),
            updated_at: a.updated_at.to_string(),
            source: a.source.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub fn handle_wiki_compile(
    resolver: &PackResolver,
    params: WikiCompileParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, title = %params.title, "wiki_compile: entry");

    let root = Path::new(&params.root);

    // Parse article type
    let article_type = match parse_article_type(&params.article_type) {
        Some(t) => t,
        None => {
            info!(elapsed_ms = %start.elapsed().as_millis(), article_type = %params.article_type, "wiki_compile: invalid articleType");
            return err_json(&format!(
                "Invalid articleType: {}. Must be ENTITY, CONCEPT, OVERVIEW, or COMPARISON",
                params.article_type
            ));
        }
    };

    // Load index
    let mut index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_compile: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    // Normalize tags
    let tags: Vec<String> = params
        .tags
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let anchors = params.anchors.unwrap_or_default();
    let memory_refs = params.memory_refs.unwrap_or_default();
    let now = OffsetDateTime::now_utc();

    let (article, old_path, status) = if let Some(ref id) = params.id {
        // Update existing article
        match index.articles.iter().position(|a| a.id == *id) {
            Some(pos) => {
                let existing = &index.articles[pos];
                let old_path = existing.path.clone();

                // Compute new path if title or type changed
                let existing_paths: Vec<String> = index
                    .articles
                    .iter()
                    .filter(|a| a.id != *id)
                    .map(|a| a.path.clone())
                    .collect();
                let new_path =
                    if existing.title != params.title || existing.article_type != article_type {
                        compute_article_path(&article_type, &params.title, &existing_paths)
                    } else {
                        old_path.clone()
                    };

                let confidence = params.confidence.unwrap_or(existing.confidence);
                let created_at = existing.created_at;

                let article = WikiArticle {
                    id: id.clone(),
                    title: params.title.clone(),
                    article_type: article_type.clone(),
                    path: new_path,
                    anchors,
                    memory_refs,
                    tags,
                    confidence,
                    staleness: Staleness::Fresh,
                    summary: params.summary.clone(),
                    created_at,
                    updated_at: now,
                    source: None,
                };
                (article, Some(old_path), "updated")
            }
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), id = %id, "wiki_compile: article id not found");
                return err_json(&format!("Article not found: {id}"));
            }
        }
    } else {
        // Create new article
        let id = Uuid::new_v4().to_string();
        let existing_paths: Vec<String> = index.articles.iter().map(|a| a.path.clone()).collect();
        let path = compute_article_path(&article_type, &params.title, &existing_paths);
        let confidence = params.confidence.unwrap_or(0.8);

        let article = WikiArticle {
            id: id.clone(),
            title: params.title.clone(),
            article_type: article_type.clone(),
            path,
            anchors,
            memory_refs,
            tags,
            confidence,
            staleness: Staleness::Fresh,
            summary: params.summary.clone(),
            created_at: now,
            updated_at: now,
            source: None,
        };
        (article, None, "created")
    };

    // Resolve pack for staleness assessment
    let pack = resolver
        .resolve(None, Some(&params.root))
        .ok()
        .map(|(_, idx)| idx);

    // Assess staleness
    let staleness = assess_staleness(&article, pack.as_ref());
    let staleness_label = staleness_str(&staleness).to_string();

    let article = WikiArticle {
        staleness,
        ..article
    };

    // Write article to disk
    let article_path = article.path.clone();
    let article_id = article.id.clone();

    // Ensure .repolens/wiki/.gitignore exists so the compiled cache is never
    // accidentally committed. wiki_ingest_external does the same before writing.
    let _ = WikiStore::ensure_gitignore(root);

    if let Err(e) = WikiStore::write_article(root, &article, &params.body) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_compile: write error");
        return err_json(&format!("Failed to write article: {e}"));
    }

    // Update index
    if let Some(pos) = index.articles.iter().position(|a| a.id == article_id) {
        index.articles[pos] = article.clone();
    } else {
        index.articles.push(article.clone());
    }
    index.updated_at = now;

    if let Err(e) = WikiStore::save_index(root, &index) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_compile: save index error");
        return err_json(&format!("Failed to save wiki index: {e}"));
    }

    // Upsert into tantivy search index
    if let Ok(tantivy) = repolens_wiki::index::WikiSearchIndex::open(root) {
        let _ = tantivy.upsert(&article, &params.body);
    }

    // Delete old file if path changed
    if let Some(ref old) = old_path {
        if *old != article_path {
            let old_file = root.join(".repolens").join("wiki").join(old);
            let _ = std::fs::remove_file(&old_file);
        }
    }

    // Append log entry
    let event = if status == "created" {
        WikiEventKind::Compile
    } else {
        WikiEventKind::Update
    };
    let log_entry = WikiLogEntry {
        timestamp: now,
        event,
        article_id: Some(article_id.clone()),
        detail: format!("{status}: {}", params.title),
    };
    let _ = WikiStore::append_log(root, log_entry);

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        id = %article_id,
        status,
        "wiki_compile: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::WikiArticle {
            article_id: article_id.clone(),
            staleness: Some(staleness_label.clone()),
        }),
        serde_json::json!({
            "status": status,
            "id": article_id,
            "path": article_path,
            "staleness": staleness_label,
        }),
    )
}

pub fn handle_wiki_read(params: WikiReadParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, id = %params.id, "wiki_read: entry");

    let root = Path::new(&params.root);

    let index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_read: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    let article = match index.articles.iter().find(|a| a.id == params.id) {
        Some(a) => a,
        None => {
            info!(elapsed_ms = %start.elapsed().as_millis(), id = %params.id, "wiki_read: article not found");
            return err_json(&format!("Article not found: {}", params.id));
        }
    };

    let body = match WikiStore::read_article(root, article) {
        Ok(b) => b,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_read: read article error");
            return err_json(&format!("Failed to read article body: {e}"));
        }
    };

    let metadata = ArticleMetadata::from_article(article);

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        id = %params.id,
        "wiki_read: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::WikiArticle {
            article_id: params.id.clone(),
            staleness: Some(metadata.staleness.clone()),
        }),
        serde_json::json!({
            "article": metadata,
            "body": body,
        }),
    )
}

pub fn handle_wiki_search(params: WikiSearchParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, query = %params.query, "wiki_search: entry");

    let root = Path::new(&params.root);
    let limit = params.limit.unwrap_or(10);

    // Parse optional type filter
    let type_filter = match params.article_type.as_deref() {
        Some(s) => match parse_article_type(s) {
            Some(t) => Some(t),
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), article_type = %s, "wiki_search: invalid articleType");
                return err_json(&format!(
                    "Invalid articleType: {s}. Must be ENTITY, CONCEPT, OVERVIEW, or COMPARISON"
                ));
            }
        },
        None => None,
    };

    let index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_search: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    let tag_filter = params
        .tags
        .map(|t| t.into_iter().map(|s| s.to_lowercase()).collect());

    let tantivy = repolens_wiki::index::WikiSearchIndex::open(root).ok();
    let phrase = params.phrase_query.unwrap_or(false);

    let results = repolens_wiki::search::search_bm25(
        &index,
        tantivy.as_ref(),
        &params.query,
        type_filter,
        tag_filter,
        phrase,
        limit,
    );

    let total = results.len();
    let result_values: Vec<serde_json::Value> = results
        .into_iter()
        .map(|r| {
            let meta = ArticleMetadata::from_article(&r.article);
            serde_json::json!({
                "article": meta,
                "score": r.score,
            })
        })
        .collect();

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        total,
        "wiki_search: exit"
    );
    let article_ids: Vec<String> = result_values
        .iter()
        .filter_map(|v| v["article"]["id"].as_str().map(|s| s.to_string()))
        .collect();
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::WikiIndex {
            source: "wiki-index.json".into(),
            article_ids,
        }),
        serde_json::json!({
            "results": result_values,
            "total": total,
        }),
    )
}

pub fn handle_wiki_index(params: WikiIndexParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, "wiki_index: entry");

    let root = Path::new(&params.root);

    // Parse optional type filter
    let type_filter = match params.article_type.as_deref() {
        Some(s) => match parse_article_type(s) {
            Some(t) => Some(t),
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), article_type = %s, "wiki_index: invalid articleType");
                return err_json(&format!(
                    "Invalid articleType: {s}. Must be ENTITY, CONCEPT, OVERVIEW, or COMPARISON"
                ));
            }
        },
        None => None,
    };

    let index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_index: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    // Filter and group by type
    let mut by_type: BTreeMap<&str, Vec<ArticleMetadata>> = BTreeMap::new();
    for key in &["entity", "concept", "overview", "comparison"] {
        by_type.insert(key, Vec::new());
    }

    let mut total = 0usize;
    for article in &index.articles {
        if let Some(ref tf) = type_filter {
            if &article.article_type != tf {
                continue;
            }
        }
        let bucket = match article.article_type {
            ArticleType::Entity => "entity",
            ArticleType::Concept => "concept",
            ArticleType::Overview => "overview",
            ArticleType::Comparison => "comparison",
        };
        by_type
            .entry(bucket)
            .or_default()
            .push(ArticleMetadata::from_article(article));
        total += 1;
    }

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        total,
        "wiki_index: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::WikiIndex {
            source: "wiki-index.json".into(),
            article_ids: vec![],
        }),
        serde_json::json!({
            "articles": by_type,
            "total": total,
        }),
    )
}

pub fn handle_wiki_lint(
    resolver: &PackResolver,
    params: WikiLintParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, "wiki_lint: entry");

    let root = Path::new(&params.root);

    // Load index
    let mut index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_lint: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    // Resolve pack once — used for staleness check and envelope provenance
    let resolved_pack = resolver.resolve(None, Some(&params.root)).ok();
    let pack = resolved_pack.as_ref().map(|(_, idx)| idx.clone());
    let pack_available = resolved_pack.is_some();

    // Flag stale articles
    flag_stale_articles(&mut index, pack.as_ref());

    // Build list of stale articles
    let stale_articles: Vec<serde_json::Value> = index
        .articles
        .iter()
        .filter(|a| a.staleness == Staleness::Stale)
        .map(|a| serde_json::json!({ "id": a.id, "title": a.title, "path": a.path }))
        .collect();

    // Load memory graph for broken memory-ref check
    let (memory_available, broken_memory_refs): (bool, Vec<serde_json::Value>) =
        match MemoryStore::load(root) {
            Ok(graph) => {
                let node_ids: std::collections::BTreeSet<&str> =
                    graph.nodes.iter().map(|n| n.id.as_str()).collect();
                let refs = index
                    .articles
                    .iter()
                    .flat_map(|article| {
                        article.memory_refs.iter().filter_map(|ref_id| {
                            if !node_ids.contains(ref_id.as_str()) {
                                Some(serde_json::json!({
                                    "articleId": article.id,
                                    "missingRef": ref_id,
                                }))
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
                (true, refs)
            }
            Err(_) => (false, vec![]),
        };

    // Compute coverage gaps
    let coverage_gaps = if let Some(ref p) = pack {
        let cov = compute_coverage(&index, p);
        serde_json::json!({
            "uncoveredFiles": cov.uncovered_files,
            "totalFiles": cov.total_files,
            "coveragePercent": (cov.coverage_percent * 10.0).round() / 10.0,
        })
    } else {
        serde_json::json!(null)
    };

    // Save updated index (staleness flags may have changed)
    let now = OffsetDateTime::now_utc();
    index.updated_at = now;
    if let Err(e) = WikiStore::save_index(root, &index) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_lint: save index error");
        return err_json(&format!("Failed to save wiki index: {e}"));
    }

    // Append log entry
    let log_entry = WikiLogEntry {
        timestamp: now,
        event: WikiEventKind::Lint,
        article_id: None,
        detail: format!(
            "stale={}, brokenRefs={}",
            stale_articles.len(),
            broken_memory_refs.len()
        ),
    };
    let _ = WikiStore::append_log(root, log_entry);

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        stale = stale_articles.len(),
        broken_refs = broken_memory_refs.len(),
        pack_available,
        memory_available,
        "wiki_lint: exit"
    );
    let mut layers = vec![KnowledgeLayer::Interpretation];
    let mut prov = vec![Provenance::WikiIndex {
        source: "wiki-index.json".into(),
        article_ids: vec![],
    }];
    if memory_available {
        prov.push(Provenance::MemoryStore {
            source: "memory.json".into(),
            node_ids: vec![],
        });
    }
    if let Some((ref pack_dir, _)) = resolved_pack {
        layers.insert(0, KnowledgeLayer::Observation);
        prov.push(Provenance::PackExtraction {
            pack_id: pack_id_from_dir(pack_dir),
            source: "multiple".into(),
        });
    }
    ok_epistemic(
        KnowledgeLayerField::Multiple(layers),
        EpistemicConfidence::Variable,
        ProvenanceField::Multiple(prov),
        serde_json::json!({
            "staleArticles": stale_articles,
            "brokenMemoryRefs": broken_memory_refs,
            "coverageGaps": coverage_gaps,
            "packAvailable": pack_available,
            "memoryAvailable": memory_available,
        }),
    )
}

pub fn handle_wiki_delete(params: WikiDeleteParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, id = %params.id, "wiki_delete: entry");

    let root = Path::new(&params.root);
    let confirm = params.confirm.unwrap_or(false);

    let mut index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_delete: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    let article = match index.articles.iter().find(|a| a.id == params.id) {
        Some(a) => a.clone(),
        None => {
            info!(elapsed_ms = %start.elapsed().as_millis(), id = %params.id, "wiki_delete: article not found");
            return err_json(&format!("Article not found: {}", params.id));
        }
    };

    if !confirm {
        // Dry run — return article metadata
        let metadata = ArticleMetadata::from_article(&article);
        info!(
            elapsed_ms = %start.elapsed().as_millis(),
            id = %params.id,
            "wiki_delete: dry_run exit"
        );
        return ok_epistemic(
            KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
            EpistemicConfidence::Variable,
            ProvenanceField::Single(Provenance::WikiArticle {
                article_id: params.id.clone(),
                staleness: None,
            }),
            serde_json::json!({
                "status": "dry_run",
                "article": metadata,
            }),
        );
    }

    // Actually delete
    if let Err(e) = WikiStore::delete_article(root, &mut index, &params.id) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_delete: delete error");
        return err_json(&format!("Failed to delete article: {e}"));
    }

    let now = OffsetDateTime::now_utc();
    index.updated_at = now;
    if let Err(e) = WikiStore::save_index(root, &index) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_delete: save index error");
        return err_json(&format!("Failed to save wiki index: {e}"));
    }

    // Remove from tantivy search index
    if let Ok(tantivy) = repolens_wiki::index::WikiSearchIndex::open(root) {
        let _ = tantivy.remove(&params.id);
    }

    // Append log entry
    let log_entry = WikiLogEntry {
        timestamp: now,
        event: WikiEventKind::Delete,
        article_id: Some(params.id.clone()),
        detail: format!("Deleted: {}", article.title),
    };
    let _ = WikiStore::append_log(root, log_entry);

    let metadata = ArticleMetadata::from_article(&article);

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        id = %params.id,
        "wiki_delete: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::WikiArticle {
            article_id: params.id.clone(),
            staleness: None,
        }),
        serde_json::json!({
            "status": "deleted",
            "article": metadata,
        }),
    )
}

pub fn handle_wiki_suggest(
    resolver: &PackResolver,
    params: WikiSuggestParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, "wiki_suggest: entry");

    let root = Path::new(&params.root);

    // Load wiki index
    let wiki_index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_suggest: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    // Resolve pack — required
    let (pack_dir, pack) = match resolver.resolve(None, Some(&params.root)) {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_suggest: resolve error");
            return err_json(&e.to_string());
        }
    };

    // Load graph from pack_dir/graph.json
    let graph_path = pack_dir.join("graph.json");
    let graph: FileGraph = match std::fs::read_to_string(&graph_path)
        .ok()
        .and_then(|s| serde_json::from_str::<FileGraph>(&s).ok())
    {
        Some(g) => g,
        None => {
            info!(elapsed_ms = %start.elapsed().as_millis(), "wiki_suggest: graph.json missing or invalid");
            return err_json("graph.json is missing or invalid");
        }
    };

    let limit = params.limit.unwrap_or(10);
    let directory = params.directory.as_deref();
    let min_importance = params.min_importance;

    let result = repolens_app::wiki_suggest::suggest(
        &wiki_index,
        &pack,
        &graph,
        directory,
        min_importance,
        limit,
    );

    let suggestions: Vec<serde_json::Value> = result
        .suggestions
        .iter()
        .map(|s| {
            let type_str = s.suggested_type.as_str();
            serde_json::json!({
                "filePath": s.file_path,
                "importance": (s.importance * 100.0).round() / 100.0,
                "reason": s.reason,
                "suggestedType": type_str,
                "suggestedTitle": s.suggested_title,
                "symbols": s.symbols,
            })
        })
        .collect();

    let coverage = serde_json::json!({
        "uncoveredFiles": result.coverage.uncovered_files,
        "totalFiles": result.coverage.total_files,
        "coveragePercent": (result.coverage.coverage_percent * 10.0).round() / 10.0,
    });

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        suggestions = suggestions.len(),
        "wiki_suggest: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Multiple(vec![
            KnowledgeLayer::Observation,
            KnowledgeLayer::Interpretation,
        ]),
        EpistemicConfidence::Variable,
        ProvenanceField::Multiple(vec![
            Provenance::WikiIndex {
                source: "wiki-index.json".into(),
                article_ids: vec![],
            },
            Provenance::PackExtraction {
                pack_id: resolver
                    .resolve(None, Some(&params.root))
                    .ok()
                    .map(|(dir, _)| pack_id_from_dir(&dir))
                    .unwrap_or_default(),
                source: "multiple".into(),
            },
        ]),
        serde_json::json!({
            "suggestions": suggestions,
            "coverage": coverage,
        }),
    )
}

pub fn handle_wiki_ingest_external(
    resolver: &PackResolver,
    params: WikiIngestExternalParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, title = %params.title, "wiki_ingest_external: entry");

    let root = Path::new(&params.root);

    // Parse article type
    let article_type = match parse_article_type(&params.article_type) {
        Some(t) => t,
        None => {
            info!(elapsed_ms = %start.elapsed().as_millis(), article_type = %params.article_type, "wiki_ingest_external: invalid articleType");
            return err_json(&format!(
                "Invalid articleType: {}. Must be ENTITY, CONCEPT, OVERVIEW, or COMPARISON",
                params.article_type
            ));
        }
    };

    // Load index
    let mut index = match WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_ingest_external: load index error");
            return err_json(&format!("Failed to load wiki index: {e}"));
        }
    };

    // Normalize tags
    let tags: Vec<String> = params
        .tags
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let anchors = params.anchors.unwrap_or_default();
    let memory_refs = params.memory_refs.unwrap_or_default();
    let now = OffsetDateTime::now_utc();

    // Build ExternalSource
    let source = ExternalSource {
        origin: params.source_origin.clone(),
        version: params.source_version.clone(),
        fetched_at: now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    };

    let (article, old_path, status) = if let Some(ref id) = params.id {
        // Update existing article
        match index.articles.iter().position(|a| a.id == *id) {
            Some(pos) => {
                let existing = &index.articles[pos];
                let old_path = existing.path.clone();

                // Compute new path if title or type changed
                let existing_paths: Vec<String> = index
                    .articles
                    .iter()
                    .filter(|a| a.id != *id)
                    .map(|a| a.path.clone())
                    .collect();
                let new_path =
                    if existing.title != params.title || existing.article_type != article_type {
                        compute_article_path(&article_type, &params.title, &existing_paths)
                    } else {
                        old_path.clone()
                    };

                let confidence = params.confidence.unwrap_or(existing.confidence);
                let created_at = existing.created_at;

                let article = WikiArticle {
                    id: id.clone(),
                    title: params.title.clone(),
                    article_type: article_type.clone(),
                    path: new_path,
                    anchors,
                    memory_refs,
                    tags,
                    confidence,
                    staleness: Staleness::Fresh,
                    summary: params.summary.clone(),
                    created_at,
                    updated_at: now,
                    source: Some(source.clone()),
                };
                (article, Some(old_path), "updated")
            }
            None => {
                info!(elapsed_ms = %start.elapsed().as_millis(), id = %id, "wiki_ingest_external: article id not found");
                return err_json(&format!("Article not found: {id}"));
            }
        }
    } else {
        // Create new article
        let id = Uuid::new_v4().to_string();
        let existing_paths: Vec<String> = index.articles.iter().map(|a| a.path.clone()).collect();
        let path = compute_article_path(&article_type, &params.title, &existing_paths);
        let confidence = params.confidence.unwrap_or(0.6);

        let article = WikiArticle {
            id: id.clone(),
            title: params.title.clone(),
            article_type: article_type.clone(),
            path,
            anchors,
            memory_refs,
            tags,
            confidence,
            staleness: Staleness::Fresh,
            summary: params.summary.clone(),
            created_at: now,
            updated_at: now,
            source: Some(source.clone()),
        };
        (article, None, "created")
    };

    // Resolve pack for staleness assessment
    let pack = resolver
        .resolve(None, Some(&params.root))
        .ok()
        .map(|(_, idx)| idx);

    // Assess staleness
    let staleness = assess_staleness(&article, pack.as_ref());
    let staleness_label = staleness_str(&staleness).to_string();

    let article = WikiArticle {
        staleness,
        ..article
    };

    // Ensure gitignore
    let _ = WikiStore::ensure_gitignore(root);

    // Write article to disk
    let article_path = article.path.clone();
    let article_id = article.id.clone();

    if let Err(e) = WikiStore::write_article(root, &article, &params.body) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_ingest_external: write error");
        return err_json(&format!("Failed to write article: {e}"));
    }

    // Update index
    if let Some(pos) = index.articles.iter().position(|a| a.id == article_id) {
        index.articles[pos] = article.clone();
    } else {
        index.articles.push(article.clone());
    }
    index.updated_at = now;

    if let Err(e) = WikiStore::save_index(root, &index) {
        info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "wiki_ingest_external: save index error");
        return err_json(&format!("Failed to save wiki index: {e}"));
    }

    // Upsert into tantivy search index
    if let Ok(tantivy) = repolens_wiki::index::WikiSearchIndex::open(root) {
        let _ = tantivy.upsert(&article, &params.body);
    }

    // Delete old file if path changed
    if let Some(ref old) = old_path {
        if *old != article_path {
            let old_file = root.join(".repolens").join("wiki").join(old);
            let _ = std::fs::remove_file(&old_file);
        }
    }

    // Append log entry
    let event = if status == "created" {
        WikiEventKind::Compile
    } else {
        WikiEventKind::Update
    };
    let log_entry = WikiLogEntry {
        timestamp: now,
        event,
        article_id: Some(article_id.clone()),
        detail: format!("(external) {status}: {}", params.title),
    };
    let _ = WikiStore::append_log(root, log_entry);

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        id = %article_id,
        status,
        "wiki_ingest_external: exit"
    );
    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Interpretation),
        EpistemicConfidence::Variable,
        ProvenanceField::Single(Provenance::WikiArticle {
            article_id: article_id.clone(),
            staleness: Some(staleness_label.clone()),
        }),
        serde_json::json!({
            "status": status,
            "id": article_id,
            "path": article_path,
            "staleness": staleness_label,
            "source": {
                "origin": source.origin,
                "version": source.version,
                "fetchedAt": source.fetched_at,
            },
        }),
    )
}
