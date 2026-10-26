use repolens_app::cleanup::{cleanup_old_packs, DEFAULT_KEEP_COUNT};
use repolens_core::model::session::RepoSession;
use repolens_core::session::SessionStore;
use std::fs;
use time::OffsetDateTime;

fn create_pack(root: &std::path::Path, name: &str) {
    let pack_dir = root.join(".repolens").join(name);
    fs::create_dir_all(&pack_dir).unwrap();
    fs::write(
        pack_dir.join("manifest.json"),
        r#"{"schemaVersion":"1.3.0","chunks":[]}"#,
    )
    .unwrap();
}

/// A pack whose manifest records a complete scan of a clean worktree,
/// i.e. one an incremental scan can use as its base.
fn create_clean_pack(root: &std::path::Path, name: &str) {
    let pack_dir = root.join(".repolens").join(name);
    fs::create_dir_all(&pack_dir).unwrap();
    fs::write(
        pack_dir.join("manifest.json"),
        r#"{"schemaVersion":"1.3.0","schemaUrl":"","timestamp":0,"chunks":[],"scanScope":"complete","worktreeDirty":false}"#,
    )
    .unwrap();
}

fn create_session(root: &std::path::Path, active_pack_name: &str) {
    let pack_path = root.join(".repolens").join(active_pack_name);
    let session = RepoSession {
        schema_version: "1.0".to_string(),
        session_id: "test-session".to_string(),
        repo_root: root.to_string_lossy().to_string(),
        active_pack_path: pack_path.to_string_lossy().to_string(),
        active_report_path: None,
        updated_at: OffsetDateTime::now_utc(),
        last_accessed: OffsetDateTime::now_utc(),
        last_scan_mode: "full".to_string(),
        reprocessed_files: Some(0),
        deleted_files: None,
        carried_forward_files: None,
    };
    SessionStore::save(root, &session).unwrap();
}

#[test]
fn no_storage_dir_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let result = cleanup_old_packs(dir.path(), 3).unwrap();
    assert!(result.deleted.is_empty());
    assert!(result.failed.is_empty());
    assert_eq!(result.kept, 0);
}

#[test]
fn fewer_packs_than_keep_deletes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    create_pack(dir.path(), "pack-20260101-100000");
    create_pack(dir.path(), "pack-20260102-100000");

    let result = cleanup_old_packs(dir.path(), 3).unwrap();
    assert!(result.deleted.is_empty());
    assert_eq!(result.kept, 2);
}

#[test]
fn deletes_oldest_keeps_newest() {
    let dir = tempfile::tempdir().unwrap();
    create_pack(dir.path(), "pack-20260101-100000");
    create_pack(dir.path(), "pack-20260102-100000");
    create_pack(dir.path(), "pack-20260103-100000");
    create_pack(dir.path(), "pack-20260104-100000");
    create_pack(dir.path(), "pack-20260105-100000");

    let result = cleanup_old_packs(dir.path(), 3).unwrap();
    assert_eq!(result.deleted.len(), 2);
    assert_eq!(result.kept, 3);
    assert!(result.deleted.contains(&"pack-20260101-100000".to_string()));
    assert!(result.deleted.contains(&"pack-20260102-100000".to_string()));
    assert!(dir.path().join(".repolens/pack-20260103-100000").exists());
    assert!(dir.path().join(".repolens/pack-20260104-100000").exists());
    assert!(dir.path().join(".repolens/pack-20260105-100000").exists());
}

#[test]
fn active_pack_is_protected() {
    let dir = tempfile::tempdir().unwrap();
    create_pack(dir.path(), "pack-20260101-100000");
    create_pack(dir.path(), "pack-20260102-100000");
    create_pack(dir.path(), "pack-20260103-100000");
    create_pack(dir.path(), "pack-20260104-100000");
    create_session(dir.path(), "pack-20260101-100000");

    let result = cleanup_old_packs(dir.path(), 2).unwrap();
    assert_eq!(result.deleted.len(), 1);
    assert!(result.deleted.contains(&"pack-20260102-100000".to_string()));
    assert_eq!(result.kept, 3);
    assert!(dir.path().join(".repolens/pack-20260101-100000").exists());
}

#[test]
fn newest_clean_pack_is_protected() {
    // #37: an older clean pack followed by more dirty packs than `keep`.
    let dir = tempfile::tempdir().unwrap();
    create_clean_pack(dir.path(), "pack-20260101-100000");
    create_clean_pack(dir.path(), "pack-20260102-100000");
    create_pack(dir.path(), "pack-20260103-100000");
    create_pack(dir.path(), "pack-20260104-100000");
    create_pack(dir.path(), "pack-20260105-100000");
    create_pack(dir.path(), "pack-20260106-100000");

    let result = cleanup_old_packs(dir.path(), 3).unwrap();
    assert_eq!(
        result.deleted,
        vec![
            "pack-20260103-100000".to_string(),
            "pack-20260101-100000".to_string()
        ]
    );
    assert_eq!(result.kept, 4);
    assert!(dir.path().join(".repolens/pack-20260102-100000").exists());
}

#[test]
fn missing_session_falls_back_to_newest_n() {
    let dir = tempfile::tempdir().unwrap();
    create_pack(dir.path(), "pack-20260101-100000");
    create_pack(dir.path(), "pack-20260102-100000");
    create_pack(dir.path(), "pack-20260103-100000");

    let result = cleanup_old_packs(dir.path(), 2).unwrap();
    assert_eq!(result.deleted.len(), 1);
    assert!(result.deleted.contains(&"pack-20260101-100000".to_string()));
    assert_eq!(result.kept, 2);
}

#[test]
fn non_pack_dirs_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    create_pack(dir.path(), "pack-20260101-100000");
    create_pack(dir.path(), "pack-20260102-100000");
    fs::create_dir_all(dir.path().join(".repolens/memory")).unwrap();

    let result = cleanup_old_packs(dir.path(), 1).unwrap();
    assert_eq!(result.deleted.len(), 1);
    assert_eq!(result.kept, 1);
    assert!(dir.path().join(".repolens/memory").exists());
}

#[test]
fn default_keep_count_is_three() {
    assert_eq!(DEFAULT_KEEP_COUNT, 3);
}
