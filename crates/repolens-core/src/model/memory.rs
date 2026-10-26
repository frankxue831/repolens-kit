use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::serde_util::iso8601;

/// Category of a memory node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryType {
    CodeAnchor,
    Decision,
    Convention,
    TradeOff,
    Pattern,
}

/// How a memory node was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemorySource {
    Explicit,
    ScanTriggered,
}

/// Typed relationship between two memory nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemoryRelation {
    Supersedes,
    RelatesTo,
    Contradicts,
    GroundedBy,
    ContradictedBy,
}

/// Grounding status for a memory node assessed against an active pack.
///
/// Computed live on recall by `repolens_app::grounding`. A snapshot of the
/// most recent assessment may also be persisted as
/// [`MemoryNode::last_grounding`] so the decay engine can slow decay for
/// `Confirmed` memories and accelerate it for `Contradicted` ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GroundingStatus {
    Confirmed,
    Contradicted,
    Unverifiable,
    Unchecked,
}

/// A single memory entry in the memory graph.
///
/// Mirrors the Java `MemoryNode` record stored in `.repolens/memory/memory.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryNode {
    pub id: String,
    pub content: String,

    #[serde(rename = "type")]
    pub memory_type: MemoryType,

    pub importance: f32,

    #[serde(with = "iso8601::required")]
    pub created: OffsetDateTime,

    #[serde(with = "iso8601::required")]
    pub last_accessed: OffsetDateTime,

    pub access_count: i32,

    #[serde(default)]
    pub anchors: Vec<String>,

    #[serde(default)]
    pub tags: Vec<String>,

    pub source: MemorySource,

    pub provenance: String,

    /// Grounding status as of the most recent recall that assessed this node.
    /// A persisted snapshot of the otherwise compute-on-recall
    /// [`GroundingStatus`], read by the decay engine. `None` = never grounded.
    #[serde(default)]
    pub last_grounding: Option<GroundingStatus>,

    /// When `last_grounding` was observed (provenance). Not read by decay today;
    /// recorded for staleness reasoning and future on-scan reuse.
    #[serde(default, with = "iso8601::optional")]
    pub last_grounded_at: Option<OffsetDateTime>,

    /// When decay was last applied to this node. Decay measures elapsed time
    /// from `max(last_accessed, last_decayed_at)` so repeated runs do not
    /// re-apply time that was already decayed. Absent in memory.json files
    /// written before this field existed (`None`); the decay engine then falls
    /// back to the graph-level `last_decay_run`, see
    /// `MemoryDecayEngine::decay`.
    #[serde(
        default,
        with = "iso8601::optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_decayed_at: Option<OffsetDateTime>,

    /// What `importance` would be without the CONTRADICTED grounding
    /// penalty, tracked by the decay engine alongside it (same time decay,
    /// edge multipliers and floors). `None` when the two are equal, which
    /// includes every node the penalty never touched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance_before_grounding: Option<f32>,

    /// Content hash (the pack's `sha256`) of each file anchor, from the
    /// latest scan when the note was saved. Empty for notes saved before
    /// this existed or without a scan; recall then says nothing about
    /// changes.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub anchor_sha256: std::collections::BTreeMap<String, String>,
}

/// A directed edge between two memory nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: MemoryRelation,
}

/// The full memory graph stored in `.repolens/memory/memory.json`.
///
/// Mirrors the Java `MemoryGraph` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGraph {
    pub schema_version: String,
    pub repo_root: String,

    #[serde(default)]
    pub nodes: Vec<MemoryNode>,

    #[serde(default)]
    pub edges: Vec<MemoryEdge>,

    #[serde(with = "iso8601::optional")]
    pub last_decay_run: Option<OffsetDateTime>,

    #[serde(with = "iso8601::required")]
    pub updated_at: OffsetDateTime,
}
