use repolens_core::model::file_entry::{FileEntry, Symbol};
use repolens_core::pack_reader::PackReader;
use repolens_core::pack_writer::{PackWriteOptions, PackWriter};
use std::path::PathBuf;

fn fixture_inputs() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("inputs");
    p
}

fn make_entry(path: &str, sha256: &str, language: &str, loc: i32) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        sha256: sha256.to_string(),
        language: language.to_string(),
        loc,
        included: true,
        reason: "included".to_string(),
        content_path: Some(format!("files/{path}")),
        symbols: vec![],
    }
}

fn make_excluded(path: &str, reason: &str) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        sha256: String::new(),
        language: String::new(),
        loc: 0,
        included: false,
        reason: reason.to_string(),
        content_path: None,
        symbols: vec![],
    }
}

fn full_options<'a>(
    head_commit: &'a str,
    worktree_dirty: bool,
    count: i32,
) -> PackWriteOptions<'a> {
    PackWriteOptions {
        scan_mode: "full",
        head_commit,
        worktree_dirty,
        scan_config_hash: "",
        base_pack_id: None,
        base_head_commit: None,
        reprocessed_files: count,
        deleted_files: None,
        carried_forward_files: None,
    }
}

#[test]
fn writes_and_reads_back_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = fixture_inputs().join("java-small");

    let entries = vec![
        make_entry("README.md", "abc123", "Markdown", 8),
        make_excluded("ignored.log", "excluded:gitignore"),
    ];

    let opts = full_options("abcdef1234", false, entries.len() as i32);
    PackWriter::new(&pack_dir, &root)
        .write(&entries, &opts)
        .unwrap();

    let index = PackReader::read(&pack_dir).unwrap();
    assert_eq!(index.manifest.schema_version, "1.3.0");
    assert_eq!(index.manifest.scan_mode.as_deref(), Some("full"));
    assert_eq!(index.entries.len(), 2);
}

#[test]
fn writes_content_files_for_included_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = fixture_inputs().join("java-small");

    let entries = vec![make_entry("README.md", "abc123", "Markdown", 8)];

    let opts = full_options("abcdef", false, entries.len() as i32);
    PackWriter::new(&pack_dir, &root)
        .write(&entries, &opts)
        .unwrap();

    let content = pack_dir.join("files").join("README.md");
    assert!(content.exists(), "content file should be written");
    let content_text = std::fs::read_to_string(&content).unwrap();
    let original = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert_eq!(content_text, original);
}

#[test]
fn chunks_entries_at_configured_size() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = fixture_inputs().join("java-small");

    let mut entries: Vec<FileEntry> = (0..5)
        .map(|i| make_excluded(&format!("file{i}.txt"), "excluded:gitignore"))
        .collect();
    entries.push(make_entry("README.md", "abc", "Markdown", 8));

    let opts = full_options("abc", false, entries.len() as i32);
    PackWriter::new(&pack_dir, &root)
        .chunk_size(3)
        .write(&entries, &opts)
        .unwrap();

    let index = PackReader::read(&pack_dir).unwrap();
    assert_eq!(
        index.manifest.chunks.len(),
        2,
        "6 entries / 3 per chunk = 2 chunks"
    );
    assert_eq!(index.entries.len(), 6);
}

#[test]
fn manifest_json_is_pretty_printed() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = fixture_inputs().join("java-small");

    let entries = vec![make_excluded("a.txt", "excluded:gitignore")];
    let opts = full_options("abc", false, entries.len() as i32);
    PackWriter::new(&pack_dir, &root)
        .write(&entries, &opts)
        .unwrap();

    let manifest_text = std::fs::read_to_string(pack_dir.join("manifest.json")).unwrap();
    assert!(
        manifest_text.contains('\n'),
        "manifest should be pretty-printed"
    );
    assert!(
        manifest_text.contains("  \"schemaVersion\""),
        "should use 2-space indent"
    );
}

#[test]
fn excluded_entries_do_not_get_content_files() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = fixture_inputs().join("java-small");

    let entries = vec![make_excluded("ignored.log", "excluded:gitignore")];

    let opts = full_options("abc", false, entries.len() as i32);
    PackWriter::new(&pack_dir, &root)
        .write(&entries, &opts)
        .unwrap();

    let files_dir = pack_dir.join("files");
    assert!(!files_dir.exists() || std::fs::read_dir(&files_dir).unwrap().count() == 0);
}

#[test]
fn missing_source_file_returns_err() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = tmp.path().join("empty-repo");
    std::fs::create_dir(&root).unwrap();

    // Entry points to a file that doesn't exist on disk
    let entry = make_entry("nonexistent.txt", "abc", "Text", 1);

    let opts = full_options("", false, 1);
    let result = PackWriter::new(&pack_dir, &root).write(&[entry], &opts);
    assert!(result.is_err(), "should fail when source file is missing");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("nonexistent.txt"),
        "error should name the file: {err_msg}"
    );
}

#[test]
fn symbols_survive_write_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let pack_dir = tmp.path().join("pack-test");
    let root = fixture_inputs().join("java-small");

    let entry = FileEntry {
        path: "App.java".to_string(),
        sha256: "abc".to_string(),
        language: "Java".to_string(),
        loc: 20,
        included: true,
        reason: "included".to_string(),
        content_path: None,
        symbols: vec![Symbol {
            symbol_type: "class".to_string(),
            name: "App".to_string(),
            qualified_name: Some("com.example.App".to_string()),
            visibility: Some("public".to_string()),
            line: None,
        }],
    };

    let opts = full_options("abc", false, 1);
    PackWriter::new(&pack_dir, &root)
        .write(&[entry], &opts)
        .unwrap();

    let index = PackReader::read(&pack_dir).unwrap();
    assert_eq!(index.entries[0].symbols.len(), 1);
    assert_eq!(index.entries[0].symbols[0].name, "App");
    assert_eq!(
        index.entries[0].symbols[0].qualified_name.as_deref(),
        Some("com.example.App")
    );
}
