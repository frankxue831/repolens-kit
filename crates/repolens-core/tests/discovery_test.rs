use repolens_core::discovery::FileDiscovery;
use std::fs;
use std::path::PathBuf;

fn fixture_inputs() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("inputs");
    p
}

#[test]
fn discovers_java_small_files() {
    let root = fixture_inputs().join("java-small");
    let records = FileDiscovery::new(&root).discover().unwrap();
    let included: Vec<_> = records.iter().filter(|r| r.included).collect();
    assert!(
        included.len() >= 4,
        "expected at least 4 included, got {}",
        included.len()
    );
    for r in &included {
        assert!(
            !r.relative_path.contains('\\'),
            "path should use forward slashes: {}",
            r.relative_path
        );
    }
}

#[test]
fn discovers_excluded_small_with_gitignore() {
    let root = fixture_inputs().join("excluded-small");
    let records = FileDiscovery::new(&root).discover().unwrap();

    let included: Vec<_> = records.iter().filter(|r| r.included).collect();
    let excluded: Vec<_> = records.iter().filter(|r| !r.included).collect();

    assert!(!included.is_empty(), "should have included files");
    assert!(!excluded.is_empty(), "should have excluded files");

    let env_excluded = excluded.iter().any(|r| r.relative_path.ends_with(".env"));
    assert!(env_excluded, "*.env files should be excluded");

    let py_included = included.iter().any(|r| r.relative_path == "src/main.py");
    assert!(py_included, "src/main.py should be included");
}

#[test]
#[ignore] // TODO: fixture java-small does not contain .git entries needed by this test
fn records_dot_git_entries_as_excluded_git() {
    let root = fixture_inputs().join("java-small");
    let records = FileDiscovery::new(&root).discover().unwrap();
    let git_files: Vec<_> = records
        .iter()
        .filter(|r| r.relative_path.starts_with(".git/") || r.relative_path == ".git")
        .collect();
    assert!(
        !git_files.is_empty(),
        "fixture parity requires .git entries"
    );
    assert!(git_files
        .iter()
        .all(|r| !r.included && r.reason == "excluded:git"));
}

#[test]
fn results_sorted_by_relative_path() {
    let root = fixture_inputs().join("java-small");
    let records = FileDiscovery::new(&root).discover().unwrap();
    let paths: Vec<&str> = records.iter().map(|r| r.relative_path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "records should be sorted by relative path");
}

#[test]
fn excluded_entries_have_exclusion_reason() {
    let root = fixture_inputs().join("excluded-small");
    let records = FileDiscovery::new(&root).discover().unwrap();
    for r in records.iter().filter(|r| !r.included) {
        assert!(
            r.reason.starts_with("excluded:"),
            "bad reason: {}",
            r.reason
        );
    }
}

#[cfg(unix)]
#[test]
fn follow_symlinks_excludes_targets_outside_repo_root() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().join("repo");
    let outside_root = dir.path().join("outside");
    fs::create_dir_all(repo_root.join("src")).unwrap();
    fs::create_dir_all(&outside_root).unwrap();
    fs::write(outside_root.join("secret.txt"), "secret").unwrap();
    symlink(&outside_root, repo_root.join("src/external")).unwrap();

    let records = FileDiscovery::new(&repo_root)
        .follow_symlinks(true)
        .discover()
        .unwrap();

    assert!(
        records
            .iter()
            .all(|r| r.relative_path != "src/external/secret.txt"),
        "discovery must not traverse into symlink targets outside the repo root"
    );
}
