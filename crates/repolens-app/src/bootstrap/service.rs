use crate::bootstrap::command_runner::BootstrapCommandRunner;
use crate::bootstrap::planner::BootstrapPlanner;
use crate::bootstrap::store::{
    ActiveRunLock, ActiveRunLockResult, BootstrapRunStore, TryActiveRunLockResult,
};
use crate::bootstrap::{
    FinishBootstrapRunRequest, StartBootstrapRunRequest, StartBootstrapRunResponse,
};
use anyhow::{anyhow, bail, ensure, Result};
use repolens_core::bootstrap_paths::BootstrapPathGuard;
use repolens_core::model::bootstrap_run::{
    BootstrapCommandResult, BootstrapDiscoverySummary, BootstrapPathValidationResult,
    BootstrapRescanResult, BootstrapRunPlan, BootstrapRunReport, BootstrapRunReportDetails,
    BootstrapRunStatus,
};
use repolens_core::storage::RepoStoragePaths;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use uuid::Uuid;

pub struct BootstrapService;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BoundedDiff {
    changed_files: Vec<String>,
    deleted_files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LiveContractState {
    changed: bool,
    reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FinishFailureContext {
    final_head_commit: Option<String>,
    live_contract_state: LiveContractState,
    diff: BoundedDiff,
    path_validation_result: BootstrapPathValidationResult,
    command_results: Vec<BootstrapCommandResult>,
    rescan_result: BootstrapRescanResult,
}

impl FinishFailureContext {
    fn into_report_details(
        self,
        finished_at: String,
        cold_start_decision: BootstrapDiscoverySummary,
    ) -> BootstrapRunReportDetails {
        BootstrapRunReportDetails {
            final_head_commit: self.final_head_commit,
            changed_files: self.diff.changed_files,
            deleted_files: self.diff.deleted_files,
            path_validation_result: self.path_validation_result,
            command_results: self.command_results,
            cold_start_decision,
            rescan_result: self.rescan_result,
            live_contract_changed: self.live_contract_state.changed,
            live_contract_change_reason: self.live_contract_state.reason,
            finished_at,
        }
    }
}

enum FinishLockOutcome {
    Locked(ActiveRunLock),
    Terminal(Box<BootstrapRunReport>),
    Inactive,
    MalformedActive(ActiveRunLock, anyhow::Error),
}

enum TryLockFinishOutcome {
    Busy,
    Resolved(FinishLockOutcome),
}

impl BootstrapService {
    pub fn new() -> Self {
        Self
    }

    pub fn start(&self, request: StartBootstrapRunRequest) -> Result<StartBootstrapRunResponse> {
        let active_path = RepoStoragePaths::bootstrap_active_path(&request.root);
        if active_path.exists() {
            if let Ok(active_run_id) = self.resolve_active_run(&request.root) {
                self.cleanup_terminal_markers_if_terminal(&request.root, &active_run_id)?;
            } else if let Some(hinted_run_id) =
                self.resolve_terminal_run_hint_from_current(&request.root)?
            {
                self.cleanup_terminal_markers_if_terminal_with_hint(&request.root, &hinted_run_id)?;
            }
            if active_path.exists() {
                bail!("bootstrap run already active: {}", active_path.display());
            }
        }

        let now = OffsetDateTime::now_utc();
        let run_id = format!(
            "run-{:04}{:02}{:02}-{:02}{:02}{:02}-{}",
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
            Uuid::new_v4()
        );
        let (plan, _contract) =
            BootstrapPlanner::build_start_plan(&request.root, &run_id, &request)?;
        let run_dir = BootstrapRunStore::begin_run(&request.root, &run_id, &plan)?;

        Ok(StartBootstrapRunResponse {
            run_id,
            run_dir: run_dir.to_string_lossy().to_string(),
            plan_path: run_dir.join("plan.json").to_string_lossy().to_string(),
            bounded_paths: plan.bounded_paths.clone(),
            required_checks: plan.required_checks.clone(),
            discovery: plan.discovery.clone(),
        })
    }

    fn resolve_active_run(&self, repo_root: &std::path::Path) -> Result<String> {
        let run_id = std::fs::read_to_string(RepoStoragePaths::bootstrap_active_path(repo_root))?;
        Self::validate_runtime_run_id(run_id.trim(), "bootstrap ACTIVE")?;
        Ok(run_id.trim().to_string())
    }

    fn validate_explicit_run_id(run_id: &str) -> Result<()> {
        ensure!(
            super::store::is_valid_runtime_run_id(run_id),
            "invalid bootstrap run id: {run_id}"
        );
        Ok(())
    }

    fn validate_runtime_run_id(run_id: &str, label: &str) -> Result<()> {
        ensure!(
            super::store::is_valid_runtime_run_id(run_id),
            "{label} is malformed or empty"
        );
        Ok(())
    }

    fn resolve_current_run(&self, repo_root: &std::path::Path) -> Result<String> {
        let current_path = RepoStoragePaths::bootstrap_current_path(repo_root);
        let run_id = std::fs::read_to_string(&current_path).map_err(|err| {
            // Repo-relative, and no OS error text when it's simply missing,
            // so the line stays short (#58 item 4).
            let shown = current_path
                .strip_prefix(repo_root)
                .unwrap_or(&current_path)
                .to_string_lossy()
                .replace('\\', "/");
            if err.kind() == std::io::ErrorKind::NotFound {
                anyhow!("bootstrap CURRENT is missing or unreadable: {shown}")
            } else {
                anyhow!("bootstrap CURRENT is missing or unreadable: {shown} ({err})")
            }
        })?;
        let run_id = run_id.trim();
        Self::validate_runtime_run_id(run_id, "bootstrap CURRENT")?;
        Ok(run_id.to_string())
    }

    fn try_load_terminal_report(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
    ) -> Result<Option<BootstrapRunReport>> {
        match BootstrapRunStore::load_report(repo_root, run_id)? {
            Some(report)
                if matches!(
                    report.status,
                    BootstrapRunStatus::Succeeded
                        | BootstrapRunStatus::Failed
                        | BootstrapRunStatus::Aborted
                        | BootstrapRunStatus::InvalidatedByHeadChange
                ) =>
            {
                Ok(Some(report))
            }
            Some(_) | None => Ok(None),
        }
    }

    fn resolve_terminal_run_hint_from_current(
        &self,
        repo_root: &std::path::Path,
    ) -> Result<Option<String>> {
        let current_path = RepoStoragePaths::bootstrap_current_path(repo_root);
        let run_id = match std::fs::read_to_string(&current_path) {
            Ok(run_id) => run_id,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let run_id = run_id.trim();
        if Self::validate_runtime_run_id(run_id, "bootstrap CURRENT").is_err() {
            return Ok(None);
        }
        if self.try_load_terminal_report(repo_root, run_id)?.is_some() {
            return Ok(Some(run_id.to_string()));
        }
        Ok(None)
    }

    pub fn finish(&self, request: FinishBootstrapRunRequest) -> Result<BootstrapRunReport> {
        let run_id = match request.run_id {
            Some(run_id) => {
                Self::validate_explicit_run_id(&run_id)?;
                run_id
            }
            None => self.resolve_current_run(&request.root)?,
        };
        if let Some(report) = self.cleanup_terminal_markers_if_terminal(&request.root, &run_id)? {
            return Ok(report);
        }

        // The whole storage folder is gone, run state included: say so in
        // one line instead of surfacing the raw OS error (#58).
        if !RepoStoragePaths::storage_dir(&request.root).exists() {
            bail!(".repolens was removed during bootstrap.");
        }
        let plan = BootstrapRunStore::load_plan(&request.root, &run_id)?;
        let lock = match self.lock_finish_run(&request.root, &run_id)? {
            FinishLockOutcome::Locked(lock) => lock,
            FinishLockOutcome::Terminal(report) => return Ok(*report),
            FinishLockOutcome::MalformedActive(_lock, err) => {
                let context = Self::collect_finish_failure_context(
                    &request.root,
                    &plan,
                    BootstrapPathValidationResult::not_run(
                        "bootstrap ACTIVE is malformed or empty",
                    ),
                    vec![],
                    BootstrapRescanResult::not_run("finish failed before rescan"),
                );
                self.persist_finish_failure(_lock, &plan, context, &err)?;
                return Err(err);
            }
            FinishLockOutcome::Inactive => {
                let err = anyhow!("bootstrap run is not active: {}", run_id);
                self.persist_finish_failure_without_active(&request.root, &run_id, &plan, &err)?;
                return Err(err);
            }
        };
        let head = match repolens_core::git::head_commit(&request.root) {
            Some(head) => head,
            None => {
                let err = anyhow!("HEAD is not resolvable");
                let live_contract_state = Self::inspect_live_contract_state(&request.root, &plan);
                self.persist_finish_failure(
                    lock,
                    &plan,
                    FinishFailureContext {
                        final_head_commit: None,
                        live_contract_state,
                        diff: BoundedDiff::default(),
                        path_validation_result: BootstrapPathValidationResult::not_run(
                            "HEAD is not resolvable",
                        ),
                        command_results: vec![],
                        rescan_result: BootstrapRescanResult::not_run(
                            "finish failed before rescan",
                        ),
                    },
                    &err,
                )?;
                return Err(err);
            }
        };
        let live_contract_state = Self::inspect_live_contract_state(&request.root, &plan);
        if head != plan.baseline_head_commit {
            let report = BootstrapRunReport::invalidated_by_head_change(
                &plan,
                head,
                live_contract_state.changed,
                live_contract_state.reason.clone(),
                Self::now_rfc3339()?,
            );
            BootstrapRunStore::finalize_terminal_transition(lock, &report)?;
            return Ok(report);
        }

        let guard = match BootstrapPathGuard::compile(&request.root, &plan.bounded_paths) {
            Ok(guard) => guard,
            Err(err) => {
                self.persist_finish_failure(
                    lock,
                    &plan,
                    FinishFailureContext {
                        final_head_commit: Some(head.clone()),
                        live_contract_state,
                        diff: BoundedDiff::default(),
                        path_validation_result: BootstrapPathValidationResult::not_run(
                            "bootstrap path guard compilation failed",
                        ),
                        command_results: vec![],
                        rescan_result: BootstrapRescanResult::not_run(
                            "finish failed before rescan",
                        ),
                    },
                    &err,
                )?;
                return Err(err);
            }
        };
        let diff = match Self::worktree_changes_with_storage(&request.root) {
            Ok(changes) => match Self::build_bounded_diff(
                &request.root,
                &plan,
                changes.changed,
                changes.deleted,
            ) {
                Ok(diff) => diff,
                Err(err) => {
                    self.persist_finish_failure(
                        lock,
                        &plan,
                        FinishFailureContext {
                            final_head_commit: Some(head.clone()),
                            live_contract_state,
                            diff: BoundedDiff::default(),
                            path_validation_result: BootstrapPathValidationResult::not_run(
                                "failed to build bounded diff",
                            ),
                            command_results: vec![],
                            rescan_result: BootstrapRescanResult::not_run(
                                "finish failed before rescan",
                            ),
                        },
                        &err,
                    )?;
                    return Err(err);
                }
            },
            Err(err) => {
                self.persist_finish_failure(
                    lock,
                    &plan,
                    FinishFailureContext {
                        final_head_commit: Some(head.clone()),
                        live_contract_state,
                        diff: BoundedDiff::default(),
                        path_validation_result: BootstrapPathValidationResult::not_run(
                            "failed to list worktree changes",
                        ),
                        command_results: vec![],
                        rescan_result: BootstrapRescanResult::not_run(
                            "finish failed before rescan",
                        ),
                    },
                    &err,
                )?;
                return Err(err);
            }
        };
        if diff.changed_files.is_empty() && diff.deleted_files.is_empty() {
            let err = anyhow!("bounded diff is empty");
            self.persist_finish_failure(
                lock,
                &plan,
                FinishFailureContext {
                    final_head_commit: Some(head.clone()),
                    live_contract_state,
                    diff,
                    path_validation_result: BootstrapPathValidationResult::passed(),
                    command_results: vec![],
                    rescan_result: BootstrapRescanResult::not_run("finish failed before rescan"),
                },
                &err,
            )?;
            return Err(err);
        }
        // Removed `.repolens/` entries first (one line each), then the first
        // boundary violation among the remaining paths, once.
        let removed: std::collections::BTreeSet<&String> = diff
            .changed_files
            .iter()
            .chain(diff.deleted_files.iter())
            .filter(|p| Self::removed_storage_entry(&request.root, &plan, p))
            .collect();
        let mut problems: Vec<String> = removed
            .iter()
            .map(|p| format!("{p} was removed during bootstrap."))
            .collect();
        // Every other problem once each (broken or escaping symlinks first),
        // then one boundary message listing every outside write (#58).
        let mut outside: Vec<String> = Vec::new();
        for path in diff
            .changed_files
            .iter()
            .chain(diff.deleted_files.iter())
            .filter(|p| !removed.contains(p))
        {
            match guard.is_outside_boundary(path) {
                Ok(true) => {
                    if !outside.contains(path) {
                        outside.push(path.clone());
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    let line = err.to_string();
                    if !problems.contains(&line) {
                        problems.push(line);
                    }
                }
            }
        }
        if !outside.is_empty() {
            problems.push(guard.boundary_report(&outside));
        }
        if !problems.is_empty() {
            let err = anyhow!(problems.join("\n"));
            self.persist_finish_failure(
                lock,
                &plan,
                FinishFailureContext {
                    final_head_commit: Some(head.clone()),
                    live_contract_state,
                    diff,
                    path_validation_result: BootstrapPathValidationResult::failed(err.to_string()),
                    command_results: vec![],
                    rescan_result: BootstrapRescanResult::not_run("finish failed before rescan"),
                },
                &err,
            )?;
            return Err(err);
        }
        let path_validation_result = BootstrapPathValidationResult::passed();

        let run_dir = RepoStoragePaths::bootstrap_run_dir(&request.root, &run_id);
        let command_results =
            match BootstrapCommandRunner::run_all(&request.root, &run_dir, &plan.required_checks) {
                Ok(results) => results,
                Err(err) => {
                    self.persist_finish_failure(
                        lock,
                        &plan,
                        FinishFailureContext {
                            final_head_commit: Some(head.clone()),
                            live_contract_state,
                            diff,
                            path_validation_result: path_validation_result.clone(),
                            command_results: vec![],
                            rescan_result: BootstrapRescanResult::not_run(
                                "finish failed before rescan",
                            ),
                        },
                        &err,
                    )?;
                    return Err(err);
                }
            };
        if command_results.iter().any(|result| !result.succeeded) {
            let report = BootstrapRunReport::failed(
                &plan,
                BootstrapRunReportDetails {
                    final_head_commit: Some(head),
                    changed_files: diff.changed_files,
                    deleted_files: diff.deleted_files,
                    path_validation_result,
                    command_results,
                    cold_start_decision: plan.discovery.clone(),
                    rescan_result: BootstrapRescanResult::not_run(
                        "required bootstrap checks failed",
                    ),
                    live_contract_changed: live_contract_state.changed,
                    live_contract_change_reason: live_contract_state.reason.clone(),
                    finished_at: Self::now_rfc3339()?,
                },
                "required bootstrap checks failed",
            );
            BootstrapRunStore::finalize_terminal_transition(lock, &report)?;
            return Ok(report);
        }

        let scan = match crate::scan::ScanService::new().run(
            &request.root,
            false,
            crate::cleanup::DEFAULT_KEEP_COUNT,
        ) {
            Ok(scan) => scan,
            Err(err) => {
                self.persist_finish_failure(
                    lock,
                    &plan,
                    FinishFailureContext {
                        final_head_commit: Some(head.clone()),
                        live_contract_state,
                        diff,
                        path_validation_result: path_validation_result.clone(),
                        command_results,
                        rescan_result: BootstrapRescanResult::failed(err.to_string()),
                    },
                    &err,
                )?;
                return Err(err);
            }
        };
        let rescan_result = BootstrapRescanResult::succeeded(
            scan.scan_mode.clone(),
            scan.pack_dir.to_string_lossy().to_string(),
        );
        let report = BootstrapRunReport::succeeded(
            &plan,
            BootstrapRunReportDetails {
                final_head_commit: Some(head),
                changed_files: diff.changed_files,
                deleted_files: diff.deleted_files,
                path_validation_result,
                command_results,
                cold_start_decision: plan.discovery.clone(),
                rescan_result,
                live_contract_changed: live_contract_state.changed,
                live_contract_change_reason: live_contract_state.reason.clone(),
                finished_at: Self::now_rfc3339()?,
            },
            scan.pack_dir,
        );
        BootstrapRunStore::finalize_terminal_transition(lock, &report)?;
        Ok(report)
    }

    pub fn abort(
        &self,
        repo_root: &std::path::Path,
        run_id: Option<&str>,
    ) -> Result<BootstrapRunReport> {
        let run_id = match run_id {
            Some(run_id) => {
                Self::validate_explicit_run_id(run_id)?;
                run_id.to_owned()
            }
            None => self.resolve_current_run(repo_root)?,
        };
        if self
            .cleanup_terminal_markers_if_terminal(repo_root, &run_id)?
            .is_some()
        {
            bail!("bootstrap run is already terminal: {}", run_id);
        }

        let plan = BootstrapRunStore::load_plan(repo_root, &run_id)?;
        let mut lock = match self.lock_abort_run(repo_root, &run_id)? {
            ActiveRunLockResult::Locked(lock) => lock,
            ActiveRunLockResult::NotActive => {
                if self
                    .cleanup_terminal_markers_if_terminal(repo_root, &run_id)?
                    .is_some()
                {
                    bail!("bootstrap run is already terminal: {}", run_id);
                }
                bail!("bootstrap run is not active: {}", run_id);
            }
        };
        if self.try_load_terminal_report(repo_root, &run_id)?.is_some() {
            if BootstrapRunStore::assert_active_run_ownership(&mut lock).is_ok() {
                BootstrapRunStore::clear_terminal_markers(lock)?;
            }
            bail!("bootstrap run is already terminal: {}", run_id);
        }
        BootstrapRunStore::assert_active_run_ownership(&mut lock)?;
        let report = BootstrapRunReport::aborted(&plan, Self::now_rfc3339()?);
        BootstrapRunStore::finalize_terminal_transition(lock, &report)?;
        Ok(report)
    }

    fn inspect_live_contract_state(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
    ) -> LiveContractState {
        match repolens_core::bootstrap::hash_file_sha256(repo_root.join("bootstrap.yaml")) {
            Ok(live_hash) if live_hash == plan.baseline_contract_hash => {
                LiveContractState::default()
            }
            Ok(_) => LiveContractState {
                changed: true,
                reason: Some("live bootstrap.yaml differs from frozen snapshot".to_string()),
            },
            Err(err) => LiveContractState {
                changed: true,
                reason: Some(format!("failed to read live bootstrap.yaml: {err}")),
            },
        }
    }

    fn build_bounded_diff(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        changed: Vec<String>,
        deleted: Vec<String>,
    ) -> Result<BoundedDiff> {
        Ok(BoundedDiff {
            changed_files: Self::normalize_diff_paths(repo_root, plan, changed)?,
            deleted_files: Self::normalize_diff_paths(repo_root, plan, deleted)?,
        })
    }

    fn collect_finish_failure_context(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        path_validation_result: BootstrapPathValidationResult,
        command_results: Vec<BootstrapCommandResult>,
        rescan_result: BootstrapRescanResult,
    ) -> FinishFailureContext {
        let diff = match Self::worktree_changes_with_storage(repo_root) {
            Ok(changes) => {
                Self::build_bounded_diff(repo_root, plan, changes.changed, changes.deleted)
                    .unwrap_or_default()
            }
            Err(_) => BoundedDiff::default(),
        };
        FinishFailureContext {
            final_head_commit: repolens_core::git::head_commit(repo_root),
            live_contract_state: Self::inspect_live_contract_state(repo_root, plan),
            diff,
            path_validation_result,
            command_results,
            rescan_result,
        }
    }

    fn normalize_diff_paths(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        paths: Vec<String>,
    ) -> Result<Vec<String>> {
        let mut filtered = std::collections::BTreeSet::new();
        for path in paths {
            for normalized in Self::classify_diff_path(repo_root, plan, &path)? {
                filtered.insert(normalized);
            }
        }
        Ok(filtered.into_iter().collect())
    }

    fn classify_diff_path(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        path: &str,
    ) -> Result<Vec<String>> {
        if Self::is_bootstrap_runtime_artifact(path)
            || Self::is_expected_cold_start_artifact(repo_root, plan, path)?
        {
            return Ok(vec![]);
        }
        if path == ".repolens" || path == ".repolens/" {
            return Self::classify_repolens_root(repo_root, plan);
        }
        if let Some(rest) = path.strip_prefix(".repolens/") {
            // Git lists `.repolens/` files one by one once RepoLens's own
            // `.repolens/.gitignore` is gone. Skip entries untouched since
            // start and report entries that went missing, so the message
            // names what actually changed (#58).
            let top = format!(".repolens/{}", rest.split('/').next().unwrap_or(rest));
            let now = RepoStoragePaths::storage_fingerprints(repo_root);
            let unchanged = plan
                .storage_baseline
                .as_ref()
                .and_then(|b| b.get(&top))
                .is_some_and(|fp| now.get(&top) == Some(fp));
            let mut out = Self::removed_baseline_entries(repo_root, plan, &now)?;
            if !unchanged {
                out.push(path.to_string());
            }
            return Ok(out);
        }
        Ok(vec![path.to_string()])
    }

    /// Top-level `.repolens/` entries recorded at start that are gone now,
    /// RepoLens's own `.gitignore` included.
    fn removed_baseline_entries(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        now: &std::collections::BTreeMap<String, String>,
    ) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for rel in plan.storage_baseline.iter().flat_map(|b| b.keys()) {
            if now.contains_key(rel) {
                continue;
            }
            let runtime = Self::is_bootstrap_runtime_artifact(rel) && rel != ".repolens/.gitignore";
            if !runtime && !Self::is_expected_cold_start_artifact(repo_root, plan, rel)? {
                out.push(rel.clone());
            }
        }
        Ok(out)
    }

    fn classify_repolens_root(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
    ) -> Result<Vec<String>> {
        let repolens_dir = RepoStoragePaths::storage_dir(repo_root);
        if !repolens_dir.exists() {
            return Ok(vec![".repolens".to_string()]);
        }

        let now = RepoStoragePaths::storage_fingerprints(repo_root);
        let mut unexpected = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(&repolens_dir)? {
            let entry = entry?;
            let rel = format!(".repolens/{}", entry.file_name().to_string_lossy());
            if Self::is_bootstrap_runtime_artifact(&rel)
                || Self::is_expected_cold_start_artifact(repo_root, plan, &rel)?
            {
                continue;
            }
            // Present at start and untouched since: not part of this run.
            let unchanged = plan
                .storage_baseline
                .as_ref()
                .and_then(|b| b.get(&rel))
                .is_some_and(|fp| now.get(&rel) == Some(fp));
            if unchanged {
                continue;
            }
            unexpected.insert(rel);
        }
        // Recorded at start but gone now: reported by `removed_storage_entry`.
        for rel in plan.storage_baseline.iter().flat_map(|b| b.keys()) {
            if !now.contains_key(rel)
                && !Self::is_bootstrap_runtime_artifact(rel)
                && !Self::is_expected_cold_start_artifact(repo_root, plan, rel)?
            {
                unexpected.insert(rel.clone());
            }
        }

        Ok(unexpected.into_iter().collect())
    }

    /// A top-level `.repolens/` entry recorded at start that no longer exists.
    fn removed_storage_entry(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        path: &str,
    ) -> bool {
        plan.storage_baseline
            .as_ref()
            .is_some_and(|b| b.contains_key(path))
            && std::fs::symlink_metadata(repo_root.join(path)).is_err()
    }

    fn persist_finish_failure(
        &self,
        lock: ActiveRunLock,
        plan: &BootstrapRunPlan,
        context: FinishFailureContext,
        err: &anyhow::Error,
    ) -> Result<()> {
        let report = BootstrapRunReport::failed(
            plan,
            context.into_report_details(Self::now_rfc3339()?, plan.discovery.clone()),
            err.to_string(),
        );
        BootstrapRunStore::finalize_terminal_transition(lock, &report)
    }

    fn persist_finish_failure_without_active(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
        plan: &BootstrapRunPlan,
        err: &anyhow::Error,
    ) -> Result<()> {
        let final_head_commit = repolens_core::git::head_commit(repo_root);
        let live_contract_state = Self::inspect_live_contract_state(repo_root, plan);
        let diff = match Self::worktree_changes_with_storage(repo_root) {
            Ok(changes) => {
                Self::build_bounded_diff(repo_root, plan, changes.changed, changes.deleted)
                    .unwrap_or_default()
            }
            Err(_) => BoundedDiff::default(),
        };
        let report = BootstrapRunReport::failed(
            plan,
            FinishFailureContext {
                final_head_commit,
                live_contract_state,
                diff,
                path_validation_result: BootstrapPathValidationResult::not_run(
                    "bootstrap run is not active",
                ),
                command_results: vec![],
                rescan_result: BootstrapRescanResult::not_run("finish failed before rescan"),
            }
            .into_report_details(Self::now_rfc3339()?, plan.discovery.clone()),
            err.to_string(),
        );
        BootstrapRunStore::finalize_terminal_transition_without_active(repo_root, run_id, &report)
    }

    fn lock_finish_run(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
    ) -> Result<FinishLockOutcome> {
        for _ in 0..BootstrapRunStore::TERMINAL_REPORT_POLL_ATTEMPTS {
            match self.try_lock_finish_run_once(repo_root, run_id)? {
                TryLockFinishOutcome::Busy => {
                    std::thread::sleep(BootstrapRunStore::TERMINAL_REPORT_POLL_INTERVAL);
                }
                TryLockFinishOutcome::Resolved(outcome) => return Ok(outcome),
            }
        }

        bail!("bootstrap run is already being finalized: {}", run_id)
    }

    fn try_lock_finish_run_once(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
    ) -> Result<TryLockFinishOutcome> {
        if let Some(report) = self.cleanup_terminal_markers_if_terminal(repo_root, run_id)? {
            return Ok(TryLockFinishOutcome::Resolved(FinishLockOutcome::Terminal(
                Box::new(report),
            )));
        }

        let mut lock = match BootstrapRunStore::try_lock_active_run(repo_root, run_id)? {
            TryActiveRunLockResult::Locked(lock) => lock,
            TryActiveRunLockResult::NotActive => {
                if let Some(report) =
                    self.cleanup_terminal_markers_if_terminal(repo_root, run_id)?
                {
                    return Ok(TryLockFinishOutcome::Resolved(FinishLockOutcome::Terminal(
                        Box::new(report),
                    )));
                }
                return Ok(TryLockFinishOutcome::Resolved(FinishLockOutcome::Inactive));
            }
            TryActiveRunLockResult::Busy => return Ok(TryLockFinishOutcome::Busy),
        };

        if let Some(report) = self.try_load_terminal_report(repo_root, run_id)? {
            if BootstrapRunStore::assert_active_run_ownership(&mut lock).is_ok() {
                BootstrapRunStore::clear_terminal_markers(lock)?;
            }
            return Ok(TryLockFinishOutcome::Resolved(FinishLockOutcome::Terminal(
                Box::new(report),
            )));
        }
        if let Err(err) = BootstrapRunStore::assert_active_run_ownership(&mut lock) {
            if err.to_string().contains("malformed or empty") {
                return Ok(TryLockFinishOutcome::Resolved(
                    FinishLockOutcome::MalformedActive(lock, err),
                ));
            }
            return Err(err);
        }
        Ok(TryLockFinishOutcome::Resolved(FinishLockOutcome::Locked(
            lock,
        )))
    }

    fn lock_abort_run(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
    ) -> Result<ActiveRunLockResult> {
        for _ in 0..BootstrapRunStore::TERMINAL_REPORT_POLL_ATTEMPTS {
            if self
                .cleanup_terminal_markers_if_terminal(repo_root, run_id)?
                .is_some()
            {
                return Ok(ActiveRunLockResult::NotActive);
            }

            match BootstrapRunStore::try_lock_active_run(repo_root, run_id)? {
                TryActiveRunLockResult::Locked(lock) => {
                    return Ok(ActiveRunLockResult::Locked(lock));
                }
                TryActiveRunLockResult::NotActive => {
                    if self
                        .cleanup_terminal_markers_if_terminal(repo_root, run_id)?
                        .is_some()
                    {
                        return Ok(ActiveRunLockResult::NotActive);
                    }
                    return Ok(ActiveRunLockResult::NotActive);
                }
                TryActiveRunLockResult::Busy => {
                    std::thread::sleep(BootstrapRunStore::TERMINAL_REPORT_POLL_INTERVAL);
                }
            }
        }

        bail!("bootstrap run is already being finalized: {}", run_id)
    }

    fn cleanup_terminal_markers_if_terminal(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
    ) -> Result<Option<BootstrapRunReport>> {
        self.cleanup_terminal_markers_if_terminal_impl(repo_root, run_id, false)
    }

    fn cleanup_terminal_markers_if_terminal_with_hint(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
    ) -> Result<Option<BootstrapRunReport>> {
        self.cleanup_terminal_markers_if_terminal_impl(repo_root, run_id, true)
    }

    fn cleanup_terminal_markers_if_terminal_impl(
        &self,
        repo_root: &std::path::Path,
        run_id: &str,
        allow_malformed_active_hint: bool,
    ) -> Result<Option<BootstrapRunReport>> {
        let report = self.try_load_terminal_report(repo_root, run_id)?;
        if report.is_some() {
            if allow_malformed_active_hint {
                BootstrapRunStore::cleanup_stale_terminal_markers_with_hint(repo_root, run_id)?;
            } else {
                BootstrapRunStore::cleanup_stale_terminal_markers(repo_root, run_id)?;
            }
        }
        Ok(report)
    }

    fn now_rfc3339() -> Result<String> {
        Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
    }

    fn is_bootstrap_runtime_artifact(repo_rel_path: &str) -> bool {
        repo_rel_path == ".repolens/bootstrap"
            || repo_rel_path.starts_with(".repolens/bootstrap/")
            // RepoLens-managed self-ignore file (see RepoStoragePaths::ensure_storage_ignored).
            || repo_rel_path == ".repolens/.gitignore"
    }

    /// Worktree changes as git sees them, plus `.repolens` itself when the
    /// only thing hiding it from git is RepoLens's own `.repolens/.gitignore`
    /// (see `RepoStoragePaths::ensure_storage_ignored`). That keeps the
    /// bootstrap boundary audit identical to the pre-self-ignore behaviour,
    /// where an untracked `.repolens/` was reported by git and audited by
    /// `classify_repolens_root`. If the user ignores `.repolens` themselves
    /// (root `.gitignore`, e.g. via `repolens init`, or `.git/info/exclude`),
    /// the folder stays out of the audit exactly as before.
    fn worktree_changes_with_storage(
        repo_root: &std::path::Path,
    ) -> Result<repolens_core::git::ChangeSet> {
        let mut changes = repolens_core::git::list_worktree_changes(repo_root)?;
        let git_sees_storage = changes
            .changed
            .iter()
            .chain(changes.deleted.iter())
            .any(|p| p == ".repolens" || p.starts_with(".repolens/"));
        if !git_sees_storage && Self::storage_hidden_only_by_self_ignore(repo_root) {
            changes.changed.push(".repolens".to_string());
        }
        Ok(changes)
    }

    fn storage_hidden_only_by_self_ignore(repo_root: &std::path::Path) -> bool {
        let storage = RepoStoragePaths::storage_dir(repo_root);
        if !storage.is_dir() {
            return false;
        }
        let self_ignored = std::fs::read_to_string(storage.join(".gitignore"))
            .map(|s| s.lines().any(|l| l.trim() == "*"))
            .unwrap_or(false);
        let lists_storage = |path: std::path::PathBuf| {
            std::fs::read_to_string(path)
                .map(|s| {
                    s.lines().any(|l| {
                        matches!(
                            l.trim(),
                            ".repolens" | ".repolens/" | "/.repolens" | "/.repolens/"
                        )
                    })
                })
                .unwrap_or(false)
        };
        let user_ignored = lists_storage(repo_root.join(".gitignore"))
            || repolens_core::git::info_exclude_path(repo_root).is_some_and(lists_storage);
        self_ignored && !user_ignored
    }

    fn is_expected_cold_start_artifact(
        repo_root: &std::path::Path,
        plan: &BootstrapRunPlan,
        repo_rel_path: &str,
    ) -> Result<bool> {
        if !plan.discovery.cold_start_used {
            return Ok(false);
        }

        if repo_rel_path == ".repolens/session.json" {
            return Ok(true);
        }

        let Some(latest_pack_path) = &plan.discovery.latest_pack_path else {
            return Ok(false);
        };
        let latest_pack_path = std::path::Path::new(latest_pack_path);
        let latest_pack_rel = latest_pack_path
            .strip_prefix(repo_root)
            .unwrap_or(latest_pack_path)
            .to_string_lossy()
            .replace('\\', "/");
        Ok(repo_rel_path == latest_pack_rel
            || repo_rel_path.starts_with(&format!("{latest_pack_rel}/")))
    }
}

impl Default for BootstrapService {
    fn default() -> Self {
        Self::new()
    }
}
