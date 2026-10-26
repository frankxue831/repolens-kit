//! Evidence-aware finding model.
//!
//! Implements the four-part calibration shape required by the trust-calibration
//! design (`docs/superpowers/specs/2026-05-11-evidence-aware-findings.md`):
//! every finding separately exposes
//!
//! 1. `evidence`     — what was observed
//! 2. `inference`    — what can be responsibly inferred
//! 3. `limitations`  — what the evidence does not cover
//! 4. `action`       — what should happen next
//!
//! The model is additive to existing `Signal`/`Recommendation`/`FitnessScore`
//! types. Legacy fields remain for compatibility; new code emits both during
//! Phase 1 and consumers should prefer `evidenceFindings` when present.

use serde::{Deserialize, Serialize};

use super::epistemic::{EpistemicConfidence, EvidenceGrade, KnowledgeLayer, Provenance};

// ---------------------------------------------------------------------------
// Top-level finding
// ---------------------------------------------------------------------------

/// Stable identifier for a class of finding (e.g. "F-NO-ENTRY-POINTS").
/// String-newtype so it is greppable across the codebase and snapshot tests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FindingId(pub String);

impl FindingId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single, evidence-tagged finding emitted by health, summary, or future
/// triage layers. New code emits this; legacy `Signal` and `Recommendation`
/// remain for back-compat (lower calibration; see `RepoSummary` doc comment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceFinding {
    pub id: FindingId,
    /// Observation-shaped title (no advice). Avoid words like
    /// "warning", "risk", "quality", or imperatives.
    pub title: String,
    pub classification: FindingClassification,
    pub severity: FindingSeverity,
    pub knowledge_layer: KnowledgeLayer,
    pub evidence_grade: EvidenceGrade,
    pub confidence: EpistemicConfidence,

    // ---- Four-part calibration block — always populated. -------------------
    /// (1) What was observed. Must be non-empty for any non-trivial finding.
    pub evidence: Vec<EvidenceItem>,
    /// (2) What can responsibly be inferred from the evidence.
    pub inference: ResponsibleInference,
    /// (3) What the evidence does not cover (relative to this finding).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<Limitation>,
    /// (4) What should happen next.
    pub action: RecommendedAction,
    // ------------------------------------------------------------------------
    pub scope: EvidenceScope,
    /// "What this finding did not check." Distinct from `limitations`, which
    /// caveats the evidence; `not_checked` lists deliberately-skipped scopes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_checked: Vec<NotCheckedArea>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repo_context: Vec<RepoContextTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub false_positive_notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

// ---------------------------------------------------------------------------
// Classification + severity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingClassification {
    Actionable,
    Expected,
    Informational,
    NotChecked,
    NeedsExternalVerification,
    SuppressedByRepoProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingSeverity {
    None,
    Low,
    Medium,
    High,
}

// ---------------------------------------------------------------------------
// Evidence items — structured, not stringly-typed
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum EvidenceItem {
    /// A counted/structural observation, ideally backed by an invariant
    /// family (doc 06 §5).
    Observation {
        /// Stable metric name from `metric_name` constants below. Greppable
        /// across repos.
        metric: String,
        observed_value: serde_json::Value,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        source_paths: Vec<String>,
        source_type: EvidenceSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invariant_id: Option<String>,
    },
    /// A threshold judgment applied to an observation. Always carries the
    /// rule, observed value, and threshold so the consumer can reason about
    /// whether the threshold is meaningful for their repo profile.
    HeuristicJudgment {
        /// Rule expression of the form "{metric} {op} {threshold}", e.g.
        /// "structure.dominantModuleShare > 0.8".
        rule: String,
        observed_metric: String,
        observed_value: serde_json::Value,
        threshold: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    /// External signal we read but did not validate (manifests, lockfiles,
    /// policy docs, CI config, root docs).
    ExternalSignal {
        path: String,
        signal_kind: ExternalSignalKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        excerpt: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum EvidenceSource {
    ImportGraph,
    FileInventory,
    SymbolExtraction { language: String },
    ProjectProfile,
    GraphMetrics,
    PolicyDocument { path: String },
    Manifest { path: String },
    ExistingSummary,
    ExternalCommandDeclaredButNotRun { command: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalSignalKind {
    Manifest,
    Lockfile,
    PolicyDoc,
    RootDoc,
    CiConfig,
}

// ---------------------------------------------------------------------------
// Responsible inference (separate from action)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponsibleInference {
    /// Narrowest claim the evidence supports. Must not exceed the evidence.
    pub statement: String,
    pub strength: InferenceStrength,
    /// Codes for thresholds, invariants, or repo-shape facts the inference
    /// rests on. Greppable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Explicit non-implications. The anti-overclaim guardrail. Mandatory
    /// for any finding whose action is `NoAction`; advisable elsewhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub does_not_imply: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceStrength {
    None,
    Suggestive,
    Strong,
}

// ---------------------------------------------------------------------------
// Scope, limitations, not-checked
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceScope {
    /// File or module paths the evidence covers. Empty means "whole repo".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    pub time: ScopeTime,
    /// Languages whose extractors saw the underlying data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeTime {
    pub pack_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limitation {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotCheckedArea {
    pub code: String,
    pub detail: String,
    pub trigger: NotCheckedTrigger,
}

/// Whether a `NotCheckedArea` should appear on every report or only when
/// repo context indicators warrant it. Crypto/timing/no-std caveats should
/// not appear on a CSV parser; release-readiness/test-pass-fail caveats
/// should appear on every report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum NotCheckedTrigger {
    Always,
    WhenIndicator { tag: RepoContextTag },
}

// ---------------------------------------------------------------------------
// Recommended action
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RecommendedAction {
    NoAction,
    ReviewIfMaintainerPain,
    ConcreteAction { steps: Vec<String> },
    ExternalVerification { commands_or_checks: Vec<String> },
}

// ---------------------------------------------------------------------------
// Repo context tags (project-shape + indicator language)
// ---------------------------------------------------------------------------

/// Tags describing what kind of repository we are looking at. Project-shape
/// variants (`RustLibrary`, `JavaService`, …) are *proven* from manifests;
/// `*Indicators` variants are *suggestive* and must be accompanied by a
/// `false_positive_notes` entry naming the literal evidence that matched.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RepoContextTag {
    // Project-shape (proven from manifests).
    RustLibrary,
    RustBinary,
    RustWorkspace,
    JavaLibrary,
    JavaService,
    JavaApplication,
    NodeLibrary,
    NodeApplication,
    GoModule,
    GoBinary,
    PythonPackage,
    PythonScript,

    // Indicator tags (suggestive — never asserted as proven).
    NoStdIndicators,
    CryptoIndicators,
    SecuritySensitiveIndicators,
    DatabaseIndicators,
    CompilerIndicators,
    InfrastructureIndicators,

    Custom { name: String },
}

impl RepoContextTag {
    /// True if the tag is in the indicator-only family, where reclassification
    /// based on this tag must always be auditable via `false_positive_notes`.
    pub fn is_indicator(&self) -> bool {
        matches!(
            self,
            Self::NoStdIndicators
                | Self::CryptoIndicators
                | Self::SecuritySensitiveIndicators
                | Self::DatabaseIndicators
                | Self::CompilerIndicators
                | Self::InfrastructureIndicators
        )
    }

    /// True if any of the tags is a Rust project-shape tag. Used to decide
    /// whether MSRV-style `not_checked` entries should fire.
    pub fn is_rust(&self) -> bool {
        matches!(
            self,
            Self::RustLibrary | Self::RustBinary | Self::RustWorkspace
        )
    }
}

// ---------------------------------------------------------------------------
// Cargo.lock state — explicit per amendment §H
// ---------------------------------------------------------------------------

/// Five reportable states for Cargo.lock detection. Rather than a boolean,
/// findings dispatch on `(CargoLockState, RepoContextTag)`.
///
/// Note: RepoLens never runs `cargo`; lockfile generation is out of scope
/// and is reflected via a permanent `LOCKFILE_GENERATION_OUT_OF_SCOPE`
/// `NotCheckedArea` rather than a `CargoLockState` variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum CargoLockState {
    /// File present in working tree and tracked by git.
    CommittedAndTracked { path: String },
    /// File not present in the working tree.
    AbsentFromRepo,
    /// File present in working tree but matched by `.gitignore`.
    PresentButGitIgnored { path: String },
    /// File present; tracking status not inspected by this scan.
    PresentTrackingUnknown { path: String },
}

// ---------------------------------------------------------------------------
// Stable metric names — single source of truth for `EvidenceItem::Observation`
// and `HeuristicJudgment` `metric`/`observed_metric` fields. Greppable; CI
// can enforce membership via `metric_names::ALL`.
// ---------------------------------------------------------------------------

/// Canonical metric names emitted by Phase 1 findings. New metrics MUST be
/// added here and referenced by constant; ad-hoc strings are not allowed.
pub mod metric_names {
    pub const PROFILE_ENTRY_POINT_COUNT: &str = "profile.entryPointCount";
    pub const PROFILE_PROJECT_TYPE: &str = "profile.projectType";
    pub const STRUCTURE_DOMINANT_MODULE_SHARE: &str = "structure.dominantModuleShare";
    pub const STRUCTURE_DOMINANT_MODULE: &str = "structure.dominantModule";
    pub const STRUCTURE_MODULE_COUNT: &str = "structure.moduleCount";
    pub const GRAPH_INBOUND_EDGES: &str = "graph.inboundEdges";
    pub const GRAPH_AVERAGE_INBOUND_EDGES: &str = "graph.averageInboundEdges";
    pub const GRAPH_ORPHAN_COUNT: &str = "graph.orphanCount";
    pub const GRAPH_ORPHAN_FILES: &str = "graph.orphanFiles";
    pub const GRAPH_ORPHAN_RATIO: &str = "graph.orphanRatio";
    pub const GRAPH_CYCLE_COUNT: &str = "graph.cycleCount";
    pub const GRAPH_CYCLE_FILES: &str = "graph.cycleFiles";
    pub const GRAPH_LARGEST_CYCLE_SIZE: &str = "graph.largestCycleSize";
    pub const SCAN_TEST_FILE_COUNT: &str = "scan.testFileCount";
    pub const SCAN_SOURCE_FILE_COUNT: &str = "scan.sourceFileCount";
    pub const SCAN_TEST_TO_SOURCE_FILE_RATIO: &str = "scan.testToSourceFileRatio";
    pub const PROFILE_LOCKFILE_STATE: &str = "profile.lockfileState";
    pub const PROFILE_BUILD_SYSTEM: &str = "profile.buildSystem";

    /// Per-fidelity-tier extraction breakdown for `F-LANGUAGE-EXTRACTION-GAP`:
    /// `{ ast|heuristic|unsupported: { loc, languages[] } }` over the languages
    /// present in the scan. Backed by the project profile's language
    /// distribution classified via `repolens_extract::extraction_fidelity`.
    pub const EXTRACTION_LANGUAGES_BY_FIDELITY: &str = "extraction.languagesByFidelity";

    // ---- Delta metrics for compare_summaries ------------------------------
    pub const DELTA_SCAN_TEST_TO_SOURCE_FILE_RATIO: &str = "delta.scan.testToSourceFileRatio";
    pub const DELTA_STRUCTURE_DOMINANT_MODULE_SHARE: &str = "delta.structure.dominantModuleShare";
    pub const DELTA_GRAPH_ORPHAN_COUNT: &str = "delta.graph.orphanCount";
    pub const DELTA_GRAPH_CYCLE_COUNT: &str = "delta.graph.cycleCount";
    pub const DELTA_SCAN_TOTAL_LOC: &str = "delta.scan.totalLoc";
    pub const DELTA_SCAN_FILE_COUNT: &str = "delta.scan.fileCount";
    pub const DELTA_SCAN_INCLUDED_COUNT: &str = "delta.scan.includedCount";

    /// All canonical metric names. CI test asserts every emitted observation
    /// metric is a member of this slice.
    pub const ALL: &[&str] = &[
        PROFILE_ENTRY_POINT_COUNT,
        PROFILE_PROJECT_TYPE,
        STRUCTURE_DOMINANT_MODULE_SHARE,
        STRUCTURE_DOMINANT_MODULE,
        STRUCTURE_MODULE_COUNT,
        GRAPH_INBOUND_EDGES,
        GRAPH_AVERAGE_INBOUND_EDGES,
        GRAPH_ORPHAN_COUNT,
        GRAPH_ORPHAN_FILES,
        GRAPH_ORPHAN_RATIO,
        GRAPH_CYCLE_COUNT,
        GRAPH_CYCLE_FILES,
        GRAPH_LARGEST_CYCLE_SIZE,
        SCAN_TEST_FILE_COUNT,
        SCAN_SOURCE_FILE_COUNT,
        SCAN_TEST_TO_SOURCE_FILE_RATIO,
        PROFILE_LOCKFILE_STATE,
        PROFILE_BUILD_SYSTEM,
        EXTRACTION_LANGUAGES_BY_FIDELITY,
        DELTA_SCAN_TEST_TO_SOURCE_FILE_RATIO,
        DELTA_STRUCTURE_DOMINANT_MODULE_SHARE,
        DELTA_GRAPH_ORPHAN_COUNT,
        DELTA_GRAPH_CYCLE_COUNT,
        DELTA_SCAN_TOTAL_LOC,
        DELTA_SCAN_FILE_COUNT,
        DELTA_SCAN_INCLUDED_COUNT,
    ];

    pub fn is_known(metric: &str) -> bool {
        ALL.contains(&metric)
    }
}

// ---------------------------------------------------------------------------
// Stable codes for limitations + not-checked areas. Same governance as
// metric names: emitted codes must be members of these tables.
// ---------------------------------------------------------------------------

pub mod limitation_codes {
    pub const INLINE_RUST_TESTS_NOT_COUNTED: &str = "INLINE_RUST_TESTS_NOT_COUNTED";
    pub const TEST_PASS_FAIL_NOT_RUN: &str = "TEST_PASS_FAIL_NOT_RUN";
    pub const DOCS_AND_CONFIG_NOT_TREATED_AS_SOURCE: &str = "DOCS_AND_CONFIG_NOT_TREATED_AS_SOURCE";
    pub const ENTRY_POINT_DETECTION_LANGUAGE_LIMITED: &str =
        "ENTRY_POINT_DETECTION_LANGUAGE_LIMITED";
    pub const FAN_IN_THRESHOLD_HEURISTIC: &str = "FAN_IN_THRESHOLD_HEURISTIC";
    /// F-HIGH-FAN-IN-FILE caps the number of per-file findings it emits to
    /// keep the response from flooding the agent's context. When the cap
    /// trips, every emitted finding carries this limitation so the agent
    /// knows additional files exceeded the threshold but were not reported
    /// individually.
    pub const FAN_IN_FINDINGS_CAPPED: &str = "FAN_IN_FINDINGS_CAPPED";
    pub const DOMINANT_MODULE_THRESHOLD_HEURISTIC: &str = "DOMINANT_MODULE_THRESHOLD_HEURISTIC";
    pub const LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL: &str =
        "LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL";
    pub const DELTA_REFLECTS_PACK_BOUNDARIES_NOT_COMMIT_DELTAS: &str =
        "DELTA_REFLECTS_PACK_BOUNDARIES_NOT_COMMIT_DELTAS";
    pub const DELTA_THRESHOLD_HEURISTIC: &str = "DELTA_THRESHOLD_HEURISTIC";
    /// Some findings consult the live working tree (e.g. Cargo.toml dep
    /// names, Cargo.lock presence) rather than pack-stored content. If
    /// the worktree changed between the pack scan and the report build
    /// time, those findings can disagree with the rest of the summary
    /// (which reflects the pack).
    pub const LIVE_ROOT_VS_PACK_TIME_DIVERGENCE_POSSIBLE: &str =
        "LIVE_ROOT_VS_PACK_TIME_DIVERGENCE_POSSIBLE";
    /// Import-graph findings reflect structural import edges only. Build-time,
    /// type-level, or runtime relationships that the import graph does not
    /// represent are out of scope (e.g., reflection, plugin loading, dynamic
    /// dispatch, macro expansion that materialises calls into non-importing
    /// modules). Used by F-DEPENDENCY-CYCLES.
    pub const IMPORT_GRAPH_STRUCTURAL_ONLY: &str = "IMPORT_GRAPH_STRUCTURAL_ONLY";
    /// RepoLens extracts symbols/relationships only for a fixed set of
    /// languages (Java, Rust, Go, JavaScript, TypeScript via full parsing;
    /// Python via regex). Files in other recognized programming languages are
    /// inventoried and appear as graph nodes but contribute no symbols and no
    /// import edges. Used by F-LANGUAGE-EXTRACTION-GAP.
    pub const SYMBOL_EXTRACTION_LANGUAGE_LIMITED: &str = "SYMBOL_EXTRACTION_LANGUAGE_LIMITED";
}

pub mod not_checked_codes {
    // Always-shown.
    pub const RUNTIME_BEHAVIOR: &str = "RUNTIME_BEHAVIOR";
    pub const TEST_PASS_FAIL: &str = "TEST_PASS_FAIL";
    pub const RELEASE_READINESS: &str = "RELEASE_READINESS";
    pub const SECURITY_AUDIT_GENERIC: &str = "SECURITY_AUDIT_GENERIC";
    pub const LICENSE_COMPATIBILITY: &str = "LICENSE_COMPATIBILITY";
    pub const EXTERNAL_DEPENDENCY_VERSIONS: &str = "EXTERNAL_DEPENDENCY_VERSIONS";
    pub const LOCKFILE_FRESHNESS: &str = "LOCKFILE_FRESHNESS";
    pub const LOCKFILE_GENERATION_OUT_OF_SCOPE: &str = "LOCKFILE_GENERATION_OUT_OF_SCOPE";
    pub const CHANGE_SAFETY: &str = "CHANGE_SAFETY";
    /// Manifest dependency names and lockfile state are read from the live
    /// working tree, not the pack snapshot. When summarizing or comparing
    /// older packs, indicator tags driven by manifest deps may diverge
    /// from what was actually scanned.
    pub const MANIFEST_AND_LOCKFILE_LIVE_ROOT_DERIVED: &str =
        "MANIFEST_AND_LOCKFILE_LIVE_ROOT_DERIVED";

    // Indicator-conditional.
    pub const CRYPTO_CORRECTNESS: &str = "CRYPTO_CORRECTNESS";
    pub const CONSTANT_TIME: &str = "CONSTANT_TIME";
    pub const TIMING_LEAKAGE: &str = "TIMING_LEAKAGE";
    pub const FUZZING_RESULTS: &str = "FUZZING_RESULTS";
    pub const MSRV: &str = "MSRV";
    pub const NO_STD_TARGET_VALIDATION: &str = "NO_STD_TARGET_VALIDATION";
    pub const MIGRATION_SAFETY: &str = "MIGRATION_SAFETY";
    pub const ABI_STABILITY: &str = "ABI_STABILITY";
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_finding() -> EvidenceFinding {
        EvidenceFinding {
            id: FindingId::new("F-NO-ENTRY-POINTS"),
            title: "No entry points detected".into(),
            classification: FindingClassification::Expected,
            severity: FindingSeverity::None,
            knowledge_layer: KnowledgeLayer::Observation,
            evidence_grade: EvidenceGrade::Unaudited,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::PROFILE_ENTRY_POINT_COUNT.into(),
                observed_value: json!(0),
                source_paths: vec!["Cargo.toml".into()],
                source_type: EvidenceSource::ProjectProfile,
                invariant_id: Some("I-PROFILE-RUST-MAIN-ENTRY-POINT".into()),
            }],
            inference: ResponsibleInference {
                statement: "No CLI binary or main function was detected.".into(),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![
                    "projectType=Library".into(),
                    "Cargo.toml lacks [[bin]]".into(),
                ],
                does_not_imply: vec![
                    "the crate is unused".into(),
                    "the crate is incorrectly configured".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::ENTRY_POINT_DETECTION_LANGUAGE_LIMITED.into(),
                detail: "Entry point detection is per-language and may miss \
                         non-standard launchers."
                    .into(),
            }],
            action: RecommendedAction::NoAction,
            scope: EvidenceScope {
                paths: vec![],
                time: ScopeTime {
                    pack_id: "pack-20260511-120000".into(),
                    commit: None,
                },
                languages: vec!["rust".into()],
            },
            not_checked: vec![],
            repo_context: vec![RepoContextTag::RustLibrary],
            false_positive_notes: vec![],
            provenance: None,
        }
    }

    #[test]
    fn finding_round_trips() {
        let f = make_finding();
        let json = serde_json::to_string(&f).expect("serialize");
        let restored: EvidenceFinding = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(f, restored);
    }

    #[test]
    fn finding_serializes_camel_case() {
        let f = make_finding();
        let v = serde_json::to_value(&f).unwrap();
        assert!(v["id"].is_string());
        assert_eq!(v["classification"], "expected");
        assert_eq!(v["severity"], "none");
        assert_eq!(v["knowledgeLayer"], "observation");
        assert_eq!(v["evidenceGrade"], "unaudited");
        assert_eq!(v["confidence"], "codified");
        assert_eq!(
            v["inference"]["statement"],
            "No CLI binary or main function was detected."
        );
        assert_eq!(v["inference"]["strength"], "suggestive");
        assert!(v["inference"]["dependsOn"].is_array());
        assert!(v["inference"]["doesNotImply"].is_array());
        assert_eq!(v["evidence"][0]["kind"], "observation");
        assert_eq!(v["evidence"][0]["metric"], "profile.entryPointCount");
        assert_eq!(v["evidence"][0]["observedValue"], 0);
        assert_eq!(v["evidence"][0]["sourceType"]["kind"], "projectProfile");
        assert_eq!(
            v["evidence"][0]["invariantId"],
            "I-PROFILE-RUST-MAIN-ENTRY-POINT"
        );
        assert_eq!(v["action"]["kind"], "noAction");
        assert_eq!(v["scope"]["time"]["packId"], "pack-20260511-120000");
        assert_eq!(v["repoContext"][0]["kind"], "rustLibrary");
    }

    #[test]
    fn finding_id_serializes_transparently() {
        let id = FindingId::new("F-TEST");
        assert_eq!(serde_json::to_value(&id).unwrap(), json!("F-TEST"));
    }

    #[test]
    fn classification_variants_camel_case() {
        for (variant, wire) in [
            (FindingClassification::Actionable, "actionable"),
            (FindingClassification::Expected, "expected"),
            (FindingClassification::Informational, "informational"),
            (FindingClassification::NotChecked, "notChecked"),
            (
                FindingClassification::NeedsExternalVerification,
                "needsExternalVerification",
            ),
            (
                FindingClassification::SuppressedByRepoProfile,
                "suppressedByRepoProfile",
            ),
        ] {
            assert_eq!(serde_json::to_value(&variant).unwrap(), json!(wire));
        }
    }

    #[test]
    fn evidence_item_heuristic_judgment_carries_rule_and_threshold() {
        let item = EvidenceItem::HeuristicJudgment {
            rule: "structure.dominantModuleShare > 0.8".into(),
            observed_metric: metric_names::STRUCTURE_DOMINANT_MODULE_SHARE.into(),
            observed_value: json!(0.92),
            threshold: json!(0.8),
            rationale: Some(
                "Heuristic threshold; not justified by an invariant. See \
                 docs/superpowers/specs/2026-05-11-evidence-aware-findings.md."
                    .into(),
            ),
        };
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["kind"], "heuristicJudgment");
        assert_eq!(v["rule"], "structure.dominantModuleShare > 0.8");
        assert_eq!(v["observedMetric"], "structure.dominantModuleShare");
        assert_eq!(v["observedValue"], 0.92);
        assert_eq!(v["threshold"], 0.8);
    }

    #[test]
    fn external_signal_kinds_camel_case() {
        for (variant, wire) in [
            (ExternalSignalKind::Manifest, "manifest"),
            (ExternalSignalKind::Lockfile, "lockfile"),
            (ExternalSignalKind::PolicyDoc, "policyDoc"),
            (ExternalSignalKind::RootDoc, "rootDoc"),
            (ExternalSignalKind::CiConfig, "ciConfig"),
        ] {
            assert_eq!(serde_json::to_value(&variant).unwrap(), json!(wire));
        }
    }

    #[test]
    fn cargo_lock_state_tagged_round_trip() {
        let states = [
            CargoLockState::CommittedAndTracked {
                path: "Cargo.lock".into(),
            },
            CargoLockState::AbsentFromRepo,
            CargoLockState::PresentButGitIgnored {
                path: "Cargo.lock".into(),
            },
            CargoLockState::PresentTrackingUnknown {
                path: "Cargo.lock".into(),
            },
        ];
        let expected_states = [
            "committedAndTracked",
            "absentFromRepo",
            "presentButGitIgnored",
            "presentTrackingUnknown",
        ];
        for (st, expected) in states.iter().zip(expected_states.iter()) {
            let v = serde_json::to_value(st).unwrap();
            assert_eq!(v["state"], *expected);
            let back: CargoLockState = serde_json::from_value(v).unwrap();
            assert_eq!(&back, st);
        }
    }

    #[test]
    fn not_checked_trigger_serializes_with_kind() {
        let always = NotCheckedTrigger::Always;
        assert_eq!(
            serde_json::to_value(&always).unwrap(),
            json!({ "kind": "always" })
        );
        let when = NotCheckedTrigger::WhenIndicator {
            tag: RepoContextTag::CryptoIndicators,
        };
        let v = serde_json::to_value(&when).unwrap();
        assert_eq!(v["kind"], "whenIndicator");
        assert_eq!(v["tag"]["kind"], "cryptoIndicators");
    }

    #[test]
    fn repo_context_tag_indicator_predicates() {
        assert!(RepoContextTag::CryptoIndicators.is_indicator());
        assert!(RepoContextTag::SecuritySensitiveIndicators.is_indicator());
        assert!(!RepoContextTag::RustLibrary.is_indicator());
        assert!(RepoContextTag::RustLibrary.is_rust());
        assert!(RepoContextTag::RustWorkspace.is_rust());
        assert!(!RepoContextTag::JavaLibrary.is_rust());
    }

    #[test]
    fn repo_context_tag_custom_serializes() {
        let tag = RepoContextTag::Custom {
            name: "monorepoRoot".into(),
        };
        let v = serde_json::to_value(&tag).unwrap();
        assert_eq!(v["kind"], "custom");
        assert_eq!(v["name"], "monorepoRoot");
    }

    #[test]
    fn metric_names_table_unique_and_known() {
        for m in metric_names::ALL {
            assert!(metric_names::is_known(m), "metric {m} should be known");
        }
        // No duplicates.
        let mut seen = std::collections::HashSet::new();
        for m in metric_names::ALL {
            assert!(seen.insert(*m), "duplicate metric name: {m}");
        }
    }

    #[test]
    fn recommended_action_variants_round_trip() {
        let actions = [
            RecommendedAction::NoAction,
            RecommendedAction::ReviewIfMaintainerPain,
            RecommendedAction::ConcreteAction {
                steps: vec!["s1".into(), "s2".into()],
            },
            RecommendedAction::ExternalVerification {
                commands_or_checks: vec!["cargo audit".into()],
            },
        ];
        let kinds = [
            "noAction",
            "reviewIfMaintainerPain",
            "concreteAction",
            "externalVerification",
        ];
        for (a, kind) in actions.iter().zip(kinds.iter()) {
            let v = serde_json::to_value(a).unwrap();
            assert_eq!(v["kind"], *kind);
            let back: RecommendedAction = serde_json::from_value(v).unwrap();
            assert_eq!(&back, a);
        }
    }
}
