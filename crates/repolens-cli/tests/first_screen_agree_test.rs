//! #32: `init` and `findings` agree, and the language-coverage note
//! (F-LANGUAGE-EXTRACTION-GAP) is shown once, below the findings, never in
//! one of `init`'s top slots.

use std::fs;
use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> String {
    let mut all = vec![args[0], "--root", root.to_str().unwrap()];
    all.extend_from_slice(&args[1..]);
    let out = Command::new(env!("CARGO_BIN_EXE_repolens"))
        .args(&all)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// Rust library + Ruby/Shell scripts + two Python files: gives Low findings,
/// Info findings and a coverage note with both an unread and a Python line.
fn mixed_repo(root: &Path) {
    fs::create_dir_all(root.join(".git")).unwrap();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let mut lib = String::new();
    for m in ["a", "b", "c", "d", "e", "f"] {
        write(
            root,
            &format!("src/{m}.rs"),
            &format!("pub fn {m}() {{}}\n"),
        );
        lib.push_str(&format!("pub mod {m};\n"));
    }
    write(root, "src/lib.rs", &lib);
    write(root, "scripts/build.sh", "#!/bin/sh\necho build\n");
    write(root, "scripts/gen.rb", "puts \"hi\"\n");
    write(root, "tools/main.py", "import helper\nhelper.run()\n");
    write(root, "tools/helper.py", "def run():\n    pass\n");
}

/// gm-crypto-web's shape: only no-action (Info) findings.
fn info_only_repo(root: &Path) {
    fs::create_dir_all(root.join(".git")).unwrap();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(root, "Cargo.lock", "version = 3\n");
    write(root, "src/lib.rs", "pub fn f() {}\n");
}

/// The findings count a screen claims: `Some(n)` from "found N thing(s)
/// worth a look", `Some(0)` from "found nothing to flag".
fn claimed_count(out: &str) -> Option<usize> {
    for l in out.lines() {
        let l = l.trim();
        if !l.starts_with("The last scan") {
            continue;
        }
        if l.contains("found nothing to flag") {
            return Some(0);
        }
        let (_, rest) = l.split_once(") found ")?;
        return rest.split_whitespace().next()?.parse().ok();
    }
    None
}

fn coverage_lines(out: &str) -> Vec<String> {
    let lines: Vec<&str> = out.lines().collect();
    let Some(i) = lines.iter().position(|l| l.starts_with("  Coverage: ")) else {
        return vec![];
    };
    let mut v = vec![lines[i].to_string()];
    for l in &lines[i + 1..] {
        if l.starts_with("            ") {
            v.push(l.to_string());
        } else {
            break;
        }
    }
    v
}

const ROW_PREFIXES: [&str; 4] = ["  High  ", "  Med   ", "  Low   ", "  Info  "];

fn rows(out: &str) -> Vec<&str> {
    out.lines()
        .filter(|l| ROW_PREFIXES.iter().any(|p| l.starts_with(p)))
        .collect()
}

fn assert_fits_80(out: &str) {
    for l in out.lines() {
        assert!(
            unicode_width::UnicodeWidthStr::width(l) <= 80,
            "over 80 columns: {l:?}"
        );
    }
}

#[test]
fn init_and_findings_agree_and_coverage_note_sits_below() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    mixed_repo(root);
    run(root, &["scan"]);
    let init = run(root, &["init"]);
    let findings = run(root, &["findings"]);
    assert_fits_80(&init);
    assert_fits_80(&findings);

    let n_init = claimed_count(&init).expect(&init);
    let n_findings = claimed_count(&findings).expect(&findings);
    assert_eq!(n_init, n_findings, "init:\n{init}\nfindings:\n{findings}");
    assert!(n_init > 0, "{findings}");

    // The coverage note: present once in each, same words, below the rows.
    let cov_init = coverage_lines(&init);
    let cov_findings = coverage_lines(&findings);
    assert!(!cov_init.is_empty(), "{init}");
    assert_eq!(
        cov_init, cov_findings,
        "init:\n{init}\nfindings:\n{findings}"
    );
    assert!(
        cov_init[0].contains("2 languages: Ruby, Shell"),
        "{cov_init:?}"
    );
    assert_eq!(init.matches("  Coverage: ").count(), 1, "{init}");
    assert_eq!(findings.matches("  Coverage: ").count(), 1, "{findings}");
    for out in [&init, &findings] {
        assert_eq!(out.matches("Python: imports read").count(), 1, "{out}");
        for r in rows(out) {
            assert!(
                !r.contains("aren't read") && !r.contains("not fully parsed"),
                "coverage note in a findings slot: {r}"
            );
        }
        let lines: Vec<&str> = out.lines().collect();
        let last_row = lines
            .iter()
            .rposition(|l| ROW_PREFIXES.iter().any(|p| l.starts_with(p)))
            .unwrap();
        let cov = out
            .lines()
            .position(|l| l.starts_with("  Coverage: "))
            .unwrap();
        assert!(
            cov > last_row,
            "coverage note must sit below the findings:\n{out}"
        );
    }

    // The top slots in init are the first rows of findings, word for word.
    let init_rows = rows(&init);
    let findings_rows = rows(&findings);
    assert!(init_rows.len() <= 3, "{init}");
    assert_eq!(
        init_rows[..],
        findings_rows[..init_rows.len()],
        "{init}\n{findings}"
    );
}

#[test]
fn info_only_scan_is_nothing_to_flag_on_both_screens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    info_only_repo(root);
    run(root, &["scan"]);
    let init = run(root, &["init"]);
    let findings = run(root, &["findings"]);
    assert_eq!(claimed_count(&init), Some(0), "{init}");
    assert_eq!(claimed_count(&findings), Some(0), "{findings}");
    // findings still shows the no-action notes, under their own heading.
    assert!(
        findings.contains("Also noted, no action needed:"),
        "{findings}"
    );
    assert!(
        rows(&findings).iter().all(|r| r.starts_with("  Info  ")),
        "{findings}"
    );
    assert_fits_80(&init);
    assert_fits_80(&findings);
}

/// The line after the headline, on each screen.
fn line_after_headline(out: &str) -> Option<&str> {
    let lines: Vec<&str> = out.lines().collect();
    let i = lines.iter().position(|l| l.starts_with("The last scan"))?;
    lines.get(i + 1).copied()
}

#[test]
fn both_screens_leave_a_blank_line_under_the_headline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    mixed_repo(root);
    run(root, &["scan"]);
    let init = run(root, &["init"]);
    let findings = run(root, &["findings"]);
    assert!(claimed_count(&init).unwrap_or(0) > 0, "{init}");
    assert_eq!(line_after_headline(&init), Some(""), "{init}");
    assert_eq!(line_after_headline(&findings), Some(""), "{findings}");
}

/// The README's `init` sample keeps the same shape as the real screen.
#[test]
fn readme_init_sample_has_the_blank_line_under_the_headline() {
    let readme = include_str!("../../../README.md");
    assert_eq!(line_after_headline(readme), Some(""), "README sample");
}
