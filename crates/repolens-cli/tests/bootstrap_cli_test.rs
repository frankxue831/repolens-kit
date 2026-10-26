use repolens_core::storage::RepoStoragePaths;
use serde_json::Value;
use std::process::Command;

fn bootstrap_contract_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bootstrap.yaml")
        .canonicalize()
        .unwrap()
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

    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/runtime.md"), "seed\n").unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"bootstrap-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
    std::fs::write(
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

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

fn error_envelope(output: &std::process::Output) -> Value {
    assert!(!output.status.success());
    serde_json::from_slice(&output.stderr).unwrap()
}

#[test]
fn bootstrap_start_emits_json() {
    let repo = init_bootstrap_repo();
    let output = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "start",
            "--root",
            repo.path().to_str().unwrap(),
            "--task-type",
            "docs_only",
            "--goal",
            "Update docs",
            "--intended-path",
            "docs/**",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["runId"].as_str().is_some());
    assert_eq!(value["boundedPaths"][0], "docs/**");
}

#[test]
fn bootstrap_start_failure_emits_json() {
    let output = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "start",
            "--root",
            "/definitely-not-a-real-bootstrap-repo",
            "--task-type",
            "docs_only",
            "--goal",
            "Update docs",
            "--intended-path",
            "docs/**",
        ])
        .output()
        .unwrap();

    let value = error_envelope(&output);
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("Cannot resolve --root"));
}

#[test]
fn bootstrap_start_parse_failure_emits_json() {
    let output = Command::new(repolens_bin())
        .args(["bootstrap", "start", "--root", "/tmp"])
        .output()
        .unwrap();

    let value = error_envelope(&output);
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("required arguments"));
}

#[test]
fn bootstrap_finish_uses_current_when_run_is_omitted() {
    let repo = init_bootstrap_repo();

    let start = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "start",
            "--root",
            repo.path().to_str().unwrap(),
            "--task-type",
            "docs_only",
            "--goal",
            "Update docs",
            "--intended-path",
            "docs/**",
        ])
        .output()
        .unwrap();
    assert!(start.status.success());
    let start_value: Value = serde_json::from_slice(&start.stdout).unwrap();
    let run_id = start_value["runId"].as_str().unwrap().to_string();

    std::fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();

    let finish = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "finish",
            "--root",
            repo.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(finish.status.success());
    let value: Value = serde_json::from_slice(&finish.stdout).unwrap();
    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["runId"], run_id);
}

#[test]
fn bootstrap_finish_requires_current_when_run_is_omitted() {
    let repo = init_bootstrap_repo();
    let start = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "start",
            "--root",
            repo.path().to_str().unwrap(),
            "--task-type",
            "docs_only",
            "--goal",
            "Update docs",
            "--intended-path",
            "docs/**",
        ])
        .output()
        .unwrap();
    assert!(start.status.success());

    std::fs::write(repo.path().join("docs/runtime.md"), "updated\n").unwrap();
    std::fs::remove_file(RepoStoragePaths::bootstrap_current_path(repo.path())).unwrap();

    let finish = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "finish",
            "--root",
            repo.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let value = error_envelope(&finish);
    assert!(value["error"].as_str().unwrap().contains("CURRENT"));
}

#[test]
fn bootstrap_abort_requires_current_when_run_is_omitted() {
    let repo = init_bootstrap_repo();
    let start = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "start",
            "--root",
            repo.path().to_str().unwrap(),
            "--task-type",
            "docs_only",
            "--goal",
            "Update docs",
            "--intended-path",
            "docs/**",
        ])
        .output()
        .unwrap();
    assert!(start.status.success());

    std::fs::write(
        RepoStoragePaths::bootstrap_current_path(repo.path()),
        "not-a-runtime-run-id\n",
    )
    .unwrap();

    let abort = Command::new(repolens_bin())
        .args([
            "bootstrap",
            "abort",
            "--root",
            repo.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let value = error_envelope(&abort);
    assert!(value["error"].as_str().unwrap().contains("CURRENT"));
}
