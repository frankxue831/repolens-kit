//! #36: redirected output never carries ANSI colour codes, and
//! back-to-back scans print no WARN lines.

use std::fs;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn repolens(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_repolens"))
        .args(args)
        .arg("--root")
        .arg(root)
        .env("RUST_LOG", "info")
        .env_remove("NO_COLOR")
        .output()
        .unwrap()
}

#[test]
fn redirected_scans_have_no_ansi_and_no_warn_lines() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for i in 0..20 {
        fs::write(
            root.join(format!("f{i}.rs")),
            format!("pub fn f{i}() {{}}\n"),
        )
        .unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);

    let first = repolens(root, &["scan"]);
    let second = repolens(root, &["scan"]);
    for out in [&first, &second] {
        assert!(out.status.success());
        let all = [out.stdout.as_slice(), out.stderr.as_slice()].concat();
        let text = String::from_utf8_lossy(&all);
        assert!(
            !text.contains('\u{1b}'),
            "ANSI escape in redirected output:\n{text}"
        );
        assert!(text.contains("INFO"), "RUST_LOG=info should log:\n{text}");
        assert!(!text.contains("WARN"), "unexpected WARN line:\n{text}");
    }
    let packs = fs::read_dir(root.join(".repolens"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("pack-"))
        .filter(|e| e.path().join("manifest.json").is_file())
        .count();
    assert_eq!(packs, 2, "both packs are valid and distinct");
}
