use serde::{Deserialize, Serialize};

use super::health::HealthReport;
use super::profile::ProjectProfile;
use super::session::RepoSession;

/// Scan statistics summarised for reporting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub mode: String,
    pub file_count: i32,
    pub included_count: i32,
    pub total_loc: i64,
    pub test_file_count: i32,
    pub source_file_count: i32,
    pub test_to_source_ratio: f64,
}

/// A file node annotated with graph degree information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedFile {
    pub path: String,
    pub in_degree: i32,
    pub out_degree: i32,
}

/// High-level graph topology metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSummary {
    pub node_count: i32,
    pub edge_count: i32,
    pub top_connected_files: Vec<ConnectedFile>,
}

/// A notable symbol with its file and connection count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSummary {
    pub name: String,
    #[serde(rename = "type")]
    pub symbol_type: String,
    pub file: String,
    pub file_connections: i32,
}

/// A notable file with language, LOC, and connection count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub path: String,
    pub language: String,
    pub loc: i32,
    pub connections: i32,
}

/// A summarised git commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitSummary {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub files_changed: i32,
}

/// A single memory insight surfaced from the persistent memory graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInsight {
    pub id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub importance: f32,
    pub last_accessed: String,
    pub access_count: i32,
    pub anchors: Vec<String>,
    pub tags: Vec<String>,
    /// Computed grounding status for this memory against the active pack.
    /// Defaults to `Unchecked` for memories with no groundable anchors or
    /// when constructed by callers that do not perform grounding.
    pub grounding_status: crate::model::memory::GroundingStatus,
}

/// Full repo summary composed by `SummaryService`.
///
/// New consumers should read `evidence_findings`, which carries the four-part
/// calibration block (observation, responsible inference, limitations,
/// action) per finding. The former `recommendations` field was retired in the
/// 2026-05-13 Phase 4B Step 3 amendment — its rules either had calibrated
/// evidence-finding replacements (high-coupling-file → F-HIGH-FAN-IN-FILE,
/// dominant-module → F-DOMINANT-MODULE, no-lockfile → F-CARGO-LOCK-STATE /
/// F-LOCKFILE-MISSING-NON-CARGO, low-test-ratio → F-TEST-INVENTORY-RATIO,
/// dependency-cycles → F-DEPENDENCY-CYCLES) or were dropped outright as
/// non-actionable (large-file: an agent reading a file already sees its LOC;
/// low-resolution-rate: tool-coverage transparency, not a repo finding).
///
/// The former `fitness_score` field was retired in the 2026-05-11 Phase 4B
/// Step 1 amendment — a single composite number collapsed all observable
/// structure into one score that agents would treat as ground truth, which
/// is the canonical "outrun the evidence" pattern the trust-calibration
/// spec exists to prevent. See
/// `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` §"Phase 4
/// amendments (Part B Step 1 — retire fitnessScore)".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSummary {
    pub schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<RepoSession>,
    pub scan: ScanSummary,
    pub profile: ProjectProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_summary: Option<GraphSummary>,
    pub top_symbols: Vec<SymbolSummary>,
    pub top_files: Vec<FileSummary>,
    /// Repo-wide symbol inventory: the count of every symbol extracted across
    /// the pack's included files. Distinct from `top_symbols.len()`, which is
    /// the top-N ranked preview capped at 20. Agents needing a symbol count
    /// must read this — `top_symbols` is a ranked sample, not an inventory.
    /// `default` keeps deserialization of pre-existing summaries working.
    #[serde(default)]
    pub total_symbols: i32,
    pub recent_commits: Vec<CommitSummary>,
    pub memory_insights: Vec<MemoryInsight>,
    /// Evidence-aware findings — the calibrated replacement for the retired
    /// `recommendations` and `health.signals` fields.
    ///
    /// Always serialized (empty array when no findings emitted) so the
    /// calibration block's `preferred: "evidenceFindings"` claim is never
    /// dangling. `default` keeps deserialization of pre-Phase-1 packs
    /// (without the field) working.
    #[serde(default)]
    pub evidence_findings: Vec<crate::model::evidence::EvidenceFinding>,
}

/// Comparison of two repo summaries over time.
///
/// `metrics` carries raw counts (file/loc/graph-node/edge deltas) that are
/// catalogue-backed observations. Consumers should prefer `evidence_findings`,
/// which emits per-metric delta findings with explicit before/after values,
/// thresholds, and `does_not_imply` non-implications.
///
/// The former `recommendations: Vec<RecommendationDelta>` field was retired in
/// the 2026-05-13 Phase 4B Step 3 amendment alongside the source `Recommendation`
/// type — the delta findings (F-DELTA-*) already calibrate the same metric
/// changes that drove recommendation add/remove events.
///
/// The former `before_fitness` / `after_fitness` / `delta_fitness` fields
/// were retired in the 2026-05-11 Phase 4B Step 1 amendment along with the
/// `FitnessScore` type itself — see `RepoSummary` doc comment for the
/// rationale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryComparison {
    pub metrics: Vec<MetricDelta>,
    /// Evidence-aware delta findings — the calibrated replacement for the
    /// retired `recommendations` delta field. Empty when no delta crosses a
    /// threshold (e.g., the LOC delta sits below the 5% F-DELTA-TOTAL-LOC
    /// threshold).
    ///
    /// Always serialized (empty array when none emitted) so the calibration
    /// block's `preferred: "evidenceFindings"` claim is never dangling.
    #[serde(default)]
    pub evidence_findings: Vec<crate::model::evidence::EvidenceFinding>,
}

/// A before/after delta for a single metric.
///
/// The former `trend: String` field was retired in the 2026-05-13 Phase 4B
/// Step 4 amendment. Its `improved`/`degraded`/`unchanged` labels were
/// excluded-tier interpretations — e.g., a LOC reduction was labelled
/// `degraded`, which inverts the intent of most refactor diffs. Calibrated
/// delta findings (`F-DELTA-*`) carry per-metric directionality with explicit
/// thresholds; consumers that need a directional read-out should derive it
/// locally from `before`/`after` instead of relying on a label that does not
/// know whether the metric is direction-aware.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDelta {
    pub name: String,
    pub before: String,
    pub after: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_insight_grounding_status_serializes_as_camel_key_screaming_value() {
        use crate::model::memory::GroundingStatus;

        let insight = MemoryInsight {
            id: "mem-1".to_string(),
            content: "prefer explicit error types".to_string(),
            memory_type: "Convention".to_string(),
            importance: 0.8,
            last_accessed: "2026-04-18T00:00:00Z".to_string(),
            access_count: 3,
            anchors: vec!["src/lib.rs".to_string()],
            tags: vec!["rust".to_string()],
            grounding_status: GroundingStatus::Confirmed,
        };
        let json = serde_json::to_string(&insight).expect("serialization failed");
        assert!(
            json.contains("\"groundingStatus\":\"CONFIRMED\""),
            "expected camelCase key with SCREAMING_SNAKE_CASE value, got: {json}"
        );
    }

    #[test]
    fn memory_insight_grounding_status_round_trip() {
        use crate::model::memory::GroundingStatus;

        let insight = MemoryInsight {
            id: "mem-2".to_string(),
            content: "modules must be declared in mod.rs".to_string(),
            memory_type: "Convention".to_string(),
            importance: 0.6,
            last_accessed: "2026-04-18T00:00:00Z".to_string(),
            access_count: 1,
            anchors: vec![],
            tags: vec![],
            grounding_status: GroundingStatus::Unchecked,
        };
        let json = serde_json::to_string(&insight).expect("serialization failed");
        let restored: MemoryInsight = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(insight, restored);
        assert_eq!(restored.grounding_status, GroundingStatus::Unchecked);
    }

    #[test]
    fn metric_delta_round_trip() {
        let delta = MetricDelta {
            name: "totalLoc".to_string(),
            before: "10000".to_string(),
            after: "12000".to_string(),
        };
        let json = serde_json::to_string(&delta).expect("serialization failed");
        let restored: MetricDelta = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(delta, restored);
    }
}
