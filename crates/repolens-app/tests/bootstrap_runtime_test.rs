use fs4::FileExt;
use repolens_app::bootstrap::{
    BootstrapRunStore, BootstrapService, FinishBootstrapRunRequest, StartBootstrapRunRequest,
    TryActiveRunLockResult,
};
use repolens_core::model::bootstrap_run::{
    BootstrapPathValidationStatus, BootstrapRescanStatus, BootstrapRunPlan, BootstrapRunReport,
    BootstrapRunReportDetails, BootstrapRunStatus,
};
use repolens_core::storage::RepoStoragePaths;
use std::fs;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use uuid::Uuid;

fn bootstrap_contract_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bootstrap.yaml")
        .canonicalize()
        .unwrap()
}

fn remove_file_if_exists(path: impl AsRef<std::path::Path>) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => panic!("expected file cleanup to succeed: {err}"),
    }
}

fn write_bootstrap_contract_with_paths(
    repo_root: &std::path::Path,
    allow_paths: &[&str],
    deny_paths: &[&str],
) {
    let contract = std::fs::read_to_string(bootstrap_contract_path()).unwrap();
    let original_paths = r#"allowPaths:
  - "docs/**"
  - "crates/**"
  - "Cargo.toml"
  - "Cargo.lock"
  - "AGENTS.md"
  - "CLAUDE.md"
denyPaths:
  - ".git/**"
  - "target/**"
"#;
    let allow_block = allow_paths
        .iter()
        .map(|path| format!("  - \"{path}\"\n"))
        .collect::<String>();
    let deny_block = deny_paths
        .iter()
        .map(|path| format!("  - \"{path}\"\n"))
        .collect::<String>();
    let replacement = format!("allowPaths:\n{allow_block}denyPaths:\n{deny_block}");
    let updated_contract = contract.replace(original_paths, &replacement);

    assert_ne!(
        updated_contract, contract,
        "bootstrap contract paths helper should update the allow/deny path block"
    );

    fs::write(repo_root.join("bootstrap.yaml"), updated_contract).unwrap();

    Command::new("git")
        .args(["add", "bootstrap.yaml"])
        .current_dir(repo_root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "test custom bootstrap paths"])
        .current_dir(repo_root)
        .output()
        .unwrap();
}

fn write_bootstrap_contract_with_missing_required_check(repo_root: &std::path::Path) {
    let contract = std::fs::read_to_string(bootstrap_contract_path()).unwrap();
    let contract = contract.replace(
        "  trust_contract:\n    - \"cargo fmt --all\"\n",
        "  trust_contract:\n    - \"/definitely-not-a-real-bootstrap-command --version\"\n",
    );

    fs::write(repo_root.join("bootstrap.yaml"), contract).unwrap();

    Command::new("git")
        .args(["add", "bootstrap.yaml"])
        .current_dir(repo_root)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "test missing required check"])
        .current_dir(repo_root)
        .output()
        .unwrap();
}

fn init_bootstrap_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "bootstrap@test.invalid"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Bootstrap Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("docs/runtime.md"), "seed\n").unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"bootstrap-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
    fs::write(
        dir.path().join("bootstrap.yaml"),
        std::fs::read(bootstrap_contract_path()).unwrap(),
    )
    .unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "seed"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    dir
}

fn start_request(root: &std::path::Path) -> StartBootstrapRunRequest {
    StartBootstrapRunRequest {
        root: root.to_path_buf(),
        task_type: "docs_only".to_string(),
        goal: "Update runtime docs".to_string(),
        intended_paths: vec!["docs/**".to_string()],
    }
}

fn trust_contract_request(root: &std::path::Path) -> StartBootstrapRunRequest {
    StartBootstrapRunRequest {
        root: root.to_path_buf(),
        task_type: "trust_contract".to_string(),
        goal: "Update trusted manifest".to_string(),
        intended_paths: vec!["Cargo.toml".to_string()],
    }
}

fn load_report(root: &std::path::Path, run_id: &str) -> BootstrapRunReport {
    let path = RepoStoragePaths::bootstrap_run_dir(root, run_id).join("report.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn load_plan(root: &std::path::Path, run_id: &str) -> BootstrapRunPlan {
    let path = RepoStoragePaths::bootstrap_run_dir(root, run_id).join("plan.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn write_report(root: &std::path::Path, run_id: &str, report: &BootstrapRunReport) {
    fs::write(
        RepoStoragePaths::bootstrap_run_dir(root, run_id).join("report.json"),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}

fn write_active(root: &std::path::Path, contents: &str) {
    fs::write(RepoStoragePaths::bootstrap_active_path(root), contents).unwrap();
}

fn alternate_run_id(base_run_id: &str) -> String {
    format!("{}{}", &base_run_id[..20], Uuid::new_v4())
}

#[test]
fn start_writes_active_current_and_plan_json() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();

    let response = service
        .start(start_request(repo.path()))
        .expect("start should succeed");

    assert!(repo.path().join(".repolens/bootstrap/ACTIVE").exists());
    assert!(repo.path().join(".repolens/bootstrap/CURRENT").exists());
    assert!(!repo
        .path()
        .join(".repolens/bootstrap/ACTIVE.transition")
        .exists());
    assert!(repo
        .path()
        .join(&response.run_dir)
        .join("plan.json")
        .exists());
    assert!(repo
        .path()
        .join(&response.run_dir)
        .join("commands")
        .exists());
}

#[test]
fn start_rejects_second_active_run() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let request = start_request(repo.path());

    service
        .start(request.clone())
        .expect("first run should start");
    let err = service.start(request).expect_err("second run should fail");
    assert!(err.to_string().contains("ACTIVE"));
}

#[test]
fn start_rejects_empty_bounded_paths() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();

    let err = service
        .start(StartBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            task_type: "docs_only".to_string(),
            goal: "No paths".to_string(),
            intended_paths: vec![],
        })
        .expect_err("empty paths should fail");

    assert!(err.to_string().contains("intended path"));
}

#[test]
fn start_rejects_unsupported_task_type() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();

    let err = service
        .start(StartBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            task_type: "bounded_rust_change".to_string(),
            goal: "Try unsupported phase".to_string(),
            intended_paths: vec!["docs/**".to_string()],
        })
        .expect_err("unsupported task type should fail");

    assert!(err.to_string().contains("unsupported task type"));
}

#[test]
fn start_rejects_paths_outside_contract_boundary() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();

    let err = service
        .start(StartBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            task_type: "docs_only".to_string(),
            goal: "Try out-of-bounds path".to_string(),
            intended_paths: vec!["foo/**".to_string()],
        })
        .expect_err("out-of-contract path should fail");

    assert!(err.to_string().contains("contract"));
}

#[test]
fn start_rolls_back_active_lock_when_store_write_fails() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let bootstrap_dir = repo.path().join(".repolens/bootstrap");
    fs::create_dir_all(bootstrap_dir.join("CURRENT")).unwrap();

    let err = service
        .start(start_request(repo.path()))
        .expect_err("store failure should fail");

    assert!(err.to_string().contains("CURRENT"));
    assert!(!bootstrap_dir.join("ACTIVE").exists());
}

#[test]
fn start_heals_stale_markers_when_terminal_report_exists() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let plan = load_plan(repo.path(), &run.run_id);
    let report = BootstrapRunReport::aborted(&plan, "2026-04-14T12:45:00Z".to_string());
    write_report(repo.path(), &run.run_id, &report);
    fs::write(repo.path().join(".git/info/exclude"), ".repolens/\n").unwrap();

    let next = service
        .start(start_request(repo.path()))
        .expect("start should heal stale terminal markers");

    assert_ne!(next.run_id, run.run_id);
    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_active_path(repo.path())).unwrap(),
        next.run_id
    );
    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap(),
        next.run_id
    );
}

#[test]
fn start_heals_malformed_active_using_terminal_current_hint() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let plan = load_plan(repo.path(), &run.run_id);
    let report = BootstrapRunReport::aborted(&plan, "2026-04-14T12:45:00Z".to_string());
    write_report(repo.path(), &run.run_id, &report);
    write_active(repo.path(), "not-a-runtime-run-id\n");
    fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        format!("{}\n", run.run_id),
    )
    .unwrap();
    fs::write(repo.path().join(".git/info/exclude"), ".repolens/\n").unwrap();

    let next = service
        .start(start_request(repo.path()))
        .expect("start should heal malformed ACTIVE when CURRENT points at a terminal run");

    assert_ne!(next.run_id, run.run_id);
    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_active_path(repo.path())).unwrap(),
        next.run_id
    );
    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap(),
        next.run_id
    );
}

#[test]
fn finish_succeeds_for_bounded_docs_change() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();

    let run = service
        .start(StartBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            task_type: "docs_only".to_string(),
            goal: "Update docs".to_string(),
            intended_paths: vec!["docs/**".to_string()],
        })
        .unwrap();

    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect("finish should succeed");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert_eq!(report.task_type, "docs_only");
    assert_eq!(report.goal, "Update docs");
    assert_eq!(report.selected_skill_ids.len(), 4);
    assert_eq!(report.intended_path_patterns, vec!["docs/**".to_string()]);
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
    assert!(report.deleted_files.is_empty());
    assert_eq!(
        report.path_validation_result.status,
        BootstrapPathValidationStatus::Passed
    );
    assert!(report.cold_start_decision.cold_start_used);
    assert_eq!(
        report.rescan_result.status,
        BootstrapRescanStatus::Succeeded
    );
    assert_eq!(
        report.rescan_result.scan_mode.as_deref(),
        Some("incremental")
    );
    assert!(!report.started_at.is_empty());
    assert!(!report.finished_at.is_empty());
    assert!(report.resulting_pack_path.is_some());
}

#[test]
fn start_rejects_nested_path_for_root_only_contract_entry() {
    let repo = init_bootstrap_repo();
    write_bootstrap_contract_with_paths(repo.path(), &["Cargo.toml"], &[".git/**", "target/**"]);

    let service = BootstrapService::new();
    let err = service
        .start(StartBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            task_type: "trust_contract".to_string(),
            goal: "Try nested manifest".to_string(),
            intended_paths: vec!["vendor/Cargo.toml".to_string()],
        })
        .expect_err("start should reject nested file for root-only contract entry");

    assert!(err.to_string().contains("contract"));
}

#[test]
fn finish_rejects_nested_change_for_root_only_intended_path() {
    let repo = init_bootstrap_repo();
    write_bootstrap_contract_with_paths(repo.path(), &["Cargo.toml"], &[".git/**", "target/**"]);
    fs::create_dir_all(repo.path().join("vendor")).unwrap();
    fs::write(
        repo.path().join("vendor/Cargo.toml"),
        "[package]\nname = \"vendor-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    Command::new("git")
        .args(["add", "vendor/Cargo.toml"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add nested manifest fixture"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let service = BootstrapService::new();
    let run = service
        .start(trust_contract_request(repo.path()))
        .expect("start should accept the root manifest");

    fs::write(repo.path().join("vendor/Cargo.toml"), "out of bounds\n").unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should reject nested manifest change");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));

    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report
        .changed_files
        .contains(&"vendor/Cargo.toml".to_string()));
    assert_eq!(
        report.failure_reason.as_deref(),
        Some("These files changed outside the approved bootstrap boundary:\n  vendor/Cargo.toml\nAllowed: Cargo.toml")
    );
}

#[test]
fn finish_records_spawn_failure_for_missing_required_check_executable() {
    let repo = init_bootstrap_repo();
    write_bootstrap_contract_with_missing_required_check(repo.path());

    let service = BootstrapService::new();
    let run = service
        .start(trust_contract_request(repo.path()))
        .expect("start should succeed with frozen contract");

    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"bootstrap-fixture\"\nversion = \"0.1.1\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect("finish should return a failed report");

    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert_eq!(report.command_results.len(), 1);
    assert!(!report.command_results[0].succeeded);
    assert!(report.command_results[0].exit_code.is_none());

    let stderr = fs::read_to_string(&report.command_results[0].stderr_log_path).unwrap();
    assert!(!stderr.trim().is_empty());
}

#[test]
fn finish_rejects_out_of_bounds_change() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::write(repo.path().join("Cargo.toml"), "out of bounds\n").unwrap();
    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should reject out-of-bounds change");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));

    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report.changed_files.contains(&"Cargo.toml".to_string()));
    assert_eq!(
        report.failure_reason.as_deref(),
        Some("These files changed outside the approved bootstrap boundary:\n  Cargo.toml\nAllowed: docs/**")
    );
}

#[test]
fn finish_is_idempotent_for_terminal_runs() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let first = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .unwrap();
    let second = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id),
        })
        .unwrap();

    assert_eq!(first.status, second.status);
    assert_eq!(first.changed_files, second.changed_files);
}

#[test]
fn finish_cleans_stale_markers_when_terminal_report_already_exists() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let plan = load_plan(repo.path(), &run.run_id);
    let report = BootstrapRunReport::aborted(&plan, "2026-04-14T12:45:00Z".to_string());
    write_report(repo.path(), &run.run_id, &report);

    let returned = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id),
        })
        .expect("finish should reuse terminal report");

    assert_eq!(returned.status, BootstrapRunStatus::Aborted);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
    assert!(!RepoStoragePaths::bootstrap_current_path(repo.path()).exists());
}

#[test]
fn abort_writes_terminal_report() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    let report = service
        .abort(repo.path(), Some(&run.run_id))
        .expect("abort should succeed");

    assert_eq!(report.status, BootstrapRunStatus::Aborted);
    assert!(!repo.path().join(".repolens/bootstrap/ACTIVE").exists());
}

#[test]
fn finish_succeeds_for_deleted_docs_change() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::remove_file(repo.path().join("docs/runtime.md")).unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id),
        })
        .expect("finish should accept deleted docs file");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert!(report
        .deleted_files
        .contains(&"docs/runtime.md".to_string()));
}

#[test]
fn finish_rejects_out_of_bounds_deletion() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::remove_file(repo.path().join("Cargo.toml")).unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should reject deleted out-of-bounds file");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report.deleted_files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn finish_persists_report_for_empty_bounded_diff() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("empty bounded diff should fail");

    assert!(err.to_string().contains("bounded diff is empty"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report.changed_files.is_empty());
    assert!(report.deleted_files.is_empty());
}

#[test]
fn abort_rejects_terminal_run() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    service.abort(repo.path(), Some(&run.run_id)).unwrap();
    let err = service
        .abort(repo.path(), Some(&run.run_id))
        .expect_err("abort should reject terminal run");

    assert!(err.to_string().contains("terminal"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Aborted);
}

#[test]
fn abort_cleans_stale_markers_when_terminal_report_already_exists() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let plan = load_plan(repo.path(), &run.run_id);
    let report = BootstrapRunReport::aborted(&plan, "2026-04-14T12:45:00Z".to_string());
    write_report(repo.path(), &run.run_id, &report);

    let err = service
        .abort(repo.path(), Some(&run.run_id))
        .expect_err("abort should still report terminal state");

    assert!(err.to_string().contains("terminal"));
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
    assert!(!RepoStoragePaths::bootstrap_current_path(repo.path()).exists());
}

#[test]
fn stale_explicit_finish_keeps_new_current_after_next_start() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let first_run = service.start(start_request(repo.path())).unwrap();
    let first_plan = load_plan(repo.path(), &first_run.run_id);

    fs::remove_file(RepoStoragePaths::bootstrap_active_path(repo.path())).unwrap();
    fs::write(repo.path().join(".git/info/exclude"), ".repolens/\n").unwrap();

    let (inactive_seen_tx, inactive_seen_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let repo_root = repo.path().to_path_buf();
    let stale_run_id = first_run.run_id.clone();
    let stale_finisher = thread::spawn(move || {
        assert!(matches!(
            BootstrapRunStore::try_lock_active_run(&repo_root, &stale_run_id).unwrap(),
            TryActiveRunLockResult::NotActive
        ));
        inactive_seen_tx.send(()).unwrap();
        resume_rx.recv().unwrap();

        let report = BootstrapRunReport::aborted(&first_plan, "2026-04-14T12:45:00Z".to_string());
        BootstrapRunStore::finalize_terminal_transition_without_active(
            &repo_root,
            &stale_run_id,
            &report,
        )
    });

    inactive_seen_rx.recv().unwrap();
    let second_run = service.start(start_request(repo.path())).unwrap();
    resume_tx.send(()).unwrap();
    stale_finisher.join().unwrap().unwrap();

    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap(),
        second_run.run_id
    );

    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: None,
        })
        .expect("finish without explicit run id should still use the new CURRENT");

    assert_eq!(report.run_id, second_run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
}

#[test]
fn loser_abort_does_not_delete_newer_markers_after_winner_finishes() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let first_run = service.start(start_request(repo.path())).unwrap();
    let first_plan = load_plan(repo.path(), &first_run.run_id);
    let active_path = RepoStoragePaths::bootstrap_active_path(repo.path());
    let current_path = RepoStoragePaths::bootstrap_current_path(repo.path());

    let winner_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&active_path)
        .unwrap();
    winner_file.lock_exclusive().unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let repo_root = repo.path().to_path_buf();
    let losing_run_id = first_run.run_id.clone();
    let loser = thread::spawn(move || {
        started_tx.send(()).unwrap();
        BootstrapService::new().abort(&repo_root, Some(&losing_run_id))
    });

    started_rx.recv().unwrap();
    thread::sleep(Duration::from_millis(100));

    let terminal_report =
        BootstrapRunReport::aborted(&first_plan, "2026-04-14T12:45:00Z".to_string());
    write_report(repo.path(), &first_run.run_id, &terminal_report);
    // Already gone counts as removed: on macOS CI these markers were
    // sometimes missing here, the race 53daf2c tolerates elsewhere (#43).
    remove_file_if_exists(&current_path);
    remove_file_if_exists(&active_path);
    fs::write(repo.path().join(".git/info/exclude"), ".repolens/\n").unwrap();

    let second_run = service
        .start(start_request(repo.path()))
        .expect("winner cleanup should allow the next run to start");

    drop(winner_file);

    let err = loser
        .join()
        .expect("loser thread should finish")
        .expect_err("loser should observe terminal state");
    assert!(err.to_string().contains("terminal"));
    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_active_path(repo.path())).unwrap(),
        second_run.run_id
    );
    assert_eq!(
        fs::read_to_string(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap(),
        second_run.run_id
    );
}

#[test]
fn trust_contract_finish_runs_required_checks_and_persists_logs() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(trust_contract_request(repo.path())).unwrap();

    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"bootstrap-fixture\"\nversion = \"0.1.1\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id),
        })
        .expect("trust_contract finish should succeed");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert_eq!(report.task_type, "trust_contract");
    assert_eq!(report.command_results.len(), 1);
    assert_eq!(
        report.command_results[0].argv,
        vec!["cargo".to_string(), "fmt".to_string(), "--all".to_string()]
    );
    assert!(report.command_results[0].succeeded);
    assert!(std::path::Path::new(&report.command_results[0].stdout_log_path).exists());
    assert!(std::path::Path::new(&report.command_results[0].stderr_log_path).exists());
    assert_eq!(report.changed_files, vec!["Cargo.toml".to_string()]);
    assert_eq!(
        fs::read(report.command_results[0].stdout_log_path.clone()).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(
        fs::read(report.command_results[0].stderr_log_path.clone()).unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn finish_rejects_when_active_run_belongs_to_other_run() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let other_run_id = alternate_run_id(&run.run_id);
    fs::write(
        RepoStoragePaths::bootstrap_active_path(repo.path()),
        format!("{}\n", other_run_id),
    )
    .unwrap();
    fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        format!("{}\n", other_run_id),
    )
    .unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should reject stale ACTIVE ownership");

    assert!(err.to_string().contains("active"));
    assert!(
        !RepoStoragePaths::bootstrap_run_dir(repo.path(), &run.run_id)
            .join("report.json")
            .exists()
    );
    assert!(RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn finish_ignores_missing_current_when_explicit_run_id_matches_active() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::remove_file(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id),
        })
        .expect("finish should ignore missing CURRENT when ACTIVE matches");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn finish_ignores_stale_current_when_explicit_run_id_matches_active() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let other_run_id = alternate_run_id(&run.run_id);
    fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        format!("{}\n", other_run_id),
    )
    .unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id),
        })
        .expect("finish should ignore stale CURRENT when ACTIVE matches");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn abort_rejects_when_active_run_belongs_to_other_run() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let other_run_id = alternate_run_id(&run.run_id);
    fs::write(
        RepoStoragePaths::bootstrap_active_path(repo.path()),
        format!("{}\n", other_run_id),
    )
    .unwrap();
    fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        format!("{}\n", other_run_id),
    )
    .unwrap();

    let err = service
        .abort(repo.path(), Some(&run.run_id))
        .expect_err("abort should reject stale ACTIVE ownership");

    assert!(err.to_string().contains("active"));
    assert!(
        !RepoStoragePaths::bootstrap_run_dir(repo.path(), &run.run_id)
            .join("report.json")
            .exists()
    );
    assert!(RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn abort_ignores_missing_current_when_explicit_run_id_matches_active() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::remove_file(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap();

    let report = service
        .abort(repo.path(), Some(&run.run_id))
        .expect("abort should ignore missing CURRENT when ACTIVE matches");

    assert_eq!(report.status, BootstrapRunStatus::Aborted);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn abort_ignores_stale_current_when_explicit_run_id_matches_active() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let other_run_id = alternate_run_id(&run.run_id);
    fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        format!("{}\n", other_run_id),
    )
    .unwrap();

    let report = service
        .abort(repo.path(), Some(&run.run_id))
        .expect("abort should ignore stale CURRENT when ACTIVE matches");

    assert_eq!(report.status, BootstrapRunStatus::Aborted);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn finish_and_abort_fail_when_current_is_missing_even_if_active_exists() {
    let service = BootstrapService::new();

    let finish_repo = init_bootstrap_repo();
    let first_run = service.start(start_request(finish_repo.path())).unwrap();
    let first_plan = load_plan(finish_repo.path(), &first_run.run_id);
    service
        .abort(finish_repo.path(), Some(&first_run.run_id))
        .expect("first run should abort");

    let finish_run_id = format!("{}{}", &first_run.run_id[..20], Uuid::new_v4());
    let finish_run_dir = RepoStoragePaths::bootstrap_run_dir(finish_repo.path(), &finish_run_id);
    fs::create_dir_all(&finish_run_dir).unwrap();
    let mut finish_plan = first_plan.clone();
    finish_plan.run_id = finish_run_id.clone();
    fs::write(
        finish_run_dir.join("plan.json"),
        serde_json::to_vec_pretty(&finish_plan).unwrap(),
    )
    .unwrap();
    fs::write(
        RepoStoragePaths::bootstrap_active_path(finish_repo.path()),
        format!("{}\n", finish_run_id),
    )
    .unwrap();
    fs::write(finish_repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let finish_err = service
        .finish(FinishBootstrapRunRequest {
            root: finish_repo.path().to_path_buf(),
            run_id: None,
        })
        .expect_err("finish should require CURRENT when run id is omitted");

    assert!(finish_err.to_string().contains("CURRENT"));
    assert!(!RepoStoragePaths::bootstrap_current_path(finish_repo.path()).exists());
    assert!(RepoStoragePaths::bootstrap_active_path(finish_repo.path()).exists());

    let abort_repo = init_bootstrap_repo();
    let third_run = service.start(start_request(abort_repo.path())).unwrap();
    let third_plan = load_plan(abort_repo.path(), &third_run.run_id);
    service
        .abort(abort_repo.path(), Some(&third_run.run_id))
        .expect("third run should start from clean state");

    let fourth_run_id = format!("{}{}", &third_run.run_id[..20], Uuid::new_v4());
    let fourth_run_dir = RepoStoragePaths::bootstrap_run_dir(abort_repo.path(), &fourth_run_id);
    fs::create_dir_all(&fourth_run_dir).unwrap();
    let mut fourth_plan = third_plan.clone();
    fourth_plan.run_id = fourth_run_id.clone();
    fs::write(
        fourth_run_dir.join("plan.json"),
        serde_json::to_vec_pretty(&fourth_plan).unwrap(),
    )
    .unwrap();
    fs::write(
        RepoStoragePaths::bootstrap_active_path(abort_repo.path()),
        format!("{}\n", fourth_run_id),
    )
    .unwrap();

    let abort_err = service
        .abort(abort_repo.path(), None)
        .expect_err("abort should require CURRENT when run id is omitted");

    assert!(abort_err.to_string().contains("CURRENT"));
    assert!(RepoStoragePaths::bootstrap_active_path(abort_repo.path()).exists());
}

#[test]
fn finish_and_abort_reject_malformed_current_when_run_id_is_omitted() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    std::fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        "not-a-runtime-run-id\n",
    )
    .unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let finish_err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: None,
        })
        .expect_err("finish should reject malformed CURRENT");
    assert!(finish_err.to_string().contains("CURRENT"));
    assert!(
        !RepoStoragePaths::bootstrap_run_dir(repo.path(), &run.run_id)
            .join("report.json")
            .exists()
    );

    let abort_err = service
        .abort(repo.path(), None)
        .expect_err("abort should reject malformed CURRENT");
    assert!(abort_err.to_string().contains("CURRENT"));
    assert!(
        !RepoStoragePaths::bootstrap_run_dir(repo.path(), &run.run_id)
            .join("report.json")
            .exists()
    );
}

#[test]
fn finish_writes_failed_report_when_active_is_malformed_and_run_id_is_explicit() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    write_active(repo.path(), "");
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should fail when ACTIVE is malformed");

    assert!(err.to_string().contains("ACTIVE"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert_eq!(
        report.failure_reason.as_deref(),
        Some("bootstrap ACTIVE is malformed or empty")
    );
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
    assert!(!RepoStoragePaths::bootstrap_current_path(repo.path()).exists());
}

#[test]
fn finish_writes_failed_report_when_path_guard_compile_fails_after_lock() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    let mut plan = load_plan(repo.path(), &run.run_id);
    plan.bounded_paths = vec!["../Cargo.toml".to_string()];
    fs::write(
        RepoStoragePaths::bootstrap_run_dir(repo.path(), &run.run_id).join("plan.json"),
        serde_json::to_vec_pretty(&plan).unwrap(),
    )
    .unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should fail when path guard compilation fails");

    assert!(err.to_string().contains("repo-relative"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert_eq!(
        report.failure_reason.as_deref(),
        Some("intended paths must be repo-relative")
    );
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
    assert!(!RepoStoragePaths::bootstrap_current_path(repo.path()).exists());
}

#[test]
fn finish_and_abort_reject_invalid_explicit_run_id() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let bad_run_id = "run-20240414-123456-../../evil";

    let finish_err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(bad_run_id.to_string()),
        })
        .expect_err("finish should reject unsafe run id");
    assert!(finish_err.to_string().contains("invalid bootstrap run id"));
    assert!(!repo.path().join(".repolens/evil").exists());

    let abort_err = service
        .abort(repo.path(), Some(bad_run_id))
        .expect_err("abort should reject unsafe run id");
    assert!(abort_err.to_string().contains("invalid bootstrap run id"));
    assert!(!repo.path().join(".repolens/evil").exists());
}

#[test]
fn finish_reports_live_contract_drift() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::write(repo.path().join("bootstrap.yaml"), "version: \"999\"\n").unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("drifted contract should fail finish for docs-only run");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report.live_contract_changed);
    assert_eq!(
        report.live_contract_change_reason.as_deref(),
        Some("live bootstrap.yaml differs from frozen snapshot")
    );
}

#[test]
fn finish_reports_live_contract_drift_when_head_is_unresolvable() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::write(repo.path().join("bootstrap.yaml"), "version: \"999\"\n").unwrap();
    fs::remove_file(repo.path().join(".git/HEAD")).unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("missing HEAD should still produce a terminal failure report");

    assert!(err.to_string().contains("HEAD is not resolvable"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report.live_contract_changed);
    assert_eq!(
        report.live_contract_change_reason.as_deref(),
        Some("live bootstrap.yaml differs from frozen snapshot")
    );
    assert!(report.final_head_commit.is_none());
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn finish_records_missing_live_bootstrap_as_drift_and_continues_validation() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    fs::remove_file(repo.path().join("bootstrap.yaml")).unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("missing live bootstrap file should still continue with frozen validation");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert!(report.live_contract_changed);
    assert!(report
        .live_contract_change_reason
        .as_deref()
        .unwrap()
        .contains("failed to read live bootstrap.yaml"));
    assert!(report.deleted_files.contains(&"bootstrap.yaml".to_string()));
}

#[test]
fn finish_writes_failed_report_when_active_is_missing_but_plan_exists() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    fs::remove_file(RepoStoragePaths::bootstrap_active_path(repo.path())).unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("finish should fail when ACTIVE disappears");

    assert!(err.to_string().contains("not active"));
    let report = load_report(repo.path(), &run.run_id);
    let expected_failure = format!("bootstrap run is not active: {}", run.run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert_eq!(
        report.failure_reason.as_deref(),
        Some(expected_failure.as_str())
    );
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
    assert!(!RepoStoragePaths::bootstrap_active_path(repo.path()).exists());
}

#[test]
fn finish_returns_stored_report_when_active_is_locked() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    let plan = load_plan(repo.path(), &run.run_id);
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let repo_root = repo.path().to_path_buf();
    let run_id = run.run_id.clone();
    let (locked_tx, locked_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        let active_path = RepoStoragePaths::bootstrap_active_path(&repo_root);
        let active_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&active_path)
            .unwrap();
        active_file.lock_exclusive().unwrap();
        locked_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(100));
        let report = BootstrapRunReport::succeeded(
            &plan,
            BootstrapRunReportDetails {
                final_head_commit: Some(plan.baseline_head_commit.clone()),
                changed_files: vec!["docs/runtime.md".to_string()],
                deleted_files: vec![],
                path_validation_result:
                    repolens_core::model::bootstrap_run::BootstrapPathValidationResult::passed(),
                command_results: vec![],
                cold_start_decision: plan.discovery.clone(),
                rescan_result:
                    repolens_core::model::bootstrap_run::BootstrapRescanResult::succeeded(
                        "incremental".to_string(),
                        repo_root
                            .join(".repolens/pack-finished")
                            .to_string_lossy()
                            .to_string(),
                    ),
                live_contract_changed: false,
                live_contract_change_reason: None,
                finished_at: "2026-04-14T12:45:00Z".to_string(),
            },
            repo_root.join(".repolens/pack-finished"),
        );
        fs::write(
            RepoStoragePaths::bootstrap_run_dir(&repo_root, &run_id).join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        remove_file_if_exists(RepoStoragePaths::bootstrap_current_path(&repo_root));
        remove_file_if_exists(active_path);
        active_file.unlock().unwrap();
    });
    locked_rx.recv().unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect("losing finisher should return stored report");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
    assert_eq!(report.finished_at, "2026-04-14T12:45:00Z");
    writer.join().unwrap();
}

#[test]
fn finish_retries_when_terminal_report_is_temporarily_empty() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let plan = load_plan(repo.path(), &run.run_id);
    let repo_root = repo.path().to_path_buf();
    let run_id = run.run_id.clone();
    let (empty_report_tx, empty_report_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        let active_path = RepoStoragePaths::bootstrap_active_path(&repo_root);
        let active_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&active_path)
            .unwrap();
        active_file.lock_exclusive().unwrap();

        let report_path =
            RepoStoragePaths::bootstrap_run_dir(&repo_root, &run_id).join("report.json");
        fs::write(&report_path, b"").unwrap();
        empty_report_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let report = BootstrapRunReport::succeeded(
            &plan,
            BootstrapRunReportDetails {
                final_head_commit: Some(plan.baseline_head_commit.clone()),
                changed_files: vec!["docs/runtime.md".to_string()],
                deleted_files: vec![],
                path_validation_result:
                    repolens_core::model::bootstrap_run::BootstrapPathValidationResult::passed(),
                command_results: vec![],
                cold_start_decision: plan.discovery.clone(),
                rescan_result:
                    repolens_core::model::bootstrap_run::BootstrapRescanResult::succeeded(
                        "incremental".to_string(),
                        repo_root
                            .join(".repolens/pack-finished")
                            .to_string_lossy()
                            .to_string(),
                    ),
                live_contract_changed: false,
                live_contract_change_reason: None,
                finished_at: "2026-04-14T12:46:00Z".to_string(),
            },
            repo_root.join(".repolens/pack-finished"),
        );
        fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        remove_file_if_exists(RepoStoragePaths::bootstrap_current_path(&repo_root));
        remove_file_if_exists(active_path);
        active_file.unlock().unwrap();
    });
    empty_report_rx.recv().unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect("losing finisher should retry empty report until it is complete");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
    assert_eq!(report.finished_at, "2026-04-14T12:46:00Z");
    writer.join().unwrap();
}

#[test]
fn finish_rejects_unexpected_repolens_artifact_change() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    fs::write(repo.path().join(".repolens/user-note.txt"), "unexpected\n").unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("unexpected repolens artifact should not be ignored");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));
    let report = load_report(repo.path(), &run.run_id);
    assert!(report
        .changed_files
        .iter()
        .any(|path| path == ".repolens/user-note.txt" || path == ".repolens"));
}

#[test]
fn finish_rejects_unrelated_pack_like_artifact_change() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();

    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    fs::create_dir_all(repo.path().join(".repolens/pack-user-edit")).unwrap();
    fs::write(
        repo.path().join(".repolens/pack-user-edit/manifest.json"),
        "{}\n",
    )
    .unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("unrelated pack-like artifact should not be ignored");

    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));
    let report = load_report(repo.path(), &run.run_id);
    assert!(report
        .changed_files
        .iter()
        .any(|path| path.starts_with(".repolens/pack-user-edit") || path == ".repolens"));
}

#[test]
fn boundary_violation_names_the_edited_repolens_file() {
    let repo = init_bootstrap_repo();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    fs::write(
        repo.path().join(".repolens/known-constraints.md"),
        "edited by hand\n",
    )
    .unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("edit inside .repolens/ must be rejected");
    assert_eq!(
        err.to_string(),
        "These files changed outside the approved bootstrap boundary:\n  .repolens/known-constraints.md\nAllowed: docs/**"
    );
    let report = load_report(repo.path(), &run.run_id);
    assert_eq!(
        report.failure_reason.as_deref(),
        Some("These files changed outside the approved bootstrap boundary:\n  .repolens/known-constraints.md\nAllowed: docs/**")
    );
}

/// `.repolens/` hidden only by RepoLens's own `.repolens/.gitignore` (no root
/// entry, #39): workspace files and packs that existed before `start` and are
/// untouched must not count as boundary violations.
#[test]
fn finish_ignores_untouched_repolens_files_hidden_by_self_ignore() {
    let repo = init_bootstrap_repo();
    repolens_app::scan::ScanService::new()
        .run(
            repo.path(),
            false,
            repolens_app::cleanup::DEFAULT_KEEP_COUNT,
        )
        .unwrap();
    fs::write(repo.path().join(".repolens/known-constraints.md"), "mine\n").unwrap();
    assert!(repo.path().join(".repolens/.gitignore").exists());
    assert!(!repo.path().join(".gitignore").exists());
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect("untouched .repolens/ files must not fail finish");

    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
}

/// Same setup, but a pre-existing `.repolens/` file is edited during the run:
/// still a boundary violation.
#[test]
fn finish_rejects_edit_to_preexisting_repolens_file() {
    let repo = init_bootstrap_repo();
    repolens_app::scan::ScanService::new()
        .run(
            repo.path(),
            false,
            repolens_app::cleanup::DEFAULT_KEEP_COUNT,
        )
        .unwrap();
    fs::write(repo.path().join(".repolens/known-constraints.md"), "mine\n").unwrap();
    let service = BootstrapService::new();
    let run = service.start(start_request(repo.path())).unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    fs::write(
        repo.path().join(".repolens/known-constraints.md"),
        "edited during the run, longer\n",
    )
    .unwrap();

    let err = service
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.run_id.clone()),
        })
        .expect_err("edit to a pre-existing .repolens/ file must be rejected");
    assert_eq!(
        err.to_string(),
        "These files changed outside the approved bootstrap boundary:\n  .repolens/known-constraints.md\nAllowed: docs/**"
    );
}

/// In a linked worktree `.git` is a file; a `.repolens/` entry in the shared
/// `info/exclude` (common git dir) still counts as the user ignoring it.
#[test]
fn finish_honors_info_exclude_in_linked_worktree() {
    let repo = init_bootstrap_repo();
    let wt_parent = tempfile::tempdir().unwrap();
    let wt = wt_parent.path().join("wt");
    let out = Command::new("git")
        .args(["worktree", "add", "-q", "-b", "wt"])
        .arg(&wt)
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(wt.join(".git").is_file());
    fs::write(repo.path().join(".git/info/exclude"), ".repolens/\n").unwrap();

    let service = BootstrapService::new();
    let run = service.start(start_request(&wt)).unwrap();
    fs::write(wt.join("docs/runtime.md"), "updated\n").unwrap();
    fs::write(wt.join(".repolens/user-note.txt"), "user-ignored\n").unwrap();

    let report = service
        .finish(FinishBootstrapRunRequest {
            root: wt.clone(),
            run_id: Some(run.run_id.clone()),
        })
        .expect("user-ignored .repolens/ in a linked worktree is skipped");
    assert_eq!(report.status, BootstrapRunStatus::Succeeded);
    assert_eq!(report.changed_files, vec!["docs/runtime.md".to_string()]);
}

/// Scan, add a `.repolens/` file and folder, start a run that edits docs.
fn start_with_storage_entries(repo: &tempfile::TempDir) -> String {
    repolens_app::scan::ScanService::new()
        .run(
            repo.path(),
            false,
            repolens_app::cleanup::DEFAULT_KEEP_COUNT,
        )
        .unwrap();
    fs::write(repo.path().join(".repolens/known-constraints.md"), "mine\n").unwrap();
    fs::create_dir_all(repo.path().join(".repolens/notes")).unwrap();
    fs::write(repo.path().join(".repolens/notes/a.md"), "note\n").unwrap();
    let run = BootstrapService::new()
        .start(start_request(repo.path()))
        .unwrap();
    fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    run.run_id
}

fn assert_single_line_failure(repo: &tempfile::TempDir, run_id: &str, expected: &str) {
    let err = BootstrapService::new()
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run_id.to_string()),
        })
        .expect_err("finish must fail");
    let shown = format!("{err:#}");
    assert_eq!(shown, expected);
    assert_eq!(shown.matches(expected).count(), 1);
    assert!(!shown.to_lowercase().contains("os error"), "{shown}");
    assert!(shown.len() < 80, "{shown}");
    let report = load_report(repo.path(), run_id);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert_eq!(report.failure_reason.as_deref(), Some(expected));
}

#[test]
fn finish_reports_removed_repolens_file() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    fs::remove_file(repo.path().join(".repolens/known-constraints.md")).unwrap();
    assert_single_line_failure(
        &repo,
        &run,
        ".repolens/known-constraints.md was removed during bootstrap.",
    );
}

#[test]
fn finish_reports_removed_repolens_folder() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    fs::remove_dir_all(repo.path().join(".repolens/notes")).unwrap();
    assert_single_line_failure(&repo, &run, ".repolens/notes was removed during bootstrap.");
}

#[cfg(unix)]
#[test]
fn finish_reports_broken_symlink_in_repolens() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    std::os::unix::fs::symlink("missing-target", repo.path().join(".repolens/dangling")).unwrap();
    assert_single_line_failure(&repo, &run, ".repolens/dangling is a broken symlink.");
}

/// One run that removes a `.repolens/` entry and writes outside the boundary
/// reports both: removed-entry lines first, then the boundary message.
#[test]
fn finish_reports_removed_entry_and_boundary_violation_together() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    fs::remove_file(repo.path().join(".repolens/known-constraints.md")).unwrap();
    fs::remove_dir_all(repo.path().join(".repolens/notes")).unwrap();
    fs::write(repo.path().join("out.txt"), "outside\n").unwrap();

    let err = BootstrapService::new()
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run.clone()),
        })
        .expect_err("finish must fail");
    let shown = format!("{err:#}");
    let expected = [
        ".repolens/known-constraints.md was removed during bootstrap.",
        ".repolens/notes was removed during bootstrap.",
        "These files changed outside the approved bootstrap boundary:",
        "  out.txt",
        "Allowed: docs/**",
    ];
    assert_eq!(shown.lines().collect::<Vec<_>>(), expected, "{shown}");
    for line in expected {
        assert_eq!(shown.matches(line).count(), 1, "{shown}");
    }
    assert_eq!(
        shown
            .matches("outside the approved bootstrap boundary")
            .count(),
        1
    );
    assert!(!shown.to_lowercase().contains("os error"), "{shown}");
    assert!(shown.lines().all(|l| l.len() < 80), "{shown}");

    let report = load_report(repo.path(), &run);
    assert_eq!(report.status, BootstrapRunStatus::Failed);
    assert_eq!(
        report.failure_reason.as_deref(),
        Some(expected.join("\n").as_str())
    );
}

fn finish_error(repo: &tempfile::TempDir, run_id: &str) -> String {
    let err = BootstrapService::new()
        .finish(FinishBootstrapRunRequest {
            root: repo.path().to_path_buf(),
            run_id: Some(run_id.to_string()),
        })
        .expect_err("finish must fail");
    format!("{err:#}")
}

fn assert_lines_once(shown: &str, expected: &[String]) {
    assert_eq!(shown.lines().collect::<Vec<_>>(), expected, "{shown}");
    for line in expected {
        assert_eq!(shown.matches(line.as_str()).count(), 1, "{shown}");
    }
    assert!(!shown.to_lowercase().contains("os error"), "{shown}");
}

/// #58 item 3: deleting RepoLens's own `.repolens/.gitignore` names that
/// file, not an untouched `.repolens/` entry git now lists.
#[test]
fn finish_names_deleted_repolens_gitignore() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    fs::remove_file(repo.path().join(".repolens/.gitignore")).unwrap();
    assert_single_line_failure(
        &repo,
        &run,
        ".repolens/.gitignore was removed during bootstrap.",
    );
}

/// #58 item 3, with a real edit: the edited entry is still reported.
#[test]
fn finish_after_gitignore_delete_still_reports_edited_entry() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    fs::remove_file(repo.path().join(".repolens/.gitignore")).unwrap();
    fs::write(
        repo.path().join(".repolens/known-constraints.md"),
        "edited\n",
    )
    .unwrap();
    let shown = finish_error(&repo, &run);
    assert_lines_once(
        &shown,
        &[
            ".repolens/.gitignore was removed during bootstrap.".to_string(),
            "These files changed outside the approved bootstrap boundary:".to_string(),
            "  .repolens/known-constraints.md".to_string(),
            "Allowed: docs/**".to_string(),
        ],
    );
}

/// #58 item 4: removing all of `.repolens/` prints one short line, no OS
/// error, with or without an explicit run id.
#[test]
fn finish_reports_removed_storage_folder_in_one_line() {
    for explicit in [true, false] {
        let repo = init_bootstrap_repo();
        let run = start_with_storage_entries(&repo);
        fs::remove_dir_all(repo.path().join(".repolens")).unwrap();
        let err = BootstrapService::new()
            .finish(FinishBootstrapRunRequest {
                root: repo.path().to_path_buf(),
                run_id: explicit.then(|| run.clone()),
            })
            .expect_err("finish must fail");
        let shown = format!("{err:#}");
        let expected = if explicit {
            ".repolens was removed during bootstrap."
        } else {
            "bootstrap CURRENT is missing or unreadable: .repolens/bootstrap/CURRENT"
        };
        assert_eq!(shown, expected, "explicit={explicit}");
        assert!(!shown.to_lowercase().contains("os error"), "{shown}");
        assert!(shown.chars().count() <= 80, "{shown}");
    }
}

/// #58 item 2: a broken symlink no longer hides outside writes; the
/// symlink line comes first, then each boundary line, each once.
#[cfg(unix)]
#[test]
fn finish_reports_broken_symlink_and_outside_writes() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    std::os::unix::fs::symlink("nowhere", repo.path().join(".repolens/dangle")).unwrap();
    fs::write(repo.path().join("out.txt"), "x\n").unwrap();
    let shown = finish_error(&repo, &run);
    assert_lines_once(
        &shown,
        &[
            ".repolens/dangle is a broken symlink.".to_string(),
            "These files changed outside the approved bootstrap boundary:".to_string(),
            "  out.txt".to_string(),
            "Allowed: docs/**".to_string(),
        ],
    );
    let report = load_report(repo.path(), &run);
    assert_eq!(report.failure_reason.as_deref(), Some(shown.as_str()));
}

/// #58 item 1: every outside write gets a line, in path order.
#[test]
fn finish_names_every_outside_write() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    fs::write(repo.path().join("out.txt"), "x\n").unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/lib.rs"), "// x\n").unwrap();
    let shown = finish_error(&repo, &run);
    assert_lines_once(
        &shown,
        &[
            "These files changed outside the approved bootstrap boundary:".to_string(),
            "  out.txt".to_string(),
            "  src/lib.rs".to_string(),
            "Allowed: docs/**".to_string(),
        ],
    );
}

/// #58 item 1: more than 10 outside writes print the first 10, then one
/// "…and N more" line.
#[test]
fn finish_caps_outside_writes_at_ten() {
    let repo = init_bootstrap_repo();
    let run = start_with_storage_entries(&repo);
    for i in 0..13 {
        fs::write(repo.path().join(format!("out{i:02}.txt")), "x\n").unwrap();
    }
    let shown = finish_error(&repo, &run);
    let mut expected =
        vec!["These files changed outside the approved bootstrap boundary:".to_string()];
    expected.extend((0..10).map(|i| format!("  out{i:02}.txt")));
    expected.push("  …and 3 more".to_string());
    expected.push("Allowed: docs/**".to_string());
    assert!(!shown.contains("out10.txt"), "{shown}");
    assert_lines_once(&shown, &expected);
}
