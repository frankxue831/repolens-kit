use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 6. F-TEST-INVENTORY-RATIO
    // -----------------------------------------------------------------------
    pub(super) fn test_inventory(&self) -> Option<EvidenceFinding> {
        let scan = self.scan?;
        if scan.source_file_count <= TEST_RATIO_MIN_SOURCE_FILES {
            return None;
        }
        let ratio = scan.test_to_source_ratio;
        if ratio >= TEST_RATIO_LOW_THRESHOLD {
            return None;
        }

        let mut limitations = vec![Limitation {
            code: limitation_codes::TEST_PASS_FAIL_NOT_RUN.into(),
            detail: "RepoLens does not run tests. Whether tests pass, fail, or \
                         exist as compilable code is out of scope."
                .into(),
        }];
        // Inline-Rust caveat is only relevant for Rust repos.
        if self.repo_context.any_rust() {
            limitations.push(Limitation {
                code: limitation_codes::INLINE_RUST_TESTS_NOT_COUNTED.into(),
                detail: "Rust source files containing #[cfg(test)] or #[test] are \
                         counted as test-bearing files, but individual inline tests \
                         are not enumerated. The ratio is by file count only."
                    .into(),
            });
        }

        Some(EvidenceFinding {
            id: FindingId::new(F_TEST_INVENTORY_RATIO),
            title: "Low detected test-file ratio".into(),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 4 (2026-05-11): `scan.testFileCount`, `scan.sourceFileCount`,
            // and `scan.testToSourceFileRatio` are backed by the test-file
            // detection family (I-TEST-DETECT-* catalogue, doc-06 §5
            // `confirmOnly`). The 0.1 ratio threshold and `sourceFileCount > 5`
            // guard are heuristics over verified inputs → `Derived`. The
            // `INLINE_RUST_TESTS_NOT_COUNTED` limitation is already attached
            // for Rust repos so consumers see the inline-test exclusion.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::SCAN_TEST_FILE_COUNT.into(),
                    observed_value: serde_json::json!(scan.test_file_count),
                    source_paths: vec![],
                    source_type: EvidenceSource::FileInventory,
                    invariant_id: None,
                },
                EvidenceItem::Observation {
                    metric: metric_names::SCAN_SOURCE_FILE_COUNT.into(),
                    observed_value: serde_json::json!(scan.source_file_count),
                    source_paths: vec![],
                    source_type: EvidenceSource::FileInventory,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!("scan.testToSourceFileRatio < {TEST_RATIO_LOW_THRESHOLD}"),
                    observed_metric: metric_names::SCAN_TEST_TO_SOURCE_FILE_RATIO.into(),
                    observed_value: serde_json::json!(ratio),
                    threshold: serde_json::json!(TEST_RATIO_LOW_THRESHOLD),
                    rationale: Some(
                        "Heuristic threshold; not justified by an invariant. \
                         The test-to-source file ratio is structural, not a \
                         coverage measurement."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "Detected test-file ratio is {:.0}% by file count ({}/{}).",
                    ratio * 100.0,
                    scan.test_file_count,
                    scan.source_file_count
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec![format!(
                    "threshold:testToSourceFileRatio<{TEST_RATIO_LOW_THRESHOLD}"
                )],
                does_not_imply: vec![
                    "test coverage is low".into(),
                    "the code is poorly tested".into(),
                    "tests do not exist".into(),
                    "tests do not pass".into(),
                ],
            },
            limitations,
            action: RecommendedAction::ReviewIfMaintainerPain,
            scope: self.scope(vec![], languages_from_profile(self.profile)),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}
