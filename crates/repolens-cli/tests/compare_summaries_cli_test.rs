//! End-to-end coverage for `repolens compare-summaries`. Builds two
//! minimal packs in a tempdir (just enough structure for
//! SummaryService::summarize + ComparisonService::compare to succeed)
//! and asserts markdown-block shape plus determinism.

use std::fs;
use std::path::Path;
use std::process::Command;

fn binary() -> &'static str {
    // Cargo sets this env var at compile time for integration tests;
    // it always points to the built `repolens` binary, so we don't
    // need to walk the target directory by hand.
    env!("CARGO_BIN_EXE_repolens")
}

fn write_minimal_pack(storage: &Path, pack_id: &str) {
    // Minimum structure SummaryService::summarize and
    // ComparisonService::compare need to emit a non-None result with
    // populated fitness scores. If the pack schema changes (files-NNNN.json
    // chunk format, manifest required fields, graph-metrics.json shape),
    // update this fixture in lockstep.
    let pack = storage.join(pack_id);
    fs::create_dir_all(&pack).unwrap();

    // manifest.json — PackReader::read parses this then reads each chunk in
    // the `chunks` array.  `timestamp` is a JavaEpochSeconds (accepts any
    // JSON number).
    fs::write(
        pack.join("manifest.json"),
        format!(
            r#"{{"schemaVersion":"1.3.0","schemaUrl":"","timestamp":1.0,"chunks":["files-0001.json"],"scanMode":"full","scanScope":"complete","basePackId":null,"baseHeadCommit":null,"headCommit":"{}","worktreeDirty":false,"scanConfigHash":"h","reprocessedFiles":1,"deletedFiles":null,"carriedForwardFiles":null}}"#,
            "0".repeat(40)
        ),
    )
    .unwrap();

    // files-0001.json — empty file list is valid; SummaryService copes with
    // zero entries (fitness scorer handles the degenerate case).
    fs::write(pack.join("files-0001.json"), r#"[]"#).unwrap();

    // graph.json — FileGraph has only `nodes` and `edges`; extra keys
    // (schemaVersion) are silently ignored by serde.
    fs::write(pack.join("graph.json"), r#"{"nodes":[],"edges":[]}"#).unwrap();

    // graph-metrics.json — GraphResolutionMetrics struct, camelCase.
    // totalRelationships > 0 so FitnessScorer does not short-circuit to a
    // degenerate resolution_rate of 1.0 that hides real scoring logic, and
    // ComparisonService gets non-None fitness scores from both packs.
    fs::write(
        pack.join("graph-metrics.json"),
        r#"{"totalRelationships":10,"resolvedEdges":8,"unresolvedEdges":2}"#,
    )
    .unwrap();
}

fn init_git_repo(path: &Path) {
    Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(path)
        .status()
        .unwrap();
    fs::write(path.join("README.md"), "# sandbox").unwrap();
    Command::new("git")
        .args(["-C", path.to_str().unwrap(), "add", "README.md"])
        .status()
        .unwrap();
    Command::new("git")
        .args([
            "-C",
            path.to_str().unwrap(),
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "init",
        ])
        .status()
        .unwrap();
}

#[test]
fn compare_summaries_emits_stable_markdown_for_two_hand_built_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    init_git_repo(root);

    let storage = root.join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    write_minimal_pack(&storage, "pack-20260101-000000");
    write_minimal_pack(&storage, "pack-20260102-000000");

    let output = Command::new(binary())
        .current_dir(root)
        .args(["compare-summaries"])
        .output()
        .expect("failed to run repolens");

    // On a comparison-infeasible input (fitness missing), we expect a
    // clear error rather than silent success. If the minimal fixture
    // does not satisfy SummaryService + ComparisonService, the test
    // fails loudly with the binary's stderr so the fixture can be
    // updated — this test deliberately does not paper over that.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compare-summaries failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert!(
        stdout.contains("## Summary Comparison"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("pack-20260101-000000"), "stdout:\n{stdout}");
    assert!(stdout.contains("pack-20260102-000000"), "stdout:\n{stdout}");
    // Fitness Scores block was retired in the 2026-05-11 Phase 4B Step 1
    // amendment — the calibrated `### Metrics` and evidence-findings
    // sections carry the picture now.
    assert!(
        !stdout.contains("### Fitness Scores"),
        "fitness block must not appear after Phase 4B Step 1; stdout:\n{stdout}"
    );
    assert!(stdout.contains("### Metrics"), "stdout:\n{stdout}");
    // Recommendations section was retired in the 2026-05-13 Phase 4B Step 3
    // amendment — calibrated delta findings live in `evidenceFindings` in
    // the JSON output, not the markdown render.
    assert!(
        !stdout.contains("### Recommendations"),
        "recommendations block must not appear after Phase 4B Step 3; stdout:\n{stdout}"
    );

    // Determinism: run a second time, assert byte-equivalent stdout.
    let rerun = Command::new(binary())
        .current_dir(root)
        .args(["compare-summaries"])
        .output()
        .expect("failed to re-run repolens");
    assert_eq!(
        output.stdout, rerun.stdout,
        "compare-summaries stdout must be deterministic for the same pair of packs"
    );
}

#[test]
fn compare_summaries_single_pack_reports_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    init_git_repo(root);

    let storage = root.join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    write_minimal_pack(&storage, "pack-20260101-000000");

    let output = Command::new(binary())
        .current_dir(root)
        .args(["compare-summaries"])
        .output()
        .expect("failed to run repolens");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Only 1 pack"), "stderr:\n{stderr}");
    assert!(output.stdout.is_empty(), "no partial stdout on failure");
}
