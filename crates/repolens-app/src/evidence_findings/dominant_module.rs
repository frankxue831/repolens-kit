use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 3. F-DOMINANT-MODULE
    // -----------------------------------------------------------------------
    pub(super) fn dominant_module(&self) -> Option<EvidenceFinding> {
        if self.profile.modules.is_empty() {
            return None;
        }
        let total_loc: i64 = self.profile.modules.iter().map(|m| m.loc as i64).sum();
        if total_loc == 0 {
            return None;
        }
        let dominant = self
            .profile
            .modules
            .iter()
            .max_by_key(|m| m.loc)
            .expect("non-empty");
        let share = dominant.loc as f64 / total_loc as f64;
        if share <= DOMINANT_MODULE_SHARE_THRESHOLD {
            return None;
        }

        let module_count = self.profile.modules.len();
        let is_single_crate_library = self.repo_context.has(&RepoContextTag::RustLibrary)
            && module_count == 1
            && !self.repo_context.has(&RepoContextTag::RustWorkspace);
        let is_workspace_anchor =
            self.repo_context.has(&RepoContextTag::RustWorkspace) && module_count >= 2;

        let (classification, severity, action, mut depends_on) =
            if is_single_crate_library || is_workspace_anchor {
                (
                    FindingClassification::Expected,
                    FindingSeverity::None,
                    RecommendedAction::NoAction,
                    vec![if is_single_crate_library {
                        "projectShape=singleCrateLibrary".into()
                    } else {
                        "projectShape=workspaceAnchor".into()
                    }],
                )
            } else {
                (
                    FindingClassification::Informational,
                    FindingSeverity::Low,
                    RecommendedAction::ReviewIfMaintainerPain,
                    vec![],
                )
            };
        depends_on.push(format!(
            "threshold:dominantModuleShare>{DOMINANT_MODULE_SHARE_THRESHOLD}"
        ));

        Some(EvidenceFinding {
            id: FindingId::new(F_DOMINANT_MODULE),
            title: format!(
                "Dominant module: '{}' holds {:.0}% of LOC",
                dominant.name,
                share * 100.0
            ),
            classification,
            severity,
            knowledge_layer: KnowledgeLayer::Heuristic,
            // Phase 3 (2026-05-11): `structure.dominantModule*`/`structure.moduleCount`
            // are backed by the module-mapping family (I-PROFILE-MODULES-* catalogue,
            // doc-06 §5 `confirmOnly`). The 0.8 dominant-share threshold is a
            // heuristic over verified inputs → `Derived`.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![
                EvidenceItem::Observation {
                    metric: metric_names::STRUCTURE_DOMINANT_MODULE.into(),
                    observed_value: serde_json::json!(dominant.name),
                    source_paths: vec![dominant.path.clone()],
                    source_type: EvidenceSource::ProjectProfile,
                    invariant_id: None,
                },
                EvidenceItem::Observation {
                    metric: metric_names::STRUCTURE_MODULE_COUNT.into(),
                    observed_value: serde_json::json!(module_count),
                    source_paths: vec![],
                    source_type: EvidenceSource::ProjectProfile,
                    invariant_id: None,
                },
                EvidenceItem::HeuristicJudgment {
                    rule: format!(
                        "structure.dominantModuleShare > {DOMINANT_MODULE_SHARE_THRESHOLD}"
                    ),
                    observed_metric: metric_names::STRUCTURE_DOMINANT_MODULE_SHARE.into(),
                    observed_value: serde_json::json!(share),
                    threshold: serde_json::json!(DOMINANT_MODULE_SHARE_THRESHOLD),
                    rationale: Some(
                        "Heuristic threshold; not justified by an invariant. In some \
                         repos this concentration is intentional."
                            .into(),
                    ),
                },
            ],
            inference: ResponsibleInference {
                statement: format!(
                    "Module '{}' contains {:.0}% of total LOC across {} top-level modules.",
                    dominant.name,
                    share * 100.0,
                    module_count
                ),
                strength: InferenceStrength::Suggestive,
                depends_on,
                does_not_imply: vec![
                    "the module is too large".into(),
                    "splitting the module would improve maintainability".into(),
                    "the module is poorly structured".into(),
                ],
            },
            limitations: vec![Limitation {
                code: limitation_codes::DOMINANT_MODULE_THRESHOLD_HEURISTIC.into(),
                detail: format!(
                    "The {DOMINANT_MODULE_SHARE_THRESHOLD} share threshold is a \
                     heuristic. Single-crate libraries, monorepo anchors, and \
                     vendored sources legitimately concentrate LOC in one module."
                ),
            }],
            action,
            scope: self.scope(vec![dominant.path.clone()], vec![]),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}
