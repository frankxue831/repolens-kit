use repolens_core::model::bootstrap_run::{
    BootstrapCommandResult, BootstrapDiscoverySummary, BootstrapPathValidationResult,
    BootstrapRescanResult, BootstrapRunPlan, BootstrapRunReport, BootstrapRunReportDetails,
    BootstrapRunStatus,
};
use repolens_core::storage::RepoStoragePaths;
use serde_json::json;
use std::path::Path;

fn sample_plan() -> BootstrapRunPlan {
    BootstrapRunPlan {
        schema_version: "1".to_string(),
        run_id: "run-1".to_string(),
        started_at: "2026-04-14T12:34:56Z".to_string(),
        task_type: "docs_only".to_string(),
        goal: "Update trust docs".to_string(),
        selected_skill_ids: vec![
            "discover_repo_state".to_string(),
            "plan_with_trust".to_string(),
            "execute_bounded_change".to_string(),
            "verify_and_rescan".to_string(),
        ],
        baseline_head_commit: "abc123".to_string(),
        baseline_porcelain: vec![],
        baseline_contract_hash: "deadbeef".repeat(8),
        contract_version: "0.1".to_string(),
        bounded_paths: vec!["docs/**".to_string()],
        required_checks: vec![vec![
            "cargo".to_string(),
            "fmt".to_string(),
            "--all".to_string(),
        ]],
        discovery: BootstrapDiscoverySummary::cold_start_not_needed(".repolens/pack-1".into()),
        storage_baseline: None,
    }
}

fn sample_command_result(succeeded: bool) -> BootstrapCommandResult {
    BootstrapCommandResult {
        argv: vec!["cargo".to_string(), "fmt".to_string(), "--all".to_string()],
        exit_code: if succeeded { Some(0) } else { Some(1) },
        stdout_log_path: ".repolens/bootstrap/run-1/stdout.log".to_string(),
        stderr_log_path: ".repolens/bootstrap/run-1/stderr.log".to_string(),
        succeeded,
    }
}

#[test]
fn bootstrap_run_plan_serializes_with_camel_case() {
    let value = serde_json::to_value(sample_plan()).unwrap();
    assert_eq!(value["schemaVersion"], json!("1"));
    assert_eq!(value["runId"], json!("run-1"));
    assert_eq!(value["startedAt"], json!("2026-04-14T12:34:56Z"));
    assert_eq!(
        value["selectedSkillIds"],
        json!([
            "discover_repo_state",
            "plan_with_trust",
            "execute_bounded_change",
            "verify_and_rescan"
        ])
    );
    assert_eq!(value["baselineHeadCommit"], json!("abc123"));
}

#[test]
fn bootstrap_run_status_serializes_with_snake_case() {
    let value = serde_json::to_value(BootstrapRunStatus::InvalidatedByHeadChange).unwrap();
    assert_eq!(value, json!("invalidated_by_head_change"));
}

#[test]
fn bootstrap_run_report_serializes_constructor_fields() {
    let plan = sample_plan();
    let succeeded = BootstrapRunReport::succeeded(
        &plan,
        BootstrapRunReportDetails {
            final_head_commit: Some("def456".to_string()),
            changed_files: vec!["docs/trust.md".to_string()],
            deleted_files: vec!["docs/obsolete.md".to_string()],
            path_validation_result: BootstrapPathValidationResult::passed(),
            command_results: vec![sample_command_result(true)],
            cold_start_decision: plan.discovery.clone(),
            rescan_result: BootstrapRescanResult::succeeded(
                "incremental".to_string(),
                "/tmp/repo/.repolens/pack-2".to_string(),
            ),
            live_contract_changed: true,
            live_contract_change_reason: Some(
                "live bootstrap.yaml differs from frozen snapshot".to_string(),
            ),
            finished_at: "2026-04-14T12:40:00Z".to_string(),
        },
        Path::new("/tmp/repo/.repolens/pack-2").to_path_buf(),
    );

    let value = serde_json::to_value(&succeeded).unwrap();
    assert_eq!(value["status"], json!("succeeded"));
    assert_eq!(value["taskType"], json!("docs_only"));
    assert_eq!(value["goal"], json!("Update trust docs"));
    assert_eq!(
        value["selectedSkillIds"],
        json!([
            "discover_repo_state",
            "plan_with_trust",
            "execute_bounded_change",
            "verify_and_rescan"
        ])
    );
    assert_eq!(value["intendedPathPatterns"], json!(["docs/**"]));
    assert_eq!(
        value["pathValidationResult"],
        json!({"status": "passed", "reason": null})
    );
    assert_eq!(
        value["coldStartDecision"],
        json!({
            "latestPackPath": ".repolens/pack-1",
            "coldStartUsed": false,
            "note": "existing pack usable"
        })
    );
    assert_eq!(
        value["rescanResult"],
        json!({
            "status": "succeeded",
            "scanMode": "incremental",
            "resultingPackPath": "/tmp/repo/.repolens/pack-2",
            "reason": null
        })
    );
    assert_eq!(value["startedAt"], json!("2026-04-14T12:34:56Z"));
    assert_eq!(value["finishedAt"], json!("2026-04-14T12:40:00Z"));
    assert_eq!(value["baselineContractHash"], json!("deadbeef".repeat(8)));
    assert_eq!(
        value["resultingPackPath"],
        json!("/tmp/repo/.repolens/pack-2")
    );
    assert_eq!(value["memoryDecision"], json!("deferred"));
    assert_eq!(value["finalHeadCommit"], json!("def456"));
    assert_eq!(value["liveContractChanged"], json!(true));
    assert_eq!(
        value["liveContractChangeReason"],
        json!("live bootstrap.yaml differs from frozen snapshot")
    );
    assert_eq!(value["deletedFiles"], json!(["docs/obsolete.md"]));
    assert_eq!(value["failureReason"], json!(null));

    let failed = BootstrapRunReport::failed(
        &plan,
        BootstrapRunReportDetails {
            final_head_commit: Some("def456".to_string()),
            changed_files: vec!["docs/trust.md".to_string()],
            deleted_files: vec!["docs/obsolete.md".to_string()],
            path_validation_result: BootstrapPathValidationResult::failed("outside boundary"),
            command_results: vec![sample_command_result(false)],
            cold_start_decision: plan.discovery.clone(),
            rescan_result: BootstrapRescanResult::not_run("checks failed before rescan"),
            live_contract_changed: true,
            live_contract_change_reason: Some("live bootstrap.yaml missing".to_string()),
            finished_at: "2026-04-14T12:41:00Z".to_string(),
        },
        "checks failed",
    );
    let value = serde_json::to_value(&failed).unwrap();
    assert_eq!(value["status"], json!("failed"));
    assert_eq!(value["resultingPackPath"], json!(null));
    assert_eq!(
        value["pathValidationResult"],
        json!({"status": "failed", "reason": "outside boundary"})
    );
    assert_eq!(
        value["rescanResult"],
        json!({
            "status": "not_run",
            "scanMode": null,
            "resultingPackPath": null,
            "reason": "checks failed before rescan"
        })
    );
    assert_eq!(value["liveContractChanged"], json!(true));
    assert_eq!(
        value["liveContractChangeReason"],
        json!("live bootstrap.yaml missing")
    );
    assert_eq!(value["deletedFiles"], json!(["docs/obsolete.md"]));
    assert_eq!(value["failureReason"], json!("checks failed"));

    let aborted = BootstrapRunReport::aborted(&plan, "2026-04-14T12:42:00Z".to_string());
    let value = serde_json::to_value(&aborted).unwrap();
    assert_eq!(value["status"], json!("aborted"));
    assert_eq!(value["finalHeadCommit"], json!(null));
    assert_eq!(value["liveContractChangeReason"], json!(null));
    assert_eq!(value["deletedFiles"], json!([]));
    assert_eq!(
        value["pathValidationResult"],
        json!({"status": "not_run", "reason": "run aborted"})
    );
    assert_eq!(value["finishedAt"], json!("2026-04-14T12:42:00Z"));

    let invalidated = BootstrapRunReport::invalidated_by_head_change(
        &plan,
        "fedcba".to_string(),
        true,
        Some("live bootstrap.yaml differs from frozen snapshot".to_string()),
        "2026-04-14T12:43:00Z".to_string(),
    );
    let value = serde_json::to_value(&invalidated).unwrap();
    assert_eq!(value["status"], json!("invalidated_by_head_change"));
    assert_eq!(value["finalHeadCommit"], json!("fedcba"));
    assert_eq!(value["liveContractChanged"], json!(true));
    assert_eq!(
        value["liveContractChangeReason"],
        json!("live bootstrap.yaml differs from frozen snapshot")
    );
    assert_eq!(
        value["rescanResult"],
        json!({
            "status": "not_run",
            "scanMode": null,
            "resultingPackPath": null,
            "reason": "run invalidated by HEAD change"
        })
    );
}

#[test]
fn bootstrap_storage_paths_keep_active_current_and_run_dir() {
    let root = Path::new("/tmp/repo");
    assert_eq!(
        RepoStoragePaths::bootstrap_dir(root),
        root.join(".repolens").join("bootstrap")
    );
    assert_eq!(
        RepoStoragePaths::bootstrap_active_path(root),
        root.join(".repolens").join("bootstrap").join("ACTIVE")
    );
    assert_eq!(
        RepoStoragePaths::bootstrap_current_path(root),
        root.join(".repolens").join("bootstrap").join("CURRENT")
    );
    assert_eq!(
        RepoStoragePaths::bootstrap_run_dir(root, "run-1"),
        root.join(".repolens").join("bootstrap").join("run-1")
    );
}
