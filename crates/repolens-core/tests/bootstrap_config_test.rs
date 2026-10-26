use std::io::Write;
use std::path::PathBuf;

use repolens_core::bootstrap::{
    load_bootstrap_config, load_bootstrap_contract_snapshot, validate_bootstrap_config,
    BootstrapSkillRegistry,
};
use repolens_core::model::bootstrap::BootstrapCommandSpec;
use serde_json::Value;
use tempfile::NamedTempFile;

fn bootstrap_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap.yaml")
}

fn legacy_bootstrap_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/legacy-bootstrap-string-commands.yaml")
}

#[test]
fn rejects_invalid_argv_commands() {
    assert!(BootstrapCommandSpec::Argv(vec![]).normalize().is_err());
    assert!(
        BootstrapCommandSpec::Argv(vec!["cargo".to_string(), "".to_string()])
            .normalize()
            .is_err()
    );
}

#[test]
fn rejects_malformed_contract_entries_in_config_loader() {
    let mut bad_commands = NamedTempFile::new().expect("temp file should create");
    write!(
        bad_commands,
        r#"
version: "0.1"
repo:
  name: "repolens-rs"
  root: "."
commands:
  build: ["cargo", "", "build"]
  test: ["cargo", "test"]
  fmt: ["cargo", "fmt", "--all"]
  clippy: ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]
  scan: ["cargo", "run", "--", "scan", "--root", "."]
  mcp: ["cargo", "run", "--", "mcp"]
taskTypes: ["docs_only"]
allowPaths: ["docs/**"]
denyPaths: [".git/**"]
requiredChecks:
  docs_only: []
memoryPolicy:
  requireAnchors: true
  rejectUnanchoredWrites: true
trustPolicy:
  repoUnderstanding: "repolens-first"
  coldStartException: "explicit-and-narrow"
  structuralEditMinimumGrade: "verified"
  planningMinimumGrade: "derived"
  unauditedUsage: "discuss-only"
skills:
  - id: "discover_repo_state"
    category: "discovery"
    purpose: "Fixture skill"
    requiresRepoLens: true
    allowsColdStart: true
    allowedEvidenceGrades: ["verified"]
    writesMemory: false
    requiresAnchors: false
"#
    )
    .expect("write should succeed");
    bad_commands.flush().expect("flush should succeed");

    let err = load_bootstrap_config(bad_commands.path()).expect_err("config should be rejected");
    assert!(err.to_string().contains("bootstrap command"));

    let mut bad_checks = NamedTempFile::new().expect("temp file should create");
    write!(
        bad_checks,
        r#"
version: "0.1"
repo:
  name: "repolens-rs"
  root: "."
commands:
  build: ["cargo", "build"]
  test: ["cargo", "test"]
  fmt: ["cargo", "fmt", "--all"]
  clippy: ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]
  scan: ["cargo", "run", "--", "scan", "--root", "."]
  mcp: ["cargo", "run", "--", "mcp"]
taskTypes: ["docs_only"]
allowPaths: ["docs/**"]
denyPaths: [".git/**"]
requiredChecks:
  docs_only:
    - ""
memoryPolicy:
  requireAnchors: true
  rejectUnanchoredWrites: true
trustPolicy:
  repoUnderstanding: "repolens-first"
  coldStartException: "explicit-and-narrow"
  structuralEditMinimumGrade: "verified"
  planningMinimumGrade: "derived"
  unauditedUsage: "discuss-only"
skills:
  - id: "discover_repo_state"
    category: "discovery"
    purpose: "Fixture skill"
    requiresRepoLens: true
    allowsColdStart: true
    allowedEvidenceGrades: ["verified"]
    writesMemory: false
    requiresAnchors: false
"#
    )
    .expect("write should succeed");
    bad_checks.flush().expect("flush should succeed");

    let err = load_bootstrap_config(bad_checks.path()).expect_err("config should be rejected");
    assert!(err.to_string().contains("requiredChecks"));
}

#[test]
fn loads_array_command_contract_and_hashes_bytes() {
    let snapshot =
        load_bootstrap_contract_snapshot(bootstrap_config_path()).expect("snapshot should load");

    assert_eq!(snapshot.raw.version, "0.1");
    assert_eq!(snapshot.frozen.version, "0.1");
    assert_eq!(snapshot.frozen.commands.fmt, vec!["cargo", "fmt", "--all"]);
    assert_eq!(
        snapshot.frozen.required_checks["bounded_rust_change"][1],
        vec![
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]
    );
    assert_eq!(snapshot.hash_sha256.len(), 64);

    let json: Value =
        serde_json::to_value(&snapshot.frozen).expect("frozen contract should serialize");
    let object = json
        .as_object()
        .expect("frozen contract should serialize to object");
    assert!(object.contains_key("requiredChecks"));
    assert!(object.contains_key("taskTypes"));
    assert!(object.contains_key("allowPaths"));
    assert!(object.contains_key("denyPaths"));
    assert!(!object.contains_key("required_checks"));
    assert!(!object.contains_key("task_types"));
    assert!(!object.contains_key("allow_paths"));
    assert!(!object.contains_key("deny_paths"));
}

#[test]
fn normalizes_legacy_string_commands() {
    let snapshot = load_bootstrap_contract_snapshot(legacy_bootstrap_config_path())
        .expect("snapshot should load");

    assert_eq!(snapshot.frozen.commands.test, vec!["cargo", "test"]);
    assert_eq!(
        snapshot.frozen.required_checks["trust_contract"],
        vec![vec!["cargo", "fmt", "--all"]]
    );
}

#[test]
fn loads_bootstrap_yaml_contract() {
    let config = load_bootstrap_config(bootstrap_config_path()).expect("config should load");

    assert_eq!(config.version, "0.1");
    assert_eq!(config.repo.name, "repolens-rs");
    assert_eq!(config.trust_policy.repo_understanding, "repolens-first");
}

#[test]
fn rejects_unanchored_memory_policy() {
    let mut config = load_bootstrap_config(bootstrap_config_path()).expect("config should load");
    config.memory_policy.require_anchors = false;

    let err = validate_bootstrap_config(&config).expect_err("config should be rejected");
    assert!(err.to_string().contains("requireAnchors"));
}

#[test]
fn rejects_config_missing_required_field() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/bad-bootstrap-missing-version.yaml");

    let err = load_bootstrap_config(path).expect_err("config should be rejected");
    assert!(err.to_string().contains("version"));
}

#[test]
fn loads_bootstrap_skill_definitions() {
    let config = load_bootstrap_config(bootstrap_config_path()).expect("config should load");

    assert_eq!(config.skills.len(), 5);
    assert_eq!(config.skills[0].id, "discover_repo_state");
    assert_eq!(config.skills[4].id, "write_anchored_memory");
    assert!(config.skills[4].writes_memory);
    assert!(config.skills[4].requires_anchors);
}

#[test]
fn builds_bootstrap_skill_registry() {
    let config = load_bootstrap_config(bootstrap_config_path()).expect("config should load");
    let registry = BootstrapSkillRegistry::from_config(&config).expect("registry should build");

    assert_eq!(registry.all().count(), 5);
    let skill = registry
        .get("write_anchored_memory")
        .expect("skill should exist");
    assert!(skill.writes_memory);
    assert!(skill.requires_anchors);
}

#[test]
fn rejects_duplicate_skill_ids() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/bad-bootstrap-duplicate-skill-id.yaml");

    let err = load_bootstrap_config(path).expect_err("config should be rejected");
    assert!(err.to_string().contains("duplicate skill id"));
}

#[test]
fn rejects_required_checks_for_unknown_task_type() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/bad-bootstrap-unknown-task-type.yaml");

    let err = load_bootstrap_config(path).expect_err("config should be rejected");
    let msg = err.to_string();
    assert!(msg.contains("requiredChecks"), "msg was: {msg}");
    assert!(msg.contains("bonded_rust_change"), "msg was: {msg}");
}

#[test]
fn rejects_memory_writing_skill_without_anchors() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/bad-bootstrap-unanchored-memory-skill.yaml");

    let err = load_bootstrap_config(path).expect_err("config should be rejected");
    assert!(err.to_string().contains("requiresAnchors"));
}
