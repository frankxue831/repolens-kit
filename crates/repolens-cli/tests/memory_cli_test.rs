//! `repolens memory save` / `repolens memory recall`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
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

fn ok(cwd: &Path, clock: Option<&str>, args: &[&str]) -> String {
    let o = rl(cwd, clock, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Importance shown on the first result line.
fn importance(out: &str) -> f64 {
    let i = out.find("= importance ").unwrap() + "= importance ".len();
    out[i..i + 4].parse().unwrap()
}

#[test]
fn save_then_recall_defaults_root_to_cwd_repo() {
    let d = repo();
    // Run from a subdirectory: root resolves to the enclosing repo.
    let sub = d.path().join("src");
    let out = ok(
        &sub,
        Some(T0),
        &[
            "memory",
            "save",
            "--type",
            "decision",
            "--anchor",
            "src/lib.rs",
            "Errors use anyhow in the CLI",
        ],
    );
    assert!(out.starts_with("Saved DECISION memory "), "{out}");
    assert!(out.contains("  Anchor: src/lib.rs"));
    assert!(d.path().join(".repolens/memory/memory.json").is_file());

    let out = ok(&sub, Some(T0), &["memory", "recall", "anyhow errors"]);
    assert!(
        out.starts_with("1 memory matches \"anyhow errors\":"),
        "{out}"
    );
    assert!(
        out.contains("1. DECISION  score 0.50 = importance 0.50 x keywords 100%"),
        "{out}"
    );
    assert!(out.contains("   Errors use anyhow in the CLI"));
    assert!(
        out.contains("Decay: last used less than an hour ago; fades while unused\n"),
        "{out}"
    );
    // No scan yet: grounding can't be checked.
    assert!(
        out.contains("Grounding: UNVERIFIABLE (no scan yet"),
        "{out}"
    );
}

#[test]
fn grounding_confirmed_and_contradicted_against_scan() {
    let d = repo();
    fs::write(d.path().join("src/gone.rs"), "pub fn g() {}\n").unwrap();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--anchor",
            "src/lib.rs",
            "lib holds the api",
        ],
    );
    ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--anchor",
            "src/missing.rs",
            "missing holds the parser",
        ],
    );
    let out = ok(d.path(), Some(T0), &["memory", "recall", "api"]);
    assert!(
        out.contains("Grounding: CONFIRMED (src/lib.rs found in pack-"),
        "{out}"
    );
    let out = ok(d.path(), Some(T0), &["memory", "recall", "parser"]);
    assert!(
        out.contains("Grounding: CONTRADICTED (src/missing.rs missing from pack-"),
        "{out}"
    );
}

#[test]
fn thirty_days_later_score_is_about_half() {
    let d = repo();
    ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--type",
            "convention",
            "Tests live next to code",
        ],
    );
    let now = ok(d.path(), Some(T0), &["memory", "recall", "tests code"]);
    assert_eq!(importance(&now), 0.50, "{now}");
    let later = ok(d.path(), Some(T30), &["memory", "recall", "tests code"]);
    let imp = importance(&later);
    assert!((imp - 0.25).abs() < 0.011, "expected ~half: {later}");
    assert!(later.contains("score 0.25"), "{later}");
    assert!(later.contains("last used 30.0 days ago"), "{later}");
}

#[test]
fn confirmed_anchor_halves_from_point_eight_and_shows_floor() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--importance",
            "0.8",
            "--anchor",
            "src/lib.rs",
            "lib is the api",
        ],
    );
    let now = ok(d.path(), Some(T0), &["memory", "recall", "api"]);
    assert_eq!(importance(&now), 0.80);
    let later = ok(d.path(), Some(T30), &["memory", "recall", "api"]);
    assert_eq!(importance(&later), 0.40, "{later}");
    // Recall wraps at 80 columns (#35), so join wrapped lines first.
    let joined = later.replace("\n   ", " ");
    assert!(
        joined.contains("fades while unused (a confirmed anchor keeps a minimum)"),
        "{later}"
    );
    // The Decay line describes behaviour; it doesn't state tuning values.
    assert!(!joined.contains("halves after"), "{later}");
    assert!(!joined.contains("or more)"), "{later}");
}

#[test]
fn state_reflects_saved_memory() {
    let d = repo();
    ok(d.path(), Some(T0), &["init"]);
    ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--type",
            "trade_off",
            "Speed over memory in the scanner",
        ],
    );
    let out = ok(d.path(), None, &["state"]);
    assert!(out.contains("  Nodes: 1"), "{out}");
    assert!(out.contains("    TRADE_OFF: 1"), "{out}");
    assert!(
        out.contains("  Newest: TRADE_OFF \"Speed over memory in the scanner\" (importance 0.50)"),
        "{out}"
    );
}

fn last_updated_line(out: &str) -> String {
    out.lines()
        .find(|l| l.starts_with("  Last updated: "))
        .unwrap()
        .to_string()
}

#[test]
fn recall_with_clock_override_writes_nothing() {
    let d = repo();
    // Saved with the real clock.
    ok(
        d.path(),
        None,
        &["memory", "save", "Readonly what if recall"],
    );
    let mem = d.path().join(".repolens/memory/memory.json");
    let before = fs::read(&mem).unwrap();
    let state_before = last_updated_line(&ok(d.path(), None, &["state"]));
    // Cross a second boundary on purpose: nothing below may depend on the
    // real clock ticking (or not) between save and the checks (#43).
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let far = (time::OffsetDateTime::now_utc() + time::Duration::days(30))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let later = ok(d.path(), Some(&far), &["memory", "recall", "readonly"]);
    assert!(later.contains("score 0.25"), "{later}");
    assert_eq!(
        fs::read(&mem).unwrap(),
        before,
        "override recall wrote the file"
    );
    // Read "Last updated" right after the override recall. A real-clock
    // recall legitimately runs decay and stamps the current time, so
    // comparing after it made this test flaky across second ticks (#43).
    let state_after = last_updated_line(&ok(d.path(), None, &["state"]));
    assert_eq!(
        state_before, state_after,
        "override recall moved Last updated"
    );

    // The override's 30-day decay was not persisted: a real-clock recall
    // still sees the undecayed score.
    let again = ok(d.path(), None, &["memory", "recall", "readonly"]);
    assert!(again.contains("score 0.50"), "{again}");
}

/// #8: a save under the clock override stamps the file's `updatedAt`
/// (shown as "Last updated" by `repolens state`) with the override too,
/// like the note's own created/last-used times, not the real clock.
#[test]
fn save_with_clock_override_stamps_last_updated() {
    let d = repo();
    ok(d.path(), Some(T0), &["memory", "save", "Pinned clock note"]);
    let mem = d.path().join(".repolens/memory/memory.json");
    let graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&mem).unwrap()).unwrap();
    let rfc = time::format_description::well_known::Rfc3339;
    let updated = time::OffsetDateTime::parse(graph["updatedAt"].as_str().unwrap(), &rfc).unwrap();
    let created =
        time::OffsetDateTime::parse(graph["nodes"][0]["created"].as_str().unwrap(), &rfc).unwrap();
    let t0 = time::OffsetDateTime::parse(T0, &rfc).unwrap();
    assert_eq!(updated, t0, "updatedAt ignored the override");
    assert_eq!(created, t0);
    let state = state_with_tz(d.path(), "UTC0");
    assert!(
        last_updated_line(&state).contains("2026-01-01 00:00:00"),
        "{state}"
    );

    // A second save at a later pinned time moves it again.
    ok(
        d.path(),
        Some(T30),
        &["memory", "save", "Second pinned note"],
    );
    let graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&mem).unwrap()).unwrap();
    let updated = time::OffsetDateTime::parse(graph["updatedAt"].as_str().unwrap(), &rfc).unwrap();
    assert_eq!(updated, time::OffsetDateTime::parse(T30, &rfc).unwrap());
}

#[test]
fn no_anchor_grounding_and_plural_and_anchor_warning() {
    let d = repo();
    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    ok(d.path(), Some(T0), &["memory", "save", "Alpha widget note"]);
    let one = ok(d.path(), Some(T0), &["memory", "recall", "widget"]);
    assert!(one.starts_with("1 memory matches \"widget\":"), "{one}");
    assert!(
        one.contains("   Grounding: none (no file anchors)\n"),
        "{one}"
    );

    let gone = ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--anchor",
            "src/gone.rs",
            "Beta widget note",
        ],
    );
    assert!(gone.starts_with("Saved "), "{gone}");
    assert!(
        gone.contains("  Warning: src/gone.rs is not a file in this repo\n"),
        "{gone}"
    );
    fs::write(d.path().join("src/new.rs"), "pub fn n() {}\n").unwrap();
    let unscanned = ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--anchor",
            "src/new.rs",
            "Gamma widget note",
        ],
    );
    assert!(
        unscanned.contains("  Warning: src/new.rs is not in the latest scan"),
        "{unscanned}"
    );
    let ok_anchor = ok(
        d.path(),
        Some(T0),
        &[
            "memory",
            "save",
            "--anchor",
            "src/lib.rs",
            "Delta widget note",
        ],
    );
    assert!(!ok_anchor.contains("Warning"), "{ok_anchor}");

    let many = ok(d.path(), Some(T0), &["memory", "recall", "widget"]);
    assert!(many.starts_with("4 memories match \"widget\":"), "{many}");
}

/// #9: `--anchor` may name a code symbol the scan found (a qualified name
/// like `Widget.run`, or a type name); that is no reason to warn. A name
/// that is neither a file nor a scanned symbol still warns.
#[test]
fn symbol_anchors_do_not_warn_as_missing_files() {
    let d = repo();
    fs::write(
        d.path().join("src/lib.rs"),
        "pub struct Widget;\nimpl Widget {\n    pub fn run(&self) {}\n}\n",
    )
    .unwrap();
    let save = |anchor: &str, content: &str| {
        ok(
            d.path(),
            Some(T0),
            &["memory", "save", "--anchor", anchor, content],
        )
    };

    // No scan yet: a non-file anchor can't be checked as a symbol.
    let early = save("Widget.run", "Run is idempotent early");
    assert!(
        early.contains(
            "  Warning: Widget.run is not a file in this repo (no scan to check \
             symbols)\n"
        ),
        "{early}"
    );

    ok(d.path(), Some(T0), &["scan", "--root", "."]);
    for (anchor, content) in [
        ("Widget.run", "Run is idempotent"),
        ("Widget", "Widget is a unit struct"),
    ] {
        let out = save(anchor, content);
        assert!(out.contains(&format!("  Anchor: {anchor}\n")), "{out}");
        assert!(!out.contains("Warning"), "{anchor}: {out}");
    }
    let recalled = ok(d.path(), Some(T0), &["memory", "recall", "idempotent"]);
    assert!(
        recalled.contains("Grounding: CONFIRMED (Widget.run found in pack-"),
        "{recalled}"
    );

    let typo = save("Widget.walk", "Walk is a typo");
    assert!(
        typo.contains(
            "  Warning: Widget.walk is not a file in this repo or a symbol in \
             the latest scan\n"
        ),
        "{typo}"
    );
    // A path still gets the file wording.
    let gone = save("src/gone.rs", "Gone file note");
    assert!(
        gone.contains("  Warning: src/gone.rs is not a file in this repo\n"),
        "{gone}"
    );
    for out in [&early, &typo, &gone] {
        for line in out.lines() {
            assert!(line.chars().count() <= 80, "{line:?}");
        }
    }
}

fn state_with_tz(cwd: &Path, tz: &str) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_repolens"))
        .current_dir(cwd)
        .arg("state")
        .env("TZ", tz)
        .env_remove("REPOLENS_CLOCK_OVERRIDE")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn state_last_updated_shows_local_offset_and_utc() {
    let d = repo();
    ok(
        d.path(),
        Some(T0),
        &["memory", "save", "Timestamps are readable"],
    );
    // Pin updatedAt so the expected strings are fixed.
    let mem = d.path().join(".repolens/memory/memory.json");
    let mut graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&mem).unwrap()).unwrap();
    graph["updatedAt"] = serde_json::Value::String(T30.to_string());
    fs::write(&mem, serde_json::to_string(&graph).unwrap()).unwrap();
    // POSIX TZ strings need no tz database: "CST-8" is UTC+8, "PST8" is UTC-8.
    let east = state_with_tz(d.path(), "CST-8");
    assert!(
        east.contains("  Last updated: 2026-01-31 08:00:00 +08:00 (2026-01-31 00:00:00 UTC)\n"),
        "{east}"
    );
    let west = state_with_tz(d.path(), "PST8");
    assert!(
        west.contains("  Last updated: 2026-01-30 16:00:00 -08:00 (2026-01-31 00:00:00 UTC)\n"),
        "{west}"
    );
    let utc = state_with_tz(d.path(), "UTC0");
    assert!(
        utc.contains("  Last updated: 2026-01-31 00:00:00 UTC\n"),
        "{utc}"
    );
    assert!(!utc.contains("+00:00:00"), "{utc}");
}

#[test]
fn duplicate_and_bad_input() {
    let d = repo();
    ok(d.path(), Some(T0), &["memory", "save", "same text"]);
    let out = ok(d.path(), Some(T0), &["memory", "save", "same text"]);
    assert!(out.starts_with("Already saved as memory "), "{out}");

    let o = rl(
        d.path(),
        Some(T0),
        &["memory", "save", "--importance", "1.5", "x"],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("importance must be between 0.0 and 1.0"));
    let o = rl(d.path(), Some(T0), &["memory", "save", "  "]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("content must not be empty"));

    let out = ok(
        d.path(),
        Some(T0),
        &["memory", "recall", "nothing matches this"],
    );
    assert_eq!(
        out.trim(),
        "No saved memory matches \"nothing matches this\"."
    );
}

#[test]
fn type_filter_and_limit() {
    let d = repo();
    ok(
        d.path(),
        Some(T0),
        &["memory", "save", "--type", "decision", "alpha decision"],
    );
    ok(
        d.path(),
        Some(T0),
        &["memory", "save", "--type", "pattern", "alpha pattern"],
    );
    let out = ok(
        d.path(),
        Some(T0),
        &["memory", "recall", "--type", "pattern", "alpha"],
    );
    assert!(
        out.contains("alpha pattern") && !out.contains("alpha decision"),
        "{out}"
    );
    let out = ok(
        d.path(),
        Some(T0),
        &["memory", "recall", "--limit", "1", "alpha"],
    );
    assert!(out.starts_with("1 of 2 memories match"), "{out}");
}

/// A memory.json this version cannot parse: one valid node plus one with a
/// node type it does not know (as a newer version could write).
const UNREADABLE_MEMORY_JSON: &str = r#"{"schemaVersion":"1.0","repoRoot":"x","nodes":[
{"id":"a","content":"keep me","type":"DECISION","importance":0.9,"created":"2026-01-01T00:00:00Z","lastAccessed":"2026-01-01T00:00:00Z","accessCount":0,"source":"EXPLICIT","provenance":"p"},
{"id":"b","content":"from a newer version","type":"GOTCHA","importance":0.5,"created":"2026-01-01T00:00:00Z","lastAccessed":"2026-01-01T00:00:00Z","accessCount":0,"source":"EXPLICIT","provenance":"p"}
],"edges":[],"lastDecayRun":null,"updatedAt":"2026-01-01T00:00:00Z"}"#;

fn write_unreadable_memory(root: &Path) -> std::path::PathBuf {
    let path = root.join(".repolens/memory/memory.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, UNREADABLE_MEMORY_JSON).unwrap();
    path
}

/// An unreadable memory.json used to load as empty and be replaced by the
/// next save or recall, deleting every stored memory.
#[test]
fn save_and_recall_refuse_to_replace_an_unreadable_memory_file() {
    let d = repo();
    let path = write_unreadable_memory(d.path());

    for args in [
        &["memory", "save", "new insight"][..],
        &["memory", "recall", "keep"][..],
    ] {
        let o = rl(d.path(), None, args);
        assert!(!o.status.success(), "{args:?} should fail");
        let stderr = String::from_utf8_lossy(&o.stderr);
        assert!(
            stderr.contains("Cannot parse memory file"),
            "{args:?}: {stderr}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            UNREADABLE_MEMORY_JSON,
            "{args:?} changed memory.json"
        );
    }
}

#[test]
fn wiki_lint_says_memory_refs_are_unavailable_when_memory_file_is_unreadable() {
    let d = repo();
    write_unreadable_memory(d.path());

    let o = rl(d.path(), None, &["wiki", "lint"]);

    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("  Broken memory refs:  unavailable ("),
        "{stdout}"
    );
}

// #56 items 1-3 end to end: "new" no longer finds "news", "used" finds
// "use", and a single stop word ("IT", "who") is searchable, but a query of
// only stop words ("the a of") is not.
#[test]
fn recall_quirks_from_56() {
    let d = repo();
    for note in [
        "news feed is cached for an hour",
        "we use rustls for TLS",
        "IT owns the VPN certificates",
    ] {
        ok(d.path(), Some(T0), &["memory", "save", note]);
    }
    let recall = |q: &str| ok(d.path(), Some(T0), &["memory", "recall", q]);
    assert_eq!(recall("new"), "No saved memory matches \"new\".\n");
    assert!(recall("news").contains("news feed is cached"));
    for q in ["used", "using"] {
        let out = recall(q);
        assert!(out.contains("we use rustls"), "{q}: {out}");
        assert!(!out.contains("news feed"), "{q}: {out}");
    }
    let out = recall("IT");
    assert!(out.contains("IT owns the VPN"), "{out}");
    assert!(!out.contains("rustls"), "{out}");
    let out = recall("who");
    assert_eq!(out, "No saved memory matches \"who\".\n");
    // Several stop words and nothing else still search nothing (#35).
    let out = recall("the a of");
    assert!(
        out.starts_with("No usable search terms in \"the a of\""),
        "{out}"
    );
    // Short plurals still meet their singular.
    ok(
        d.path(),
        Some(T0),
        &["memory", "save", "tags are sorted by name"],
    );
    assert!(recall("tag").contains("tags are sorted"));
}

/// #56 QA F1: -ed/-ing words don't meet unrelated 2-letter words, as on
/// main, while used/using still meet "use" and apis meets "api".
#[test]
fn recall_short_forms_dont_cross_match() {
    let d = repo();
    ok(
        d.path(),
        Some(T0),
        &["memory", "save", "Ping us before the release freeze"],
    );
    let recall = |q: &str| ok(d.path(), Some(T0), &["memory", "recall", q]);
    for q in ["used", "using", "need", "seed", "feed", "bring"] {
        assert_eq!(recall(q), format!("No saved memory matches \"{q}\".\n"));
    }
    for note in ["we use rustls for TLS", "the public api is frozen"] {
        ok(d.path(), Some(T0), &["memory", "save", note]);
    }
    for q in ["used", "using"] {
        let out = recall(q);
        assert!(out.contains("we use rustls"), "{q}: {out}");
        assert!(!out.contains("Ping us"), "{q}: {out}");
    }
    assert!(recall("apis").contains("public api is frozen"));
}
