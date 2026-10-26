use repolens_core::storage::RepoStoragePaths;
use std::path::Path;

#[test]
fn storage_dir_is_dot_repolens() {
    let root = Path::new("/tmp/my-repo");
    let dir = RepoStoragePaths::storage_dir(root);
    assert_eq!(dir, root.join(".repolens"));
}

#[test]
fn config_path_is_under_storage_dir() {
    let root = Path::new("/tmp/my-repo");
    let path = RepoStoragePaths::config_path(root);
    assert_eq!(path, root.join(".repolens").join("config.json"));
}

#[test]
fn session_path_is_under_storage_dir() {
    let root = Path::new("/tmp/my-repo");
    let path = RepoStoragePaths::session_path(root);
    assert_eq!(path, root.join(".repolens").join("session.json"));
}

#[test]
fn pack_dir_contains_timestamp() {
    let root = Path::new("/tmp/my-repo");
    let ts = time::OffsetDateTime::from_unix_timestamp(1711720000).unwrap();
    let dir = RepoStoragePaths::pack_dir(root, ts);
    let name = dir.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("pack-"), "got: {name}");
    assert_eq!(name.len(), "pack-YYYYMMDD-HHMMSS".len(), "got: {name}");
}

#[test]
fn repo_root_from_pack_dir_round_trips_pack_dir() {
    let root = Path::new("/tmp/my-repo");
    let ts = time::OffsetDateTime::from_unix_timestamp(1711720000).unwrap();
    let pack_dir = RepoStoragePaths::pack_dir(root, ts);
    assert_eq!(
        RepoStoragePaths::repo_root_from_pack_dir(&pack_dir),
        Some(root)
    );
}

#[test]
fn repo_root_from_pack_dir_returns_none_for_unrelated_layout() {
    assert_eq!(
        RepoStoragePaths::repo_root_from_pack_dir(Path::new("/tmp/some-bare-dir")),
        None
    );
    assert_eq!(
        RepoStoragePaths::repo_root_from_pack_dir(Path::new("/tmp/my-repo/other/pack-x")),
        None
    );
}

#[test]
fn is_git_repo_detects_git_directory() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    assert!(RepoStoragePaths::is_git_repo(tmp.path()));
}

#[test]
fn is_git_repo_detects_git_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".git"), "gitdir: /some/path").unwrap();
    assert!(RepoStoragePaths::is_git_repo(tmp.path()));
}

#[test]
fn is_git_repo_returns_false_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!RepoStoragePaths::is_git_repo(tmp.path()));
}
