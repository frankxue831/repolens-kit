use std::fs;
use std::path::Path;
use std::process::Command;

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

fn run_brief(root: &Path, task: &str) -> std::process::Output {
    Command::new(repolens_bin())
        .args(["brief", "--task", task, "--root", root.to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens brief")
}

fn run_init(root: &Path) -> std::process::Output {
    Command::new(repolens_bin())
        .args(["init", "--root", root.to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens init")
}

fn brief_path(root: &Path) -> std::path::PathBuf {
    root.join(".repolens/current-agent-brief.md")
}

#[test]
fn brief_stdout_prints_full_brief_and_writes_nothing() {
    // Dogfood round 2 friction finding 2: the unconditional artifact write
    // is a contract violation for agents operating read-only. `--stdout`
    // prints the full brief and skips the write entirely.
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success(), "init must succeed");
    // `repolens init` scaffolds the artifact as a template; the --stdout
    // contract is that brief leaves it byte-identical (no write at all).
    let scaffold = fs::read(brief_path(dir.path())).expect("init scaffolds the brief artifact");

    let out = Command::new(repolens_bin())
        .args(["brief", "--stdout", "--root", dir.path().to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens brief --stdout");
    assert!(
        out.status.success(),
        "brief --stdout must succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("# Current Agent Brief"),
        "--stdout must print the full brief body; stdout={stdout}"
    );
    assert!(
        stdout.contains("No task was provided"),
        "task-less --stdout brief must carry the default task; stdout={stdout}"
    );
    let after = fs::read(brief_path(dir.path())).unwrap();
    assert_eq!(
        scaffold, after,
        "--stdout must leave .repolens/current-agent-brief.md byte-identical"
    );
}

#[test]
fn brief_default_mode_still_writes_artifact() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success(), "init must succeed");

    let out = run_brief(dir.path(), "any task");
    assert!(out.status.success());
    assert!(
        brief_path(dir.path()).exists(),
        "default mode must keep writing the artifact"
    );
}

#[test]
fn brief_errors_when_workspace_missing() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_brief(dir.path(), "anything");
    assert!(
        !out.status.success(),
        "expected non-zero exit when .repolens/ is absent"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("RepoLens workspace not found"),
        "missing workspace-not-found message; stderr={stderr}"
    );
    assert!(
        stderr.contains("repolens init"),
        "missing 'run repolens init' guidance; stderr={stderr}"
    );
}

#[test]
fn brief_writes_brief_after_init() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let out = run_brief(dir.path(), "add a flag");
    assert!(
        out.status.success(),
        "brief failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        brief_path(dir.path()).exists(),
        "current-agent-brief.md not written"
    );
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(body.contains("# Current Agent Brief"));
}

#[test]
fn brief_includes_task_string_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    let task = "add a --quiet flag with $weird *chars*";
    let out = run_brief(dir.path(), task);
    assert!(out.status.success());
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(
        body.contains(task),
        "task string not embedded verbatim: body=\n{body}"
    );
}

#[test]
fn brief_includes_known_constraints_content() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "# Known Constraints\n\n- All JSON output uses camelCase (Java contract parity).\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "add a field").status.success());
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(
        body.contains("All JSON output uses camelCase"),
        "filled constraint missing from brief: body=\n{body}"
    );
}

#[test]
fn brief_includes_session_memory_content() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/session-memory.md"),
        "# Session Memory\n\n## Session 2026-04-25 — feature X\n\nshipped X under crates/foo.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "feature Y").status.success());
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(
        body.contains("shipped X under crates/foo."),
        "session memory body missing"
    );
}

#[test]
fn brief_includes_friction_log_content() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/agent-friction-log.md"),
        "# Friction Log\n\n- F-001 symbol-lookup misses Rust qualified names.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "x").status.success());
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(
        body.contains("symbol-lookup misses Rust qualified names"),
        "friction log body missing"
    );
}

#[test]
fn brief_warns_when_workspace_is_template_only() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    // Do NOT fill any template; brief over a pristine init should warn.
    assert!(run_brief(dir.path(), "task").status.success());
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    let warning_count = body
        .matches("little or no filled workspace content")
        .count();
    assert!(
        warning_count >= 5,
        "expected warnings for every required workspace template; count={warning_count}; body=\n{body}"
    );
}

#[test]
fn brief_handles_missing_optional_input_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::remove_file(dir.path().join(".repolens/eval-results.md")).unwrap();
    let out = run_brief(dir.path(), "ok");
    assert!(
        out.status.success(),
        "brief should succeed when optional eval-results.md is missing; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn brief_handles_missing_required_input_with_warning_not_error() {
    // Required workspace files have a warning placeholder if missing,
    // but the command itself still succeeds — the workspace exists.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::remove_file(dir.path().join(".repolens/known-constraints.md")).unwrap();
    let out = run_brief(dir.path(), "task");
    assert!(
        out.status.success(),
        "brief should not hard-fail on missing input file"
    );
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(
        body.contains("known-constraints.md") && body.contains("missing"),
        "expected missing-file marker for known-constraints.md; body=\n{body}"
    );
}

#[test]
fn brief_is_deterministic_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "# Known\n\n- C1: do X\n- C2: do Y\n",
    )
    .unwrap();

    assert!(run_brief(dir.path(), "task").status.success());
    let first = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let second = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert_eq!(
        first, second,
        "brief output is not deterministic across runs"
    );
}

#[test]
fn brief_prints_success_and_next_step_lines() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    let out = run_brief(dir.path(), "task");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Agent preflight brief written to"),
        "expected success line; stdout={stdout}"
    );
    assert!(
        stdout.contains("current-agent-brief.md"),
        "expected target path in output; stdout={stdout}"
    );
    assert!(
        stdout.contains("Next:") && stdout.contains("coding agent"),
        "expected 'Next:' guidance line; stdout={stdout}"
    );
}

#[test]
fn brief_overwrites_existing_brief() {
    // The brief is generated each run; existing content is replaced.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(brief_path(dir.path()), "STALE PRIOR BRIEF\n").unwrap();
    assert!(run_brief(dir.path(), "fresh task").status.success());
    let body = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert!(
        !body.contains("STALE PRIOR BRIEF"),
        "stale brief survived overwrite"
    );
    assert!(body.contains("fresh task"));
}

#[test]
fn brief_produces_no_external_calls() {
    // We can't directly assert "no LLM/MCP" but we can assert no network is
    // needed: the test runs in a tempdir with no network, no scan pack, and
    // no memory store, and still succeeds.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    let out = run_brief(dir.path(), "task");
    assert!(
        out.status.success(),
        "brief should not require any external service; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ===========================================================================
// Round 3.5 — brief quality and dogfood hardening
// ===========================================================================

/// Helper: read the generated brief body for assertions.
fn read_brief(root: &Path) -> String {
    fs::read_to_string(brief_path(root)).unwrap()
}

#[test]
fn brief_includes_workspace_status_section_and_table() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    assert!(
        body.contains("## Workspace Status"),
        "brief missing Workspace Status section"
    );
    // Markdown table header
    assert!(
        body.contains("| File | Status | Note |"),
        "Workspace Status table header missing"
    );
    // Every required file should be listed
    for name in [
        "repo-overview.md",
        "known-constraints.md",
        "session-memory.md",
        "agent-friction-log.md",
        "eval-results.md",
    ] {
        assert!(
            body.contains(&format!("`{name}`")),
            "Workspace Status row missing for {name}"
        );
    }
}

#[test]
fn brief_workspace_status_marks_pristine_init_as_template_only() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let template_only_count = body.matches("| TemplateOnly |").count();
    assert!(
        template_only_count >= 4,
        "expected pristine init to mark required files as TemplateOnly; count={template_only_count}; body=\n{body}"
    );
}

#[test]
fn brief_workspace_status_marks_filled_file_as_filled() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/repo-overview.md"),
        "# Repo Overview\n\nThis project is a Rust workspace with eight crates and a CLI.\nIt has well-defined conventions documented elsewhere.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    // Locate the repo-overview row and confirm it says Filled.
    let row = body
        .lines()
        .find(|l| l.contains("`repo-overview.md`"))
        .expect("repo-overview row not found");
    assert!(
        row.contains("| Filled |"),
        "expected repo-overview to be Filled; row={row}"
    );
}

#[test]
fn brief_workspace_status_marks_partially_filled_as_partial() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    // One stray placeholder + substantial real content → Partial.
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "# Known Constraints\n\n- C-001: All JSON output uses camelCase.\n\
         - C-002: Atomic writes via temp + rename.\n\
         - C-003: TODO finish wording — _<placeholder>_\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let row = body
        .lines()
        .find(|l| l.contains("`known-constraints.md`"))
        .expect("known-constraints row not found");
    assert!(
        row.contains("| Partial |"),
        "expected partial fill to be classified Partial; row={row}"
    );
}

#[test]
fn brief_workspace_status_marks_missing_file_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::remove_file(dir.path().join(".repolens/eval-results.md")).unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let row = body
        .lines()
        .find(|l| l.contains("`eval-results.md`"))
        .expect("eval-results row not found");
    assert!(
        row.contains("| Missing |"),
        "expected deleted file to be Missing; row={row}"
    );
}

#[test]
fn brief_workspace_status_marks_empty_file_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    // After leading-comment strip, body is empty.
    fs::write(
        dir.path().join(".repolens/eval-results.md"),
        "<!-- header --> \n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let row = body
        .lines()
        .find(|l| l.contains("`eval-results.md`"))
        .expect("eval-results row not found");
    assert!(
        row.contains("| Empty |"),
        "expected empty body to be Empty; row={row}"
    );
}

#[test]
fn brief_workspace_status_marks_short_real_constraint_as_filled() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "Do not write memory.json.",
    )
    .unwrap();
    assert!(
        run_brief(dir.path(), "Implement minimal repolens remember --file")
            .status
            .success()
    );
    let body = read_brief(dir.path());

    let row = body
        .lines()
        .find(|l| l.contains("`known-constraints.md`"))
        .expect("known-constraints row not found");
    assert!(
        row.contains("| Filled |"),
        "expected short real constraint to be Filled; row={row}"
    );
    assert!(
        !row.contains("| Empty |") && !row.contains("| TemplateOnly |"),
        "short real constraint should not be Empty or TemplateOnly; row={row}"
    );
    assert!(
        body.contains("Do not write memory.json."),
        "short real constraint body missing from brief"
    );

    let constraints_section_start = body
        .find("## Known Constraints")
        .expect("Known Constraints section missing");
    let next_section_start = body[constraints_section_start + 1..]
        .find("\n## ")
        .map(|i| constraints_section_start + 1 + i)
        .unwrap_or(body.len());
    let constraints_section = &body[constraints_section_start..next_section_start];
    assert!(
        !constraints_section.contains("_File `.repolens/known-constraints.md` is empty._")
            && !constraints_section.contains("Workspace contents (raw template)"),
        "short real constraint section should not render as Empty or TemplateOnly"
    );
}

#[test]
fn brief_includes_memory_safety_notice_for_remember_task() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(
        dir.path(),
        "Implement minimal repolens remember --file <session-summary.md>",
    )
    .status
    .success());
    let body = read_brief(dir.path());

    assert!(
        body.contains("## Memory Safety Notice"),
        "memory task should trigger Memory Safety Notice section"
    );
    assert!(
        body.contains("unverified hypotheses"),
        "Memory Safety Notice body missing canonical phrase"
    );
}

#[test]
fn brief_includes_memory_safety_notice_for_each_memory_keyword() {
    // Spot-check several memory keywords; the notice should fire for each.
    for task in [
        "memorize a constraint",
        "Tune Recall Performance",
        "ingest the new pack",
        "edit memory.json carefully",
        "extend MemoryStore quarantine",
        "write a session summary",
    ] {
        let dir = tempfile::tempdir().unwrap();
        run_init(dir.path());
        assert!(run_brief(dir.path(), task).status.success(), "task={task}");
        let body = read_brief(dir.path());
        assert!(
            body.contains("## Memory Safety Notice"),
            "expected Memory Safety Notice for task '{task}'"
        );
    }
}

#[test]
fn brief_omits_memory_safety_notice_for_unrelated_task() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(dir.path(), "Add a --quiet flag to scan")
        .status
        .success());
    let body = read_brief(dir.path());
    assert!(
        !body.contains("## Memory Safety Notice"),
        "non-memory task should not trigger the notice"
    );
}

#[test]
fn brief_includes_repo_freshness_checklist() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    assert!(
        body.contains("Repo freshness checklist"),
        "Repo freshness checklist sub-section missing"
    );
    for needle in [
        "current branch",
        "HEAD commit",
        "git status --short",
        "run its `--help`",
        "stale checkout",
    ] {
        assert!(
            body.contains(needle),
            "freshness checklist missing item containing: {needle}"
        );
    }
}

#[test]
fn brief_does_not_classify_run_name_as_template_only() {
    // Round 3.5 false-positive regression test: real Rust prose with
    // `run_<name>` and `Vec<T>` and `<session-summary.md>` should NOT
    // trip the templated detector.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "# Known Constraints\n\n\
         - **C-006:** Subcommands wire via `Commands::<Name>(args) => <name>::run_<name>(args)`.\n\
         - **C-007:** Generic helpers may use `Vec<T>` and `Option<T>` freely.\n\
         - **C-008:** `repolens remember --file <session-summary.md>` is the planned spelling.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    // Workspace Status should mark known-constraints.md as Filled.
    let row = body
        .lines()
        .find(|l| l.contains("`known-constraints.md`"))
        .expect("known-constraints row not found");
    assert!(
        row.contains("| Filled |"),
        "code-shaped strings must not trip the templated detector; row={row}"
    );

    // The Known Constraints section should embed the body verbatim — no
    // collapsing into a `<details>` template-warning block.
    assert!(
        body.contains("`Commands::<Name>(args) => <name>::run_<name>(args)`"),
        "filled constraint body did not reach the brief verbatim"
    );
    // Also verify the section was NOT wrapped in the templated <details>.
    let constraints_section_start = body
        .find("## Known Constraints")
        .expect("Known Constraints section missing");
    let next_section_start = body[constraints_section_start..]
        .find("\n## ")
        .map(|i| constraints_section_start + i)
        .unwrap_or(body.len());
    let constraints_section = &body[constraints_section_start..next_section_start];
    assert!(
        !constraints_section.contains("Workspace contents (raw template)"),
        "filled constraints should not be wrapped in the template <details> block"
    );
}

#[test]
fn brief_pristine_init_still_warns_template_only() {
    // Regression: Round 3.5 must not weaken the pristine-warning behavior.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let warning_count = body
        .matches("little or no filled workspace content")
        .count();
    assert!(
        warning_count >= 5,
        "expected per-section warnings on pristine init; count={warning_count}"
    );
}

// ===========================================================================
// Round 5.2 — TemplateOnly hardening
// ===========================================================================

#[test]
fn brief_pristine_init_does_not_inline_raw_known_constraints_placeholders() {
    // Round 5.2: a fresh init's known-constraints.md is a raw template
    // (TemplateOnly). The brief must NOT inline raw placeholder text such
    // as `C-001 _<short-name>_` anywhere — an agent reading the markdown
    // source could otherwise mistake the placeholder for a real constraint.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    assert!(run_brief(dir.path(), "audit constraints").status.success());
    let body = read_brief(dir.path());

    for placeholder in [
        "C-001 _<short-name>_",
        "C-002 _<short-name>_",
        "_<short-name>_",
    ] {
        assert!(
            !body.contains(placeholder),
            "raw template placeholder leaked into brief: {placeholder}; body=\n{body}"
        );
    }
    // The notice must still appear so the agent knows the section is templated.
    assert!(
        body.contains("little or no filled workspace content"),
        "expected TemplateOnly notice for known-constraints; body=\n{body}"
    );
}

#[test]
fn brief_filled_known_constraints_still_appears_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "# Known Constraints\n\n\
         - **C-001:** All JSON output uses camelCase (Java contract parity).\n\
         - **C-002:** Atomic writes via temp + rename.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "audit").status.success());
    let body = read_brief(dir.path());
    assert!(
        body.contains("All JSON output uses camelCase (Java contract parity).")
            && body.contains("Atomic writes via temp + rename."),
        "filled constraints must still appear verbatim; body=\n{body}"
    );
}

#[test]
fn brief_short_real_known_constraint_is_filled_not_template_only() {
    // Regression: Round 5.2 must not regress the Round 3.5 short-real-content
    // path. A short real entry should be Filled (no TemplateOnly notice).
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "Do not write memory.json.",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let constraints_section_start = body
        .find("## Known Constraints")
        .expect("Known Constraints section missing");
    let next_section_start = body[constraints_section_start + 1..]
        .find("\n## ")
        .map(|i| constraints_section_start + 1 + i)
        .unwrap_or(body.len());
    let section = &body[constraints_section_start..next_section_start];
    assert!(
        section.contains("Do not write memory.json."),
        "short real content missing from Known Constraints section"
    );
    assert!(
        !section.contains("little or no filled workspace content"),
        "short real content must not be classified TemplateOnly; section=\n{section}"
    );
}

#[test]
fn brief_partial_known_constraints_warns_above_body() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/known-constraints.md"),
        "# Known Constraints\n\n\
         - **C-001:** Real shipped constraint.\n\
         - **C-002:** Atomic writes via temp + rename.\n\
         - **C-003:** TODO finalize wording — _<placeholder>_\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "task").status.success());
    let body = read_brief(dir.path());

    let constraints_section_start = body
        .find("## Known Constraints")
        .expect("Known Constraints section missing");
    let next_section_start = body[constraints_section_start + 1..]
        .find("\n## ")
        .map(|i| constraints_section_start + 1 + i)
        .unwrap_or(body.len());
    let section = &body[constraints_section_start..next_section_start];

    let warn_pos = section
        .find("Some workspace placeholders remain")
        .expect("partial warning missing");
    let body_pos = section
        .find("Real shipped constraint")
        .expect("real content missing");
    assert!(
        warn_pos < body_pos,
        "Partial warning must sit ABOVE the body; section=\n{section}"
    );
}

// ===========================================================================
// Round 5.2 — Latest "Pick up here" lift
// ===========================================================================

#[test]
fn brief_lifts_pick_up_here_block_to_top_of_session_memory() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/session-memory.md"),
        "# Session Memory\n\n\
         ## Session 2026-04-26 — feature X\n\n\
         ### What I did\n\nshipped a thing\n\n\
         ### Pick up here\n\nFinish wiring foo to bar in `crates/baz/src/lib.rs`.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "continue work").status.success());
    let body = read_brief(dir.path());

    let prior_pos = body
        .find("## Prior Session Memory")
        .expect("Prior Session Memory section missing");
    let lift_pos = body
        .find("Latest \"Pick up here\"")
        .expect("lift heading missing");
    let agent_friction_pos = body
        .find("## Agent Friction Warnings")
        .expect("Agent Friction Warnings section missing");
    assert!(
        prior_pos < lift_pos && lift_pos < agent_friction_pos,
        "lift heading must sit inside the Prior Session Memory section"
    );

    // The next-step phrase must appear at least twice: once in the lifted
    // block at the top of the section, and once in the full session memory
    // body below the `---` divider.
    let next_step = "Finish wiring foo to bar";
    let prior_section = &body[prior_pos..agent_friction_pos];
    let occurrences = prior_section.matches(next_step).count();
    assert!(
        occurrences >= 2,
        "expected next-step phrase in both the lift and the full body below; \
         occurrences={occurrences}; section=\n{prior_section}"
    );

    // Safety framing must be present in the lift framing line.
    for phrase in ["agent-provided", "unverified", "hypothesis"] {
        assert!(
            prior_section.contains(phrase),
            "lift must preserve safety framing phrase '{phrase}'"
        );
    }

    // The lifted block itself must precede the divider; the full body must
    // follow it.
    let divider_pos = prior_section
        .find("\n---\n")
        .expect("expected `\\n---\\n` divider between lift and full body");
    let lift_to_div = &prior_section[..divider_pos];
    let after_div = &prior_section[divider_pos..];
    assert!(
        lift_to_div.contains(next_step),
        "lifted block must contain the next-step phrase before the divider"
    );
    assert!(
        after_div.contains(next_step),
        "full session memory below divider must also contain the next-step phrase"
    );
}

#[test]
fn brief_omits_lift_when_session_memory_has_no_pick_up_here() {
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    fs::write(
        dir.path().join(".repolens/session-memory.md"),
        "# Session Memory\n\n## Session 2026-04-26\n\nShipped X.\n",
    )
    .unwrap();
    assert!(run_brief(dir.path(), "continue").status.success());
    let body = read_brief(dir.path());
    assert!(
        !body.contains("Latest \"Pick up here\""),
        "no lift heading should appear when no Pick up here exists"
    );
    assert!(
        body.contains("Shipped X."),
        "session memory body still surfaced"
    );
}

#[test]
fn brief_after_remember_lifts_pick_up_here_from_imported_summary() {
    // End-to-end: remember-import a summary that contains a Pick up here
    // section, then run brief; the lift should appear at the top of Prior
    // Session Memory and surface the imported next-step.
    let dir = tempfile::tempdir().unwrap();
    run_init(dir.path());
    let summary = dir.path().join("session-summary.md");
    fs::write(
        &summary,
        "# Session Summary — 2026-04-26\n\n\
         ## What I did\n\nshipped Round 5.2 scaffolding\n\n\
         ## Pick up here\n\nWire eval-runs provenance into the eval list output.\n",
    )
    .unwrap();
    let out = std::process::Command::new(repolens_bin())
        .args([
            "remember",
            "--root",
            dir.path().to_str().unwrap(),
            "--file",
            summary.to_str().unwrap(),
        ])
        .output()
        .expect("remember failed to launch");
    assert!(
        out.status.success(),
        "remember failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(run_brief(dir.path(), "continue Round 5.2").status.success());
    let body = read_brief(dir.path());

    assert!(
        body.contains("Latest \"Pick up here\""),
        "lift heading missing after remember + brief"
    );
    assert!(
        body.contains("Wire eval-runs provenance into the eval list output."),
        "lifted next-step missing from brief"
    );
}

// -- #34: `brief --task` uses the task -----------------------------------

fn run_ok(root: &Path, args: &[&str]) -> String {
    let out = Command::new(repolens_bin())
        .args(args)
        .arg("--root")
        .arg(root)
        .output()
        .expect("failed to invoke repolens");
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A small scanned repo: an SM4 module, an i18n module, and a long path.
fn scanned_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src/modes")).unwrap();
    fs::create_dir_all(root.join("www")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub mod modes;\npub fn sm4_cbc_encrypt() {}\npub fn sm3_hash() {}\n",
    )
    .unwrap();
    fs::write(root.join("src/modes.rs"), "pub mod cbc;\n").unwrap();
    fs::write(root.join("src/modes/cbc.rs"), "pub fn unpad_block() {}\n").unwrap();
    fs::write(
        root.join("www/i18n.js"),
        "function detectLanguage() { return 'en'; }\n",
    )
    .unwrap();
    let long = format!("docs/{}/sm4_notes.md", "very_long_directory_name".repeat(3));
    fs::create_dir_all(root.join(&long).parent().unwrap()).unwrap();
    fs::write(root.join(&long), "# SM4 notes\n").unwrap();
    run_init(root);
    run_ok(root, &["scan"]);
    dir
}

fn brief_stdout(root: &Path, task: &str) -> String {
    run_ok(root, &["brief", "--stdout", "--task", task])
}

fn section(body: &str) -> String {
    body.split("## Files and Notes for This Task")
        .nth(1)
        .map(|rest| rest.split("\n## ").next().unwrap().to_string())
        .unwrap_or_default()
}

fn cols(s: &str) -> usize {
    s.chars()
        .map(|c| match c {
            '…' => 2,
            c => unicode_width::UnicodeWidthChar::width(c).unwrap_or(0),
        })
        .sum()
}

#[test]
fn brief_task_lists_matching_files_first_and_drops_repolens_only_advice() {
    let dir = scanned_repo();
    let body = brief_stdout(dir.path(), "change SM4 CBC padding handling");
    let sec = section(&body);
    assert!(!sec.is_empty(), "section missing:\n{body}");
    let files: Vec<&str> = sec.lines().filter(|l| l.starts_with("- `")).collect();
    assert!(
        files[0].starts_with("- `src/lib.rs`: matches sm4, cbc"),
        "src/lib.rs should rank first: {files:?}"
    );
    assert!(sec.contains("`src/modes/cbc.rs`: matches cbc"), "{sec}");
    assert!(!body.contains("cargo run -- --help"));
    assert!(!body.contains("dogfood"));
    assert!(!body.contains("_No file or saved note matched"));
}

#[test]
fn brief_two_tasks_give_different_file_pointers() {
    let dir = scanned_repo();
    let a = section(&brief_stdout(dir.path(), "change SM4 CBC padding"));
    let b = section(&brief_stdout(dir.path(), "detect the page language"));
    assert!(
        a.contains("src/lib.rs") && !a.contains("www/i18n.js"),
        "{a}"
    );
    assert!(
        b.contains("www/i18n.js") && !b.contains("src/lib.rs"),
        "{b}"
    );
}

#[test]
fn brief_task_without_match_says_so_once_and_in_the_terminal() {
    let dir = scanned_repo();
    let task = "change the release checklist wording";
    let body = brief_stdout(dir.path(), task);
    let line = "_No file or saved note matched the task's words; this is the general brief._";
    assert_eq!(body.matches(line).count(), 1, "{body}");
    assert!(!body.contains("## Files and Notes for This Task"));

    let stdout = run_ok(dir.path(), &["brief", "--task", task]);
    let summary = "No file or saved note matched the task; the brief is the general one.";
    assert_eq!(stdout.matches(summary).count(), 1, "{stdout}");
    let file = fs::read_to_string(brief_path(dir.path())).unwrap();
    assert_eq!(file.matches(line).count(), 1);
}

#[test]
fn brief_task_of_only_stop_words_says_so() {
    let dir = scanned_repo();
    let body = brief_stdout(dir.path(), "the of a");
    assert!(
        body.contains("_The task has only common words like \"the\"; this is the general brief._")
    );
    assert!(!body.contains("_No file or saved note matched"));
}

#[test]
fn brief_without_task_has_no_match_lines() {
    let dir = scanned_repo();
    let body = run_ok(dir.path(), &["brief", "--stdout"]);
    assert!(body.contains("No task was provided"));
    assert!(!body.contains("## Files and Notes for This Task"));
    assert!(!body.contains("_No file or saved note matched"));
    assert!(!body.contains("only common words"));
    let stdout = run_ok(dir.path(), &["brief"]);
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
}

#[test]
fn brief_task_matches_saved_notes_and_reports_counts() {
    let dir = scanned_repo();
    run_ok(
        dir.path(),
        &[
            "memory",
            "save",
            "--type",
            "decision",
            "SM4 padding uses PKCS7; 保存一个很长的中文备注用于检查列宽是否按两列计算，并且需要截断显示",
        ],
    );
    let body = brief_stdout(dir.path(), "SM4 padding");
    let sec = section(&body);
    let note = sec
        .lines()
        .find(|l| l.starts_with("- DECISION "))
        .unwrap_or_else(|| panic!("note missing: {sec}"));
    assert!(note.ends_with(" (sm4, padding)"), "{note}");
    assert!(note.contains('…'), "long note should be cut: {note}");
    // Paths are never cut, so only the long fixture path may pass 80.
    for l in sec
        .lines()
        .filter(|l| !l.contains("very_long_directory_name"))
    {
        assert!(cols(l) <= 80, "line over 80 columns ({}): {l}", cols(l));
    }
    let stdout = run_ok(dir.path(), &["brief", "--task", "SM4 padding"]);
    assert!(
        stdout.contains("Matched 2 files and 1 saved note to the task."),
        "{stdout}"
    );
}

#[test]
fn brief_task_never_cuts_a_long_path() {
    let dir = scanned_repo();
    let body = brief_stdout(dir.path(), "sm4 notes");
    let sec = section(&body);
    let long = format!("docs/{}/sm4_notes.md", "very_long_directory_name".repeat(3));
    assert!(
        sec.contains(&format!("- `{long}`:\n  matches sm4, notes")),
        "{sec}"
    );
    for l in sec.lines().filter(|l| !l.contains(&long)) {
        assert!(cols(l) <= 80, "{l}");
    }
}

#[test]
fn brief_task_is_deterministic() {
    let dir = scanned_repo();
    let a = brief_stdout(dir.path(), "sm4 cbc i18n");
    let b = brief_stdout(dir.path(), "sm4 cbc i18n");
    assert_eq!(a, b);
}

#[test]
fn brief_and_validate_plan_agree_on_known_constraints() {
    let dir = scanned_repo();
    let kc = dir.path().join(".repolens/known-constraints.md");
    // Drafts only: validate-plan loads none, and the brief says so.
    fs::write(
        &kc,
        "# Known Constraints\n\n### D-001 Edition\n\n- **Statement:** edition 2021.\n",
    )
    .unwrap();
    let plan = dir.path().join("plan.md");
    fs::write(&plan, "# Plan\n- edit src/lib.rs\n").unwrap();
    let vp = run_ok(
        dir.path(),
        &["validate-plan", "--file", plan.to_str().unwrap()],
    );
    assert!(vp.contains("no real constraints loaded"), "{vp}");
    let body = run_ok(dir.path(), &["brief", "--stdout"]);
    assert!(
        body.contains("| No C-NNN rules yet; none enforced. |"),
        "{body}"
    );
    assert_status_rows_fit(&body);
    assert!(body.contains("_No checked rules (C-NNN) yet"));

    // One checked rule: both see it.
    fs::write(
        &kc,
        "# Known Constraints\n\n### C-001 camelCase JSON\n\n- **Statement:** JSON output fields use camelCase.\n",
    )
    .unwrap();
    let vp = run_ok(
        dir.path(),
        &["validate-plan", "--file", plan.to_str().unwrap()],
    );
    assert!(!vp.contains("no real constraints loaded"), "{vp}");
    let body = run_ok(dir.path(), &["brief", "--stdout"]);
    assert!(
        body.contains("| validate-plan enforces 1 C-NNN rule. |"),
        "{body}"
    );
    assert!(!body.contains("rule(s)"), "{body}");
    assert_status_rows_fit(&body);
    assert!(!body.contains("_No checked rules (C-NNN) yet"));

    // Two checked rules: plural.
    fs::write(
        &kc,
        "# Known Constraints\n\n### C-001 camelCase JSON\n\n- **Statement:** JSON output fields use camelCase.\n\n### C-002 No unsafe\n\n- **Statement:** no unsafe code.\n",
    )
    .unwrap();
    let body = run_ok(dir.path(), &["brief", "--stdout"]);
    assert!(
        body.contains("| validate-plan enforces 2 C-NNN rules. |"),
        "{body}"
    );
    assert_status_rows_fit(&body);
}

/// The known-constraints status row fits in 80 columns (#34 QA).
fn assert_status_rows_fit(body: &str) {
    let row = body
        .lines()
        .find(|l| l.starts_with("| `known-constraints.md` |"))
        .expect("known-constraints row");
    assert!(
        row.chars().count() <= 80,
        "{} cols: {row}",
        row.chars().count()
    );
}
