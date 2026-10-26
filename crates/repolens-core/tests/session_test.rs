use repolens_core::model::session::RepoSession;
use repolens_core::session::SessionStore;
use time::OffsetDateTime;

fn make_session(root: &std::path::Path) -> RepoSession {
    let now = OffsetDateTime::now_utc();
    RepoSession {
        schema_version: "1.0".to_string(),
        session_id: "test-session-id".to_string(),
        repo_root: root.to_string_lossy().to_string(),
        active_pack_path: root
            .join(".repolens/pack-test")
            .to_string_lossy()
            .to_string(),
        active_report_path: None,
        updated_at: now,
        last_accessed: now,
        last_scan_mode: "full".to_string(),
        reprocessed_files: Some(10),
        deleted_files: None,
        carried_forward_files: None,
    }
}

#[test]
fn save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let session = make_session(root);
    SessionStore::save(root, &session).unwrap();

    let loaded = SessionStore::load(root).unwrap();
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.session_id, session.session_id);
    assert_eq!(loaded.repo_root, session.repo_root);
    assert_eq!(loaded.last_scan_mode, "full");
    assert_eq!(loaded.reprocessed_files, Some(10));
}

#[test]
fn load_returns_none_when_no_session() {
    let tmp = tempfile::tempdir().unwrap();
    let loaded = SessionStore::load(tmp.path()).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn save_creates_storage_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let session = make_session(root);
    SessionStore::save(root, &session).unwrap();

    assert!(root.join(".repolens").is_dir());
    assert!(root.join(".repolens/session.json").is_file());
}

#[test]
fn load_corrupt_session_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join(".repolens")).unwrap();
    std::fs::write(root.join(".repolens/session.json"), "not valid json").unwrap();

    let loaded = SessionStore::load(root).unwrap();
    assert!(loaded.is_none(), "corrupt session should return None");
}

#[test]
fn session_json_is_pretty_printed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let session = make_session(root);
    SessionStore::save(root, &session).unwrap();

    let text = std::fs::read_to_string(root.join(".repolens/session.json")).unwrap();
    assert!(text.contains('\n'), "session.json should be pretty-printed");
}
