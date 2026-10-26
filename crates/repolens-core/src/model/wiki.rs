use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::serde_util::iso8601;

/// Category of a wiki article.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArticleType {
    Entity,
    Concept,
    Overview,
    Comparison,
}

impl ArticleType {
    /// Returns the lowercase directory name for this article type.
    pub fn dir_name(&self) -> &'static str {
        match self {
            ArticleType::Entity => "entity",
            ArticleType::Concept => "concept",
            ArticleType::Overview => "overview",
            ArticleType::Comparison => "comparison",
        }
    }

    /// Returns the SCREAMING_SNAKE_CASE wire representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            ArticleType::Entity => "ENTITY",
            ArticleType::Concept => "CONCEPT",
            ArticleType::Overview => "OVERVIEW",
            ArticleType::Comparison => "COMPARISON",
        }
    }
}

/// Freshness state of a wiki article relative to the current codebase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Staleness {
    Fresh,
    Stale,
    Unverifiable,
}

/// Source attribution for externally-sourced wiki articles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSource {
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub fetched_at: String,
}

/// A single wiki article entry tracked in the wiki index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiArticle {
    pub id: String,
    pub title: String,
    pub article_type: ArticleType,
    pub path: String,

    #[serde(default)]
    pub anchors: Vec<String>,

    #[serde(default)]
    pub memory_refs: Vec<String>,

    #[serde(default)]
    pub tags: Vec<String>,

    pub confidence: f32,
    pub staleness: Staleness,
    pub summary: String,

    #[serde(with = "iso8601::required")]
    pub created_at: OffsetDateTime,

    #[serde(with = "iso8601::required")]
    pub updated_at: OffsetDateTime,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub source: Option<ExternalSource>,
}

/// The wiki index stored in `.repolens/wiki/index.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiIndex {
    pub schema_version: String,

    #[serde(with = "iso8601::required")]
    pub updated_at: OffsetDateTime,

    #[serde(default)]
    pub articles: Vec<WikiArticle>,
}

/// The kind of event recorded in the wiki log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WikiEventKind {
    Compile,
    Update,
    StaleFlag,
    Lint,
    Delete,
}

/// A single entry in the wiki event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiLogEntry {
    #[serde(with = "iso8601::required")]
    pub timestamp: OffsetDateTime,

    pub event: WikiEventKind,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub article_id: Option<String>,

    pub detail: String,
}

/// The wiki event log stored in `.repolens/wiki/log.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiLog {
    #[serde(default)]
    pub entries: Vec<WikiLogEntry>,
}
