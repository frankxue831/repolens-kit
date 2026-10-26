use repolens_core::discovery::FileDiscovery;
use repolens_core::metadata;
use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::session::RepoSession;
use repolens_core::pack_reader::PackReader;
use repolens_core::pack_writer::{PackWriteOptions, PackWriter};
use repolens_core::session::SessionStore;
use std::path::PathBuf;
use time::OffsetDateTime;

fn fixture_inputs() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("inputs");
    p
}

/// Full round-trip: discover → metadata → write pack → read pack → verify.
#[test]
fn java_small_roundtrip() {
    let root = fixture_inputs().join("java-small");
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");

    // 1. Discover files
    let records = FileDiscovery::new(&root).discover().unwrap();
    assert!(!records.is_empty());

    // 2. Build FileEntry list with metadata
    let entries: Vec<FileEntry> = records
        .iter()
        .map(|r| {
            if r.included {
                let sha = metadata::sha256_hex(&r.absolute_path).unwrap_or_default();
                let loc = metadata::count_lines(&r.absolute_path).unwrap_or(0);
                FileEntry {
                    path: r.relative_path.clone(),
                    sha256: sha,
                    language: String::new(), // language detection is Phase 3
                    loc,
                    included: true,
                    reason: "included".to_string(),
                    content_path: Some(format!("files/{}", r.relative_path)),
                    symbols: vec![],
                }
            } else {
                FileEntry {
                    path: r.relative_path.clone(),
                    sha256: String::new(),
                    language: String::new(),
                    loc: 0,
                    included: false,
                    reason: r.reason.clone(),
                    content_path: None,
                    symbols: vec![],
                }
            }
        })
        .collect();

    // 3. Write pack
    let opts = PackWriteOptions {
        scan_mode: "full",
        head_commit: "",
        worktree_dirty: false,
        scan_config_hash: "",
        base_pack_id: None,
        base_head_commit: None,
        reprocessed_files: entries.len() as i32,
        deleted_files: None,
        carried_forward_files: None,
    };
    PackWriter::new(&pack_dir, &root)
        .write(&entries, &opts)
        .unwrap();

    // 4. Read it back
    let index = PackReader::read(&pack_dir).unwrap();
    assert_eq!(index.manifest.schema_version, "1.3.0");
    assert_eq!(index.manifest.scan_mode.as_deref(), Some("full"));
    assert_eq!(index.entries.len(), entries.len());

    // 5. Verify included entries have valid metadata
    let included: Vec<_> = index.entries.iter().filter(|e| e.included).collect();
    for e in &included {
        assert!(
            !e.sha256.is_empty(),
            "included entry {} should have sha256",
            e.path
        );
        assert!(
            e.loc > 0 || e.path.ends_with(".xml"),
            "included entry {} should have LOC",
            e.path
        );
        assert!(
            e.content_path.is_some(),
            "included entry {} should have content_path",
            e.path
        );

        let content_file = pack_dir.join(e.content_path.as_ref().unwrap());
        assert!(
            content_file.exists(),
            "content file should exist: {}",
            content_file.display()
        );
    }
}

/// End-to-end: discover + write pack + save session for excluded-small.
#[test]
fn excluded_small_roundtrip() {
    let root = fixture_inputs().join("excluded-small");
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let session_root = tmp.path().join("repo");
    std::fs::create_dir(&session_root).unwrap();

    // Discover
    let records = FileDiscovery::new(&root).discover().unwrap();

    let included_count = records.iter().filter(|r| r.included).count();
    let excluded_count = records.iter().filter(|r| !r.included).count();
    assert!(included_count > 0, "should have included files");
    assert!(excluded_count > 0, "should have excluded files");

    // Build entries
    let entries: Vec<FileEntry> = records
        .iter()
        .map(|r| {
            if r.included {
                FileEntry {
                    path: r.relative_path.clone(),
                    sha256: metadata::sha256_hex(&r.absolute_path).unwrap_or_default(),
                    language: String::new(),
                    loc: metadata::count_lines(&r.absolute_path).unwrap_or(0),
                    included: true,
                    reason: "included".to_string(),
                    content_path: Some(format!("files/{}", r.relative_path)),
                    symbols: vec![],
                }
            } else {
                FileEntry {
                    path: r.relative_path.clone(),
                    sha256: String::new(),
                    language: String::new(),
                    loc: 0,
                    included: false,
                    reason: r.reason.clone(),
                    content_path: None,
                    symbols: vec![],
                }
            }
        })
        .collect();

    // Write pack
    let opts = PackWriteOptions {
        scan_mode: "full",
        head_commit: "",
        worktree_dirty: false,
        scan_config_hash: "",
        base_pack_id: None,
        base_head_commit: None,
        reprocessed_files: entries.len() as i32,
        deleted_files: None,
        carried_forward_files: None,
    };
    PackWriter::new(&pack_dir, &root)
        .write(&entries, &opts)
        .unwrap();

    // Save session
    let now = OffsetDateTime::now_utc();
    let session = RepoSession {
        schema_version: "1.0".to_string(),
        session_id: "test-session-id".to_string(),
        repo_root: session_root.to_string_lossy().to_string(),
        active_pack_path: pack_dir.to_string_lossy().to_string(),
        active_report_path: None,
        updated_at: now,
        last_accessed: now,
        last_scan_mode: "full".to_string(),
        reprocessed_files: Some(entries.len() as i32),
        deleted_files: None,
        carried_forward_files: None,
    };
    SessionStore::save(&session_root, &session).unwrap();

    // Verify session loads back
    let loaded = SessionStore::load(&session_root).unwrap().unwrap();
    assert_eq!(loaded.session_id, "test-session-id");

    // Read pack back and verify
    let index = PackReader::read(&pack_dir).unwrap();
    let read_included = index.entries.iter().filter(|e| e.included).count();
    let read_excluded = index.entries.iter().filter(|e| !e.included).count();
    assert_eq!(read_included, included_count);
    assert_eq!(read_excluded, excluded_count);
}
