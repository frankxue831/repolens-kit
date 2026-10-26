//! #59: the full-scan fallback after scans on a dirty worktree explains
//! what to do, once per scan, and never on an incremental scan.

use std::fs;
use std::path::Path;
use std::process::Command;

const LINES: [&str; 3] = [
    "Fell back to a full scan: no earlier scan ran on a clean worktree.",
    "For incremental scans, commit or ignore your changes, or run git stash -u,",
    "then scan again. That scan is still full; later scans are incremental.",
];

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn scan(root: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_repolens"))
        .args(["scan", "--root", root.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .expect("run repolens scan");
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn assert_no_fallback_lines(out: &str) {
    for line in LINES {
        assert!(!out.contains(line), "{out}");
    }
    assert!(!out.contains("Fell back"), "{out}");
}

#[test]
fn dirty_fallback_explains_once_per_scan_and_not_when_incremental() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@t.com"]);
    git(root, &["config", "user.name", "T"]);
    for i in 0..5 {
        fs::write(
            root.join(format!("f{i}.rs")),
            format!("pub fn f{i}() {{}}\n"),
        )
        .unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("f0.rs"), "pub fn f0() { }\n").unwrap();

    // First-ever scan: nothing to fall back from.
    assert_no_fallback_lines(&scan(root));

    // Every earlier pack is dirty: the three lines, once each, in order,
    // never the old pack-internal reason, never once per file.
    for _ in 0..2 {
        let out = scan(root);
        assert!(out.starts_with("Scan complete (full)"), "{out}");
        for line in LINES {
            assert_eq!(out.matches(line).count(), 1, "{out}");
        }
        let shown: Vec<&str> = out
            .lines()
            .map(str::trim)
            .filter(|l| LINES.contains(l))
            .collect();
        assert_eq!(shown, LINES, "{out}");
        assert!(!out.contains("worktree was dirty"), "{out}");
        assert!(!out.contains("usable base"), "{out}");
        // Every line we wrote fits 80 columns. "Pack written to:" holds the
        // absolute temp-dir path, which is machine-dependent (on macOS,
        // /private/var/folders/<id>/T/... is over 80 on its own).
        assert!(
            out.lines()
                .filter(|l| !l.starts_with("Pack written to: "))
                .all(|l| l.chars().count() <= 80),
            "{out}"
        );
    }

    // Commit: that scan is still full (all earlier packs are dirty)...
    git(root, &["commit", "-qam", "edit"]);
    let out = scan(root);
    assert!(out.starts_with("Scan complete (full)"), "{out}");
    assert_eq!(out.matches(LINES[0]).count(), 1, "{out}");

    // ...and the one after it is incremental, with no fallback lines.
    let out = scan(root);
    assert!(out.starts_with("Scan complete (incremental)"), "{out}");
    assert_no_fallback_lines(&out);
}
