//! Multi-language byte-identity golden for live scan output.
//!
//! Scans each fixture with the real `ScanService` and snapshots the
//! canonicalized pack *content* (files-*.json, graph.json, graph-metrics.json,
//! project-profile.json — manifest.json excluded). Catches multi-language
//! pack-byte drift in `cargo test --workspace`, hence in CI, with no workflow
//! change. The local on-demand equivalent is `scripts/golden-pack-diff.sh`.
//!
//! Spec: `docs/superpowers/specs/2026-06-07-byte-identity-ci-net-design.md`.

use std::fs;
use std::path::{Path, PathBuf};

use repolens_app::scan::ScanService;
use repolens_core::metadata;
use repolens_core::model::file_entry::FileEntry;
use tempfile::TempDir;

/// `fixtures/inputs/` resolved from this crate's manifest dir.
/// `CARGO_MANIFEST_DIR` = `<workspace>/crates/repolens-app`; pop twice, descend.
fn fixture_inputs_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // workspace root
    p.push("fixtures");
    p.push("inputs");
    p
}

/// Recursively copy `src` into `dst`, skipping any `.repolens/` directory.
fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap_or_else(|e| panic!("mkdir {}: {e}", dst.display()));
    for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("readdir {}: {e}", src.display())) {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".repolens" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap_or_else(|e| panic!("copy {}: {e}", from.display()));
        }
    }
}

/// Copy `fixtures/inputs/<fixture>` into a NAMED child `<temp>/<fixture>` so the
/// scanned root's basename is deterministic. The project profile derives its
/// `name` from the root basename for workspace-only Cargo / Maven, so scanning a
/// random tempdir basename directly would bake a non-portable `name` into
/// `project-profile.json`. Returns the temp guard (keep alive) and the child to scan.
fn copy_fixture_into_named_child(fixture: &str) -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let child = tmp.path().join(fixture);
    let src = fixture_inputs_dir().join(fixture);
    assert!(src.is_dir(), "fixture not found: {}", src.display());
    copy_dir_recursive(&src, &child);
    (tmp, child)
}

/// Load every `files-*.json` chunk (sorted by name) and flatten into one Vec.
fn load_all_entries(pack_dir: &Path) -> Vec<FileEntry> {
    let mut chunks: Vec<PathBuf> = fs::read_dir(pack_dir)
        .unwrap_or_else(|e| panic!("readdir {}: {e}", pack_dir.display()))
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name();
            let s = name.to_string_lossy();
            (s.starts_with("files-") && s.ends_with(".json")).then(|| e.path())
        })
        .collect();
    chunks.sort();
    chunks
        .iter()
        .flat_map(|p| {
            let json =
                fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            serde_json::from_str::<Vec<FileEntry>>(&json)
                .unwrap_or_else(|e| panic!("parse {}: {e}", p.display()))
        })
        .collect()
}

/// For every included entry, hash `pack_dir/<contentPath>` and assert it equals
/// the recorded `sha256`. Covers index<->blob integrity for THIS freshly-scanned
/// pack (the scenario invariant only covers captured packs). Blobs stay out of
/// the snapshot.
fn assert_sha256_parity(pack_dir: &Path) {
    for entry in load_all_entries(pack_dir) {
        if !entry.included {
            continue;
        }
        let cp = entry
            .content_path
            .as_deref()
            .unwrap_or_else(|| panic!("included entry {:?} has no contentPath", entry.path));
        let blob = pack_dir.join(cp);
        let actual =
            metadata::sha256_hex(&blob).unwrap_or_else(|e| panic!("hash {}: {e}", blob.display()));
        assert_eq!(
            actual, entry.sha256,
            "sha256 mismatch for {:?}: index={}, on-disk={}",
            entry.path, entry.sha256, actual
        );
    }
}

/// Replace environment-specific tokens. The canonical (`/private/var/...`) form
/// is replaced BEFORE the raw (`/var/...`) form: on macOS the canonical path
/// contains the raw path as a substring, so raw-first would corrupt it.
fn canonicalize(text: &str, canon_root: &str, raw_root: &str, pack_id: &str) -> String {
    text.replace(canon_root, "__REPO_ROOT__")
        .replace(raw_root, "__REPO_ROOT__")
        .replace(pack_id, "pack-CANONICAL")
}

/// Read every top-level `*.json` in `pack_dir` except `manifest.json`, sorted by
/// name, canonicalize each, and concatenate under per-file headers. The raw
/// `files/` blobs live in a subdirectory and are intentionally not included.
fn render_and_canonicalize(
    pack_dir: &Path,
    canon_root: &str,
    raw_root: &str,
    pack_id: &str,
) -> String {
    let mut files: Vec<PathBuf> = fs::read_dir(pack_dir)
        .unwrap_or_else(|e| panic!("readdir {}: {e}", pack_dir.display()))
        .filter_map(|e| {
            let e = e.ok()?;
            if !e.file_type().ok()?.is_file() {
                return None;
            }
            let p = e.path();
            let name = p.file_name()?.to_string_lossy().into_owned();
            (name.ends_with(".json") && name != "manifest.json").then_some(p)
        })
        .collect();
    files.sort();

    let mut out = String::new();
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        let text = fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        out.push_str(&format!("===== {name} =====\n"));
        out.push_str(&canonicalize(&text, canon_root, raw_root, pack_id));
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// One scan: copy → scan the named child → assert sha256 parity → render +
/// canonicalize → assert no path leak → return the document.
fn scan_once(fixture: &str) -> String {
    let (tmp, child) = copy_fixture_into_named_child(fixture);
    let result = ScanService::new()
        .run(&child, true, 1)
        .unwrap_or_else(|e| panic!("{fixture}: scan failed: {e}"));
    let pack_dir = result.pack_dir;

    assert_sha256_parity(&pack_dir);

    let pack_id = pack_dir.file_name().unwrap().to_string_lossy().into_owned();
    let raw_root = child.to_string_lossy().into_owned();
    let canon_root = fs::canonicalize(&child)
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", child.display()))
        .to_string_lossy()
        .into_owned();

    let doc = render_and_canonicalize(&pack_dir, &canon_root, &raw_root, &pack_id);

    assert!(
        !doc.contains(&raw_root),
        "{fixture}: raw scan-root path leaked into golden"
    );
    assert!(
        !doc.contains(&canon_root),
        "{fixture}: canonical scan-root path leaked into golden"
    );

    drop(tmp); // temp dir removed now that `doc` owns all bytes
    doc
}

/// Scan twice and require byte-identical canonicalized output before returning
/// the golden. This is the determinism safety net.
fn render_pack_golden(fixture: &str) -> String {
    let a = scan_once(fixture);
    let b = scan_once(fixture);
    assert_eq!(
        a, b,
        "{fixture}: scan output is not deterministic across two runs (after canonicalization)"
    );
    a
}

#[test]
fn mixed_small_pack_bytes() {
    insta::assert_snapshot!("mixed_small_pack_bytes", render_pack_golden("mixed-small"));
}

#[test]
fn rust_small_pack_bytes() {
    insta::assert_snapshot!("rust_small_pack_bytes", render_pack_golden("rust-small"));
}

#[test]
fn java_small_pack_bytes() {
    insta::assert_snapshot!("java_small_pack_bytes", render_pack_golden("java-small"));
}
