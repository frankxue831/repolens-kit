//! Evidence for the LOC observation invariants named in
//! `docs/superpowers/specs/2026-04-15-loc-family-catalogue.md`.

use repolens_core::metadata;
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
fn sha256_of_known_file_matches_fixture() {
    let path = fixture_inputs().join("java-small").join("README.md");
    let hash = metadata::sha256_hex(&path).unwrap();
    assert_eq!(
        hash,
        "96f4b4aa866a15d0d81b898ba365335e538140fedcd86468434c34f3064be1da"
    );
}

#[test]
fn sha256_of_java_file_matches_fixture() {
    let path = fixture_inputs()
        .join("java-small")
        .join("src/main/java/com/example/App.java");
    let hash = metadata::sha256_hex(&path).unwrap();
    assert_eq!(
        hash,
        "46d0b56263db3b3ed6fd494683140e5d5f818ec3028ae21ddbf504da43e5bc2c"
    );
}

#[test]
fn sha256_of_nonexistent_file_is_err() {
    let path = fixture_inputs().join("does-not-exist.txt");
    assert!(metadata::sha256_hex(&path).is_err());
}

#[test]
fn loc_counts_all_lines_including_blank() {
    let path = fixture_inputs().join("java-small").join("README.md");
    let count = metadata::count_lines(&path).unwrap();
    assert_eq!(count, 8);
}

#[test]
fn loc_of_java_file_matches_fixture() {
    let path = fixture_inputs()
        .join("java-small")
        .join("src/main/java/com/example/App.java");
    let count = metadata::count_lines(&path).unwrap();
    assert_eq!(count, 20);
}

#[test]
fn loc_of_helper_matches_fixture() {
    let path = fixture_inputs()
        .join("java-small")
        .join("src/main/java/com/example/Helper.java");
    let count = metadata::count_lines(&path).unwrap();
    assert_eq!(count, 11);
}

#[test]
fn loc_counts_final_line_without_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-trailing-newline.txt");
    std::fs::write(&path, "alpha\nbeta").unwrap();
    let count = metadata::count_lines(&path).unwrap();
    assert_eq!(count, 2);
}

#[test]
fn text_file_is_not_binary() {
    let path = fixture_inputs().join("java-small").join("README.md");
    assert!(!metadata::is_binary(&path).unwrap());
}

#[test]
fn binary_file_detected() {
    let path = fixture_inputs()
        .join("excluded-small")
        .join("assets")
        .join("blob.bin");
    assert!(metadata::is_binary(&path).unwrap());
}

#[test]
fn nonexistent_file_binary_check_is_err() {
    let path = fixture_inputs().join("does-not-exist.bin");
    assert!(metadata::is_binary(&path).is_err());
}
