//! #14: recall explains the grounding penalty, says when faded matches
//! were removed, and notes anchored files changed since a note was saved.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const T0: &str = "2026-01-01T00:00:00Z";
const T30: &str = "2026-01-31T00:00:00Z";
const T90: &str = "2026-04-01T00:00:00Z";

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(st.success());
    };
    git(&["init", "-q"]);
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    fs::write(dir.path().join("src/sm4.rs"), "pub fn s() {}\n").unwrap();
    dir
}

fn rl(cwd: &Path, clock: Option<&str>, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_repolens"));
    c.current_dir(cwd).args(args).env_remove("COLUMNS");
    match clock {
        Some(t) => c.env("REPOLENS_CLOCK_OVERRIDE", t),
        None => c.env_remove("REPOLENS_CLOCK_OVERRIDE"),
    };
    c.output().unwrap()
}

fn ok(cwd: &Path, clock: Option<&str>, args: &[&str]) -> String {
    let o = rl(cwd, clock, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn save(cwd: &Path, args: &[&str]) {
    let mut all = vec!["memory", "save"];
    all.extend_from_slice(args);
    ok(cwd, Some(T0), &all);
}

/// Display columns, CJK and `…` counting 2.
fn cols(line: &str) -> usize {
    line.chars()
        .map(|c| {
            if c == '…' || ('\u{2E80}'..='\u{FFEF}').contains(&c) {
                2
            } else {
                1
            }
        })
        .sum()
}

fn assert_width(out: &str) {
    for line in out.lines() {
        assert!(cols(line) <= 80, "{} cols: {line:?}\n{out}", cols(line));
        assert!(!line.contains('\t'), "tab in {line:?}");
    }
}

/// The score line of the hit whose note is `content`.
fn score_line<'a>(out: &'a str, content: &str) -> &'a str {
    let lines: Vec<&str> = out.lines().collect();
    let i = lines
        .iter()
        .position(|l| l.trim() == content)
        .unwrap_or_else(|| panic!("{content} not in\n{out}"));
    lines[i - 1]
}

/// (score, importance, grounding factor, keywords) parsed from a score line.
fn terms(line: &str) -> (f64, f64, Option<f64>, f64) {
    let num = |key: &str| -> Option<f64> {
        let i = line.find(key)? + key.len();
        let rest = &line[i..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        rest[..end].parse().ok()
    };
    (
        num("score ").unwrap(),
        num("importance ").unwrap(),
        num("grounding "),
        num("keywords ").unwrap() / 100.0,
    )
}

#[test]
fn score_line_shows_the_grounding_factor_only_when_it_applies() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    save(
        d.path(),
        &["--anchor", "does/not/exist.rs", "ghost anchor note"],
    );
    save(d.path(), &["--anchor", "src/lib.rs", "ghost lib note"]);
    save(d.path(), &["--type", "decision", "ghost plain note"]);

    let now = ok(d.path(), Some(T0), &["memory", "recall", "ghost"]);
    assert!(
        now.contains("score 0.50 = importance 0.50 x keywords 100%"),
        "{now}"
    );

    let out = ok(d.path(), Some(T30), &["memory", "recall", "ghost"]);
    assert_width(&out);
    let ghost = score_line(&out, "ghost anchor note");
    assert!(ghost.contains(" x grounding 0.50 x "), "{out}");
    let (s, i, g, k) = terms(ghost);
    assert!(
        (i - 0.25).abs() < 0.011,
        "importance agrees with Decay: {out}"
    );
    assert!((i * g.unwrap() * k - s).abs() <= 0.01, "{ghost}");

    for content in ["ghost lib note", "ghost plain note"] {
        let line = score_line(&out, content);
        assert!(!line.contains("grounding"), "{out}");
        let (s, i, g, k) = terms(line);
        assert_eq!(g, None);
        assert!((i * k - s).abs() <= 0.01, "{line}");
    }
    assert!(!out.contains("0.05"), "cutoff must stay unprinted: {out}");
}

#[test]
fn faded_matches_are_reported_as_removed() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    save(
        d.path(),
        &["--anchor", "does/not/exist.rs", "ghost anchor note"],
    );
    save(d.path(), &["--anchor", "src/lib.rs", "ghost lib note"]);
    save(d.path(), &["--anchor", "src/sm4.rs", "sm4 cipher note"]);
    save(d.path(), &["--anchor", "does/not/gone.rs", "sm4 gone note"]);
    save(d.path(), &["--anchor", "does/not/away.rs", "sm4 away note"]);

    // All matches removed: exactly two lines.
    let out = ok(d.path(), Some(T90), &["memory", "recall", "anchor"]);
    assert_eq!(
        out,
        "No memory to show for \"anchor\".\n\
         1 match was removed: it went unused and faded below the recall cutoff.\n"
    );

    // One shown, one removed: the line once, after a blank line.
    let out = ok(d.path(), Some(T90), &["memory", "recall", "ghost"]);
    assert_width(&out);
    let line = "1 more match was removed: it went unused and faded below the recall cutoff.";
    assert_eq!(out.matches(line).count(), 1, "{out}");
    assert!(out.ends_with(&format!("\n\n{line}\n")), "{out}");
    assert!(!out.contains("hidden"), "{out}");
    assert!(!out.contains("No saved memory matches"), "{out}");

    // Plural.
    let out = ok(d.path(), Some(T90), &["memory", "recall", "sm4"]);
    assert_width(&out);
    assert!(out.ends_with(
        "\n\n2 more matches were removed: they went unused and faded below the recall cutoff.\n"
    ), "{out}");

    // Every form picks "match" or "matches"; never "match(es)".
    for q in ["anchor", "ghost", "sm4"] {
        let out = ok(d.path(), Some(T90), &["memory", "recall", q]);
        assert!(!out.contains("(es)"), "{out}");
    }

    // --type applies: the removed matches are PATTERNs.
    let out = ok(
        d.path(),
        Some(T90),
        &["memory", "recall", "anchor", "--type", "decision"],
    );
    assert_eq!(out, "No saved memory matches \"anchor\".\n");

    // Stop-word queries keep their own message.
    let out = ok(d.path(), Some(T90), &["memory", "recall", "the of"]);
    assert!(out.starts_with("No usable search terms"), "{out}");
    assert!(!out.contains("removed"), "{out}");
    assert!(!out.contains("0.05"), "{out}");
}

#[test]
fn changed_anchor_is_noted_until_reverted() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    save(
        d.path(),
        &[
            "--anchor",
            "src/lib.rs",
            "--anchor",
            "src/sm4.rs",
            "two file note",
        ],
    );
    let joined = |out: &str| out.replace("\n   ", " ");

    let out = ok(d.path(), Some(T0), &["memory", "recall", "two file"]);
    assert!(!out.contains("changed since"), "{out}");

    fs::write(d.path().join("src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    let out = ok(d.path(), Some(T0), &["memory", "recall", "two file"]);
    assert_width(&out);
    let j = joined(&out);
    assert!(
        j.contains("; src/lib.rs changed since this note was saved)"),
        "only the changed file is listed: {out}"
    );
    assert!(!j.contains("sm4.rs changed"), "{out}");
    assert!(j.contains("Grounding: CONFIRMED ("), "{out}");

    fs::write(d.path().join("src/sm4.rs"), "pub fn s() { /* edited */ }\n").unwrap();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    let out = ok(d.path(), Some(T0), &["memory", "recall", "two file"]);
    assert_width(&out);
    assert!(
        joined(&out).contains("; src/lib.rs, src/sm4.rs changed since this note was saved)"),
        "{out}"
    );

    fs::write(d.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    fs::write(d.path().join("src/sm4.rs"), "pub fn s() {}\n").unwrap();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    let out = ok(d.path(), Some(T0), &["memory", "recall", "two file"]);
    assert!(!out.contains("changed since"), "gone after revert: {out}");
}

#[test]
fn notes_saved_without_file_hashes_say_nothing_extra() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    save(d.path(), &["--anchor", "src/lib.rs", "older note"]);
    // Simulate a note saved before hashes existed.
    let p = d.path().join(".repolens/memory/memory.json");
    let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    let node = &mut v["nodes"][0];
    assert!(node.get("anchorSha256").is_some(), "{node}");
    node.as_object_mut().unwrap().remove("anchorSha256");
    fs::write(&p, serde_json::to_string(&v).unwrap()).unwrap();

    fs::write(d.path().join("src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    let out = ok(d.path(), Some(T0), &["memory", "recall", "older"]);
    assert!(
        out.contains("Grounding: CONFIRMED (src/lib.rs found in"),
        "{out}"
    );
    assert!(!out.contains("changed since"), "{out}");
}

#[test]
fn new_lines_fit_80_columns_with_cjk_and_tabs() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    save(
        d.path(),
        &[
            "--anchor",
            "does/not/exist.rs",
            "保存一个很长的中文备注，用来检查每一行都不会超过八十列的宽度限制，即使有罚分项也一样",
        ],
    );
    save(
        d.path(),
        &["--anchor", "src/lib.rs", "tab\tseparated\tnote 中文"],
    );
    fs::write(d.path().join("src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    for q in ["中文备注", "separated", "note"] {
        for t in [T0, T30, T90] {
            assert_width(&ok(d.path(), Some(t), &["memory", "recall", q]));
        }
    }
}
