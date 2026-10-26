use repolens_core::pack_reader::PackReader;
use std::fs;
use std::path::PathBuf;

fn scenarios_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("scenarios");
    p
}

#[test]
fn reads_java_small_pack() {
    let pack_dir = scenarios_dir().join("java-small").join("pack");
    let index = PackReader::read(&pack_dir).unwrap();
    assert_eq!(index.manifest.schema_version, "1.3.0");
    assert_eq!(index.manifest.chunks, vec!["files-0001.json"]);
    assert!(!index.entries.is_empty());
}

#[test]
fn reads_all_entries_from_chunks() {
    let pack_dir = scenarios_dir().join("java-small").join("pack");
    let index = PackReader::read(&pack_dir).unwrap();
    let included = index.entries.iter().filter(|e| e.included).count();
    let excluded = index.entries.iter().filter(|e| !e.included).count();
    assert!(
        included >= 4,
        "expected at least 4 included entries, got {included}"
    );
    assert!(
        excluded > 0,
        "expected excluded (.git) entries, got {excluded}"
    );
}

#[test]
fn reads_mixed_small_pack() {
    let pack_dir = scenarios_dir().join("mixed-small").join("pack");
    let index = PackReader::read(&pack_dir).unwrap();
    assert_eq!(index.manifest.schema_version, "1.3.0");
    assert!(!index.entries.is_empty());
}

#[test]
fn reads_excluded_small_pack() {
    let pack_dir = scenarios_dir().join("excluded-small").join("pack");
    let index = PackReader::read(&pack_dir).unwrap();
    let included: Vec<_> = index.entries.iter().filter(|e| e.included).collect();
    let excluded: Vec<_> = index.entries.iter().filter(|e| !e.included).collect();
    assert!(!included.is_empty());
    assert!(!excluded.is_empty());
    let gitignore_excluded = excluded.iter().any(|e| e.reason == "excluded:gitignore");
    assert!(gitignore_excluded, "expected excluded:gitignore entries");
}

#[test]
fn content_path_resolves_to_existing_file() {
    let pack_dir = scenarios_dir().join("java-small").join("pack");
    let index = PackReader::read(&pack_dir).unwrap();
    for entry in index.entries.iter().filter(|e| e.content_path.is_some()) {
        let content_file = pack_dir.join(entry.content_path.as_ref().unwrap());
        assert!(
            content_file.exists(),
            "content file should exist: {}",
            content_file.display()
        );
    }
}

#[test]
fn nonexistent_pack_dir_returns_err() {
    let result = PackReader::read(&PathBuf::from("/tmp/nonexistent-pack-dir"));
    assert!(result.is_err());
}

#[test]
fn rejects_entry_with_escaping_content_path() {
    let dir = tempfile::tempdir().unwrap();
    let pack_dir = dir.path();

    fs::write(
        pack_dir.join("manifest.json"),
        r#"{
  "schemaVersion":"1.3.0",
  "schemaUrl":"",
  "timestamp":0,
  "chunks":["files-0001.json"]
}"#,
    )
    .unwrap();
    fs::write(
        pack_dir.join("files-0001.json"),
        r#"[
  {
    "path":"src/app.rs",
    "sha256":"abc",
    "language":"Rust",
    "loc":1,
    "included":true,
    "reason":"included",
    "contentPath":"../outside.rs",
    "symbols":[]
  }
]"#,
    )
    .unwrap();

    let err = PackReader::read(pack_dir).unwrap_err();
    assert!(
        err.to_string().contains("content path"),
        "unexpected error: {err}"
    );
}
