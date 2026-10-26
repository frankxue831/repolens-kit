use std::fs;
use std::path::Path;
use std::process::Command;

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

fn run_state(root: &Path) -> std::process::Output {
    Command::new(repolens_bin())
        .args(["state", "--root", root.to_str().unwrap()])
        .output()
        .expect("failed to invoke repolens state")
}

fn write_session_json(root: &Path, pack_id: &str) {
    let active_pack = root.join(".repolens").join(pack_id);
    fs::write(
        root.join(".repolens/session.json"),
        format!(
            r#"{{
  "schemaVersion": "1.0",
  "sessionId": "session-1",
  "repoRoot": "{}",
  "activePackPath": "{}",
  "activeReportPath": null,
  "updatedAt": "2026-01-01T00:00:00Z",
  "lastAccessed": "2026-01-01T00:00:00Z",
  "lastScanMode": "full",
  "reprocessedFiles": null,
  "deletedFiles": null,
  "carriedForwardFiles": null
}}"#,
            root.display(),
            active_pack.display()
        ),
    )
    .unwrap();
}

fn write_memory_json(root: &Path) {
    let memory_dir = root.join(".repolens/memory");
    fs::create_dir_all(&memory_dir).unwrap();
    fs::write(
        memory_dir.join("memory.json"),
        format!(
            r#"{{
  "schemaVersion": "1.0",
  "repoRoot": "{}",
  "nodes": [
    {{
      "id": "n1",
      "content": "Use explicit output wording for memory surfaces.",
      "type": "PATTERN",
      "importance": 0.5,
      "created": "2026-01-01T00:00:00Z",
      "lastAccessed": "2026-01-01T00:00:00Z",
      "accessCount": 0,
      "anchors": [],
      "tags": ["ux"],
      "source": "EXPLICIT",
      "provenance": "test"
    }}
  ],
  "edges": [
    {{ "sourceId": "n1", "targetId": "n1", "relation": "RELATES_TO" }}
  ],
  "lastDecayRun": null,
  "updatedAt": "2026-01-01T00:00:00Z"
}}"#,
            root.display()
        ),
    )
    .unwrap();
}

#[test]
fn state_reports_all_three_surfaces() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(storage.join("pack-20260101-000000")).unwrap();
    fs::write(storage.join("pack-20260101-000000/manifest.json"), "{}").unwrap();
    write_session_json(dir.path(), "pack-20260101-000000");
    write_memory_json(dir.path());
    fs::write(
        storage.join("session-memory.md"),
        "## Session\n\nReal handoff.\n",
    )
    .unwrap();

    let out = run_state(dir.path());

    assert!(
        out.status.success(),
        "state failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("RepoLens state"), "missing title: {stdout}");
    assert!(
        stdout.contains("Scan packs"),
        "missing scan section: {stdout}"
    );
    assert!(
        stdout.contains("Surface: derived repo context, regenerable"),
        "scan surface not explained: {stdout}"
    );
    assert!(
        stdout.contains("Packs on disk: 1"),
        "pack count missing: {stdout}"
    );
    assert!(
        stdout.contains("Active pack: pack-20260101-000000"),
        "active pack missing: {stdout}"
    );
    assert!(
        stdout.contains("Last scan mode: full"),
        "last scan mode missing: {stdout}"
    );
    assert!(
        stdout.contains("Typed memory graph"),
        "missing typed memory section: {stdout}"
    );
    assert!(stdout.contains("Nodes: 1"), "node count missing: {stdout}");
    assert!(stdout.contains("Edges: 1"), "edge count missing: {stdout}");
    assert!(
        stdout.contains("PATTERN: 1"),
        "type count missing: {stdout}"
    );
    assert!(
        stdout.contains("repolens remember does not write this file"),
        "typed-memory caveat missing: {stdout}"
    );
    assert!(
        stdout.contains("Session handoff markdown"),
        "missing handoff section: {stdout}"
    );
    assert!(
        stdout.contains("Status: filled"),
        "filled handoff status missing: {stdout}"
    );
    assert!(
        stdout.contains("not typed memory"),
        "handoff caveat missing: {stdout}"
    );
}

#[test]
fn state_reports_missing_workspace_without_creating_it() {
    let dir = tempfile::tempdir().unwrap();

    let out = run_state(dir.path());

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("RepoLens workspace: missing"),
        "missing workspace not reported: {stdout}"
    );
    assert!(
        stdout.contains("Run `repolens init` or `repolens scan`"),
        "missing next-step guidance: {stdout}"
    );
    assert!(
        !dir.path().join(".repolens").exists(),
        "state must not create .repolens/"
    );
}

#[test]
fn state_reports_corrupt_memory_without_modifying_it() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(storage.join("memory")).unwrap();
    let memory_path = storage.join("memory/memory.json");
    fs::write(&memory_path, "not valid json {{{\n").unwrap();
    let before = fs::read_to_string(&memory_path).unwrap();

    let out = run_state(dir.path());

    assert!(
        out.status.success(),
        "state should report corrupt memory without failing; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Status: unreadable"),
        "corrupt memory status missing: {stdout}"
    );
    assert_eq!(
        fs::read_to_string(&memory_path).unwrap(),
        before,
        "state must not rewrite corrupt memory.json"
    );
}

#[test]
fn state_reports_corrupt_session_json_separately_from_missing_session() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    fs::write(storage.join("session.json"), "not valid json {{{\n").unwrap();

    let out = run_state(dir.path());

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Active pack: (unknown; session.json unreadable)"),
        "corrupt session.json should not be reported as missing: {stdout}"
    );
    assert!(
        stdout.contains("Session error:"),
        "corrupt session.json error detail missing: {stdout}"
    );
}

#[test]
fn state_flags_active_pack_missing_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    write_session_json(dir.path(), "pack-20260101-000000");

    let out = run_state(dir.path());

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Active pack: pack-20260101-000000 (missing on disk)"),
        "missing active pack should be flagged: {stdout}"
    );
}

#[test]
fn state_distinguishes_template_only_session_handoff() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    fs::write(
        storage.join("session-memory.md"),
        "<!-- template -->\n# Session Memory\n\n_<session-summary>_\n_<next-steps>_\n_<risks>_\n",
    )
    .unwrap();

    let out = run_state(dir.path());

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Status: template-only"),
        "template-only handoff status missing: {stdout}"
    );
}
