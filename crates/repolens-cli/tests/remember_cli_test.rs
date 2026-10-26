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

fn run_remember(root: &Path, file: &Path) -> std::process::Output {
    Command::new(repolens_bin())
        .args([
            "remember",
            "--root",
            root.to_str().unwrap(),
            "--file",
            file.to_str().unwrap(),
        ])
        .output()
        .expect("failed to invoke repolens remember")
}

fn run_brief(root: &Path, task: &str) -> std::process::Output {
    Command::new(repolens_bin())
        .args(["brief", "--task", task, "--root", root.to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens brief")
}

fn session_memory_path(root: &Path) -> PathBuf {
    root.join(".repolens/session-memory.md")
}

fn memory_graph_path(root: &Path) -> PathBuf {
    root.join(".repolens/memory/memory.json")
}

fn write_summary(root: &Path, body: &str) -> PathBuf {
    let path = root.join("session-summary.md");
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn remember_errors_when_workspace_missing() {
    let dir = tempfile::tempdir().unwrap();
    let summary = write_summary(dir.path(), "Implemented the thing.\n");

    let out = run_remember(dir.path(), &summary);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("RepoLens workspace not found. Run 'repolens init' first."),
        "missing init guidance; stderr={stderr}"
    );
}

#[test]
fn remember_errors_when_input_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let missing = dir.path().join("missing-summary.md");

    let out = run_remember(dir.path(), &missing);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Session summary file not found"),
        "missing file message unclear; stderr={stderr}"
    );
    assert!(
        stderr.contains("missing-summary.md"),
        "missing file path absent; stderr={stderr}"
    );
}

#[test]
fn remember_errors_when_input_file_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(dir.path(), " \n\t\n");

    let out = run_remember(dir.path(), &summary);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Session summary file is empty"),
        "empty file message unclear; stderr={stderr}"
    );
}

#[test]
fn remember_replaces_pristine_template_with_imported_summary() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(
        dir.path(),
        "Round 4 implemented workspace markdown handoff.\n",
    );

    let out = run_remember(dir.path(), &summary);

    assert!(
        out.status.success(),
        "remember failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = fs::read_to_string(session_memory_path(dir.path())).unwrap();
    assert!(body.contains("## Session Summary: Imported from session-summary.md"));
    assert!(body.contains("Round 4 implemented workspace markdown handoff."));
    assert!(
        !body.contains("YYYY-MM-DDTHH:MM:SSZ"),
        "pristine timestamp placeholder should not be preserved: body=\n{body}"
    );
    assert!(
        !body.contains("_<one-line task>_"),
        "pristine task placeholder should not be preserved: body=\n{body}"
    );
    assert!(
        !body.contains("Older sessions follow below"),
        "pristine template footer should not be preserved as memory: body=\n{body}"
    );
}

#[test]
fn remember_prepends_to_existing_filled_session_memory() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    fs::write(
        session_memory_path(dir.path()),
        "## Earlier Session\n\nKeep this existing handoff.\n",
    )
    .unwrap();
    let summary = write_summary(dir.path(), "New handoff content.\n");

    let out = run_remember(dir.path(), &summary);

    assert!(out.status.success());
    let body = fs::read_to_string(session_memory_path(dir.path())).unwrap();
    assert!(
        body.starts_with("## Session Summary: Imported from session-summary.md"),
        "new entry was not prepended: body=\n{body}"
    );
    let new_pos = body.find("New handoff content.").unwrap();
    let old_pos = body.find("## Earlier Session").unwrap();
    assert!(
        new_pos < old_pos,
        "new entry should appear above existing memory: body=\n{body}"
    );
}

#[test]
fn remember_preserves_existing_filled_session_memory_below_new_entry() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    fs::write(
        session_memory_path(dir.path()),
        "## Earlier Session\n\nExisting notes must remain.\n",
    )
    .unwrap();
    let summary = write_summary(dir.path(), "New notes.\n");

    assert!(run_remember(dir.path(), &summary).status.success());

    let body = fs::read_to_string(session_memory_path(dir.path())).unwrap();
    assert!(body.contains("## Earlier Session"));
    assert!(body.contains("Existing notes must remain."));
}

#[test]
fn remember_marks_imported_entry_as_unverified_agent_hypothesis() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(dir.path(), "Session handoff.\n");

    assert!(run_remember(dir.path(), &summary).status.success());

    let body = fs::read_to_string(session_memory_path(dir.path())).unwrap();
    assert!(body.contains("**Source:** session-summary.md"));
    assert!(body.contains("agent-provided"));
    assert!(body.contains("unverified"));
    assert!(body.contains("hypothesis until confirmed"));
}

#[test]
fn remember_content_appears_in_subsequent_brief() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(dir.path(), "Do not change MemoryGraph for this handoff.\n");

    assert!(run_remember(dir.path(), &summary).status.success());
    assert!(run_brief(dir.path(), "Implement next task")
        .status
        .success());

    let brief = fs::read_to_string(dir.path().join(".repolens/current-agent-brief.md")).unwrap();
    assert!(
        brief.contains("Do not change MemoryGraph for this handoff."),
        "imported memory missing from brief: brief=\n{brief}"
    );
    assert!(
        brief.contains("| `session-memory.md` | Filled |"),
        "session-memory.md should be marked Filled after remember: brief=\n{brief}"
    );
}

#[test]
fn remember_does_not_create_memory_graph_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(dir.path(), "Workspace markdown only.\n");

    assert!(run_remember(dir.path(), &summary).status.success());

    assert!(
        !memory_graph_path(dir.path()).exists(),
        "remember must not create .repolens/memory/memory.json"
    );
}

#[test]
fn remember_does_not_modify_existing_memory_graph_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let memory_path = memory_graph_path(dir.path());
    fs::create_dir_all(memory_path.parent().unwrap()).unwrap();
    fs::write(&memory_path, "{\"sentinel\":true}\n").unwrap();
    let summary = write_summary(dir.path(), "Do not touch typed memory.\n");

    assert!(run_remember(dir.path(), &summary).status.success());

    let body = fs::read_to_string(memory_path).unwrap();
    assert_eq!(body, "{\"sentinel\":true}\n");
}

#[test]
fn remember_prints_success_line_and_next_step() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(dir.path(), "Output contract.\n");

    let out = run_remember(dir.path(), &summary);

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Session summary recorded in .repolens/session-memory.md"),
        "success line missing; stdout={stdout}"
    );
    assert!(
        stdout.contains(
            "This recorded workspace markdown only; .repolens/memory/memory.json was not modified."
        ),
        "typed-memory non-write clarification missing; stdout={stdout}"
    );
    assert!(
        stdout.contains("Use MCP memorize_insight for anchored typed long-term memory."),
        "typed-memory next step missing; stdout={stdout}"
    );
    assert!(
        stdout.contains(
            "Next: run repolens brief --task \"<next task>\" to include this handoff in the next preflight brief."
        ),
        "next step missing; stdout={stdout}"
    );
}

#[test]
fn remember_demotes_imported_h1_to_h2_in_session_memory() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(
        dir.path(),
        "# Session Summary — 2026-04-25\n\n## What I did\n\nstuff\n",
    );

    assert!(run_remember(dir.path(), &summary).status.success());

    let body = fs::read_to_string(session_memory_path(dir.path())).unwrap();
    // Wrapper headings are still h2/h3.
    assert!(body.contains("## Session Summary: Imported from session-summary.md"));
    assert!(body.contains("### Imported Content"));
    // Imported h1 demoted to h2; imported h2 demoted to h3.
    assert!(
        body.contains("## Session Summary — 2026-04-25"),
        "imported h1 was not demoted to h2: body=\n{body}"
    );
    assert!(
        body.contains("### What I did"),
        "imported h2 was not demoted to h3: body=\n{body}"
    );
    // No raw h1 from the imported summary should survive anywhere in the file.
    assert!(
        !body
            .lines()
            .any(|l| l.starts_with("# ") && l.starts_with("# Session Summary —")),
        "imported h1 leaked into session-memory.md: body=\n{body}"
    );
}

#[test]
fn remember_does_not_demote_headings_inside_fenced_code_block() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(
        dir.path(),
        "# Real heading\n\
         \n\
         ```sh\n\
         # this is a shell comment, not a heading\n\
         echo hi\n\
         ```\n\
         \n\
         ## After fence\n",
    );

    assert!(run_remember(dir.path(), &summary).status.success());

    let body = fs::read_to_string(session_memory_path(dir.path())).unwrap();
    // Real headings demoted.
    assert!(
        body.contains("## Real heading"),
        "real h1 was not demoted: body=\n{body}"
    );
    assert!(
        body.contains("### After fence"),
        "real h2 was not demoted: body=\n{body}"
    );
    // Shell comment inside fence preserved verbatim — must not have been
    // demoted to `## this is a shell comment...`.
    assert!(
        body.contains("# this is a shell comment, not a heading"),
        "fence content was lost: body=\n{body}"
    );
    assert!(
        !body.contains("## this is a shell comment"),
        "heading inside fence was incorrectly demoted: body=\n{body}"
    );
}

#[test]
fn brief_after_remember_has_no_extra_h1_from_session_memory() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let summary = write_summary(
        dir.path(),
        "# Session Summary — 2026-04-25\n\nKey handoff line.\n",
    );

    assert!(run_remember(dir.path(), &summary).status.success());
    assert!(run_brief(dir.path(), "Continue from previous session")
        .status
        .success());

    let brief = fs::read_to_string(dir.path().join(".repolens/current-agent-brief.md")).unwrap();

    // The imported handoff is still surfaced…
    assert!(
        brief.contains("Key handoff line."),
        "imported content missing from brief: brief=\n{brief}"
    );
    // …but the imported summary's h1 must NOT appear in the brief as h1.
    // (The brief already inlines workspace-file h1s like `# Repository Overview`
    // — that is pre-existing template behavior and out of scope for this
    // hotfix. The hotfix scope is solely: an h1 from the *imported summary*
    // does not survive into the brief.)
    assert!(
        !brief.lines().any(|l| l == "# Session Summary — 2026-04-25"),
        "imported h1 line leaked into brief: brief=\n{brief}"
    );
    // The demoted form should be present instead.
    assert!(
        brief.contains("## Session Summary — 2026-04-25"),
        "imported h1 was not demoted to h2 in brief: brief=\n{brief}"
    );
}

#[test]
fn remember_does_not_modify_input_summary_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run_init(dir.path()).status.success());
    let original = "# Session Summary — 2026-04-25\n\n## What I did\n\nstuff\n";
    let summary = write_summary(dir.path(), original);

    assert!(run_remember(dir.path(), &summary).status.success());

    let after = fs::read_to_string(&summary).unwrap();
    assert_eq!(
        after, original,
        "remember must not modify the input summary file"
    );
}

#[test]
fn remember_output_is_deterministic_for_same_initial_state_and_input() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    assert!(run_init(dir_a.path()).status.success());
    assert!(run_init(dir_b.path()).status.success());
    let summary_a = write_summary(dir_a.path(), "Same imported content.\n");
    let summary_b = write_summary(dir_b.path(), "Same imported content.\n");

    assert!(run_remember(dir_a.path(), &summary_a).status.success());
    assert!(run_remember(dir_b.path(), &summary_b).status.success());

    let body_a = fs::read_to_string(session_memory_path(dir_a.path())).unwrap();
    let body_b = fs::read_to_string(session_memory_path(dir_b.path())).unwrap();
    assert_eq!(body_a, body_b);
}
