use std::fs;
use std::process::Command;

const TEMPLATES: &[&str] = &[
    "repo-overview.md",
    "known-constraints.md",
    "session-memory.md",
    "agent-friction-log.md",
    "current-agent-brief.md",
    "eval-results.md",
];

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

fn run_init(root: &std::path::Path, extra: &[&str]) -> std::process::Output {
    let mut args = vec!["init", "--root", root.to_str().unwrap()];
    args.extend_from_slice(extra);
    Command::new(repolens_bin())
        .args(&args)
        .output()
        .expect("failed to invoke repolens init")
}

#[test]
fn init_creates_all_six_files_in_fresh_dir() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_init(dir.path(), &[]);
    assert!(
        out.status.success(),
        "init failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let storage = dir.path().join(".repolens");
    assert!(storage.exists(), ".repolens/ should exist");
    for name in TEMPLATES {
        let path = storage.join(name);
        assert!(path.exists(), "{} should exist", name);
        let body = fs::read_to_string(&path).unwrap();
        assert!(!body.is_empty(), "{} should not be empty", name);
        // Template headers begin with an HTML comment marker.
        assert!(
            body.starts_with("<!--"),
            "{} should start with template header",
            name
        );
    }
}

#[test]
fn init_prints_success_summary_and_next_steps() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_init(dir.path(), &[]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("RepoLens is set up: 6 files in .repolens/"),
        "missing status line: {stdout}"
    );
    assert!(
        stdout.contains("Next:"),
        "missing 'Next:' guidance line: {stdout}"
    );
    assert!(
        stdout.contains("known-constraints.md"),
        "next-step line should name known-constraints.md"
    );
}

#[test]
fn init_skips_existing_files_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    let preserved = storage.join("repo-overview.md");
    fs::write(&preserved, "USER EDIT — must survive init\n").unwrap();

    let out = run_init(dir.path(), &[]);
    assert!(out.status.success());

    let kept = fs::read_to_string(&preserved).unwrap();
    assert_eq!(
        kept, "USER EDIT — must survive init\n",
        "existing file must be preserved without --force"
    );

    // Other five files should be created
    for name in TEMPLATES.iter().filter(|n| **n != "repo-overview.md") {
        assert!(
            storage.join(name).exists(),
            "{} should be created when not pre-existing",
            name
        );
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Kept your edited file: repo-overview.md"),
        "summary should report the kept file: {stdout}"
    );
    assert!(stdout.contains("(5 new)"), "{stdout}");
}

#[test]
fn init_force_overwrites_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join(".repolens");
    fs::create_dir_all(&storage).unwrap();
    let target = storage.join("repo-overview.md");
    fs::write(&target, "stale content\n").unwrap();

    let out = run_init(dir.path(), &["--force"]);
    assert!(out.status.success());

    let body = fs::read_to_string(&target).unwrap();
    assert_ne!(
        body, "stale content\n",
        "--force should overwrite existing file"
    );
    assert!(
        body.starts_with("<!--"),
        "after --force, file should match template (starts with <!--)"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("--force: backed up 1 edited file to .repolens/backup/"),
        "--force should report the backup first: {stdout}"
    );
}

#[test]
fn init_creates_storage_directory_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!dir.path().join(".repolens").exists());

    let out = run_init(dir.path(), &[]);
    assert!(out.status.success());
    assert!(
        dir.path().join(".repolens").is_dir(),
        ".repolens/ must be created"
    );
}

#[test]
fn init_is_idempotent_on_clean_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let first = run_init(dir.path(), &[]);
    assert!(first.status.success());

    // Capture content after first init
    let snapshots: Vec<(String, String)> = TEMPLATES
        .iter()
        .map(|n| {
            let body = fs::read_to_string(dir.path().join(".repolens").join(n)).unwrap();
            (n.to_string(), body)
        })
        .collect();

    let second = run_init(dir.path(), &[]);
    assert!(second.status.success());

    for (name, body) in snapshots {
        let after = fs::read_to_string(dir.path().join(".repolens").join(&name)).unwrap();
        assert_eq!(after, body, "{name} must be unchanged on idempotent rerun");
    }
}

fn git(root: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?} failed");
}

fn git_out(root: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Committed repo with a tracked root `.gitignore` holding `gitignore`.
fn committed_repo(gitignore: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@t.com"]);
    git(root, &["config", "user.name", "T"]);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join(".gitignore"), gitignore).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    dir
}

fn scan_mode(root: &std::path::Path) -> String {
    // Pack ids have one-second resolution.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let before: std::collections::BTreeSet<_> = packs(root);
    let out = Command::new(repolens_bin())
        .args(["scan", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let new: Vec<_> = packs(root).difference(&before).cloned().collect();
    assert_eq!(new.len(), 1, "{new:?}");
    let manifest =
        fs::read_to_string(root.join(".repolens").join(&new[0]).join("manifest.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    v["scanMode"].as_str().unwrap().to_string()
}

fn packs(root: &std::path::Path) -> std::collections::BTreeSet<String> {
    fs::read_dir(root.join(".repolens"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("pack-"))
                .collect()
        })
        .unwrap_or_default()
}

/// #39: in a git repo init says git ignores `.repolens/`, leaves the root
/// `.gitignore` alone and the tree clean, so scans go incremental at once.
#[test]
fn init_in_git_repo_keeps_tree_clean_and_scans_incremental() {
    let dir = committed_repo("target/\n");
    let root = dir.path();

    let out = run_init(root, &[]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[0], "RepoLens is set up: 6 files in .repolens/",
        "{stdout}"
    );
    assert_eq!(
        lines[1], "Git ignores .repolens/ via its own .gitignore; your files are unchanged.",
        "{stdout}"
    );
    assert_eq!(stdout.matches("git").count(), 1, "one git line: {stdout}");
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        "target/\n"
    );
    assert!(root.join(".repolens/.gitignore").is_file());
    assert_eq!(git_out(root, &["status", "--porcelain"]), "");

    let mut modes = Vec::new();
    for _ in 0..3 {
        modes.push(scan_mode(root));
        assert_eq!(git_out(root, &["status", "--porcelain"]), "", "{modes:?}");
    }
    assert_eq!(modes, ["full", "incremental", "incremental"]);
}

/// A root `.gitignore` that already lists `.repolens/` stays byte-identical.
#[test]
fn init_leaves_existing_repolens_entry_alone() {
    let body = "target/\n.repolens/";
    let dir = committed_repo(body);
    let root = dir.path();
    for _ in 0..2 {
        let out = run_init(root, &[]);
        assert!(out.status.success());
    }
    assert_eq!(fs::read_to_string(root.join(".gitignore")).unwrap(), body);
    assert_eq!(git_out(root, &["status", "--porcelain"]), "");
}

/// Deleting `.repolens/.gitignore` is repaired by the next scan.
#[test]
fn scan_recreates_deleted_self_ignore() {
    let dir = committed_repo("target/\n");
    let root = dir.path();
    assert!(run_init(root, &[]).status.success());
    assert_eq!(scan_mode(root), "full");
    fs::remove_file(root.join(".repolens/.gitignore")).unwrap();
    assert_eq!(scan_mode(root), "incremental");
    assert!(root.join(".repolens/.gitignore").is_file());
    assert_eq!(git_out(root, &["status", "--porcelain"]), "");
    assert_eq!(scan_mode(root), "incremental");
}

/// Outside a git repository init says nothing about git and writes no
/// root `.gitignore`.
#[test]
fn init_outside_git_says_nothing_about_git() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_init(dir.path(), &[]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().next(),
        Some("RepoLens is set up: 6 files in .repolens/")
    );
    assert!(!stdout.to_lowercase().contains("git"), "{stdout}");
    assert!(!dir.path().join(".gitignore").exists());
    assert!(dir.path().join(".repolens/.gitignore").is_file());
}
