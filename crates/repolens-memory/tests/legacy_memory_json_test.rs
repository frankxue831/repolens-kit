//! A memory.json written before `lastDecayedAt` existed must still load, decay
//! from the graph-level `lastDecayRun` (no double decay), and gain the new
//! field on save.

use repolens_memory::decay::MemoryDecayEngine;
use repolens_memory::store::MemoryStore;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Shape of memory.json as saved by the previous release: graph-level
/// `lastDecayRun`, nodes without `lastDecayedAt`. Node "a" was last accessed
/// on 2026-01-01 and already decayed through 2026-01-31 (0.8 -> 0.4); node
/// "b" was accessed after that run, on 2026-02-10.
const LEGACY: &str = r#"{
  "schemaVersion": "1.0",
  "repoRoot": "/tmp/legacy",
  "nodes": [
    {"id": "a", "content": "legacy alpha", "type": "DECISION", "importance": 0.4,
     "created": "2026-01-01T00:00:00Z", "lastAccessed": "2026-01-01T00:00:00Z",
     "accessCount": 0, "anchors": [], "tags": [], "source": "EXPLICIT",
     "provenance": "mcp:memorize_insight"},
    {"id": "b", "content": "legacy beta", "type": "PATTERN", "importance": 0.6,
     "created": "2026-01-01T00:00:00Z", "lastAccessed": "2026-02-10T00:00:00Z",
     "accessCount": 0, "anchors": [], "tags": [], "source": "EXPLICIT",
     "provenance": "mcp:memorize_insight"}
  ],
  "edges": [],
  "lastDecayRun": "2026-01-31T00:00:00Z",
  "updatedAt": "2026-02-10T00:00:00Z"
}"#;

fn t(s: &str) -> OffsetDateTime {
    OffsetDateTime::parse(s, &Rfc3339).unwrap()
}

#[test]
fn legacy_memory_json_loads_and_decays_without_double_counting() {
    let dir = tempfile::tempdir().unwrap();
    let mem_dir = dir.path().join(".repolens/memory");
    std::fs::create_dir_all(&mem_dir).unwrap();
    std::fs::write(mem_dir.join("memory.json"), LEGACY).unwrap();

    let graph = MemoryStore::load(dir.path()).expect("legacy memory.json must load");
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.nodes.iter().all(|n| n.last_decayed_at.is_none()));

    // Decay on 2026-03-02.
    let now = t("2026-03-02T00:00:00Z");
    let out = MemoryDecayEngine::decay(&graph, now);
    let imp = |id: &str| out.nodes.iter().find(|n| n.id == id).unwrap().importance;

    // "a": only 30 more days since lastDecayRun (not 60 since access).
    assert!((imp("a") - 0.2).abs() < 1e-4, "a = {}", imp("a"));
    // "b": 20 days since its access (access is later than lastDecayRun).
    let expected_b = 0.6 * 0.5_f32.powf(20.0 / 30.0);
    assert!((imp("b") - expected_b).abs() < 1e-4, "b = {}", imp("b"));
    assert!(out.nodes.iter().all(|n| n.last_decayed_at == Some(now)));

    // Saving writes the new field; reloading round-trips it.
    MemoryStore::save(dir.path(), &out).unwrap();
    let raw = std::fs::read_to_string(mem_dir.join("memory.json")).unwrap();
    assert!(raw.contains("lastDecayedAt"), "{raw}");
    let reloaded = MemoryStore::load(dir.path()).unwrap();
    assert!(reloaded
        .nodes
        .iter()
        .all(|n| n.last_decayed_at == Some(now)));

    // A second run one day later applies one day, not 31.
    let again = MemoryDecayEngine::decay(&reloaded, t("2026-03-03T00:00:00Z"));
    let a2 = again.nodes.iter().find(|n| n.id == "a").unwrap().importance;
    assert!(
        (a2 - 0.2 * 0.5_f32.powf(1.0 / 30.0)).abs() < 1e-4,
        "a2 = {a2}"
    );
}
