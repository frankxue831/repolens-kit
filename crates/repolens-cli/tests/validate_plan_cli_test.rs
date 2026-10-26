use std::fs;
use std::path::Path;
use std::process::Command;

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

fn run_init(root: &Path) -> std::process::Output {
    Command::new(repolens_bin())
        .args(["init", "--root", root.to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens init")
}

fn run_validate_plan(root: &Path, plan: &Path) -> std::process::Output {
    Command::new(repolens_bin())
        .args([
            "validate-plan",
            "--root",
            root.to_str().unwrap(),
            "--file",
            plan.to_str().unwrap(),
        ])
        .output()
        .expect("failed to invoke repolens validate-plan")
}

fn write_constraints(root: &Path, body: &str) {
    fs::write(root.join(".repolens/known-constraints.md"), body).unwrap();
}

fn write_plan(root: &Path, body: &str) -> std::path::PathBuf {
    let path = root.join("plan.md");
    fs::write(&path, body).unwrap();
    path
}

const CAMEL_CONSTRAINT_BODY: &str = "# Known Constraints\n\n\
     ### C-001 JSON camelCase\n\
     - **Statement:** All JSON output uses camelCase (Java contract parity).\n\
     - **Evidence:** CLAUDE.md Key Patterns\n";

// ---------------------------------------------------------------------------
// CLI parsing / help
// ---------------------------------------------------------------------------

#[test]
fn validate_plan_help_states_warnings_only_and_uncertainty() {
    let out = Command::new(repolens_bin())
        .args(["validate-plan", "--help"])
        .output()
        .expect("failed to invoke validate-plan --help");
    assert!(
        out.status.success(),
        "validate-plan --help should exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stdout_lower = stdout.to_ascii_lowercase();
    assert!(
        stdout_lower.contains("warnings only"),
        "--help must include the 'warnings only' framing; stdout=\n{stdout}"
    );
    assert!(
        stdout_lower.contains("does not prove plans valid")
            || stdout_lower.contains("does not prove the plan is valid"),
        "--help must include the explicit no-proof statement; stdout=\n{stdout}"
    );
    assert!(
        stdout_lower.contains("not a replacement for tests")
            || stdout_lower.contains("review against source files and tests")
            || stdout_lower.contains("not a replacement for tests or human review"),
        "--help must say this is not a replacement for tests/human review; stdout=\n{stdout}"
    );
}

#[test]
fn validate_plan_help_subcommand_appears_in_top_level_help() {
    let out = Command::new(repolens_bin())
        .args(["--help"])
        .output()
        .expect("failed to invoke --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("validate-plan"),
        "top-level --help must list the validate-plan subcommand; stdout=\n{stdout}"
    );
}

#[test]
fn validate_plan_requires_file_arg() {
    let out = Command::new(repolens_bin())
        .args(["validate-plan"])
        .output()
        .expect("failed to invoke validate-plan with no args");
    assert!(
        !out.status.success(),
        "validate-plan with no --file should fail to parse"
    );
}

// ---------------------------------------------------------------------------
// Workspace / file-not-found errors
// ---------------------------------------------------------------------------

#[test]
fn validate_plan_errors_when_workspace_missing() {
    let dir = tempfile::tempdir().unwrap();
    let plan = write_plan(dir.path(), "# Plan\nsomething\n");
    let out = run_validate_plan(dir.path(), &plan);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("RepoLens workspace not found") && stderr.contains("repolens init"),
        "stderr should explain how to fix; stderr={stderr}"
    );
}

#[test]
fn validate_plan_errors_when_plan_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let missing = dir.path().join("nonexistent-plan.md");
    let out = run_validate_plan(dir.path(), &missing);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Cannot read plan file"),
        "stderr should say it cannot read the plan; stderr={stderr}"
    );
}

// ---------------------------------------------------------------------------
// ACE-002-motivated rule set (VP001 / VP002 / VP003)
// ---------------------------------------------------------------------------

#[test]
fn validate_plan_warns_on_total_count_under_camelcase_constraint() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    let plan = write_plan(
        dir.path(),
        "# Plan\nAdd a new field total_count to the compare-summaries output.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(
        out.status.success(),
        "default exit should be 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[VP001]"),
        "expected VP001 warning for snake_case under camelCase constraint; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("total_count"),
        "VP001 should name the offending token"
    );
    assert!(
        stdout.contains("C-001"),
        "VP001 should cite the matching constraint id"
    );
    assert!(
        stdout.contains("RepoLens found possible plan risks"),
        "must include the canonical findings disclaimer"
    );
    assert!(
        stdout.contains("review against source files and tests")
            || stdout.contains("Review against source files and tests"),
        "must include the 'review against source files and tests' uncertainty language"
    );
}

#[test]
fn validate_plan_no_snake_warning_when_plan_uses_camelcase_with_verification_step() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    let plan = write_plan(
        dir.path(),
        "# Plan\nFirst locate the compare-summaries output type before editing. \
         Per C-001, add a new field totalCount to the output. \
         Then update the snapshot tests via `cargo insta review`.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("[VP001]"),
        "no snake_case warning should fire when plan uses camelCase; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("[VP002]"),
        "no missing-citation warning when plan cites C-001; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("[VP003]"),
        "no grounding warning when plan states verification + names the surface; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("found no possible plan risks"),
        "expected the no-findings disclaimer; stdout=\n{stdout}"
    );
}

#[test]
fn validate_plan_warns_when_output_change_lacks_test_or_doc_mention() {
    // VP004 acceptance (positive): a plan that satisfies VP001/VP002/VP003
    // (camelCase, cites C-001, names a file, states verification) but never
    // mentions tests/snapshots/docs should emit VP004.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    let plan = write_plan(
        dir.path(),
        "# Plan\nFirst locate the compare-summaries output type in \
         `crates/repolens-app/src/summary.rs` before editing. \
         Per C-001, add the totalCount field.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[VP004]"),
        "expected VP004 when output change has no test/snapshot/doc mention; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("tests / snapshots / docs"),
        "VP004 message should call out tests / snapshots / docs explicitly; stdout=\n{stdout}"
    );
    // Sanity: VP001/VP002/VP003 should NOT fire on this plan.
    assert!(
        !stdout.contains("[VP001]"),
        "VP001 should not fire on a plan that uses camelCase; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("[VP002]"),
        "VP002 should not fire when plan cites C-001; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("[VP003]"),
        "VP003 should not fire when plan names file + verification; stdout=\n{stdout}"
    );
}

#[test]
fn validate_plan_silent_on_vp004_when_plan_mentions_cargo_test() {
    // VP004 acceptance (negative): same plan, plus "Then run `cargo test`",
    // should remain silent on VP004.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    let plan = write_plan(
        dir.path(),
        "# Plan\nFirst locate the compare-summaries output type in \
         `crates/repolens-app/src/summary.rs` before editing. \
         Per C-001, add the totalCount field. Then run `cargo test`.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("[VP004]"),
        "VP004 should be silent when plan mentions tests; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("found no possible plan risks"),
        "expected the no-findings disclaimer; stdout=\n{stdout}"
    );
}

#[test]
fn validate_plan_warns_when_plan_lacks_grounding_or_verification() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    // Cite the constraint to silence VP002, use camelCase to silence VP001 —
    // so VP003 is the load-bearing assertion.
    let plan = write_plan(
        dir.path(),
        "# Plan\nPer C-001, add totalCount to the output.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[VP003]"),
        "expected VP003 grounding warning; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("file/module/type/struct") && stdout.contains("verification step"),
        "VP003 message should mention both grounding paths; stdout=\n{stdout}"
    );
}

#[test]
fn validate_plan_emits_no_constraint_warnings_when_no_known_constraints() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    // Pristine init leaves known-constraints.md as TemplateOnly.
    let plan = write_plan(
        dir.path(),
        "# Plan\nAdd a new field total_count to the compare-summaries output.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no real constraints loaded"),
        "should print the no-constraints note; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Skipping constraint-aware checks (VP001, VP002, VP004)"),
        "should list every constraint-aware check skipped without constraints; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("[VP001]") && !stdout.contains("[VP002]") && !stdout.contains("[VP004]"),
        "constraint-aware checks must not fire when no real constraints exist; stdout=\n{stdout}"
    );
    // VP003 may still fire — that check is constraint-independent.
}

#[test]
fn validate_plan_emits_no_warnings_when_no_constraints_and_grounded_plan() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let plan = write_plan(
        dir.path(),
        "# Plan\nEdit `crates/repolens-app/src/summary.rs` to add a totalCount field. \
         Run `cargo test`.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no real constraints loaded"),
        "TemplateOnly constraints should print the note; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("found no possible plan risks"),
        "no warnings expected; stdout=\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Determinism + workspace safety
// ---------------------------------------------------------------------------

#[test]
fn validate_plan_output_is_deterministic_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    let plan = write_plan(
        dir.path(),
        "# Plan\nAdd a new field total_count to the compare-summaries output.\n",
    );

    let a = run_validate_plan(dir.path(), &plan).stdout;
    let b = run_validate_plan(dir.path(), &plan).stdout;
    assert_eq!(a, b, "validate-plan stdout must be byte-deterministic");
}

#[test]
fn validate_plan_does_not_create_or_modify_repolens_files() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);

    // Snapshot every file under .repolens/ before the run.
    let workspace = dir.path().join(".repolens");
    let mut before: Vec<(std::path::PathBuf, String)> = fs::read_dir(&workspace)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.path(), fs::read_to_string(e.path()).unwrap()))
        .collect();
    before.sort_by(|a, b| a.0.cmp(&b.0));

    let plan = write_plan(
        dir.path(),
        "# Plan\nAdd a new field total_count to the compare-summaries output.\n",
    );
    assert!(run_validate_plan(dir.path(), &plan).status.success());

    for (path, expected) in &before {
        let actual = fs::read_to_string(path).unwrap();
        assert_eq!(
            &actual,
            expected,
            "validate-plan modified workspace file: {}",
            path.display()
        );
    }
    // Memory graph must not be created either.
    assert!(
        !dir.path().join(".repolens/memory/memory.json").exists(),
        "validate-plan must not write the typed memory graph"
    );
}

#[test]
fn validate_plan_disclaimer_present_even_when_clean() {
    // A plan that triggers nothing should still surface the warnings-only
    // framing — the load-bearing rule is "warnings only, even on clean output".
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    write_constraints(dir.path(), CAMEL_CONSTRAINT_BODY);
    let plan = write_plan(
        dir.path(),
        "# Plan\nFirst locate the compare-summaries output type in \
         `crates/repolens-app/src/summary.rs` before editing. \
         Per C-001, add the totalCount field. \
         Then run `cargo test` and `cargo insta review`.\n",
    );

    let out = run_validate_plan(dir.path(), &plan);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("warnings only") && stdout.contains("guardrail, not proof"),
        "header must always print; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("does not prove the plan is valid"),
        "no-findings disclaimer must always print; stdout=\n{stdout}"
    );
}
