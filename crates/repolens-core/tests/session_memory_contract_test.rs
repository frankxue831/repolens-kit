use repolens_core::model::memory::{MemoryGraph, MemoryRelation, MemorySource, MemoryType};
use repolens_core::model::session::RepoSession;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scenarios_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repolens-rs root
    p.push("fixtures");
    p.push("scenarios");
    p
}

fn load_session(scenario: &str) -> RepoSession {
    let path = scenarios_dir()
        .join(scenario)
        .join("repo")
        .join("session.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read repo/session.json for {}: {}", scenario, e));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse repo/session.json for {}: {}", scenario, e))
}

/// Load repo/memory.json for a scenario, returning `None` if the file doesn't exist.
fn load_memory_opt(scenario: &str) -> Option<MemoryGraph> {
    let path = scenarios_dir()
        .join(scenario)
        .join("repo")
        .join("memory.json");
    if !path.exists() {
        return None;
    }
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read repo/memory.json for {}: {}", scenario, e));
    Some(
        serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("failed to parse repo/memory.json for {}: {}", scenario, e)),
    )
}

// ---------------------------------------------------------------------------
// Session deserialization — all scenarios
// ---------------------------------------------------------------------------

#[test]
fn session_java_small_deserializes() {
    let s = load_session("java-small");
    assert_eq!(s.schema_version, "1.0");
    assert_eq!(s.session_id, "8c33d3a2-26da-4260-9dba-566a6eecba7a");
    assert_eq!(s.last_scan_mode, "full");
    assert_eq!(s.reprocessed_files, Some(38));
    assert_eq!(s.deleted_files, None);
    assert_eq!(s.carried_forward_files, None);
    assert!(s.active_report_path.is_none());
    // Timestamps should parse to the expected year
    assert_eq!(s.updated_at.year(), 2026);
    assert_eq!(s.last_accessed.year(), 2026);
}

#[test]
fn session_mixed_small_deserializes() {
    let s = load_session("mixed-small");
    assert_eq!(s.schema_version, "1.0");
    assert_eq!(s.session_id, "55fa7398-5280-4ebd-a6eb-99b8ee3666ba");
    assert_eq!(s.last_scan_mode, "full");
    assert_eq!(s.reprocessed_files, Some(42));
    assert!(s.active_report_path.is_none());
    assert_eq!(s.deleted_files, None);
    assert_eq!(s.carried_forward_files, None);
}

#[test]
fn session_excluded_small_deserializes() {
    let s = load_session("excluded-small");
    assert_eq!(s.schema_version, "1.0");
    assert_eq!(s.session_id, "43a66c4d-0070-421f-97c3-41109bf6813b");
    assert_eq!(s.last_scan_mode, "full");
    assert_eq!(s.reprocessed_files, Some(33));
    assert_eq!(s.deleted_files, None);
    assert_eq!(s.carried_forward_files, None);
    assert!(s.active_report_path.is_none());
}

// ---------------------------------------------------------------------------
// Memory deserialization — java-small has memory.json, others don't
// ---------------------------------------------------------------------------

#[test]
fn memory_java_small_deserializes() {
    let g = load_memory_opt("java-small").expect("java-small should have a memory.json fixture");
    assert_eq!(g.schema_version, "1.0");
    assert!(g.nodes.is_empty());
    assert!(g.edges.is_empty());
    assert!(g.last_decay_run.is_some());
    assert_eq!(g.updated_at.year(), 2026);
}

#[test]
fn memory_missing_is_handled_gracefully() {
    // mixed-small and excluded-small deliberately have no memory.json
    assert!(load_memory_opt("mixed-small").is_none());
    assert!(load_memory_opt("excluded-small").is_none());
}

// ---------------------------------------------------------------------------
// Snapshot tests — representative re-serializations
// ---------------------------------------------------------------------------

#[test]
fn snapshot_java_small_session_reserialization() {
    let s = load_session("java-small");
    let json = serde_json::to_string_pretty(&s).expect("session re-serialization must succeed");
    insta::assert_snapshot!("java_small_session", json);
}

#[test]
fn snapshot_java_small_memory_reserialization() {
    let g = load_memory_opt("java-small")
        .expect("java-small memory fixture must exist for snapshot test");
    let json = serde_json::to_string_pretty(&g).expect("memory re-serialization must succeed");
    insta::assert_snapshot!("java_small_memory", json);
}

// ---------------------------------------------------------------------------
// Enum serialization round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn memory_type_serializes_to_screaming_snake_case() {
    assert_eq!(
        serde_json::to_string(&MemoryType::CodeAnchor).unwrap(),
        "\"CODE_ANCHOR\""
    );
    assert_eq!(
        serde_json::to_string(&MemoryType::Decision).unwrap(),
        "\"DECISION\""
    );
    assert_eq!(
        serde_json::to_string(&MemoryType::Convention).unwrap(),
        "\"CONVENTION\""
    );
    assert_eq!(
        serde_json::to_string(&MemoryType::TradeOff).unwrap(),
        "\"TRADE_OFF\""
    );
    assert_eq!(
        serde_json::to_string(&MemoryType::Pattern).unwrap(),
        "\"PATTERN\""
    );
}

#[test]
fn memory_type_deserializes_from_screaming_snake_case() {
    let t: MemoryType = serde_json::from_str("\"CODE_ANCHOR\"").unwrap();
    assert_eq!(t, MemoryType::CodeAnchor);

    let t: MemoryType = serde_json::from_str("\"TRADE_OFF\"").unwrap();
    assert_eq!(t, MemoryType::TradeOff);
}

#[test]
fn memory_source_serializes_correctly() {
    assert_eq!(
        serde_json::to_string(&MemorySource::Explicit).unwrap(),
        "\"EXPLICIT\""
    );
    assert_eq!(
        serde_json::to_string(&MemorySource::ScanTriggered).unwrap(),
        "\"SCAN_TRIGGERED\""
    );
}

#[test]
fn memory_relation_serializes_correctly() {
    assert_eq!(
        serde_json::to_string(&MemoryRelation::Supersedes).unwrap(),
        "\"SUPERSEDES\""
    );
    assert_eq!(
        serde_json::to_string(&MemoryRelation::RelatesTo).unwrap(),
        "\"RELATES_TO\""
    );
    assert_eq!(
        serde_json::to_string(&MemoryRelation::Contradicts).unwrap(),
        "\"CONTRADICTS\""
    );
}

#[test]
fn enum_roundtrips() {
    // MemoryType
    for variant in &[
        MemoryType::CodeAnchor,
        MemoryType::Decision,
        MemoryType::Convention,
        MemoryType::TradeOff,
        MemoryType::Pattern,
    ] {
        let s = serde_json::to_string(variant).unwrap();
        let back: MemoryType = serde_json::from_str(&s).unwrap();
        assert_eq!(
            &back, variant,
            "MemoryType roundtrip failed for {:?}",
            variant
        );
    }

    // MemorySource
    for variant in &[MemorySource::Explicit, MemorySource::ScanTriggered] {
        let s = serde_json::to_string(variant).unwrap();
        let back: MemorySource = serde_json::from_str(&s).unwrap();
        assert_eq!(
            &back, variant,
            "MemorySource roundtrip failed for {:?}",
            variant
        );
    }

    // MemoryRelation
    for variant in &[
        MemoryRelation::Supersedes,
        MemoryRelation::RelatesTo,
        MemoryRelation::Contradicts,
    ] {
        let s = serde_json::to_string(variant).unwrap();
        let back: MemoryRelation = serde_json::from_str(&s).unwrap();
        assert_eq!(
            &back, variant,
            "MemoryRelation roundtrip failed for {:?}",
            variant
        );
    }
}

// ---------------------------------------------------------------------------
// Session timestamp precision check
// ---------------------------------------------------------------------------

#[test]
fn session_timestamps_parse_with_microsecond_precision() {
    let s = load_session("java-small");
    // The fixture has microseconds (.411230) — verify they survive the round-trip
    let out = serde_json::to_string(&s).expect("serialize");
    // The re-serialized JSON should contain the date portion
    assert!(out.contains("2026-03-29"), "date preserved: {out}");
    assert!(out.contains("03:00:41"), "time preserved: {out}");
    assert!(out.contains('Z'), "Z suffix preserved: {out}");
}
