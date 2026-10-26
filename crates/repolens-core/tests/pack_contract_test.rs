use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::manifest::Manifest;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Scenario names reused across all observation-invariant tests.
const SCENARIOS: &[&str] = &["java-small", "mixed-small", "excluded-small"];

/// Local copy of the `is_groundable` check from
/// `crates/repolens-app/src/grounding.rs`. Duplicated here rather than
/// imported to avoid a reverse dep from `repolens-core` onto `repolens-app`.
/// The grandfathered-families invariant catalogue names this lockstep
/// maintenance rule: if the real helper changes, this copy and the dependent
/// invariants must change with it.
fn is_groundable(anchor: &str) -> bool {
    anchor.contains('/') || anchor.contains('.')
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Returns the path to `fixtures/scenarios/`.
///
/// `CARGO_MANIFEST_DIR` points to `crates/repolens-core/`; we pop twice to
/// reach the `repolens-rs` workspace root, then descend into `fixtures/scenarios`.
fn scenarios_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repolens-rs root
    p.push("fixtures");
    p.push("scenarios");
    p
}

/// Load and deserialize `manifest.json` for the named scenario.
fn load_manifest(scenario: &str) -> Manifest {
    let path = scenarios_dir()
        .join(scenario)
        .join("pack")
        .join("manifest.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse manifest for {}: {}", scenario, e))
}

/// Load all `files-*.json` chunk files for the named scenario.
fn load_chunks(scenario: &str) -> Vec<Vec<FileEntry>> {
    let pack_dir = scenarios_dir().join(scenario).join("pack");
    let mut chunk_files: Vec<_> = fs::read_dir(&pack_dir)
        .unwrap_or_else(|e| panic!("failed to read pack dir for {}: {}", scenario, e))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("files-") && name_str.ends_with(".json") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    chunk_files.sort();

    chunk_files
        .iter()
        .map(|path| {
            let json = fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
            serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e))
        })
        .collect()
}

/// Flatten all entries across all chunks for a scenario.
fn all_entries(scenario: &str) -> Vec<FileEntry> {
    load_chunks(scenario).into_iter().flatten().collect()
}

// ---------------------------------------------------------------------------
// Manifest deserialization — all scenarios
// ---------------------------------------------------------------------------

#[test]
fn manifest_java_small_deserializes() {
    let m = load_manifest("java-small");
    assert_eq!(m.schema_version, "1.3.0");
    assert_eq!(m.chunks, vec!["files-0001.json"]);
    assert_eq!(m.scan_mode.as_deref(), Some("full"));
    assert_eq!(m.scan_scope.as_deref(), Some("complete"));
    assert_eq!(m.base_pack_id, None);
    assert_eq!(m.reprocessed_files, Some(38));
    assert_eq!(m.deleted_files, None);
    assert_eq!(m.carried_forward_files, None);
    assert_eq!(
        m.head_commit.as_deref(),
        Some("7f9ccf83a2556e4f47cf0c4dae9ed0fac0fda238")
    );
    assert_eq!(m.worktree_dirty, Some(false));
}

#[test]
fn manifest_mixed_small_deserializes() {
    let m = load_manifest("mixed-small");
    assert_eq!(m.schema_version, "1.3.0");
    assert!(!m.chunks.is_empty());
    assert_eq!(m.scan_mode.as_deref(), Some("full"));
}

#[test]
fn manifest_excluded_small_deserializes() {
    let m = load_manifest("excluded-small");
    assert_eq!(m.schema_version, "1.3.0");
    assert!(!m.chunks.is_empty());
    assert_eq!(m.scan_mode.as_deref(), Some("full"));
}

// ---------------------------------------------------------------------------
// Chunk deserialization — all scenarios
// ---------------------------------------------------------------------------

#[test]
fn chunks_java_small_deserialize() {
    let entries = all_entries("java-small");
    assert!(!entries.is_empty(), "expected at least one entry");
    // At least one included file should exist
    let included_count = entries.iter().filter(|e| e.included).count();
    assert!(included_count > 0, "expected at least one included entry");
}

#[test]
fn chunks_mixed_small_deserialize() {
    let entries = all_entries("mixed-small");
    assert!(!entries.is_empty());
    let included_count = entries.iter().filter(|e| e.included).count();
    assert!(included_count > 0);
}

#[test]
fn chunks_excluded_small_deserialize() {
    let entries = all_entries("excluded-small");
    assert!(!entries.is_empty());
}

// ---------------------------------------------------------------------------
// excluded-small: both included AND excluded entries must be present
// ---------------------------------------------------------------------------

#[test]
fn excluded_small_has_both_included_and_excluded_entries() {
    let entries = all_entries("excluded-small");

    let included: Vec<_> = entries.iter().filter(|e| e.included).collect();
    let excluded: Vec<_> = entries.iter().filter(|e| !e.included).collect();

    assert!(
        !included.is_empty(),
        "excluded-small scenario must contain at least one included entry"
    );
    assert!(
        !excluded.is_empty(),
        "excluded-small scenario must contain at least one excluded entry"
    );

    // Excluded entries should have an exclusion reason
    for entry in &excluded {
        assert!(
            !entry.reason.is_empty(),
            "excluded entry {} must have a non-empty reason",
            entry.path
        );
    }
}

// ---------------------------------------------------------------------------
// Symbol deserialization
// ---------------------------------------------------------------------------

#[test]
fn included_entries_may_have_symbols() {
    let entries = all_entries("java-small");
    // Find any entry with symbols to verify symbol deserialization works
    let with_symbols: Vec<_> = entries.iter().filter(|e| !e.symbols.is_empty()).collect();
    // java-small has Java source files — symbols should be present
    assert!(
        !with_symbols.is_empty(),
        "expected at least one entry with symbols in java-small"
    );

    // Verify symbol fields on a sample
    let sample = &with_symbols[0].symbols[0];
    assert!(
        !sample.symbol_type.is_empty(),
        "symbol_type must not be empty"
    );
    assert!(!sample.name.is_empty(), "symbol name must not be empty");
}

#[test]
fn excluded_entries_have_empty_symbols_vec() {
    let entries = all_entries("java-small");
    for entry in entries.iter().filter(|e| !e.included) {
        assert!(
            entry.symbols.is_empty(),
            "excluded entry {} should have empty symbols",
            entry.path
        );
    }
}

// ---------------------------------------------------------------------------
// Snapshot tests (insta) — representative manifest + chunk
// ---------------------------------------------------------------------------

#[test]
fn snapshot_java_small_manifest_reserialization() {
    let m = load_manifest("java-small");
    // Use to_string_pretty (not to_value) because arbitrary_precision leaks
    // internal Number representation through Value.
    let json = serde_json::to_string_pretty(&m).expect("re-serialization must succeed");
    insta::assert_snapshot!("java_small_manifest", json);
}

#[test]
fn snapshot_java_small_first_chunk_reserialization() {
    let chunks = load_chunks("java-small");
    assert!(!chunks.is_empty(), "expected at least one chunk");
    let sample: Vec<_> = chunks[0].iter().take(3).collect();
    let json = serde_json::to_string_pretty(&sample).expect("re-serialization must succeed");
    insta::assert_snapshot!("java_small_first_chunk_sample", json);
}

// ---------------------------------------------------------------------------
// Observation invariants for the grandfathered grounding families.
//
// These tests implement the rubric-conforming catalogue defined in
// `docs/superpowers/specs/2026-04-13-grandfathered-families-invariant-catalogue.md`
// for the two families named in
// `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` §5:
//
// - (a) Included file presence in the active pack — matched by
//       `repolens-app/src/grounding.rs` via exact string equality on
//       `FileEntry.path`.
// - (b) Exact qualified symbols — matched via exact equality on
//       `FileEntry.symbols[].qualified_name`, restricted to the "groundable"
//       subset (anchors containing `/` or `.`).
//
// Each test name is its invariant ID in lowercase snake_case.
// ---------------------------------------------------------------------------

// ----- Family (a): included file presence --------------------------------

/// I-FILE-PATH-RELATIVE — every `FileEntry.path` is a non-empty relative
/// path: not starting with `/`, not containing `..` segments.
///
/// Guards against absolute-path leakage or parent-escape anchors, either of
/// which would let a naive grounding consumer match across repository
/// boundaries.
#[test]
fn i_file_path_relative() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            assert!(
                !entry.path.is_empty(),
                "{scenario}: FileEntry.path must not be empty"
            );
            assert!(
                !entry.path.starts_with('/'),
                "{scenario}: FileEntry.path must be relative, got absolute: {:?}",
                entry.path
            );
            for segment in entry.path.split('/') {
                assert_ne!(
                    segment, "..",
                    "{scenario}: FileEntry.path must not contain `..` segment: {:?}",
                    entry.path
                );
            }
        }
    }
}

/// I-FILE-PATH-FORWARD-SLASH — every `FileEntry.path` uses forward slashes
/// only, never backslashes.
///
/// Guards cross-platform consistency: memory anchors are canonically
/// forward-slash, and a Windows-style entry path would silently fail to
/// match a forward-slash anchor under `entry.path == anchor_str`.
#[test]
fn i_file_path_forward_slash() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            assert!(
                !entry.path.contains('\\'),
                "{scenario}: FileEntry.path must not contain backslash: {:?}",
                entry.path
            );
        }
    }
}

/// I-FILE-PATH-UNIQUE — within a single pack, `FileEntry.path` values are
/// unique across all chunks and all entries (included and excluded).
///
/// Guards against double-counting during grounding: a duplicated path would
/// let the same anchor match twice and inflate confirmed-anchor counts in
/// ways memory-layer consumers would not expect.
#[test]
fn i_file_path_unique() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        let total = entries.len();
        let unique: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            unique.len(),
            total,
            "{scenario}: FileEntry.path values must be unique across the pack \
             (total={total}, unique={})",
            unique.len()
        );
    }
}

/// I-FILE-PATH-SERDE-ROUNDTRIP — every `FileEntry.path` survives a serde
/// round-trip (serialize → deserialize) byte-for-byte.
///
/// Guards against serde drift that would break `entry.path == anchor_str`
/// equality in `grounding.rs`. This is a weaker form of the full pack-writer
/// round-trip; the stronger form lives in `repolens-app` scan tests.
#[test]
fn i_file_path_serde_roundtrip() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            let json = serde_json::to_string(entry)
                .unwrap_or_else(|e| panic!("{scenario}: serialize failed: {e}"));
            let decoded: FileEntry = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{scenario}: deserialize failed: {e}"));
            assert_eq!(
                decoded.path, entry.path,
                "{scenario}: FileEntry.path changed across serde round-trip"
            );
        }
    }
}

// ----- Family (b): exact qualified symbols (groundable subset) -----------

/// I-SYMBOL-QNAME-NONEMPTY — every `Symbol.qualified_name` that is `Some(_)`
/// holds a non-empty string.
///
/// Guards against `Some("")` slipping through. An empty qualified_name is
/// rejected by `is_groundable`, but this invariant is defense in depth for
/// any future consumer that does not re-filter.
#[test]
fn i_symbol_qname_nonempty() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            for symbol in &entry.symbols {
                if let Some(q) = symbol.qualified_name.as_deref() {
                    assert!(
                        !q.is_empty(),
                        "{scenario}: empty qualified_name in file {:?} on symbol name={:?}",
                        entry.path,
                        symbol.name
                    );
                }
            }
        }
    }
}

/// I-SYMBOL-QNAME-UNIQUE-IN-FILE — within a single `FileEntry`, groundable
/// `qualified_name` values are unique.
///
/// Guards the grounding contract: if two symbols in the same file share a
/// groundable qualified_name, one anchor would match twice via the `any()`
/// walk in `grounding.rs`, producing subtle double-counting. Scoped per-file
/// because cross-file duplication is legitimate (two crates may define
/// `mod foo`).
#[test]
fn i_symbol_qname_unique_in_file() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            let mut seen: HashSet<&str> = HashSet::new();
            for symbol in &entry.symbols {
                let Some(q) = symbol.qualified_name.as_deref() else {
                    continue;
                };
                if !is_groundable(q) {
                    continue;
                }
                assert!(
                    seen.insert(q),
                    "{scenario}: duplicate groundable qualified_name {:?} within file {:?}",
                    q,
                    entry.path
                );
            }
        }
    }
}

/// I-SYMBOL-QNAME-TRIMMED — no `Symbol.qualified_name` has leading or
/// trailing whitespace.
///
/// Guards against trim drift: exact-equality matching in `grounding.rs`
/// would silently fail if the extractor emitted `"  com.foo.Bar "` and the
/// memory anchor was `"com.foo.Bar"`. Internal whitespace is legitimate
/// (Java method signatures like `void main(String[])`), so the invariant
/// checks only the edges.
#[test]
fn i_symbol_qname_trimmed() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            for symbol in &entry.symbols {
                if let Some(q) = symbol.qualified_name.as_deref() {
                    assert_eq!(
                        q,
                        q.trim(),
                        "{scenario}: qualified_name has leading/trailing whitespace in file {:?}: {:?}",
                        entry.path,
                        q
                    );
                }
            }
        }
    }
}

// ----- Family (c): file content extraction ------------------------------
//
// These invariants implement the rubric-conforming catalogue defined in
// `docs/superpowers/specs/2026-04-13-file-content-invariant-catalogue.md`
// for the third grandfathered family — file content extraction served by
// the `read_file_content` MCP tool. The grounding contract for this family
// is "the bytes returned for a captured entry are the bytes that were
// captured into the pack at scan time."
//
// Path-traversal defense (canonicalize + prefix check in
// `crates/repolens-mcp/src/lib.rs`) is intentionally NOT a separate
// invariant here. It is defense in depth covered upstream by
// `I-FILE-PATH-RELATIVE` (which guarantees no `..` segments in entry
// paths) and by `I-FILE-CONTENT-PATH-PREFIX` below (which guarantees the
// stored content path is a `files/` child of the pack directory).

/// I-FILE-CONTENT-INDEX-COMPLETE — every included `FileEntry` has
/// `Some(content_path)`.
///
/// Guards the index-completeness contract: a memory anchor against a
/// captured file must be able to find its bytes in the pack. An included
/// entry with `content_path: None` would silently break that lookup.
#[test]
fn i_file_content_index_complete() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            if !entry.included {
                continue;
            }
            assert!(
                entry.content_path.is_some(),
                "{scenario}: included FileEntry {:?} must have Some(content_path)",
                entry.path
            );
        }
    }
}

/// I-FILE-CONTENT-EXCLUDED-NO-CONTENT — every excluded `FileEntry` has
/// `content_path: None`.
///
/// Guards the inverse contract: the pack must not store bytes for files
/// that were excluded from the scan (gitignore, rules engine, etc.).
/// A leaked excluded entry with content would let a memory anchor
/// successfully read bytes that should not have been captured.
#[test]
fn i_file_content_excluded_no_content() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            if entry.included {
                continue;
            }
            assert!(
                entry.content_path.is_none(),
                "{scenario}: excluded FileEntry {:?} must have content_path: None, got {:?}",
                entry.path,
                entry.content_path
            );
        }
    }
}

/// I-FILE-CONTENT-PATH-PREFIX — every `Some(content_path)` is a relative
/// path that starts with `files/` and contains no `..` segments.
///
/// Guards the path-traversal defense in depth: the canonicalize+contain
/// check in `read_file_content` is dead code in normal operation only
/// because every stored content path is a `files/` child of the pack
/// directory. This invariant makes that property explicit and testable.
#[test]
fn i_file_content_path_prefix() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            let Some(cp) = entry.content_path.as_deref() else {
                continue;
            };
            assert!(
                !cp.starts_with('/'),
                "{scenario}: content_path must be relative, got absolute: {:?}",
                cp
            );
            assert!(
                cp.starts_with("files/"),
                "{scenario}: content_path must start with `files/`, got {:?} for entry {:?}",
                cp,
                entry.path
            );
            for segment in cp.split('/') {
                assert_ne!(
                    segment, "..",
                    "{scenario}: content_path must not contain `..` segment: {:?}",
                    cp
                );
            }
        }
    }
}

/// I-FILE-CONTENT-PRESENT-ON-DISK — for every included `FileEntry`, the
/// file at `pack/<content_path>` exists and is readable.
///
/// Guards against captured-content drift: an entry pointing at a missing
/// file means the pack is corrupt or partially written, and any memory
/// anchor would silently fail content lookup.
#[test]
fn i_file_content_present_on_disk() {
    for scenario in SCENARIOS {
        let pack_dir = scenarios_dir().join(scenario).join("pack");
        let entries = all_entries(scenario);
        for entry in &entries {
            let Some(cp) = entry.content_path.as_deref() else {
                continue;
            };
            let abs = pack_dir.join(cp);
            assert!(
                abs.is_file(),
                "{scenario}: missing content file at {} for entry {:?}",
                abs.display(),
                entry.path
            );
        }
    }
}

/// I-FILE-CONTENT-SHA256-PARITY — for every included `FileEntry`, the
/// SHA-256 of the bytes at `pack/<content_path>` equals
/// `entry.sha256`.
///
/// Strongest content-integrity guarantee: the bytes returned by
/// `read_file_content` are exactly the bytes captured at scan time. A
/// drift here would mean grounding a memory anchor against the file
/// returns content that no longer matches what RepoLens claimed it
/// captured — silent corruption.
#[test]
fn i_file_content_sha256_parity() {
    for scenario in SCENARIOS {
        let pack_dir = scenarios_dir().join(scenario).join("pack");
        let entries = all_entries(scenario);
        for entry in &entries {
            let Some(cp) = entry.content_path.as_deref() else {
                continue;
            };
            assert!(
                !entry.sha256.is_empty(),
                "{scenario}: included FileEntry {:?} must have non-empty sha256",
                entry.path
            );
            let abs = pack_dir.join(cp);
            let bytes = fs::read(&abs).unwrap_or_else(|e| {
                panic!(
                    "{scenario}: failed to read content file {} for entry {:?}: {e}",
                    abs.display(),
                    entry.path
                )
            });
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let actual = hex::encode(hasher.finalize());
            assert_eq!(
                actual, entry.sha256,
                "{scenario}: sha256 mismatch for entry {:?}: pack records {}, on-disk content hashes to {}",
                entry.path, entry.sha256, actual
            );
        }
    }
}

/// I-SYMBOL-QNAME-SERDE-ROUNDTRIP — every `Symbol.qualified_name` survives a
/// serde round-trip on its containing `FileEntry` byte-for-byte.
///
/// Guards against serde drift for the other half of the grounding equality
/// check: `s.qualified_name.as_deref() == Some(anchor_str)`.
#[test]
fn i_symbol_qname_serde_roundtrip() {
    for scenario in SCENARIOS {
        let entries = all_entries(scenario);
        for entry in &entries {
            if entry.symbols.is_empty() {
                continue;
            }
            let json = serde_json::to_string(entry)
                .unwrap_or_else(|e| panic!("{scenario}: serialize failed: {e}"));
            let decoded: FileEntry = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{scenario}: deserialize failed: {e}"));
            assert_eq!(
                decoded.symbols.len(),
                entry.symbols.len(),
                "{scenario}: symbol count changed across serde round-trip for {:?}",
                entry.path
            );
            for (original, decoded) in entry.symbols.iter().zip(decoded.symbols.iter()) {
                assert_eq!(
                    decoded.qualified_name, original.qualified_name,
                    "{scenario}: qualified_name changed across serde round-trip for file {:?}, symbol name={:?}",
                    entry.path, original.name
                );
            }
        }
    }
}
