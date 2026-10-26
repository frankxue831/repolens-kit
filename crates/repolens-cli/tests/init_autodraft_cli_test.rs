//! `repolens init` auto-drafts untouched workspace files from the repo.

use std::fs;
use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

const README_PARA: &str = "Widget turns sprockets into gears, quickly and safely.";

fn write_repo(root: &Path) {
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".github/workflows")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2021\"\nrust-version = \"1.80\"\n",
    )
    .unwrap();
    fs::write(
        root.join("README.md"),
        format!("# Widget\n\n[![ci](x)](y)\n\n{README_PARA}\n\n## Usage\n\nmore text\n"),
    )
    .unwrap();
    fs::write(
        root.join(".github/workflows/ci.yml"),
        "name: CI\non: [push]\njobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - name: Install tool\n        run: cargo install cargo-nextest\n      - name: Tests\n        run: cargo test --locked\n      - name: Fmt\n        run: |\n          cargo fmt --all -- --check\n          echo done\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    fs::write(root.join("AGENTS.md"), "# Agents\n").unwrap();
}

fn run(root: &Path, args: &[&str]) -> String {
    let mut all = vec![args[0], "--root", root.to_str().unwrap()];
    all.extend_from_slice(&args[1..]);
    let out = Command::new(bin()).args(&all).output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if args[0] == "init" {
        assert_fits_80(&stdout);
    }
    stdout
}

/// Every `init` output line fits in 80 columns.
fn assert_fits_80(out: &str) {
    for line in out.lines() {
        assert!(line.chars().count() <= 80, "line over 80 columns: {line:?}");
    }
}

fn read(root: &Path, name: &str) -> String {
    fs::read_to_string(root.join(".repolens").join(name)).unwrap()
}

fn template(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/templates/repolens")
        .join(name);
    fs::read_to_string(p).unwrap()
}

/// The "(auto-drafted)" marker is on section headings and the Status
/// field only, not on every line.
fn assert_marked_on_headings(body: &str) {
    let start = body.find("-->").unwrap() + 3;
    for line in body[start..].lines() {
        let t = line.trim_start();
        if t.starts_with("### D-") || t.starts_with("- **Status:**") {
            assert!(line.contains("(auto-drafted)"), "unmarked: {line}");
        } else if t.starts_with("- ") || t.starts_with("> ") {
            assert!(!line.contains("(auto-drafted)"), "marker on a line: {line}");
        }
    }
}

#[test]
fn drafts_come_from_real_repo_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["scan"]);
    let out = run(root, &["init"]);
    assert!(
        out.contains("Drafted for you to check (nothing is enforced yet):"),
        "{out}"
    );
    assert!(
        out.contains("  known-constraints.md  4 draft constraints, D-001 to D-004"),
        "{out}"
    );
    assert!(
        out.contains("  Read from: README.md, Cargo.toml, 1 CI workflow\n"),
        "{out}"
    );

    let kc = read(root, "known-constraints.md");
    assert!(kc.contains("rust-version = \"1.80\""), "{kc}");
    assert!(kc.contains("edition = \"2021\""));
    assert!(kc.contains("`cargo test --locked`"));
    assert!(kc.contains("`.github/workflows/ci.yml:11`"), "{kc}");
    assert!(kc.contains("`cargo fmt --all -- --check`"));
    assert!(kc.contains("`.github/workflows/ci.yml:14`"), "{kc}");
    // Setup commands and non-check lines are not turned into constraints.
    assert!(!kc.contains("cargo install"), "{kc}");
    assert!(!kc.contains("echo done"));
    // Nothing that is not in the repo.
    assert!(!kc.contains("clippy"));
    assert!(!kc.contains("package.json"));
    assert!(
        !kc.contains("### C-"),
        "drafts must not use enforced C- ids"
    );
    assert_marked_on_headings(&kc);
    assert!(kc.contains("## Drafted constraints (auto-drafted)"));
    assert!(
        kc.contains("- **Status:** draft (auto-drafted), not enforced until renamed to `C-NNN`")
    );

    let ov = read(root, "repo-overview.md");
    assert!(ov.contains("`widget`"));
    assert!(ov.contains(&format!("> {README_PARA}")), "{ov}");
    assert!(ov.contains("`src/`"));
    assert!(ov.contains("`AGENTS.md`"));
    assert!(ov.contains("Rust (1)"), "{ov}");
    assert!(!ov.contains("more text"));
    assert_marked_on_headings(&ov);
    assert!(ov.contains("## Description (auto-drafted)"));
    assert!(
        ov.contains(" files have tests (from scan ") || ov.contains(" files has tests (from scan "),
        "{ov}"
    );
    assert!(!ov.contains("test-bearing"));

    // Other workspace files are still the shipped templates.
    assert_eq!(
        read(root, "session-memory.md"),
        template("session-memory.md")
    );

    // validate-plan does not enforce drafts.
    fs::write(root.join("plan.md"), "# Plan\n- change src/lib.rs\n").unwrap();
    let vp = Command::new(bin())
        .args(["validate-plan", "--root", root.to_str().unwrap(), "--file"])
        .arg(root.join("plan.md"))
        .output()
        .unwrap();
    let vp_out =
        String::from_utf8_lossy(&vp.stdout).to_string() + &String::from_utf8_lossy(&vp.stderr);
    assert!(vp_out.contains("no real constraints loaded"), "{vp_out}");
}

#[test]
fn edited_files_are_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["init"]);
    let edited = "# Known Constraints\n\n### C-001 mine\n\n- **Statement:** keep it.\n";
    fs::write(root.join(".repolens/known-constraints.md"), edited).unwrap();
    let before_overview = read(root, "repo-overview.md");
    assert!(before_overview.contains("No scan found") || before_overview.contains("Scan:**"));

    run(root, &["scan"]);
    let out = run(root, &["init"]);
    assert!(
        out.contains("\nKept your edited file: known-constraints.md\n"),
        "{out}"
    );
    assert!(!out.contains("D-001 to"), "{out}");
    assert_eq!(read(root, "known-constraints.md"), edited);
    // The untouched previous draft is refreshed with scan facts.
    let ov = read(root, "repo-overview.md");
    assert!(ov.contains("from scan `pack-"), "{ov}");

    // Once the user edits the overview too, it is left alone.
    let mut ov_edit = ov.clone();
    ov_edit.push_str("my note\n");
    fs::write(root.join(".repolens/repo-overview.md"), &ov_edit).unwrap();
    run(root, &["init"]);
    assert_eq!(read(root, "repo-overview.md"), ov_edit);
}

#[test]
fn no_draft_keeps_blank_templates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["scan"]);
    let out = run(root, &["init", "--no-draft"]);
    assert!(
        out.contains("\nBlank templates created (--no-draft).\n"),
        "{out}"
    );
    assert!(
        out.trim_end()
            .ends_with("Next: fill in .repolens/known-constraints.md."),
        "{out}"
    );
    assert!(!out.contains("Drafted for you"));
    assert_eq!(
        read(root, "known-constraints.md"),
        template("known-constraints.md")
    );
    assert_eq!(read(root, "repo-overview.md"), template("repo-overview.md"));
}

#[test]
fn drafting_is_deterministic() {
    // Same pack: re-drafting yields identical bytes.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["scan"]);
    run(root, &["init"]);
    let kc1 = read(root, "known-constraints.md");
    let ov1 = read(root, "repo-overview.md");
    run(root, &["init", "--force"]);
    assert_eq!(read(root, "known-constraints.md"), kc1);
    assert_eq!(read(root, "repo-overview.md"), ov1);

    // Two identical repos without a scan draft identical files.
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    write_repo(a.path());
    write_repo(b.path());
    run(a.path(), &["init"]);
    run(b.path(), &["init"]);
    for f in ["known-constraints.md", "repo-overview.md"] {
        assert_eq!(read(a.path(), f), read(b.path(), f), "{f}");
    }
}

fn lines(out: &str) -> Vec<&str> {
    out.lines().collect()
}

#[test]
fn screen_without_scan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    let out = run(root, &["init"]);
    let l = lines(&out);
    assert_eq!(l[0], "RepoLens is set up: 6 files in .repolens/");
    assert_eq!(l[1], "");
    assert_eq!(
        l[2],
        "No scan yet. Run `repolens scan` first to see findings."
    );
    assert!(!out.contains("Top findings"));
    assert!(
        !out.contains("session-memory.md"),
        "file list is --verbose only"
    );
    // Exactly one Next sentence, last.
    assert_eq!(out.matches("Next:").count(), 1);
    assert_eq!(
        l[l.len() - 2],
        "Next: open .repolens/known-constraints.md. Rename a draft from D-NNN to C-NNN"
    );
    assert_eq!(
        l[l.len() - 1],
        "once you've checked it; only C-NNN rules are enforced by validate-plan."
    );
}

#[test]
fn screen_after_scan_lists_findings_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    fs::write(root.join(".gitignore"), "target\n").unwrap();
    run(root, &["scan"]);
    let out = run(root, &["init"]);
    let l = lines(&out);
    assert_eq!(l[0], "RepoLens is set up: 6 files in .repolens/");
    let i = l
        .iter()
        .position(|x| x.starts_with("The last scan (pack-"))
        .expect(&out);
    assert_eq!(i, 2, "findings come right after the status line: {out}");
    let rows: Vec<&str> = l
        .iter()
        .copied()
        .filter(|x| {
            x.starts_with("  High  ") || x.starts_with("  Med   ") || x.starts_with("  Low   ")
        })
        .collect();
    assert!(rows.len() <= 3, "{out}");
    let rank = |r: &str| match &r[2..6] {
        "High" => 0,
        "Med " => 1,
        _ => 2,
    };
    let ranks: Vec<i32> = rows.iter().map(|r| rank(r)).collect();
    let mut sorted = ranks.clone();
    sorted.sort();
    assert_eq!(ranks, sorted, "sorted by severity: {out}");
    // The rows are exactly the first rows of `repolens findings`.
    let findings = run(root, &["findings"]);
    for r in &rows {
        assert!(
            findings.contains(&format!("{r}\n")),
            "{r} not in:\n{findings}"
        );
    }
    let drafts = l
        .iter()
        .position(|x| x.starts_with("Drafted for you"))
        .unwrap();
    assert!(drafts > i);
}

#[test]
fn verbose_lists_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    let out = run(root, &["init", "--verbose"]);
    assert!(out.contains("Files:"), "{out}");
    for f in [
        "repo-overview.md",
        "known-constraints.md",
        "session-memory.md",
        "agent-friction-log.md",
        "current-agent-brief.md",
        "eval-results.md",
    ] {
        assert!(out.contains(&format!("  {f}")), "{f}: {out}");
    }
}

fn backups(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(root.join(".repolens/backup"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn force_backs_up_edited_files_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["init"]);

    // Nothing edited: no backup, no extra line.
    let out = run(root, &["init", "--force"]);
    assert!(out.starts_with("RepoLens is set up:"), "{out}");
    assert!(backups(root).is_empty());

    let kc = "# Known Constraints\n\n### C-001 mine\n";
    let sm = "# Session memory\n\nmy notes\n";
    fs::write(root.join(".repolens/known-constraints.md"), kc).unwrap();
    fs::write(root.join(".repolens/session-memory.md"), sm).unwrap();
    let out = run(root, &["init", "--force"]);
    let b = backups(root);
    assert_eq!(b.len(), 1);
    let first = out.lines().next().unwrap();
    assert_eq!(
        first,
        format!(
            "--force: backed up 2 edited files to .repolens/backup/{}/",
            b[0]
        )
    );
    assert!(out
        .lines()
        .nth(1)
        .unwrap()
        .starts_with("RepoLens is set up:"));
    let bdir = root.join(".repolens/backup").join(&b[0]);
    assert_eq!(
        fs::read_to_string(bdir.join("known-constraints.md")).unwrap(),
        kc
    );
    assert_eq!(
        fs::read_to_string(bdir.join("session-memory.md")).unwrap(),
        sm
    );
    // Untouched files are not copied.
    assert!(!bdir.join("eval-results.md").exists());
    // The workspace copy was rewritten.
    assert_ne!(read(root, "known-constraints.md"), kc);
}

#[test]
fn two_force_runs_make_two_backup_folders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["init"]);
    fs::write(root.join(".repolens/session-memory.md"), "first\n").unwrap();
    run(root, &["init", "--force"]);
    fs::write(root.join(".repolens/session-memory.md"), "second\n").unwrap();
    run(root, &["init", "--force"]);
    let b = backups(root);
    assert_eq!(b.len(), 2, "{b:?}");
    let bodies: Vec<String> = b
        .iter()
        .map(|d| {
            fs::read_to_string(
                root.join(".repolens/backup")
                    .join(d)
                    .join("session-memory.md"),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(bodies, vec!["first\n".to_string(), "second\n".to_string()]);
}

#[test]
fn backups_survive_clean_and_are_not_read_by_brief_or_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    run(root, &["scan"]);
    run(root, &["init"]);
    let sentinel = "SENTINEL-IN-BACKUP-7f3a";
    let kc =
        format!("# Known Constraints\n\n### C-001 {sentinel}\n\n- **Statement:** {sentinel}\n");
    fs::write(root.join(".repolens/known-constraints.md"), &kc).unwrap();
    run(root, &["init", "--force"]);
    let b = backups(root);
    assert_eq!(b.len(), 1);

    run(root, &["scan"]);
    run(root, &["scan"]);
    run(root, &["clean", "--keep", "1"]);
    let kept = root
        .join(".repolens/backup")
        .join(&b[0])
        .join("known-constraints.md");
    assert_eq!(fs::read_to_string(&kept).unwrap(), kc);

    for args in [vec!["brief", "--stdout"], vec!["state"]] {
        let out = run(root, &args);
        assert!(!out.contains(sentinel), "{args:?} read the backup: {out}");
        assert!(!out.contains("backup/"), "{args:?} mentions backup/: {out}");
    }
    let mut all = vec!["validate-plan", "--root", root.to_str().unwrap(), "--file"];
    fs::write(root.join("plan.md"), "# Plan\n- change src/lib.rs\n").unwrap();
    let plan = root.join("plan.md");
    all.push(plan.to_str().unwrap());
    let vp = Command::new(bin()).args(&all).output().unwrap();
    let vp_out =
        String::from_utf8_lossy(&vp.stdout).to_string() + &String::from_utf8_lossy(&vp.stderr);
    assert!(!vp_out.contains(sentinel), "{vp_out}");
}

#[test]
fn readme_link_only_first_paragraph_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    fs::write(
        root.join("README.md"),
        "# Widget\n\n**Live:** https://example.com/widget/ (redirects there)\n\nWidget does one thing well.\n",
    )
    .unwrap();
    run(root, &["init"]);
    let ov = read(root, "repo-overview.md");
    assert!(ov.contains("> Widget does one thing well."), "{ov}");
    assert!(!ov.contains("example.com"));

    fs::write(
        root.join("README.md"),
        "# Widget\n\n[Docs](https://example.com)\n",
    )
    .unwrap();
    run(root, &["init"]);
    let ov = read(root, "repo-overview.md");
    assert!(ov.contains("No description found in README.md."), "{ov}");
}
