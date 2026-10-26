use std::fs;
use std::path::{Path, PathBuf};
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

fn run_eval_list() -> std::process::Output {
    Command::new(repolens_bin())
        .args(["eval", "list"])
        .output()
        .expect("failed to invoke repolens eval list")
}

fn run_eval_list_with_root(root: &Path) -> std::process::Output {
    Command::new(repolens_bin())
        .args(["eval", "list", "--root", root.to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens eval list --root")
}

fn run_eval_start(root: &Path, case: &str, run_id: &str) -> std::process::Output {
    Command::new(repolens_bin())
        .args([
            "eval",
            "start",
            "--case",
            case,
            "--run-id",
            run_id,
            "--root",
            root.to_str().unwrap(),
        ])
        .output()
        .expect("failed to invoke repolens eval start")
}

fn run_dir(root: &Path, case: &str, run_id: &str) -> PathBuf {
    root.join(".repolens")
        .join("eval-runs")
        .join(format!("{case}-{run_id}"))
}

#[test]
fn eval_list_includes_all_ace_cases() {
    let out = run_eval_list();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();

    for id in [
        "ACE-001", "ACE-002", "ACE-003", "ACE-004", "ACE-005", "ACE-006", "ACE-007", "ACE-008",
        "ACE-009", "ACE-010", "ACE-011",
    ] {
        assert!(
            stdout.contains(id),
            "eval list output missing {id}; stdout=\n{stdout}"
        );
    }
    // Title for ACE-010 — verifies parsing of em-dash titles.
    assert!(
        stdout.contains("Stale-checkout detection before shipped-command dogfood"),
        "ACE-010 title missing; stdout=\n{stdout}"
    );
    // Title for ACE-011 — verifies the newest case is reachable from the
    // embedded doc + parser + formatter pipeline.
    assert!(
        stdout.contains("Evidence finding integration (calibrated findings consumption)"),
        "ACE-011 title missing; stdout=\n{stdout}"
    );
}

#[test]
fn eval_list_is_deterministic() {
    let a = run_eval_list();
    let b = run_eval_list();
    assert!(a.status.success() && b.status.success());
    assert_eq!(
        a.stdout, b.stdout,
        "eval list output should be deterministic"
    );
}

#[test]
fn eval_list_root_flag_does_not_require_workspace() {
    // Point `--root` at a temp dir with no `.repolens/`. The list command
    // must succeed: it reads the embedded ACE catalogue and ignores `--root`.
    let dir = tempfile::tempdir().unwrap();
    let out = run_eval_list_with_root(dir.path());
    assert!(
        out.status.success(),
        "eval list --root should succeed without .repolens/; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("ACE-010"),
        "eval list --root output missing ACE-010; stdout=\n{stdout}"
    );
    assert!(
        !dir.path().join(".repolens").exists(),
        "eval list --root must not create .repolens/"
    );
}

#[test]
fn eval_list_root_flag_output_matches_no_root() {
    // The whole point of accepting `--root` for surface consistency: the
    // value is ignored, so output is byte-identical.
    let dir = tempfile::tempdir().unwrap();
    let plain = run_eval_list();
    let with_root = run_eval_list_with_root(dir.path());
    assert!(plain.status.success());
    assert!(with_root.status.success());
    assert_eq!(
        plain.stdout, with_root.stdout,
        "eval list and eval list --root must produce byte-identical output"
    );
}

#[test]
fn eval_start_errors_when_workspace_missing() {
    let dir = tempfile::tempdir().unwrap();
    // No `repolens init` — `.repolens/` does not exist.

    let out = run_eval_start(dir.path(), "ACE-010", "stale-checkout-001");

    assert!(!out.status.success(), "should fail without .repolens/");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("RepoLens workspace not found. Run 'repolens init' first."),
        "missing init guidance; stderr={stderr}"
    );
}

#[test]
fn eval_start_creates_run_dir_and_four_files() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());

    let out = run_eval_start(dir.path(), "ACE-010", "stale-checkout-001");
    assert!(
        out.status.success(),
        "eval start failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let run = run_dir(dir.path(), "ACE-010", "stale-checkout-001");
    assert!(run.is_dir(), "run dir not created: {}", run.display());
    for name in [
        "case.md",
        "scorecard.md",
        "agent-transcript.md",
        "result.md",
    ] {
        let path = run.join(name);
        assert!(path.is_file(), "{name} not written at {}", path.display());
    }
}

#[test]
fn eval_start_case_md_contains_case_id_and_section() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "stale-checkout-001")
        .status
        .success());

    let case_md =
        fs::read_to_string(run_dir(dir.path(), "ACE-010", "stale-checkout-001").join("case.md"))
            .unwrap();
    assert!(
        case_md.contains("ACE-010"),
        "case.md missing ACE-010 id; body=\n{case_md}"
    );
    assert!(
        case_md.contains("Stale-checkout detection before shipped-command dogfood"),
        "case.md missing ACE-010 title; body=\n{case_md}"
    );
    // Body extracted from the embedded eval doc.
    assert!(
        case_md.contains("**Setup."),
        "case.md missing case body Setup section; body=\n{case_md}"
    );
    assert!(
        case_md.contains("**Pass criteria."),
        "case.md missing case body Pass criteria; body=\n{case_md}"
    );
    // Repo-orientation reminder must be present (per spec).
    assert!(
        case_md.contains("git status --short"),
        "case.md missing repo-orientation reminder; body=\n{case_md}"
    );
    // No unsubstituted placeholders.
    assert!(
        !case_md.contains("{{"),
        "case.md has unsubstituted placeholder; body=\n{case_md}"
    );
}

#[test]
fn eval_start_case_md_for_ace_011_carries_load_bearing_criteria() {
    // ACE-011 is the MCP-consumption case (the closer for the Phase 4B
    // measurement gap). Its case body is load-bearing: it must specify the
    // MCP-before-drafting requirement, the recorded-response citation rule
    // that disambiguates calibrated F-* finding ids from friction-log
    // F-YYYY-MM-DD-NNN ids, the wire-shape note that pins the JSON keys
    // (`.evidence[]`, `.inference`, etc.) so criteria 3/4 are enforceable
    // against the actual MCP shape, and the validate-plan pre-flight step.
    // If any of these are dropped from the embedded eval doc by accident,
    // ACE-011 stops measuring what it is designed to measure.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-011", "ace-011-smoketest")
        .status
        .success());

    let case_md =
        fs::read_to_string(run_dir(dir.path(), "ACE-011", "ace-011-smoketest").join("case.md"))
            .unwrap();

    // Item 1 must require MCP consumption before drafting (not just any
    // MCP consumption at any time — the timing matters because a
    // citation-bolt-on retrofit would otherwise still look compliant).
    assert!(
        case_md.contains("Consume the calibrated finding surface before drafting"),
        "ACE-011 case.md missing 'Consume the calibrated finding surface before drafting' criterion 1 heading; body=\n{case_md}"
    );
    assert!(
        case_md.contains("before writing the first-draft plan"),
        "ACE-011 case.md missing the chronological 'before writing the first-draft plan' rule; body=\n{case_md}"
    );

    // Item 2 must require the cited id to match the recorded response.
    // The friction-log F-YYYY-MM-DD-NNN exclusion is a consequence of
    // this stricter rule, not a substitute. Both must be present.
    assert!(
        case_md.contains("Cite a finding id that matches the recorded response"),
        "ACE-011 case.md missing the recorded-response-match rule for criterion 2; body=\n{case_md}"
    );
    assert!(
        case_md.contains("F-YYYY-MM-DD-NNN"),
        "ACE-011 case.md missing friction-log id-shape exclusion; body=\n{case_md}"
    );

    // The Wire-shape note is load-bearing because criteria 3 and 4 cite
    // exact JSON keys (`.evidence[]`, `.inference`, `.limitations[]`,
    // `.action`). If those keys ever drift (paraphrased to
    // `observation` / `responsibleInference` etc.), the case is
    // unenforceable against the real MCP shape. All four top-level keys
    // are checked so a partial drift (e.g. someone re-paraphrases
    // `.action` while leaving the others alone) still fails the test.
    assert!(
        case_md.contains("Wire-shape note"),
        "ACE-011 case.md missing the Wire-shape note that pins the JSON keys; body=\n{case_md}"
    );
    for key in [".evidence[]", ".inference", ".limitations[]", ".action"] {
        assert!(
            case_md.contains(key),
            "ACE-011 case.md missing the actual JSON key `{key}` from the four-part calibration block; body=\n{case_md}"
        );
    }

    // Item 5 (validate-plan pre-flight) is shared with ACE-002 as the
    // round-6.1 process gate. Skipping the check would mask whether the
    // case is enforcing the linter discipline.
    assert!(
        case_md.contains("validate-plan"),
        "ACE-011 case.md missing validate-plan pre-flight criterion; body=\n{case_md}"
    );

    // Round-7 false-FAIL fix: criterion 1 must credit `repolens findings`
    // (the byte-identical CLI surface) as valid consumption, while keeping
    // the "`repolens brief` alone does not count" exclusion intact.
    assert!(
        case_md.contains("repolens findings"),
        "ACE-011 case.md must credit `repolens findings` as valid calibrated-finding consumption; body=\n{case_md}"
    );
    assert!(
        case_md.contains("does not satisfy this criterion either"),
        "ACE-011 case.md must preserve the `repolens brief`-alone exclusion; body=\n{case_md}"
    );
}

#[test]
fn eval_start_scorecard_contains_required_metrics() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "stale-checkout-001")
        .status
        .success());

    let scorecard = fs::read_to_string(
        run_dir(dir.path(), "ACE-010", "stale-checkout-001").join("scorecard.md"),
    )
    .unwrap();

    for metric in [
        "time_to_useful_context",
        "repeated_discovery_count",
        "grep_fallback_count",
        "wrong_file_attempts",
        "clarification_count",
        "known_constraint_miss",
        "first_plan_quality",
        "human_intervention_count",
        "session_handoff_quality",
        "stale_checkout_reconciled",
        "memory_safety_notice_used",
    ] {
        assert!(
            scorecard.contains(metric),
            "scorecard.md missing metric {metric}; body=\n{scorecard}"
        );
    }
    assert!(
        scorecard.contains("ACE-010"),
        "scorecard.md missing case id"
    );
    for metadata in [
        "Pack id",
        "Schema version",
        "Worktree dirty",
        "docs/evals/agent-context-evals.md",
    ] {
        assert!(
            scorecard.contains(metadata),
            "scorecard.md missing metadata/reference {metadata}; body=\n{scorecard}"
        );
    }
    assert!(
        !scorecard.contains("../../../../docs/evals/agent-context-evals.md"),
        "scorecard.md contains broken generated docs link; body=\n{scorecard}"
    );
    assert!(
        scorecard.contains("Pass / Fail"),
        "scorecard.md missing verdict section"
    );
    assert!(
        !scorecard.contains("{{"),
        "scorecard.md has unsubstituted placeholder; body=\n{scorecard}"
    );
}

#[test]
fn eval_start_case_md_for_ace_002_carries_tightened_pass_criteria() {
    // Round 5.2 follow-up: the generated `case.md` for ACE-002 must
    // surface the four-part tightened pass bar, the `pass`/`partial`/
    // `fail` scoring vocabulary, and the explicit fail rule for pure
    // string-replacing agents — so a reviewer reading just the per-run
    // case.md (without the canonical doc) sees the load-bearing rule.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-002", "tightened-criteria")
        .status
        .success());

    let case_md =
        fs::read_to_string(run_dir(dir.path(), "ACE-002", "tightened-criteria").join("case.md"))
            .unwrap();

    // Title carried into the wrapper heading.
    assert!(
        case_md.contains("ACE-002") && case_md.contains("Constraint adherence"),
        "case.md missing ACE-002 title; body=\n{case_md}"
    );
    // Bait note + tightened pass criteria.
    for phrase in [
        "Bait",
        "totalCount",
        "total_count",
        "first-draft plan",
        "Respect the constraint",
        "Name the affected output surface",
        "Ground the implementation path",
        "Include verification",
        "tests / snapshots / docs",
    ] {
        assert!(
            case_md.contains(phrase),
            "ACE-002 case.md missing tightened-criteria phrase '{phrase}'; body=\n{case_md}"
        );
    }
    // Scoring vocabulary present and load-bearing.
    for verdict in ["`pass`", "`partial`", "`fail`"] {
        assert!(
            case_md.contains(verdict),
            "ACE-002 case.md missing scoring verdict {verdict}"
        );
    }
    // Round 6.1: validate-plan is now a Pre-flight self-check workflow
    // requirement (item 5). The Round 5.2 no-escape-hatch rule (item 1's
    // "first draft must not propose `total_count`") still applies, so
    // validate-plan cannot be used as a second-draft fixup.
    assert!(
        case_md.contains("validate-plan"),
        "ACE-002 case.md must surface the Round 6.1 validate-plan workflow requirement"
    );
    assert!(
        case_md.contains("Pre-flight self-check") || case_md.contains("pre-flight self-check"),
        "ACE-002 case.md must label the validate-plan step as a pre-flight self-check, \
         not a content fixup"
    );
    assert!(
        case_md.contains("first draft must not propose `total_count`"),
        "ACE-002 case.md must preserve the first-draft no-escape-hatch rule from Round 5.2"
    );
}

#[test]
fn eval_start_transcript_and_result_have_run_id_and_no_placeholders() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-001", "run-1")
        .status
        .success());

    for name in ["agent-transcript.md", "result.md"] {
        let body = fs::read_to_string(run_dir(dir.path(), "ACE-001", "run-1").join(name)).unwrap();
        assert!(
            body.contains("ACE-001"),
            "{name} missing case id; body=\n{body}"
        );
        assert!(
            body.contains("run-1"),
            "{name} missing run id; body=\n{body}"
        );
        assert!(
            !body.contains("{{"),
            "{name} has unsubstituted placeholder; body=\n{body}"
        );
    }
}

#[test]
fn eval_start_refuses_existing_run_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "stale-checkout-001")
        .status
        .success());

    // Mutate one of the files so we can detect overwrite if it happened.
    let case_md = run_dir(dir.path(), "ACE-010", "stale-checkout-001").join("case.md");
    fs::write(&case_md, "USER EDIT — DO NOT OVERWRITE").unwrap();

    let second = run_eval_start(dir.path(), "ACE-010", "stale-checkout-001");
    assert!(!second.status.success(), "second start should fail");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected 'already exists' error; stderr={stderr}"
    );

    // User edit is preserved — refuse-on-existing must NOT have rewritten anything.
    assert_eq!(
        fs::read_to_string(&case_md).unwrap(),
        "USER EDIT — DO NOT OVERWRITE"
    );
}

#[test]
fn eval_start_unknown_case_errors_with_suggestion() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());

    let out = run_eval_start(dir.path(), "ACE-999", "run-1");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Unknown ACE case 'ACE-999'"),
        "stderr should name the case; stderr={stderr}"
    );
    assert!(
        stderr.contains("repolens eval list"),
        "stderr should suggest list; stderr={stderr}"
    );
}

#[test]
fn eval_start_invalid_run_id_rejected() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());

    for bad in ["..", "../etc", "a/b", "a\\b", "a..b", "with space", ""] {
        let out = run_eval_start(dir.path(), "ACE-010", bad);
        assert!(
            !out.status.success(),
            "should reject run-id {bad:?}; stdout={}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
    // Confirm that no eval-runs directory was ever created from these.
    let evals = dir.path().join(".repolens").join("eval-runs");
    if evals.exists() {
        if let Some(entry) = fs::read_dir(&evals).unwrap().next() {
            let entry = entry.unwrap();
            panic!(
                "rejected run-id should not have created a dir: {}",
                entry.path().display()
            );
        }
    }
}

/// Strip the Round 5.2 provenance block (between
/// `<!-- repolens:provenance -->` markers) before comparing two scaffolded
/// artifacts. The provenance fields (timestamp, source commit) legitimately
/// vary across runs/workspaces; the rest of the artifact must not.
fn strip_provenance(body: &str) -> String {
    const BEGIN: &str = "<!-- repolens:provenance -->";
    const END: &str = "<!-- /repolens:provenance -->";
    let Some(start) = body.find(BEGIN) else {
        return body.to_string();
    };
    let after = &body[start..];
    let Some(end_rel) = after.find(END) else {
        return body.to_string();
    };
    let end = start + end_rel + END.len();
    let mut out = String::with_capacity(body.len());
    out.push_str(&body[..start]);
    out.push_str(body[end..].trim_start_matches(['\n', '\r']));
    out
}

#[test]
fn eval_start_output_is_deterministic_across_two_workspaces() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    assert!(run_init(dir_a.path()).status.success());
    assert!(run_init(dir_b.path()).status.success());

    assert!(
        run_eval_start(dir_a.path(), "ACE-010", "stale-checkout-001")
            .status
            .success()
    );
    assert!(
        run_eval_start(dir_b.path(), "ACE-010", "stale-checkout-001")
            .status
            .success()
    );

    for name in [
        "case.md",
        "scorecard.md",
        "agent-transcript.md",
        "result.md",
    ] {
        let a =
            fs::read_to_string(run_dir(dir_a.path(), "ACE-010", "stale-checkout-001").join(name))
                .unwrap();
        let b =
            fs::read_to_string(run_dir(dir_b.path(), "ACE-010", "stale-checkout-001").join(name))
                .unwrap();
        assert_eq!(
            strip_provenance(&a),
            strip_provenance(&b),
            "{name} non-provenance content should be byte-identical across workspaces"
        );
    }
}

#[test]
fn eval_start_artifacts_carry_provenance_block() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-2-prov")
        .status
        .success());

    for name in [
        "case.md",
        "scorecard.md",
        "agent-transcript.md",
        "result.md",
    ] {
        let body = fs::read_to_string(run_dir(dir.path(), "ACE-010", "round-5-2-prov").join(name))
            .unwrap();
        assert!(
            body.contains("<!-- repolens:provenance -->"),
            "{name} missing provenance opening marker; body=\n{body}"
        );
        assert!(
            body.contains("<!-- /repolens:provenance -->"),
            "{name} missing provenance closing marker; body=\n{body}"
        );
        for label in [
            "generated_by:",
            "template_set:",
            "scorecard_schema:",
            "generated_at:",
            "source_commit:",
        ] {
            assert!(
                body.contains(label),
                "{name} missing provenance label {label}; body=\n{body}"
            );
        }
        assert!(
            body.contains("repolens@"),
            "{name} provenance missing crate version tag; body=\n{body}"
        );
        assert!(
            body.contains("ace-eval@round-5.2"),
            "{name} provenance missing template_set id; body=\n{body}"
        );
        assert!(
            body.contains("repolens-scorecard@round-5.2"),
            "{name} provenance missing scorecard_schema id; body=\n{body}"
        );
    }
}

#[test]
fn eval_start_provenance_block_precedes_first_rendered_heading() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(
        run_eval_start(dir.path(), "ACE-010", "round-5-2-prov-order")
            .status
            .success()
    );

    let scorecard = fs::read_to_string(
        run_dir(dir.path(), "ACE-010", "round-5-2-prov-order").join("scorecard.md"),
    )
    .unwrap();
    let prov_pos = scorecard
        .find("<!-- repolens:provenance -->")
        .expect("provenance marker missing");
    let heading_pos = scorecard
        .find("# Eval Scorecard")
        .expect("scorecard heading missing");
    assert!(
        prov_pos < heading_pos,
        "provenance block must precede the rendered heading; scorecard=\n{scorecard}"
    );
}

#[test]
fn eval_start_provenance_records_source_commit_when_repo_present() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    // Make `dir` an actual git repo so `git rev-parse HEAD` succeeds.
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .expect("git failed");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    git(&["commit", "--allow-empty", "-qm", "init"]);

    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-001", "round-5-2-commit")
        .status
        .success());

    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-001", "round-5-2-commit").join("scorecard.md"))
            .unwrap();
    // The recorded commit should be a real SHA (not "unknown"). Find the
    // line and confirm it carries hex.
    let line = scorecard
        .lines()
        .find(|l| l.contains("source_commit:"))
        .expect("source_commit line missing");
    assert!(
        !line.contains("unknown"),
        "expected real commit SHA in provenance, got: {line}"
    );
    let has_hex_sha = line
        .split('`')
        .any(|tok| tok.len() >= 7 && tok.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(
        has_hex_sha,
        "source_commit line should contain a hex SHA; line={line}"
    );
}

#[test]
fn eval_start_provenance_records_unknown_commit_when_repo_absent() {
    let dir = tempfile::tempdir().unwrap();
    // Do NOT git-init `dir` — `git rev-parse HEAD` should fail and the
    // provenance must degrade to `unknown` (never abort the scaffold).
    assert!(run_init(dir.path()).status.success());
    let out = run_eval_start(dir.path(), "ACE-001", "round-5-2-no-git");
    assert!(
        out.status.success(),
        "eval start must succeed even when source commit is unavailable; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-001", "round-5-2-no-git").join("scorecard.md"))
            .unwrap();
    let line = scorecard
        .lines()
        .find(|l| l.contains("source_commit:"))
        .expect("source_commit line missing");
    assert!(
        line.contains("unknown"),
        "expected source_commit=unknown when no git repo; line={line}"
    );
}

#[test]
fn eval_start_does_not_create_memory_graph_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "stale-checkout-001")
        .status
        .success());

    let memory_path = dir.path().join(".repolens/memory/memory.json");
    assert!(
        !memory_path.exists(),
        "eval start must not create .repolens/memory/memory.json"
    );
}

#[test]
fn scorecard_includes_metric_definitions_section() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-1-defs")
        .status
        .success());

    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-010", "round-5-1-defs").join("scorecard.md"))
            .unwrap();

    assert!(
        scorecard.contains("## Metric Definitions"),
        "scorecard.md missing '## Metric Definitions' heading; body=\n{scorecard}"
    );
}

#[test]
fn scorecard_defines_every_listed_metric() {
    // Every metric in the scorecard's value table must have a corresponding
    // definition heading in the Metric Definitions section.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-1-each")
        .status
        .success());

    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-010", "round-5-1-each").join("scorecard.md"))
            .unwrap();

    for metric in [
        "time_to_useful_context",
        "repeated_discovery_count",
        "grep_fallback_count",
        "wrong_file_attempts",
        "clarification_count",
        "known_constraint_miss",
        "first_plan_quality",
        "human_intervention_count",
        "session_handoff_quality",
        "stale_checkout_reconciled",
        "memory_safety_notice_used",
    ] {
        let heading = format!("### `{metric}`");
        assert!(
            scorecard.contains(&heading),
            "scorecard.md missing definition heading {heading}; body=\n{scorecard}"
        );
    }
}

#[test]
fn scorecard_definitions_state_accepted_values_for_each_metric() {
    // Every metric definition should specify what may be written in the
    // Value column. We assert that "Accepted values." appears at least
    // 11 times — once per metric.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-1-accepted")
        .status
        .success());

    let scorecard = fs::read_to_string(
        run_dir(dir.path(), "ACE-010", "round-5-1-accepted").join("scorecard.md"),
    )
    .unwrap();

    let count = scorecard.matches("**Accepted values.**").count();
    assert!(
        count >= 11,
        "expected >=11 'Accepted values.' entries in scorecard.md, found {count}; body=\n{scorecard}"
    );
}

#[test]
fn scorecard_stale_checkout_definition_lists_categorical_values() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-1-stale")
        .status
        .success());

    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-010", "round-5-1-stale").join("scorecard.md"))
            .unwrap();

    // Find the stale_checkout_reconciled definition block, then verify it
    // mentions every accepted categorical value.
    let heading = "### `stale_checkout_reconciled`";
    let start = scorecard
        .find(heading)
        .unwrap_or_else(|| panic!("missing heading {heading}; body=\n{scorecard}"));
    // Scope to the next heading so we do not match values in a sibling
    // definition.
    let rest = &scorecard[start..];
    let end = rest[heading.len()..]
        .find("\n### ")
        .map(|i| heading.len() + i)
        .unwrap_or(rest.len());
    let block = &rest[..end];

    for value in ["`pass`", "`partial`", "`fail`", "`n/a`"] {
        assert!(
            block.contains(value),
            "stale_checkout_reconciled definition missing {value}; block=\n{block}"
        );
    }
}

#[test]
fn scorecard_memory_safety_definition_mentions_memory_related_tasks() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-1-mem")
        .status
        .success());

    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-010", "round-5-1-mem").join("scorecard.md"))
            .unwrap();

    let heading = "### `memory_safety_notice_used`";
    let start = scorecard
        .find(heading)
        .unwrap_or_else(|| panic!("missing heading {heading}; body=\n{scorecard}"));
    let rest = &scorecard[start..];
    let end = rest[heading.len()..]
        .find("\n### ")
        .map(|i| heading.len() + i)
        .unwrap_or_else(|| {
            rest[heading.len()..]
                .find("\n## ")
                .map(|i| heading.len() + i)
                .unwrap_or(rest.len())
        });
    let block = &rest[..end];

    // The definition must explicitly call out memory-related task types.
    let lower = block.to_ascii_lowercase();
    assert!(
        lower.contains("memory"),
        "memory_safety_notice_used definition does not mention 'memory'; block=\n{block}"
    );
    for term in ["capture", "recall", "session"] {
        assert!(
            lower.contains(term),
            "memory_safety_notice_used definition should mention memory-related task type '{term}'; block=\n{block}"
        );
    }
}

/// Helper: extract the body of a `### \`<metric>\`` definition block from
/// the scorecard, scoped up to the next `### ` or `## ` heading.
fn metric_block<'a>(scorecard: &'a str, metric: &str) -> &'a str {
    let heading = format!("### `{metric}`");
    let start = scorecard
        .find(&heading)
        .unwrap_or_else(|| panic!("missing heading {heading}; body=\n{scorecard}"));
    let rest = &scorecard[start..];
    let after_heading = &rest[heading.len()..];
    let next_sub = after_heading.find("\n### ").map(|i| heading.len() + i);
    let next_top = after_heading.find("\n## ").map(|i| heading.len() + i);
    let end = match (next_sub, next_top) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => rest.len(),
    };
    &rest[..end]
}

#[test]
fn scorecard_session_handoff_definition_states_done_remaining_next_step_bar() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-004", "round-5-2-handoff")
        .status
        .success());

    let scorecard = fs::read_to_string(
        run_dir(dir.path(), "ACE-004", "round-5-2-handoff").join("scorecard.md"),
    )
    .unwrap();
    let block = metric_block(&scorecard, "session_handoff_quality");
    let lower = block.to_ascii_lowercase();
    for needle in ["what was done", "what remains", "next concrete step"] {
        assert!(
            lower.contains(needle),
            "session_handoff_quality def should state explicit pass bar containing '{needle}'; block=\n{block}"
        );
    }
    for value in ["`pass`", "`partial`", "`fail`", "`n/a`", "blank"] {
        assert!(
            block.contains(value),
            "session_handoff_quality def missing accepted value {value}; block=\n{block}"
        );
    }
}

#[test]
fn scorecard_memory_safety_definition_recognizes_remember_framing_and_passive_pass() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-004", "round-5-2-memsafety")
        .status
        .success());

    let scorecard = fs::read_to_string(
        run_dir(dir.path(), "ACE-004", "round-5-2-memsafety").join("scorecard.md"),
    )
    .unwrap();
    let block = metric_block(&scorecard, "memory_safety_notice_used");
    let lower = block.to_ascii_lowercase();
    for needle in [
        "remember",
        "agent-provided",
        "unverified",
        "hypothesis",
        "passive pass",
        "active pass",
    ] {
        assert!(
            lower.contains(needle),
            "memory_safety_notice_used def should mention '{needle}'; block=\n{block}"
        );
    }
}

#[test]
fn scorecard_wrong_file_attempts_definition_distinguishes_blank_n_a_and_zero() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-008", "round-5-2-wrongfile")
        .status
        .success());

    let scorecard = fs::read_to_string(
        run_dir(dir.path(), "ACE-008", "round-5-2-wrongfile").join("scorecard.md"),
    )
    .unwrap();
    let block = metric_block(&scorecard, "wrong_file_attempts");
    let lower = block.to_ascii_lowercase();
    assert!(
        block.contains("`0`") || block.contains("`0` "),
        "wrong_file_attempts def should explicitly mention `0`; block=\n{block}"
    );
    assert!(
        block.contains("`n/a`"),
        "wrong_file_attempts def should explicitly mention `n/a`; block=\n{block}"
    );
    for needle in ["plan-only", "unknown", "insufficient"] {
        assert!(
            lower.contains(needle),
            "wrong_file_attempts def should explain when to use blank vs n/a vs 0; block=\n{block}"
        );
    }
    assert!(
        lower.contains("not zero") || lower.contains("not \"zero\""),
        "wrong_file_attempts def should clarify blank ≠ zero; block=\n{block}"
    );
}

#[test]
fn scorecard_explains_blank_vs_n_a() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    assert!(run_eval_start(dir.path(), "ACE-010", "round-5-1-blank")
        .status
        .success());

    let scorecard =
        fs::read_to_string(run_dir(dir.path(), "ACE-010", "round-5-1-blank").join("scorecard.md"))
            .unwrap();

    let lower = scorecard.to_ascii_lowercase();
    assert!(
        lower.contains("leave blank") || lower.contains("leave a cell blank"),
        "scorecard.md should tell reviewers when to leave a cell blank; body=\n{scorecard}"
    );
    assert!(
        scorecard.contains("`n/a`"),
        "scorecard.md should tell reviewers when to use `n/a`; body=\n{scorecard}"
    );
    assert!(
        lower.contains("do not infer") || lower.contains("do not fabricate"),
        "scorecard.md should warn reviewers not to fabricate values; body=\n{scorecard}"
    );
}

#[test]
fn eval_start_does_not_modify_workspace_files() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());

    // Snapshot the workspace files before the eval run.
    let workspace = dir.path().join(".repolens");
    let mut before: Vec<(PathBuf, String)> = fs::read_dir(&workspace)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.path(), fs::read_to_string(e.path()).unwrap()))
        .collect();
    before.sort_by(|a, b| a.0.cmp(&b.0));

    assert!(run_eval_start(dir.path(), "ACE-010", "stale-checkout-001")
        .status
        .success());

    // Verify each top-level workspace file is byte-identical post-run.
    for (path, expected) in &before {
        let actual = fs::read_to_string(path).unwrap();
        assert_eq!(
            &actual,
            expected,
            "eval start modified workspace file: {}",
            path.display()
        );
    }
}
