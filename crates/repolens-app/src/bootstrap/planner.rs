use crate::bootstrap::StartBootstrapRunRequest;
use crate::discover::PackResolver;
use crate::scan::ScanService;
use anyhow::{bail, Result};
use repolens_core::bootstrap::load_bootstrap_contract_snapshot;
use repolens_core::bootstrap_paths::{path_matches_any_pattern, BootstrapPathGuard};
use repolens_core::git::capture_clean_worktree_snapshot;
use repolens_core::model::bootstrap::{
    BootstrapSkill, BootstrapSkillCategory, FrozenBootstrapContract,
};
use repolens_core::model::bootstrap_run::{BootstrapDiscoverySummary, BootstrapRunPlan};
use repolens_core::storage::RepoStoragePaths;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct BootstrapPlanner;

impl BootstrapPlanner {
    pub fn build_start_plan(
        repo_root: &Path,
        run_id: &str,
        request: &StartBootstrapRunRequest,
    ) -> Result<(BootstrapRunPlan, FrozenBootstrapContract)> {
        let snapshot = load_bootstrap_contract_snapshot(repo_root.join("bootstrap.yaml"))?;
        let required_checks = validate_task_type(&snapshot.frozen, &request.task_type)?;
        validate_contract_boundary(&snapshot.frozen, &request.intended_paths)?;
        let _guard = BootstrapPathGuard::compile(repo_root, &request.intended_paths)?;
        let baseline = capture_clean_worktree_snapshot(repo_root)?;
        let storage_baseline = RepoStoragePaths::storage_fingerprints(repo_root);

        let discovery = resolve_discovery(repo_root)?;

        let plan = BootstrapRunPlan {
            schema_version: "1".to_string(),
            run_id: run_id.to_string(),
            started_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
            task_type: request.task_type.clone(),
            goal: request.goal.clone(),
            selected_skill_ids: selected_skill_ids_from_contract(&snapshot.frozen),
            baseline_head_commit: baseline.head_commit,
            baseline_porcelain: baseline.porcelain,
            baseline_contract_hash: snapshot.hash_sha256,
            contract_version: snapshot.frozen.version.clone(),
            bounded_paths: request.intended_paths.clone(),
            required_checks,
            discovery,
            storage_baseline: Some(storage_baseline),
        };

        Ok((plan, snapshot.frozen))
    }
}

fn selected_skill_ids_from_contract(contract: &FrozenBootstrapContract) -> Vec<String> {
    contract
        .skills
        .iter()
        .filter(|skill| is_phase_1_skill(skill))
        .map(|skill| skill.id.clone())
        .collect()
}

fn is_phase_1_skill(skill: &BootstrapSkill) -> bool {
    matches!(
        skill.category,
        BootstrapSkillCategory::Discovery
            | BootstrapSkillCategory::Planning
            | BootstrapSkillCategory::Execution
            | BootstrapSkillCategory::Verification
    )
}

fn resolve_discovery(repo_root: &Path) -> Result<BootstrapDiscoverySummary> {
    match PackResolver::new().discover(repo_root) {
        Ok(found) => Ok(BootstrapDiscoverySummary::cold_start_not_needed(
            found.latest_pack.to_string_lossy().to_string(),
        )),
        Err(err) if should_cold_start_discovery_error(&err) => {
            ScanService::new().run(repo_root, false, crate::cleanup::DEFAULT_KEEP_COUNT)?;
            let found = PackResolver::new().discover(repo_root)?;
            Ok(BootstrapDiscoverySummary {
                latest_pack_path: Some(found.latest_pack.to_string_lossy().to_string()),
                cold_start_used: true,
                note: "cold-start scan restored RepoLens perception".to_string(),
            })
        }
        Err(err) => Err(err),
    }
}

fn should_cold_start_discovery_error(err: &anyhow::Error) -> bool {
    crate::discover::is_no_usable_pack_error(err)
}

fn validate_task_type(
    contract: &FrozenBootstrapContract,
    task_type: &str,
) -> Result<Vec<Vec<String>>> {
    if !matches!(task_type, "docs_only" | "trust_contract") {
        bail!("unsupported task type for Phase 1 bootstrap start: {task_type}");
    }

    if !contract
        .task_types
        .iter()
        .any(|declared| declared == task_type)
    {
        bail!("task type not allowed by bootstrap contract: {task_type}");
    }

    contract
        .required_checks
        .get(task_type)
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!("task type missing required checks in bootstrap contract: {task_type}")
        })
}

fn validate_contract_boundary(
    contract: &FrozenBootstrapContract,
    requested_paths: &[String],
) -> Result<()> {
    for requested_path in requested_paths {
        let allowed = path_matches_any_pattern(&contract.allow_paths, requested_path)?;
        if !allowed {
            bail!(
                "intended path falls outside bootstrap contract boundary: {}",
                requested_path
            );
        }

        if path_matches_any_pattern(&contract.deny_paths, requested_path)? {
            bail!(
                "intended path is denied by bootstrap contract boundary: {}",
                requested_path
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_skill_ids_follow_the_frozen_contract_skill_list() {
        let snapshot = load_bootstrap_contract_snapshot(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap.yaml"),
        )
        .unwrap();

        assert_eq!(
            selected_skill_ids_from_contract(&snapshot.frozen),
            vec![
                "discover_repo_state".to_string(),
                "plan_with_trust".to_string(),
                "execute_bounded_change".to_string(),
                "verify_and_rescan".to_string(),
            ]
        );
    }

    #[test]
    fn cold_start_only_applies_to_missing_pack_errors() {
        let no_pack = crate::discover::no_usable_pack_error(std::path::Path::new("/tmp/repo"));
        assert!(should_cold_start_discovery_error(&no_pack));

        let other = anyhow::anyhow!("permission denied");
        assert!(!should_cold_start_discovery_error(&other));
    }

    #[test]
    fn contract_boundary_validation_surfaces_matcher_errors() {
        let mut contract = load_bootstrap_contract_snapshot(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap.yaml"),
        )
        .unwrap()
        .frozen;
        contract.allow_paths = vec!["/tmp/Cargo.toml".to_string()];

        let err = validate_contract_boundary(&contract, &["Cargo.toml".to_string()])
            .expect_err("invalid matcher configuration should surface");

        assert!(err.to_string().contains("repo-relative"));
    }
}
