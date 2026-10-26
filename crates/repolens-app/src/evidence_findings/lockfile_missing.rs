use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 8. F-LOCKFILE-MISSING-NON-CARGO
    //
    // Replaces the retired `no-lockfile` signal for non-Cargo build systems.
    // Cargo-specific cases are covered by `F-CARGO-LOCK-STATE`.
    // -----------------------------------------------------------------------
    pub(super) fn lockfile_missing_non_cargo(&self) -> Option<EvidenceFinding> {
        let risk = self.dependency_risk?;
        // Build system must be detected. No build system → no expectation.
        let build_system = risk.build_system.as_deref()?;
        // Cargo is handled by F-CARGO-LOCK-STATE with proper invariant grounding.
        if build_system.eq_ignore_ascii_case("cargo") {
            return None;
        }
        if risk.lockfile_present {
            return None;
        }
        // Maven, Gradle and plain pip/setuptools have no standard lockfile,
        // so the advice could not be acted on (#25).
        let details = self
            .profile
            .build_system
            .as_ref()
            .and_then(|b| b.details.as_deref());
        let known =
            repolens_core::model::health::lockfiles_for_build_system(build_system, details)?;
        // Libraries conventionally leave application lockfiles out, as with
        // Cargo.lock for Rust libraries.
        if self.repo_context.has(&RepoContextTag::NodeLibrary)
            || self.repo_context.has(&RepoContextTag::PythonPackage)
            || self.repo_context.has(&RepoContextTag::JavaLibrary)
        {
            return None;
        }
        let known_list = known.join(", ");

        Some(EvidenceFinding {
            id: FindingId::new(F_LOCKFILE_MISSING_NON_CARGO),
            title: format!("No lockfile found for {build_system} build system"),
            classification: FindingClassification::Actionable,
            severity: FindingSeverity::Low,
            // Heuristic: "build system present implies a lockfile should be
            // committed for reproducibility." Doc-06 §5 lockfile-state catalogue
            // currently scopes to Cargo only; cross-language lockfile-state
            // invariants are a follow-up. Until then this finding is
            // heuristic-grade.
            knowledge_layer: KnowledgeLayer::Heuristic,
            evidence_grade: EvidenceGrade::Unaudited,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::PROFILE_BUILD_SYSTEM.into(),
                    observed_value: serde_json::json!(build_system),
                    source_paths: self
                        .profile
                        .build_system
                        .as_ref()
                        .map(|b| vec![b.config_file.clone()])
                        .unwrap_or_default(),
                    source_type: EvidenceSource::ProjectProfile,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: "build system has standard lockfiles && !lockfile_present \
                           && build_system != Cargo && !library"
                        .into(),
                    observed_metric: metric_names::PROFILE_LOCKFILE_STATE.into(),
                    observed_value: serde_json::json!({
                        "buildSystem": build_system,
                        "lockfilePresent": false,
                    }),
                    threshold: serde_json::json!({
                        "rule": "warn when build system present without matching lockfile"
                    }),
                    rationale: Some(
                        "Heuristic threshold; not justified by an invariant. \
                         A build system without a committed lockfile typically \
                         indicates non-reproducible dependency resolution, but \
                         intentional unpinned-dev workflows and pre-release \
                         repos may legitimately lack one."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "A {build_system} build system was detected but none of its \
                     lockfiles ({known_list}) is present. Dependency resolution may \
                     not be reproducible across machines."
                ),
                strength: InferenceStrength::Suggestive,
                depends_on: vec!["build_system.detected".into()],
                does_not_imply: vec![
                    "the project is broken".into(),
                    "the project is misconfigured".into(),
                    "a lockfile would resolve all reproducibility concerns".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::LIVE_ROOT_VS_PACK_TIME_DIVERGENCE_POSSIBLE.into(),
                detail: "Lockfile presence is detected from the pack's file inventory. \
                         A lockfile added or removed in the working tree after the scan \
                         is not reflected."
                    .into(),
            }],
            action: RecommendedAction::ReviewIfMaintainerPain,
            scope: self.scope(vec![], languages_from_profile(self.profile)),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}
