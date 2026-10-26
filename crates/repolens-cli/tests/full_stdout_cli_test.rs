//! #59 item 2: a stdout that fails with an error other than a closed pipe
//! (`> /dev/full`) ends the command with exit code 1 and one line on
//! stderr, never a silent success (help, version) or a panic (exit 101).
#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::{Command, Output, Stdio};

const MESSAGE: &str = "error: cannot write to standard output: ";

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let st = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(st.success());
    fs::create_dir_all(dir.path().join("src")).unwrap();
    for i in 0..5 {
        fs::write(
            dir.path().join(format!("src/m{i}.rs")),
            format!("pub fn f{i}() {{}}\n"),
        )
        .unwrap();
    }
    dir
}

fn into_dev_full(cwd: &Path, args: &[&str]) -> Output {
    let full = OpenOptions::new().write(true).open("/dev/full").unwrap();
    Command::new(env!("CARGO_BIN_EXE_repolens"))
        .env_remove("REPOLENS_CLOCK_OVERRIDE")
        .current_dir(cwd)
        .args(args)
        .stdout(full)
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn assert_clean_failure(args: &[&str], o: &Output) {
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{args:?}: {:?} {err}", o.status);
    assert!(!err.contains("panicked"), "{args:?} panicked: {err}");
    let lines: Vec<&str> = err.lines().filter(|l| l.contains(MESSAGE)).collect();
    assert_eq!(lines.len(), 1, "{args:?}: message must print once: {err}");
    assert!(lines[0].chars().count() <= 80, "{args:?}: {}", lines[0]);
}

#[test]
fn help_and_version_into_a_full_device_fail_with_one_line() {
    let d = repo();
    for args in [
        &["--help"][..],
        &["-h"][..],
        &["--version"][..],
        &["scan", "--help"][..],
        &["memory", "--help"][..],
    ] {
        let o = into_dev_full(d.path(), args);
        assert_clean_failure(args, &o);
    }
}

#[test]
fn commands_into_a_full_device_fail_without_panicking() {
    let d = repo();
    let args = ["scan", "--full"];
    let o = into_dev_full(d.path(), &args);
    assert_clean_failure(&args, &o);
    // The scan finished its work before printing, so its pack exists and
    // later commands can read it.
    for args in [
        &["findings"][..],
        &["findings", "--json"][..],
        &["brief", "--stdout"][..],
    ] {
        let o = into_dev_full(d.path(), args);
        assert_clean_failure(args, &o);
    }
}
