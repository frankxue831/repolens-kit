//! A closed stdout (`repolens ... | head -1`) ends output quietly: no panic
//! text on stderr and the command's usual exit code (0 on success).

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let st = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(st.success());
    fs::create_dir_all(dir.path().join("src")).unwrap();
    for i in 0..20 {
        fs::write(
            dir.path().join(format!("src/m{i}.rs")),
            format!("pub fn f{i}() {{}}\n"),
        )
        .unwrap();
    }
    dir
}

fn rl() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_repolens"));
    c.env_remove("REPOLENS_CLOCK_OVERRIDE");
    c
}

fn ok(cwd: &Path, args: &[&str]) {
    let o = rl().current_dir(cwd).args(args).output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Run with stdout connected to a pipe whose read end is already closed, so
/// the first write fails with BrokenPipe every time.
fn into_closed_pipe(cwd: &Path, args: &[&str]) -> Output {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    rl().current_dir(cwd)
        .args(args)
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn assert_quiet(args: &[&str], o: &Output) {
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!err.contains("panicked"), "{args:?} panicked: {err}");
    assert!(!err.contains("Broken pipe"), "{args:?}: {err}");
    assert!(o.status.success(), "{args:?}: {:?} {err}", o.status);
}

#[test]
fn scan_and_memory_recall_into_a_closed_pipe_exit_quietly() {
    let d = repo();
    let root = d.path();
    let args = ["scan", "--full"];
    let o = into_closed_pipe(root, &args);
    assert_quiet(&args, &o);
    // The scan still finished and wrote its pack.
    ok(
        root,
        &[
            "memory",
            "save",
            "--type",
            "decision",
            "--anchor",
            "src/m0.rs",
            "news feed is cached for an hour",
        ],
    );
    for args in [
        &["memory", "recall", "new"][..],
        &["memory", "recall", "zzzz"][..],
        &["findings"][..],
        &["--help"][..],
    ] {
        let o = into_closed_pipe(root, args);
        assert_quiet(args, &o);
    }
}

#[test]
fn memory_recall_piped_into_head_exits_quietly() {
    let d = repo();
    let root = d.path();
    ok(root, &["scan", "--full"]);
    // About 400 KB of recall output: far more than a pipe buffer plus what
    // `head -1` reads, so later writes always hit the closed pipe.
    let long = "release ".repeat(5_000);
    // Distinct notes: identical content is saved only once.
    for i in 0..10 {
        let note = format!("note {i}: {long}");
        ok(
            root,
            &[
                "memory",
                "save",
                "--type",
                "decision",
                "--anchor",
                "src/m0.rs",
                &note,
            ],
        );
    }
    for args in [
        &["memory", "recall", "release", "--limit", "10"][..],
        &["scan", "--full"][..],
    ] {
        let mut child = rl()
            .current_dir(root)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let head = Command::new("head")
            .arg("-1")
            .stdin(child.stdout.take().unwrap())
            .stdout(Stdio::piped())
            .output()
            .unwrap();
        assert!(head.status.success());
        assert!(!head.stdout.is_empty(), "{args:?}: no first line");
        let o = child.wait_with_output().unwrap();
        assert_quiet(args, &o);
    }
}

/// #59: help and version into a closed pipe still exit 0 quietly.
#[test]
fn help_into_a_closed_pipe_exits_quietly() {
    let d = repo();
    for args in [&["--help"][..], &["--version"][..], &["scan", "--help"][..]] {
        let o = into_closed_pipe(d.path(), args);
        assert_quiet(args, &o);
    }
}
