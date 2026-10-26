//! Evidence-aware delta findings for `SummaryComparison`. Emits per-metric
//! `EvidenceFinding`s in parallel to the legacy `delta_fitness: f64` field.
//! Each finding carries the before/after observation pair, the heuristic
//! threshold rule, and explicit `does_not_imply` non-implications.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` Phase 2.

use repolens_core::model::epistemic::{EpistemicConfidence, EvidenceGrade, KnowledgeLayer};
use repolens_core::model::evidence::{
    limitation_codes, metric_names, EvidenceFinding, EvidenceItem, EvidenceScope, EvidenceSource,
    FindingClassification, FindingId, FindingSeverity, InferenceStrength, Limitation,
    RecommendedAction, ResponsibleInference, ScopeTime,
};
use repolens_core::model::summary::RepoSummary;
use repolens_profile::repo_context::RepoContext;

// ---------------------------------------------------------------------------
// Thresholds — minimum absolute delta for a finding to fire. Below the
// threshold we say nothing rather than present noise.
// ---------------------------------------------------------------------------

const TEST_RATIO_DELTA_THRESHOLD: f64 = 0.05;
const DOMINANT_MODULE_SHARE_DELTA_THRESHOLD: f64 = 0.05;
const ORPHAN_COUNT_DELTA_THRESHOLD: i64 = 5;
const TOTAL_LOC_RELATIVE_DELTA_THRESHOLD: f64 = 0.05;
const FILE_COUNT_RELATIVE_DELTA_THRESHOLD: f64 = 0.10;

// ---------------------------------------------------------------------------
// Finding IDs
// ---------------------------------------------------------------------------

pub const F_DELTA_TEST_FILE_RATIO: &str = "F-DELTA-TEST-FILE-RATIO";
pub const F_DELTA_DOMINANT_MODULE_SHARE: &str = "F-DELTA-DOMINANT-MODULE-SHARE";
pub const F_DELTA_ORPHAN_COUNT: &str = "F-DELTA-ORPHAN-COUNT";
pub const F_DELTA_CYCLE_COUNT: &str = "F-DELTA-CYCLE-COUNT";
pub const F_DELTA_TOTAL_LOC: &str = "F-DELTA-TOTAL-LOC";
pub const F_DELTA_FILE_COUNT: &str = "F-DELTA-FILE-COUNT";

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct DeltaFindingsBuilder<'a> {
    pub before_pack_id: &'a str,
    pub after_pack_id: &'a str,
    pub before: &'a RepoSummary,
    pub after: &'a RepoSummary,
    /// Repo context detected from the *after* summary, used for indicator
    /// tags and the `repo_context` field on emitted findings.
    pub repo_context: &'a RepoContext,
}

impl<'a> DeltaFindingsBuilder<'a> {
    pub fn build(&self) -> Vec<EvidenceFinding> {
        let mut out = Vec::new();
        if let Some(f) = self.delta_test_file_ratio() {
            out.push(f);
        }
        if let Some(f) = self.delta_dominant_module_share() {
            out.push(f);
        }
        if let Some(f) = self.delta_orphan_count() {
            out.push(f);
        }
        if let Some(f) = self.delta_cycle_count() {
            out.push(f);
        }
        if let Some(f) = self.delta_total_loc() {
            out.push(f);
        }
        if let Some(f) = self.delta_file_count() {
            out.push(f);
        }
        out
    }

    fn scope(&self) -> EvidenceScope {
        // Reuse the commit from the after-summary's per-pack findings if
        // SummaryService already populated it from the pack manifest.
        // This keeps compare evidence aligned with the same head commit
        // as the underlying summary.
        let commit = self
            .after
            .evidence_findings
            .first()
            .and_then(|f| f.scope.time.commit.clone());
        EvidenceScope {
            paths: vec![],
            time: ScopeTime {
                pack_id: self.after_pack_id.to_string(),
                commit,
            },
            languages: self
                .after
                .profile
                .language_distribution
                .keys()
                .cloned()
                .collect(),
        }
    }

    fn standard_limitations() -> Vec<Limitation> {
        vec![
            Limitation {
                code: limitation_codes::DELTA_REFLECTS_PACK_BOUNDARIES_NOT_COMMIT_DELTAS.into(),
                detail: "RepoLens compares packs, not commits. If the two scans were taken at \
                         non-canonical points (e.g. during an incremental scan), the delta may \
                         not align with a commit boundary."
                    .into(),
            },
            Limitation {
                code: limitation_codes::DELTA_THRESHOLD_HEURISTIC.into(),
                detail: "The threshold below which deltas are not reported is heuristic. Small \
                         changes that fall below the threshold are not surfaced even though \
                         they may be meaningful in some contexts."
                    .into(),
            },
        ]
    }

    fn indicator_notes(&self) -> Vec<String> {
        self.repo_context
            .notes
            .iter()
            .map(|m| m.note.clone())
            .collect()
    }

    // -----------------------------------------------------------------------
    // F-DELTA-TEST-FILE-RATIO
    // -----------------------------------------------------------------------
    fn delta_test_file_ratio(&self) -> Option<EvidenceFinding> {
        let before = self.before.scan.test_to_source_ratio;
        let after = self.after.scan.test_to_source_ratio;
        let delta = after - before;
        if delta.abs() < TEST_RATIO_DELTA_THRESHOLD {
            return None;
        }
        let direction = if delta > 0.0 { "rose" } else { "fell" };
        let mut limitations = Self::standard_limitations();
        // Inline #[cfg(test)] caveat is Rust-specific. For Java/Node/Python
        // deltas the test-file ratio already reflects whichever convention
        // those repos use, so emitting this limitation would be misleading.
        if self.repo_context.any_rust() {
            limitations.push(Limitation {
                code: limitation_codes::INLINE_RUST_TESTS_NOT_COUNTED.into(),
                detail: "Rust #[cfg(test)] inline tests are not enumerated by RepoLens. The \
                         test-file ratio reflects only files whose path or attributes mark them \
                         as tests; an inline-test refactor will move this metric without \
                         changing test inventory."
                    .into(),
            });
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_DELTA_TEST_FILE_RATIO),
            title: format!(
                "Detected test-file ratio {direction} from {:.0}% to {:.0}%",
                before * 100.0,
                after * 100.0
            ),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 4 (2026-05-11): `delta.scan.testToSourceFileRatio` rolls
            // up the test-file detection family (I-TEST-DETECT-* catalogue,
            // doc-06 §5 `confirmOnly`). The 5pp delta threshold is a
            // heuristic over verified inputs → `Derived`. Carries the
            // `INLINE_RUST_TESTS_NOT_COUNTED` limitation for Rust repos.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::SCAN_TEST_TO_SOURCE_FILE_RATIO.into(),
                    observed_value: serde_json::json!({
                        "before": before,
                        "after": after,
                    }),
                    source_paths: vec![],
                    source_type: EvidenceSource::FileInventory,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "abs({}) >= {TEST_RATIO_DELTA_THRESHOLD}",
                        metric_names::DELTA_SCAN_TEST_TO_SOURCE_FILE_RATIO
                    ),
                    observed_metric: metric_names::DELTA_SCAN_TEST_TO_SOURCE_FILE_RATIO.into(),
                    observed_value: serde_json::json!(delta),
                    threshold: serde_json::json!(TEST_RATIO_DELTA_THRESHOLD),
                    rationale: Some(
                        "Heuristic threshold; the test-file ratio is structural, not a \
                         coverage measurement."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "The detected test-file ratio changed by {:.0} percentage points between \
                     packs.",
                    delta * 100.0
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![format!(
                    "threshold:abs(deltaTestFileRatio)>={TEST_RATIO_DELTA_THRESHOLD}"
                )],
                does_not_imply: vec![
                    "test coverage changed".into(),
                    "code quality changed".into(),
                    "tests started or stopped passing".into(),
                ],
            },
            limitations,
            action: RecommendedAction::ReviewIfMaintainerPain,
            scope: self.scope(),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }

    // -----------------------------------------------------------------------
    // F-DELTA-DOMINANT-MODULE-SHARE
    // -----------------------------------------------------------------------
    fn delta_dominant_module_share(&self) -> Option<EvidenceFinding> {
        // Require BOTH sides to have a measured value. A missing health
        // report on either side means the metric was not observed and
        // we must not silently coerce it to zero.
        let before = self
            .before
            .health
            .as_ref()
            .map(|h| h.structure.dominant_module_share)?;
        let after = self
            .after
            .health
            .as_ref()
            .map(|h| h.structure.dominant_module_share)?;
        let delta = after - before;
        if delta.abs() < DOMINANT_MODULE_SHARE_DELTA_THRESHOLD {
            return None;
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_DELTA_DOMINANT_MODULE_SHARE),
            title: format!(
                "Dominant module share moved from {:.0}% to {:.0}%",
                before * 100.0,
                after * 100.0
            ),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 3 (2026-05-11): `structure.dominantModuleShare` is backed
            // by the module-mapping family (catalogued in I-PROFILE-MODULES-*).
            // The 0.05 delta threshold is a heuristic over verified inputs →
            // `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::STRUCTURE_DOMINANT_MODULE_SHARE.into(),
                    observed_value: serde_json::json!({
                        "before": before,
                        "after": after,
                    }),
                    source_paths: vec![],
                    source_type: EvidenceSource::ProjectProfile,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "abs({}) >= {DOMINANT_MODULE_SHARE_DELTA_THRESHOLD}",
                        metric_names::DELTA_STRUCTURE_DOMINANT_MODULE_SHARE
                    ),
                    observed_metric: metric_names::DELTA_STRUCTURE_DOMINANT_MODULE_SHARE.into(),
                    observed_value: serde_json::json!(delta),
                    threshold: serde_json::json!(DOMINANT_MODULE_SHARE_DELTA_THRESHOLD),
                    rationale: None,
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "The share of LOC concentrated in the dominant top-level module changed by \
                     {:.0} percentage points between packs.",
                    delta * 100.0
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![],
                does_not_imply: vec![
                    "the dominant module became too large".into(),
                    "the codebase became better or worse organised".into(),
                    "splitting the module would help".into(),
                ],
            },
            limitations: Self::standard_limitations(),
            action: RecommendedAction::NoAction,
            scope: self.scope(),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }

    // -----------------------------------------------------------------------
    // F-DELTA-ORPHAN-COUNT
    // -----------------------------------------------------------------------
    fn delta_orphan_count(&self) -> Option<EvidenceFinding> {
        // Require both sides to have measured graph_health.
        let before = self
            .before
            .health
            .as_ref()
            .and_then(|h| h.graph_health.as_ref())
            .map(|gh| gh.orphan_count as i64)?;
        let after = self
            .after
            .health
            .as_ref()
            .and_then(|h| h.graph_health.as_ref())
            .map(|gh| gh.orphan_count as i64)?;
        let delta = after - before;
        if delta.abs() < ORPHAN_COUNT_DELTA_THRESHOLD {
            return None;
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_DELTA_ORPHAN_COUNT),
            title: format!("Graph orphan count moved from {before} to {after}"),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 3 (2026-05-11): `graph.orphanCount` is backed by the
            // orphan-detection family (I-GRAPH-ORPHANS-*). The ±5 delta
            // threshold is a heuristic over verified inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::GRAPH_ORPHAN_COUNT.into(),
                    observed_value: serde_json::json!({
                        "before": before,
                        "after": after,
                    }),
                    source_paths: vec![],
                    source_type: EvidenceSource::ImportGraph,
                    invariant_id: Some("I-GRAPH-ORPHANS-RESPECT-ENTRY-POINTS".into()),
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "abs({}) >= {ORPHAN_COUNT_DELTA_THRESHOLD}",
                        metric_names::DELTA_GRAPH_ORPHAN_COUNT
                    ),
                    observed_metric: metric_names::DELTA_GRAPH_ORPHAN_COUNT.into(),
                    observed_value: serde_json::json!(delta),
                    threshold: serde_json::json!(ORPHAN_COUNT_DELTA_THRESHOLD),
                    rationale: None,
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "The number of files with zero inbound structural import edges (entry \
                     points excluded) changed by {delta} between packs. Newly orphan files \
                     may still import other modules."
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![],
                does_not_imply: vec![
                    "dead code was added or removed".into(),
                    "files that became orphans are unused at runtime".into(),
                ],
            },
            limitations: Self::standard_limitations(),
            action: RecommendedAction::ReviewIfMaintainerPain,
            scope: self.scope(),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }

    // -----------------------------------------------------------------------
    // F-DELTA-CYCLE-COUNT
    // -----------------------------------------------------------------------
    fn delta_cycle_count(&self) -> Option<EvidenceFinding> {
        // Require both sides to have measured graph_health.
        let before = self
            .before
            .health
            .as_ref()
            .and_then(|h| h.graph_health.as_ref())
            .map(|gh| gh.cycle_count as i64)?;
        let after = self
            .after
            .health
            .as_ref()
            .and_then(|h| h.graph_health.as_ref())
            .map(|gh| gh.cycle_count as i64)?;
        let delta = after - before;
        if delta == 0 {
            return None;
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_DELTA_CYCLE_COUNT),
            title: format!("Dependency cycle count moved from {before} to {after}"),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            // Phase 3 (2026-05-11): `graph.cycleCount` is backed by the
            // graph-cycles family (I-GRAPH-CYCLES-* catalogue, doc-06 §5
            // `confirmOnly`). Any non-zero change is reported (no threshold)
            // so this is a pure observation → `Derived`.
            knowledge_layer: KnowledgeLayer::Observation,
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::GRAPH_CYCLE_COUNT.into(),
                observed_value: serde_json::json!({
                    "before": before,
                    "after": after,
                }),
                source_paths: vec![],
                source_type: EvidenceSource::ImportGraph,
                invariant_id: Some("I-GRAPH-CYCLES-EXCLUDE-DECLARES".into()),
            }],
            inference: ResponsibleInference {
                statement: format!(
                    "The number of import-graph cycles changed by {delta} between packs."
                ),
                strength: InferenceStrength::Strong,
                depends_on: vec![],
                does_not_imply: vec![
                    "the code became harder or easier to test".into(),
                    "introduced cycles cause runtime problems".into(),
                ],
            },
            limitations: Self::standard_limitations(),
            action: if delta > 0 {
                RecommendedAction::ReviewIfMaintainerPain
            } else {
                RecommendedAction::NoAction
            },
            scope: self.scope(),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }

    // -----------------------------------------------------------------------
    // F-DELTA-TOTAL-LOC
    // -----------------------------------------------------------------------
    fn delta_total_loc(&self) -> Option<EvidenceFinding> {
        let before = self.before.scan.total_loc;
        let after = self.after.scan.total_loc;
        if before == 0 {
            return None;
        }
        let delta = after - before;
        let relative = (delta as f64) / (before as f64);
        if relative.abs() < TOTAL_LOC_RELATIVE_DELTA_THRESHOLD {
            return None;
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_DELTA_TOTAL_LOC),
            title: format!(
                "Total LOC moved from {before} to {after} ({:+.0}%)",
                relative * 100.0
            ),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::None,
            // Thresholded delta — fires only when relative change >= 5%.
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 3 (2026-05-11): `delta.scan.totalLoc` rolls up the LOC
            // family (I-LOC-COUNTS-* catalogue, doc-06 §5 `confirmOnly`).
            // The 5% relative-change threshold is a heuristic over verified
            // inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::DELTA_SCAN_TOTAL_LOC.into(),
                    observed_value: serde_json::json!({
                        "before": before,
                        "after": after,
                        "delta": delta,
                        "relative": relative,
                    }),
                    source_paths: vec![],
                    source_type: EvidenceSource::FileInventory,
                    invariant_id: Some("I-LOC-COUNTS-NONNEGATIVE".into()),
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "abs(delta.scan.totalLocRelative) >= {TOTAL_LOC_RELATIVE_DELTA_THRESHOLD}"
                    ),
                    observed_metric: metric_names::DELTA_SCAN_TOTAL_LOC.into(),
                    observed_value: serde_json::json!(relative),
                    threshold: serde_json::json!(TOTAL_LOC_RELATIVE_DELTA_THRESHOLD),
                    rationale: Some(
                        "Heuristic threshold; only relative changes at or above 5% are \
                         surfaced as findings."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "Total lines of code in included files changed by {delta} ({:+.0}%) \
                     between packs.",
                    relative * 100.0
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![format!(
                    "threshold:abs(totalLocRelative)>={TOTAL_LOC_RELATIVE_DELTA_THRESHOLD}"
                )],
                does_not_imply: vec![
                    "more or less work was done".into(),
                    "the codebase became more or less complex".into(),
                ],
            },
            limitations: Self::standard_limitations(),
            action: RecommendedAction::NoAction,
            scope: self.scope(),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }

    // -----------------------------------------------------------------------
    // F-DELTA-FILE-COUNT
    // -----------------------------------------------------------------------
    fn delta_file_count(&self) -> Option<EvidenceFinding> {
        // Use `included_count` so the finding reflects files RepoLens
        // actually scanned, not the total pack-entry count (which can move
        // when excluded fixtures or vendor files are added/removed).
        let before = self.before.scan.included_count as i64;
        let after = self.after.scan.included_count as i64;
        if before == 0 {
            return None;
        }
        let delta = after - before;
        let relative = (delta as f64) / (before as f64);
        if relative.abs() < FILE_COUNT_RELATIVE_DELTA_THRESHOLD {
            return None;
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_DELTA_FILE_COUNT),
            title: format!(
                "Included file count moved from {before} to {after} ({:+.0}%)",
                relative * 100.0
            ),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::None,
            // Thresholded delta — fires only when relative change >= 10%.
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 3 (2026-05-11): `delta.scan.includedCount` rolls up the
            // file-presence family (I-FILE-PATH-* catalogue, doc-06 §5
            // `contradictionEligible`). The 10% relative-change threshold
            // is a heuristic over verified inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::DELTA_SCAN_INCLUDED_COUNT.into(),
                    observed_value: serde_json::json!({
                        "before": before,
                        "after": after,
                        "delta": delta,
                        "relative": relative,
                    }),
                    source_paths: vec![],
                    source_type: EvidenceSource::FileInventory,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "abs(delta.scan.includedCountRelative) >= {FILE_COUNT_RELATIVE_DELTA_THRESHOLD}"
                    ),
                    observed_metric: metric_names::DELTA_SCAN_INCLUDED_COUNT.into(),
                    observed_value: serde_json::json!(relative),
                    threshold: serde_json::json!(FILE_COUNT_RELATIVE_DELTA_THRESHOLD),
                    rationale: Some(
                        "Heuristic threshold; only relative changes at or above 10% are \
                         surfaced as findings."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "Included file count changed by {delta} ({:+.0}%) between packs.",
                    relative * 100.0
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![format!(
                    "threshold:abs(includedCountRelative)>={FILE_COUNT_RELATIVE_DELTA_THRESHOLD}"
                )],
                does_not_imply: vec!["meaningful work was added or removed".into()],
            },
            limitations: Self::standard_limitations(),
            action: RecommendedAction::NoAction,
            scope: self.scope(),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::evidence::{FindingClassification as FC, RepoContextTag};
    use repolens_core::model::health::{DependencyRisk, GraphHealthInput, HealthReport, Structure};
    use repolens_core::model::profile::{ProjectProfile, ProjectType};
    use repolens_core::model::summary::ScanSummary;
    use repolens_profile::repo_context::RepoContext;
    use std::collections::BTreeMap;

    fn make_profile() -> ProjectProfile {
        ProjectProfile {
            name: "demo".into(),
            build_system: None,
            frameworks: vec![],
            project_type: ProjectType::Library,
            entry_points: vec![],
            description: None,
            language_distribution: BTreeMap::from([("rust".into(), 1)]),
            modules: vec![],
        }
    }

    fn make_summary(
        test_ratio: f64,
        dom_share: f64,
        orphan: usize,
        cycles: usize,
        loc: i64,
        files: i32,
    ) -> RepoSummary {
        RepoSummary {
            schema_version: "1.0".into(),
            session: None,
            scan: ScanSummary {
                mode: "full".into(),
                file_count: files,
                included_count: files,
                total_loc: loc,
                test_file_count: 1,
                source_file_count: files - 1,
                test_to_source_ratio: test_ratio,
            },
            profile: make_profile(),
            health: Some(HealthReport {
                structure: Structure {
                    module_count: 1,
                    dominant_module: "src".into(),
                    dominant_module_share: dom_share,
                    entry_point_count: 0,
                },
                dependency_risk: DependencyRisk {
                    build_system: None,
                    framework_count: 0,
                    missing_version_count: 0,
                    lockfile_present: true,
                    risk_level: "low".into(),
                },
                graph_health: Some(GraphHealthInput {
                    cycle_count: cycles,
                    largest_cycle_size: 0,
                    files_in_cycles_count: 0,
                    orphan_count: orphan,
                    orphan_files: vec![],
                    test_to_source_ratio: test_ratio,
                }),
                evidence_findings: vec![],
            }),
            graph_summary: None,
            top_symbols: vec![],
            top_files: vec![],
            total_symbols: 0,
            recent_commits: vec![],
            memory_insights: vec![],
            evidence_findings: vec![],
        }
    }

    fn empty_ctx() -> RepoContext {
        RepoContext {
            tags: vec![RepoContextTag::RustLibrary],
            notes: vec![],
        }
    }

    #[test]
    fn no_findings_when_deltas_below_threshold() {
        let ctx = empty_ctx();
        let before = make_summary(0.10, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.11, 0.51, 1, 0, 1010, 101);
        let findings = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        assert!(
            findings.is_empty(),
            "expected no findings; got {findings:?}"
        );
    }

    #[test]
    fn test_ratio_drop_emits_finding_with_inline_caveat() {
        let ctx = empty_ctx();
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.10, 0.50, 0, 0, 1000, 100); // -10pp
        let findings = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        let f = findings
            .iter()
            .find(|f| f.id.as_str() == F_DELTA_TEST_FILE_RATIO)
            .expect("test-file-ratio delta finding");
        assert_eq!(f.classification, FC::Informational);
        assert!(f
            .limitations
            .iter()
            .any(|l| l.code == limitation_codes::INLINE_RUST_TESTS_NOT_COUNTED));
        assert!(f
            .inference
            .does_not_imply
            .iter()
            .any(|s| s.contains("coverage")));
        // Phase 4 (2026-05-11): promoted to Derived via the test-file
        // detection invariant catalogue.
        assert_eq!(
            f.evidence_grade,
            EvidenceGrade::Derived,
            "F-DELTA-TEST-FILE-RATIO was promoted to Derived in Phase 4 (2026-05-11)"
        );
    }

    #[test]
    fn delta_findings_carry_real_pack_ids_in_scope() {
        let ctx = empty_ctx();
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.10, 0.50, 0, 0, 1000, 100);
        let findings = DeltaFindingsBuilder {
            before_pack_id: "pack-20260411-120000",
            after_pack_id: "pack-20260512-090000",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        assert!(!findings.is_empty());
        for f in &findings {
            // scope.time.packId carries the after-pack identifier so
            // consumers can correlate the finding with the actual pack.
            assert_eq!(
                f.scope.time.pack_id,
                "pack-20260512-090000",
                "{}: scope.time.packId must carry the real after-pack id, got {:?}",
                f.id.as_str(),
                f.scope.time.pack_id
            );
        }
    }

    #[test]
    fn cycle_count_change_emits_finding_with_observation_layer() {
        let ctx = empty_ctx();
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.20, 0.50, 0, 1, 1000, 100);
        let findings = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        let f = findings
            .iter()
            .find(|f| f.id.as_str() == F_DELTA_CYCLE_COUNT)
            .expect("cycle-count delta finding");
        assert_eq!(f.knowledge_layer, KnowledgeLayer::Observation);
        // Newly introduced cycles → ReviewIfMaintainerPain, not NoAction.
        assert!(matches!(
            f.action,
            RecommendedAction::ReviewIfMaintainerPain
        ));
    }

    #[test]
    fn loc_relative_delta_emits_finding_at_threshold() {
        let ctx = empty_ctx();
        // 5% relative delta (1000 → 1050).
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.20, 0.50, 0, 0, 1050, 100);
        let findings = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        let f = findings
            .iter()
            .find(|f| f.id.as_str() == F_DELTA_TOTAL_LOC)
            .expect("loc delta finding");
        assert_eq!(f.severity, FindingSeverity::None);
        // does_not_imply must disclaim "more work was done".
        assert!(f
            .inference
            .does_not_imply
            .iter()
            .any(|s| s.contains("work")));
    }

    #[test]
    fn all_delta_findings_carry_pack_boundary_caveat() {
        let ctx = empty_ctx();
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.10, 0.60, 10, 1, 1500, 150);
        let findings = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        assert!(!findings.is_empty());
        for f in &findings {
            assert!(
                f.limitations.iter().any(|l| {
                    l.code == limitation_codes::DELTA_REFLECTS_PACK_BOUNDARIES_NOT_COMMIT_DELTAS
                }),
                "{} missing pack-boundary caveat",
                f.id.as_str()
            );
        }
    }

    #[test]
    fn metric_names_in_delta_findings_are_known() {
        let ctx = empty_ctx();
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.10, 0.60, 10, 1, 1500, 150);
        let findings = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &ctx,
        }
        .build();
        for f in &findings {
            for ev in &f.evidence {
                match ev {
                    EvidenceItem::Observation { metric, .. } => {
                        assert!(metric_names::is_known(metric), "unknown metric {metric}");
                    }
                    EvidenceItem::HeuristicJudgment {
                        observed_metric, ..
                    } => {
                        assert!(
                            metric_names::is_known(observed_metric),
                            "unknown observed_metric {observed_metric}"
                        );
                    }
                    EvidenceItem::ExternalSignal { .. } => {}
                }
            }
        }
    }

    #[test]
    fn inline_rust_caveat_only_fires_for_rust_repos() {
        // Rust repo: caveat present.
        let rust_ctx = empty_ctx();
        let before = make_summary(0.20, 0.50, 0, 0, 1000, 100);
        let after = make_summary(0.10, 0.50, 0, 0, 1000, 100);
        let rust_finding = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &rust_ctx,
        }
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DELTA_TEST_FILE_RATIO)
        .expect("rust test-ratio delta finding");
        assert!(rust_finding
            .limitations
            .iter()
            .any(|l| l.code == limitation_codes::INLINE_RUST_TESTS_NOT_COUNTED));

        // Java repo: caveat absent.
        let java_ctx = RepoContext {
            tags: vec![RepoContextTag::JavaService],
            notes: vec![],
        };
        let java_finding = DeltaFindingsBuilder {
            before_pack_id: "p1",
            after_pack_id: "p2",
            before: &before,
            after: &after,
            repo_context: &java_ctx,
        }
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DELTA_TEST_FILE_RATIO)
        .expect("java test-ratio delta finding");
        assert!(!java_finding
            .limitations
            .iter()
            .any(|l| l.code == limitation_codes::INLINE_RUST_TESTS_NOT_COUNTED));
    }
}
