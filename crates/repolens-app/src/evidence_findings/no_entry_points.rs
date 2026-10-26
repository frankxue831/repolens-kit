use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 1. F-NO-ENTRY-POINTS
    // -----------------------------------------------------------------------
    pub(super) fn no_entry_points(&self) -> Option<EvidenceFinding> {
        let count = self.profile.entry_points.len() as i32;
        if count > 0 {
            return None;
        }
        let is_library = matches!(self.profile.project_type, ProjectType::Library)
            || self.repo_context.has(&RepoContextTag::RustLibrary)
            || self.repo_context.has(&RepoContextTag::JavaLibrary)
            || self.repo_context.has(&RepoContextTag::NodeLibrary)
            || self.repo_context.has(&RepoContextTag::PythonPackage);

        let (classification, severity, action, depends_on) = if is_library {
            (
                FindingClassification::Expected,
                FindingSeverity::None,
                RecommendedAction::NoAction,
                vec!["projectType=Library".into()],
            )
        } else {
            (
                FindingClassification::Informational,
                FindingSeverity::Low,
                RecommendedAction::ReviewIfMaintainerPain,
                vec!["projectType=NotLibrary".into()],
            )
        };

        Some(EvidenceFinding {
            id: FindingId::new(F_NO_ENTRY_POINTS),
            title: "No entry points detected".into(),
            classification,
            severity,
            knowledge_layer: KnowledgeLayer::Observation,
            // Phase 3 (2026-05-11): `profile.entryPointCount` is backed by
            // the entry-point detection family (I-PROFILE-*-ENTRY-POINT
            // catalogue, doc-06 §5 `confirmOnly`). Threshold-free
            // observation → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::PROFILE_ENTRY_POINT_COUNT.into(),
                observed_value: serde_json::json!(0),
                source_paths: self
                    .profile
                    .build_system
                    .as_ref()
                    .map(|b| vec![b.config_file.clone()])
                    .unwrap_or_default(),
                source_type: EvidenceSource::ProjectProfile,
                invariant_id: None,
            }],
            inference: ResponsibleInference {
                statement: "No CLI binary or main function was detected by the per-language \
                            entry-point extractors."
                    .into(),
                strength: InferenceStrength::Suggestive,
                depends_on,
                does_not_imply: vec![
                    "the crate or project is unused".into(),
                    "the project is incorrectly configured".into(),
                    "the project has no public API".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::ENTRY_POINT_DETECTION_LANGUAGE_LIMITED.into(),
                detail: "Entry-point detection is per-language. Non-standard launchers \
                         (custom shell scripts, embedded firmware reset vectors, plugin \
                         hosts) are not enumerated."
                    .into(),
            }],
            action,
            scope: self.scope(vec![], languages_from_profile(self.profile)),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}
