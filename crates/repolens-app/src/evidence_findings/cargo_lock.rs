use super::*;

impl<'a> EvidenceFindingsBuilder<'a> {
    // -----------------------------------------------------------------------
    // 5. F-CARGO-LOCK-STATE
    // -----------------------------------------------------------------------
    pub(super) fn cargo_lock(&self) -> Option<EvidenceFinding> {
        let state = self.lockfile_state?;
        let is_rust = self.repo_context.any_rust();
        if !is_rust {
            return None;
        }
        // A Cargo workspace whose members expose no binary target (no
        // `[[bin]]`, `src/main.rs` or `src/bin/`) gets `RustLibrary`, not
        // `RustBinary`. Such library-only workspaces conventionally leave
        // Cargo.lock out (#16), so they take the library path. Only a binary
        // target makes a missing or ignored lockfile worth a look.
        let is_binary = self
            .cargo_has_binary
            .unwrap_or_else(|| self.repo_context.has(&RepoContextTag::RustBinary));
        let is_workspace = self.repo_context.has(&RepoContextTag::RustWorkspace);
        let is_library = !is_binary
            && (self.cargo_has_binary.is_some()
                || self.repo_context.has(&RepoContextTag::RustLibrary));
        let is_binary_or_workspace = is_binary;

        let (title, classification, severity, action, statement, does_not_imply) =
            match (state, is_library, is_binary_or_workspace) {
                (CargoLockState::CommittedAndTracked { .. }, _, _) => (
                    "Cargo.lock is committed and tracked".to_string(),
                    FindingClassification::Informational,
                    FindingSeverity::None,
                    RecommendedAction::NoAction,
                    "Cargo.lock has an entry in the git index, so git tracks it.".to_string(),
                    vec!["Cargo.lock content is current".into()],
                ),
                (CargoLockState::PresentTrackingUnknown { .. }, _, _) => (
                    "Cargo.lock present (tracking status not inspected)".to_string(),
                    FindingClassification::Informational,
                    FindingSeverity::None,
                    RecommendedAction::NoAction,
                    "Cargo.lock is present in the working tree and not matched by .gitignore, \
                 but the git index has no entry for it (or this is not a git repository)."
                        .to_string(),
                    vec!["Cargo.lock is committed to git".into()],
                ),
                (CargoLockState::AbsentFromRepo, true, _) => (
                    if is_workspace {
                        "Cargo.lock absent (library-only workspace)".to_string()
                    } else {
                        "Cargo.lock absent (library)".to_string()
                    },
                    FindingClassification::Informational,
                    FindingSeverity::None,
                    RecommendedAction::NoAction,
                    "Cargo.lock is not present. Library crates conventionally do not \
                 commit Cargo.lock; absence is not necessarily a defect."
                        .to_string(),
                    vec!["the project is misconfigured".into()],
                ),
                (CargoLockState::AbsentFromRepo, _, true) => (
                    "Cargo.lock absent (has a binary target)".to_string(),
                    FindingClassification::Actionable,
                    FindingSeverity::Low,
                    RecommendedAction::ReviewIfMaintainerPain,
                    "Cargo.lock is not present. Projects with a binary target conventionally \
                 commit Cargo.lock for reproducible builds and deploys."
                        .to_string(),
                    vec!["the project is broken".into()],
                ),
                (CargoLockState::AbsentFromRepo, false, false) => (
                    "Cargo.lock absent".to_string(),
                    FindingClassification::Informational,
                    FindingSeverity::None,
                    RecommendedAction::ReviewIfMaintainerPain,
                    "Cargo.lock is not present. Project shape is undetermined; \
                 conventions for this combination are not encoded."
                        .to_string(),
                    vec![],
                ),
                (CargoLockState::PresentButGitIgnored { .. }, _, true) => (
                    "Cargo.lock git-ignored (has a binary target)".to_string(),
                    FindingClassification::Actionable,
                    FindingSeverity::Low,
                    RecommendedAction::ReviewIfMaintainerPain,
                    "Cargo.lock exists in the working tree but matches a .gitignore pattern, \
                 and the project has a binary target. May be intentional or unintended."
                        .to_string(),
                    vec!["the project is broken".into()],
                ),
                (CargoLockState::PresentButGitIgnored { .. }, _, _) => (
                    "Cargo.lock present but git-ignored".to_string(),
                    FindingClassification::Informational,
                    FindingSeverity::None,
                    RecommendedAction::NoAction,
                    "Cargo.lock exists in the working tree but matches a .gitignore pattern. \
                 For libraries this is the conventional default."
                        .to_string(),
                    vec![
                        "the project is broken".into(),
                        "the gitignore entry is unintended".into(),
                    ],
                ),
            };

        let state_label = serde_json::to_value(state)
            .ok()
            .and_then(|v| v.get("state").and_then(|s| s.as_str()).map(str::to_string))
            .unwrap_or_else(|| "unknown".into());

        let lockfile_path = match state {
            CargoLockState::CommittedAndTracked { path }
            | CargoLockState::PresentButGitIgnored { path }
            | CargoLockState::PresentTrackingUnknown { path } => path.clone(),
            CargoLockState::AbsentFromRepo => "Cargo.lock".into(),
        };

        Some(EvidenceFinding {
            id: FindingId::new(F_CARGO_LOCK_STATE),
            title,
            classification,
            severity,
            knowledge_layer: KnowledgeLayer::Observation,
            // Phase 4 (2026-05-11): `profile.lockfileState` is backed by the
            // lockfile-state family (I-LOCKFILE-STATE-* catalogue, doc-06 §5
            // `confirmOnly`). Per-state recommended-action dispatch is a
            // derivation over the catalogued state → `Derived`. The
            // `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation is
            // already attached below so consumers see the partial check.
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::PROFILE_LOCKFILE_STATE.into(),
                observed_value: serde_json::json!(state_label),
                source_paths: vec![lockfile_path],
                source_type: EvidenceSource::FileInventory,
                invariant_id: None,
            }],
            inference: ResponsibleInference {
                statement,
                strength: InferenceStrength::Suggestive,
                depends_on: vec![format!(
                    "lockfileState={}",
                    serde_json::to_value(state)
                        .ok()
                        .and_then(|v| v.get("state").and_then(|s| s.as_str()).map(str::to_string))
                        .unwrap_or_default()
                )],
                does_not_imply,
            },
            limitations: vec![
                Limitation {
                    code: limitation_codes::LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL.into(),
                    detail: "A Cargo.lock with a git index entry is reported as \
                             tracked. Otherwise only a conservative root .gitignore match \
                             is checked (no wildcards or nested .gitignore files), so an \
                             untracked, unignored lockfile is reported as 'unknown'."
                        .into(),
                },
                Limitation {
                    code: limitation_codes::LIVE_ROOT_VS_PACK_TIME_DIVERGENCE_POSSIBLE.into(),
                    detail: "Lockfile state is read from the live working tree, not from \
                             pack-stored content. If the worktree changed between the pack \
                             scan and this report, this finding may disagree with the rest \
                             of the summary."
                        .into(),
                },
            ],
            action,
            scope: self.scope(vec![], vec!["rust".into()]),
            not_checked: vec![],
            repo_context: self.repo_context.tags.clone(),
            false_positive_notes: self.indicator_notes(),
            provenance: None,
        })
    }
}
