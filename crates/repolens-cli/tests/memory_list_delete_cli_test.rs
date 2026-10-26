//! `repolens memory list` / `repolens memory forget` (#54, #81).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use unicode_width::UnicodeWidthStr;

const T0: &str = "2026-01-01T00:00:00Z";
const T30: &str = "2026-01-31T00:00:00Z";

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let st = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(st.success());
    dir
}

fn rl(cwd: &Path, clock: Option<&str>, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_repolens"));
    c.current_dir(cwd).args(args);
    match clock {
        Some(t) => c.env("REPOLENS_CLOCK_OVERRIDE", t),
        None => c.env_remove("REPOLENS_CLOCK_OVERRIDE"),
    };
    c.output().unwrap()
}

fn ok(cwd: &Path, args: &[&str]) -> String {
    let o = rl(cwd, Some(T0), args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Run a command that must fail; returns stderr.
fn fails(cwd: &Path, args: &[&str]) -> String {
    let o = rl(cwd, Some(T0), args);
    assert!(!o.status.success(), "{args:?} should fail");
    assert!(
        o.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn memory_json(d: &Path) -> String {
    fs::read_to_string(d.join(".repolens/memory/memory.json")).unwrap()
}

/// Every file under `.repolens/memory` with its bytes; `None` when the
/// directory is absent.
fn memory_dir(d: &Path) -> Option<Vec<(String, Vec<u8>)>> {
    let dir = d.join(".repolens/memory");
    if !d.join(".repolens").exists() {
        return None;
    }
    let mut out: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().to_string(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    out.sort();
    Some(out)
}

/// Rewrite every saved id to start with `prefix`, keeping them distinct.
fn give_ids_prefix(d: &Path, prefix: &str) -> Vec<String> {
    let mut json = memory_json(d);
    let mut new = Vec::new();
    for id in ids(d) {
        let n = format!("{prefix}{}", &id[prefix.len()..]);
        json = json.replace(&id, &n);
        new.push(n);
    }
    fs::write(d.join(".repolens/memory/memory.json"), json).unwrap();
    new
}

fn ids(d: &Path) -> Vec<String> {
    let g: serde_json::Value = serde_json::from_str(&memory_json(d)).unwrap();
    g["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
}

/// Display width with `…` counted as 2, as some CJK terminals draw it.
fn cols(line: &str) -> usize {
    line.width() + line.matches('…').count()
}

fn assert_fits(out: &str) {
    for line in out.lines() {
        assert!(cols(line) <= 80, "{} cols: {line:?}", cols(line));
    }
}

#[test]
fn list_shows_saved_importance_read_only_within_80_columns() {
    let d = repo();
    let empty = ok(d.path(), &["memory", "list"]);
    assert!(empty.contains("No saved memories yet."), "{empty}");
    assert!(!d.path().join(".repolens").exists(), "list created files");

    let long = "Pack writes are atomic: the scanner writes to a temp dir and renames \
                it into place, so a crash never leaves a half-written pack behind";
    let cjk =
        "缓存层在每次扫描后失效，因为索引文件的哈希值会改变，所以不要手动编辑索引目录里的文件";
    ok(d.path(), &["memory", "save", long]);
    let one = ok(d.path(), &["memory", "list"]);
    assert!(one.starts_with("1 saved memory, oldest first:\n"), "{one}");
    ok(
        d.path(),
        &[
            "memory",
            "save",
            "--type",
            "decision",
            "--importance",
            "0.8",
            cjk,
        ],
    );
    ok(
        d.path(),
        &["memory", "save", "--type", "trade_off", "Short note"],
    );
    let all = ids(d.path());
    let before = memory_dir(d.path());

    // A month later the stored value is shown as is; nothing is written.
    let o = rl(d.path(), Some(T30), &["memory", "list"]);
    assert!(o.status.success());
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        out.starts_with("3 saved memories, oldest first:\n"),
        "{out}"
    );
    assert!(out.contains("1. PATTERN  saved importance 0.50\n"), "{out}");
    assert!(
        out.contains("2. DECISION  saved importance 0.80\n"),
        "{out}"
    );
    assert!(
        out.contains("3. TRADE_OFF  saved importance 0.50\n"),
        "{out}"
    );
    for id in &all {
        assert!(out.contains(&format!("   id: {id}\n")), "{out}");
    }
    assert!(out.contains("   Pack writes are atomic"), "{out}");
    assert!(out.contains("   缓存层"), "{out}");
    assert!(out.contains("   Short note\n"), "{out}");
    assert!(!out.contains("faded"), "{out}");
    assert!(
        out.ends_with("\n\nImportance is the value when last saved; recall applies fading.\n"),
        "{out}"
    );
    assert_fits(&out);
    assert_eq!(memory_dir(d.path()), before, "list wrote");

    let ls = ok(d.path(), &["memory", "ls"]);
    assert!(ls.starts_with("3 saved memories"), "{ls}");
}

#[test]
fn forget_previews_by_default_and_deletes_with_yes() {
    let d = repo();
    ok(d.path(), &["memory", "save", "Keep this note"]);
    let long = format!(
        "Drop this note: {}\nsecond line is not shown",
        "缓存层失效 ".repeat(20)
    );
    let saved = ok(d.path(), &["memory", "save", &long]);
    let all = ids(d.path());
    let short = &all[1][..8];
    assert!(saved.contains(short), "{saved}");
    let before = memory_dir(d.path());

    let preview = ok(d.path(), &["memory", "forget", short]);
    let lines: Vec<&str> = preview.lines().collect();
    assert_eq!(lines.len(), 4, "{preview}");
    assert_eq!(lines[0], "Would delete 1 memory:");
    assert_eq!(lines[1], format!("  {}  PATTERN", all[1]));
    assert!(
        lines[2].starts_with("  Drop this note: 缓存层失效"),
        "{preview}"
    );
    assert!(lines[2].ends_with('…'), "{preview}");
    assert_eq!(lines[3], "Run again with --yes to delete it.");
    assert!(!preview.contains("second line"), "{preview}");
    assert_fits(&preview);
    assert_eq!(memory_dir(d.path()), before, "preview wrote");

    // Ids are hex: any case matches, like MCP forget_insight.
    let upper = short.to_uppercase();
    let done = ok(d.path(), &["memory", "forget", &upper, "--yes"]);
    assert!(done.starts_with("Deleted 1 memory:\n"), "{done}");
    assert!(done.contains(&format!("  {}  PATTERN\n", all[1])), "{done}");
    assert_fits(&done);
    assert_eq!(ids(d.path()), vec![all[0].clone()]);

    // There is no hidden `rm` alias: it is rejected and deletes nothing.
    fails(d.path(), &["memory", "rm", &all[0], "--yes"]);
    assert_eq!(ids(d.path()), vec![all[0].clone()]);

    // The full id works too.
    let full = ok(d.path(), &["memory", "forget", &all[0], "--yes"]);
    assert_eq!(
        full,
        format!(
            "Deleted 1 memory:\n  {}  PATTERN\n  Keep this note\n",
            all[0]
        )
    );
    assert!(ids(d.path()).is_empty());

    // `delete` is gone.
    let o = rl(d.path(), Some(T0), &["memory", "delete", "abcd"]);
    assert!(!o.status.success());
}

#[test]
fn forget_refuses_unknown_short_and_ambiguous_ids_without_writing() {
    // Empty repo: nothing is created, whatever the id.
    let d = repo();
    for yes in [false, true] {
        let with = |id: &str| -> Vec<String> {
            let mut v = vec!["memory".to_string(), "forget".into(), id.into()];
            if yes {
                v.push("--yes".into());
            }
            v
        };
        let run = |id: &str| {
            let args = with(id);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            fails(d.path(), &args)
        };
        assert_eq!(
            run("ffff1234"),
            "No memory with id ffff1234. Nothing was deleted.\n"
        );
        assert_eq!(run("abc"), "Use at least 4 characters of the id.\n");
        assert!(!d.path().join(".repolens").exists(), "forget created files");
    }

    // With notes: unknown, short and ambiguous ids leave the store as is.
    for i in 0..12 {
        ok(d.path(), &["memory", "save", &format!("Note number {i}")]);
    }
    let shared = give_ids_prefix(d.path(), "abcd");
    let before = memory_dir(d.path());
    for yes in [false, true] {
        let run = |id: &str| {
            let mut args = vec!["memory", "forget", id];
            if yes {
                args.push("--yes");
            }
            fails(d.path(), &args)
        };
        assert_eq!(
            run("ffff"),
            "No memory with id ffff. Nothing was deleted.\n"
        );
        assert_eq!(run("ab"), "Use at least 4 characters of the id.\n");
        assert_eq!(run("a"), "Use at least 4 characters of the id.\n");

        let err = run("ABCD");
        let lines: Vec<&str> = err.lines().collect();
        assert_eq!(lines[0], "ABCD matches 12 memories; use a longer prefix.");
        assert_eq!(lines.len(), 12, "{err}");
        for (line, id) in lines[1..11].iter().zip(&shared) {
            assert!(
                line.starts_with(&format!("  {id}  PATTERN  Note number")),
                "{err}"
            );
        }
        assert_eq!(lines[11], "  …and 2 more");
        assert_fits(&err);
        assert_eq!(memory_dir(d.path()), before, "a refused forget wrote");
    }

    let err = fails(d.path(), &["memory", "forget", "  ", "--yes"]);
    assert!(err.contains("id must not be empty"), "{err}");
    // A full id always works.
    let out = ok(d.path(), &["memory", "forget", &shared[3]]);
    assert!(out.contains(&shared[3]), "{out}");
}

#[test]
fn ambiguous_prefix_of_two_lists_both() {
    let d = repo();
    ok(d.path(), &["memory", "save", "First note"]);
    ok(d.path(), &["memory", "save", "Second note"]);
    let shared = give_ids_prefix(d.path(), "abcd1234");
    let before = memory_dir(d.path());
    let err = fails(d.path(), &["memory", "forget", "abcd", "--yes"]);
    assert_eq!(
        err,
        format!(
            "abcd matches 2 memories; use a longer prefix.\n  {}  PATTERN  First note\n  {}  PATTERN  Second note\n",
            shared[0], shared[1]
        )
    );
    assert_eq!(memory_dir(d.path()), before);
}
