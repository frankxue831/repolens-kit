use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapRunStatus {
    Active,
    Succeeded,
    Failed,
    Aborted,
    InvalidatedByHeadChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapDiscoverySummary {
    pub latest_pack_path: Option<String>,
    pub cold_start_used: bool,
    pub note: String,
}

impl BootstrapDiscoverySummary {
    pub fn cold_start_not_needed(latest_pack_path: String) -> Self {
        Self {
            latest_pack_path: Some(latest_pack_path),
            cold_start_used: false,
            note: "existing pack usable".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapPathValidationStatus {
    NotRun,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPathValidationResult {
    pub status: BootstrapPathValidationStatus,
    pub reason: Option<String>,
}

impl BootstrapPathValidationResult {
    pub fn not_run(reason: impl Into<String>) -> Self {
        Self {
            status: BootstrapPathValidationStatus::NotRun,
            reason: Some(reason.into()),
        }
    }

    pub fn passed() -> Self {
        Self {
            status: BootstrapPathValidationStatus::Passed,
            reason: None,
        }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            status: BootstrapPathValidationStatus::Failed,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapRescanStatus {
    NotRun,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapRescanResult {
    pub status: BootstrapRescanStatus,
    pub scan_mode: Option<String>,
    pub resulting_pack_path: Option<String>,
    pub reason: Option<String>,
}

impl BootstrapRescanResult {
    pub fn not_run(reason: impl Into<String>) -> Self {
        Self {
            status: BootstrapRescanStatus::NotRun,
            scan_mode: None,
            resulting_pack_path: None,
            reason: Some(reason.into()),
        }
    }

    pub fn succeeded(scan_mode: String, resulting_pack_path: String) -> Self {
        Self {
            status: BootstrapRescanStatus::Succeeded,
            scan_mode: Some(scan_mode),
            resulting_pack_path: Some(resulting_pack_path),
            reason: None,
        }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            status: BootstrapRescanStatus::Failed,
            scan_mode: None,
            resulting_pack_path: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapRunPlan {
    pub schema_version: String,
    pub run_id: String,
    pub started_at: String,
    pub task_type: String,
    pub goal: String,
    pub selected_skill_ids: Vec<String>,
    pub baseline_head_commit: String,
    pub baseline_porcelain: Vec<String>,
    pub baseline_contract_hash: String,
    pub contract_version: String,
    pub bounded_paths: Vec<String>,
    pub required_checks: Vec<Vec<String>>,
    pub discovery: BootstrapDiscoverySummary,
    /// `.repolens/` entries present at start, with fingerprints (see
    /// `RepoStoragePaths::storage_fingerprints`). Entries unchanged since start
    /// are not boundary violations. Absent in plans written before this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_baseline: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapCommandResult {
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout_log_path: String,
    pub stderr_log_path: String,
    pub succeeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapRunReportDetails {
    pub final_head_commit: Option<String>,
    pub changed_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub path_validation_result: BootstrapPathValidationResult,
    pub command_results: Vec<BootstrapCommandResult>,
    pub cold_start_decision: BootstrapDiscoverySummary,
    pub rescan_result: BootstrapRescanResult,
    pub live_contract_changed: bool,
    pub live_contract_change_reason: Option<String>,
    pub finished_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapRunReport {
    pub schema_version: String,
    pub run_id: String,
    pub status: BootstrapRunStatus,
    pub started_at: String,
    pub finished_at: String,
    pub task_type: String,
    pub goal: String,
    pub selected_skill_ids: Vec<String>,
    pub intended_path_patterns: Vec<String>,
    pub baseline_head_commit: String,
    pub final_head_commit: Option<String>,
    pub contract_version: String,
    pub baseline_contract_hash: String,
    pub live_contract_changed: bool,
    pub live_contract_change_reason: Option<String>,
    pub changed_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub path_validation_result: BootstrapPathValidationResult,
    pub command_results: Vec<BootstrapCommandResult>,
    pub cold_start_decision: BootstrapDiscoverySummary,
    pub rescan_result: BootstrapRescanResult,
    pub resulting_pack_path: Option<String>,
    pub failure_reason: Option<String>,
    pub memory_decision: String,
}

impl BootstrapRunReport {
    pub fn succeeded(
        plan: &BootstrapRunPlan,
        details: BootstrapRunReportDetails,
        resulting_pack_path: std::path::PathBuf,
    ) -> Self {
        Self {
            schema_version: "1".to_string(),
            run_id: plan.run_id.clone(),
            status: BootstrapRunStatus::Succeeded,
            started_at: plan.started_at.clone(),
            finished_at: details.finished_at,
            task_type: plan.task_type.clone(),
            goal: plan.goal.clone(),
            selected_skill_ids: plan.selected_skill_ids.clone(),
            intended_path_patterns: plan.bounded_paths.clone(),
            baseline_head_commit: plan.baseline_head_commit.clone(),
            final_head_commit: details.final_head_commit,
            contract_version: plan.contract_version.clone(),
            baseline_contract_hash: plan.baseline_contract_hash.clone(),
            live_contract_changed: details.live_contract_changed,
            live_contract_change_reason: details.live_contract_change_reason,
            changed_files: details.changed_files,
            deleted_files: details.deleted_files,
            path_validation_result: details.path_validation_result,
            command_results: details.command_results,
            cold_start_decision: details.cold_start_decision,
            rescan_result: details.rescan_result,
            resulting_pack_path: Some(resulting_pack_path.to_string_lossy().to_string()),
            failure_reason: None,
            memory_decision: "deferred".to_string(),
        }
    }

    pub fn failed(
        plan: &BootstrapRunPlan,
        details: BootstrapRunReportDetails,
        failure_reason: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: "1".to_string(),
            run_id: plan.run_id.clone(),
            status: BootstrapRunStatus::Failed,
            started_at: plan.started_at.clone(),
            finished_at: details.finished_at,
            task_type: plan.task_type.clone(),
            goal: plan.goal.clone(),
            selected_skill_ids: plan.selected_skill_ids.clone(),
            intended_path_patterns: plan.bounded_paths.clone(),
            baseline_head_commit: plan.baseline_head_commit.clone(),
            final_head_commit: details.final_head_commit,
            contract_version: plan.contract_version.clone(),
            baseline_contract_hash: plan.baseline_contract_hash.clone(),
            live_contract_changed: details.live_contract_changed,
            live_contract_change_reason: details.live_contract_change_reason,
            changed_files: details.changed_files,
            deleted_files: details.deleted_files,
            path_validation_result: details.path_validation_result,
            command_results: details.command_results,
            cold_start_decision: details.cold_start_decision,
            rescan_result: details.rescan_result,
            resulting_pack_path: None,
            failure_reason: Some(failure_reason.into()),
            memory_decision: "deferred".to_string(),
        }
    }

    pub fn aborted(plan: &BootstrapRunPlan, finished_at: String) -> Self {
        Self {
            schema_version: "1".to_string(),
            run_id: plan.run_id.clone(),
            status: BootstrapRunStatus::Aborted,
            started_at: plan.started_at.clone(),
            finished_at,
            task_type: plan.task_type.clone(),
            goal: plan.goal.clone(),
            selected_skill_ids: plan.selected_skill_ids.clone(),
            intended_path_patterns: plan.bounded_paths.clone(),
            baseline_head_commit: plan.baseline_head_commit.clone(),
            final_head_commit: None,
            contract_version: plan.contract_version.clone(),
            baseline_contract_hash: plan.baseline_contract_hash.clone(),
            live_contract_changed: false,
            live_contract_change_reason: None,
            changed_files: vec![],
            deleted_files: vec![],
            path_validation_result: BootstrapPathValidationResult::not_run("run aborted"),
            command_results: vec![],
            cold_start_decision: plan.discovery.clone(),
            rescan_result: BootstrapRescanResult::not_run("run aborted"),
            resulting_pack_path: None,
            failure_reason: None,
            memory_decision: "deferred".to_string(),
        }
    }

    pub fn invalidated_by_head_change(
        plan: &BootstrapRunPlan,
        final_head_commit: String,
        live_contract_changed: bool,
        live_contract_change_reason: Option<String>,
        finished_at: String,
    ) -> Self {
        Self {
            schema_version: "1".to_string(),
            run_id: plan.run_id.clone(),
            status: BootstrapRunStatus::InvalidatedByHeadChange,
            started_at: plan.started_at.clone(),
            finished_at,
            task_type: plan.task_type.clone(),
            goal: plan.goal.clone(),
            selected_skill_ids: plan.selected_skill_ids.clone(),
            intended_path_patterns: plan.bounded_paths.clone(),
            baseline_head_commit: plan.baseline_head_commit.clone(),
            final_head_commit: Some(final_head_commit),
            contract_version: plan.contract_version.clone(),
            baseline_contract_hash: plan.baseline_contract_hash.clone(),
            live_contract_changed,
            live_contract_change_reason,
            changed_files: vec![],
            deleted_files: vec![],
            path_validation_result: BootstrapPathValidationResult::not_run(
                "run invalidated by HEAD change",
            ),
            command_results: vec![],
            cold_start_decision: plan.discovery.clone(),
            rescan_result: BootstrapRescanResult::not_run("run invalidated by HEAD change"),
            resulting_pack_path: None,
            failure_reason: None,
            memory_decision: "deferred".to_string(),
        }
    }
}
