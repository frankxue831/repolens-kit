//! Integration tests for MCP memory tools: memorize, recall, forget.

use std::collections::BTreeSet;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;

use repolens_core::model::memory::{
    GroundingStatus, MemoryGraph, MemoryNode, MemorySource, MemoryType as CoreMemoryType,
};
use repolens_mcp::RepoLensMcpServer;
use repolens_memory::store::{self as mem_store, MemoryStore};
use time::OffsetDateTime;
use uuid::Uuid;

fn setup_server() -> (tempfile::TempDir, RepoLensMcpServer) {
    let dir = tempfile::TempDir::new().unwrap();
    let server = RepoLensMcpServer::new();
    (dir, server)
}

fn memory_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".repolens/memory/memory.json")
}

fn load_graph(root: &std::path::Path) -> MemoryGraph {
    let raw = fs::read_to_string(memory_path(root)).expect("memory.json should exist");
    serde_json::from_str(&raw).expect("memory.json should be valid MemoryGraph")
}

fn create_pack_with_files(root: &std::path::Path, file_paths: &[&str]) {
    let pack_dir = root.join(".repolens/pack-20260405-120000");
    fs::create_dir_all(&pack_dir).unwrap();

    let entries: Vec<serde_json::Value> = file_paths
        .iter()
        .map(|p| {
            serde_json::json!({
                "path": p,
                "sha256": "abc123",
                "language": "rust",
                "loc": 100,
                "included": true,
                "reason": "",
                "contentPath": null,
                "symbols": []
            })
        })
        .collect();

    fs::write(
        pack_dir.join("manifest.json"),
        r#"{"schemaVersion":"1.0","schemaUrl":"","timestamp":1774753241.0,"chunks":["files-0001.json"]}"#,
    )
    .unwrap();
    fs::write(
        pack_dir.join("files-0001.json"),
        serde_json::to_string(&entries).unwrap(),
    )
    .unwrap();

    let session_dir = root.join(".repolens");
    fs::write(
        session_dir.join("session.json"),
        serde_json::json!({
            "schemaVersion": "1.0",
            "sessionId": "test-grounding-session",
            "repoRoot": root.to_string_lossy(),
            "activePackPath": pack_dir.to_string_lossy(),
            "updatedAt": "2026-04-05T12:00:00Z",
            "lastAccessed": "2026-04-05T12:00:00Z",
            "lastScanMode": "full",
            "reprocessedFiles": 0
        })
        .to_string(),
    )
    .unwrap();
}

fn memorize_with_anchors(
    root: &std::path::Path,
    content: &str,
    memory_type: CoreMemoryType,
    anchors: Vec<&str>,
) -> String {
    let mut graph = MemoryStore::load(root).unwrap_or_else(|_| mem_store::empty_graph(root));
    let now = OffsetDateTime::now_utc();
    let node_id = Uuid::new_v4().to_string();
    let node = MemoryNode {
        id: node_id.clone(),
        content: content.to_string(),
        memory_type,
        importance: 0.8,
        created: now,
        last_accessed: now,
        access_count: 0,
        anchors: anchors.into_iter().map(|s| s.to_string()).collect(),
        tags: vec![],
        source: MemorySource::Explicit,
        provenance: "test".to_string(),
        last_grounding: None,
        last_grounded_at: None,
        last_decayed_at: None,
        importance_before_grounding: None,
        anchor_sha256: Default::default(),
    };
    mem_store::add_node(&mut graph, node);
    MemoryStore::save(root, &graph).unwrap();
    node_id
}

fn assert_no_tool_error(json: &serde_json::Value) {
    assert!(json.get("error").is_none(), "tool returned error: {json:#}");
}

// ---------------------------------------------------------------------------
// Test 1: memorize creates a new memory node
// ---------------------------------------------------------------------------

#[test]
fn memorize_creates_memory() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let result = server.call_memorize_insight(
        root,
        "The scanning pipeline has 5 stages",
        Some("CONVENTION"),
        Some(0.8),
        Some(vec!["architecture", "scanning"]),
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["status"], "created", "status should be 'created'");
    assert!(json["id"].is_string(), "id should be a string UUID");

    assert!(
        memory_path(dir.path()).exists(),
        ".repolens/memory/memory.json should exist after memorize"
    );
}

// ---------------------------------------------------------------------------
// Test 2: memorize detects exact-content duplicates
// ---------------------------------------------------------------------------

#[test]
fn memorize_detects_duplicate() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let first = server.call_memorize_insight(root, "same content", None, None, None, None);
    let j1: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(
        j1["status"], "created",
        "first call should create the memory"
    );
    let created_id = j1["id"].as_str().unwrap().to_string();

    let second = server.call_memorize_insight(root, "same content", None, None, None, None);
    let j2: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(
        j2["status"], "duplicate",
        "second call should detect duplicate"
    );
    assert_eq!(
        j2["duplicateOf"].as_str().unwrap(),
        created_id,
        "duplicateOf should match the original node id"
    );
}

// ---------------------------------------------------------------------------
// Test 3: memorize with supersedes creates a SUPERSEDES edge
// ---------------------------------------------------------------------------

#[test]
fn memorize_with_supersedes() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create the old memory
    let old =
        server.call_memorize_insight(root, "old approach: use sync IO", None, None, None, None);
    let old_json: serde_json::Value = serde_json::from_str(&old).unwrap();
    let old_id = old_json["id"].as_str().unwrap().to_string();

    // Create the new memory that supersedes the old one
    let new = server.call_memorize_insight(
        root,
        "new approach: use async IO everywhere",
        Some("DECISION"),
        Some(0.9),
        None,
        Some(&old_id),
    );
    let new_json: serde_json::Value = serde_json::from_str(&new).unwrap();
    assert_eq!(new_json["status"], "created");
    let new_id = new_json["id"].as_str().unwrap().to_string();

    // Verify edge in persisted graph
    let graph = load_graph(dir.path());
    assert_eq!(
        graph.edges.len(),
        1,
        "should have exactly 1 SUPERSEDES edge"
    );
    let edge = &graph.edges[0];
    assert_eq!(edge.source_id, new_id, "source should be the new node");
    assert_eq!(edge.target_id, old_id, "target should be the old node");
    assert_eq!(
        format!("{:?}", edge.relation),
        "Supersedes",
        "relation should be Supersedes"
    );
}

#[test]
fn concurrent_memorize_preserves_all_unique_nodes() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = Arc::new(dir.path().to_string_lossy().into_owned());
    let workers = 24;
    let barrier = Arc::new(Barrier::new(workers));

    let handles: Vec<_> = (0..workers)
        .map(|i| {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let server = RepoLensMcpServer::new();
                let result = server.call_memorize_insight(
                    &root,
                    &format!("concurrent memory insight {i}"),
                    Some("PATTERN"),
                    Some(0.6),
                    Some(vec!["concurrency"]),
                    None,
                );
                serde_json::from_str::<serde_json::Value>(&result).expect("valid JSON")
            })
        })
        .collect();

    let responses: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("worker should not panic"))
        .collect();
    for json in &responses {
        assert_no_tool_error(json);
        assert_eq!(json["status"], "created", "unexpected response: {json:#}");
    }

    let graph = load_graph(dir.path());
    assert_eq!(graph.nodes.len(), workers);
    let contents: BTreeSet<_> = graph.nodes.iter().map(|n| n.content.as_str()).collect();
    for i in 0..workers {
        assert!(
            contents.contains(format!("concurrent memory insight {i}").as_str()),
            "missing persisted memory {i}; contents={contents:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 4: recall ranks results by keyword relevance
// ---------------------------------------------------------------------------

#[test]
fn recall_ranks_by_keyword_relevance() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    server.call_memorize_insight(
        root,
        "authentication login security for users",
        None,
        None,
        None,
        None,
    );
    server.call_memorize_insight(
        root,
        "database schema migration strategy",
        None,
        None,
        None,
        None,
    );
    server.call_memorize_insight(
        root,
        "login page redesign with new layout",
        None,
        None,
        None,
        None,
    );

    let result = server.call_recall_longterm(root, "login authentication", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(
        results.len() >= 2,
        "should return at least 2 relevant results"
    );

    // The first result should contain login or authentication (high-relevance)
    let first_content = results[0]["content"].as_str().unwrap();
    assert!(
        first_content.contains("login") || first_content.contains("authentication"),
        "first result should be about login/authentication, got: {first_content}"
    );

    // The database memory should not appear in results (no matching keywords)
    let any_database = results
        .iter()
        .any(|r| r["content"].as_str().unwrap_or("").contains("database"));
    assert!(
        !any_database,
        "database memory should NOT appear for 'login authentication' query"
    );
}

// ---------------------------------------------------------------------------
// Test 5: recall increments access_count on returned nodes
// ---------------------------------------------------------------------------

#[test]
fn recall_touches_returned_nodes() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    server.call_memorize_insight(
        root,
        "caching strategy for API responses",
        None,
        None,
        None,
        None,
    );

    // Recall the memory
    let result = server.call_recall_longterm(root, "caching strategy", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(
        !json["results"].as_array().unwrap().is_empty(),
        "recall should find the memory"
    );

    // Check that access_count was incremented in persisted graph
    let graph = load_graph(dir.path());
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(
        graph.nodes[0].access_count, 1,
        "access_count should be 1 after one recall"
    );
}

// ---------------------------------------------------------------------------
// Test 6: forget dry-run does not delete
// ---------------------------------------------------------------------------

#[test]
fn forget_dry_run_does_not_delete() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    server.call_memorize_insight(
        root,
        "temporary workaround for rate limiting",
        None,
        None,
        None,
        None,
    );

    // Dry-run forget (confirm = false)
    let result = server.call_forget_insight(root, None, Some("workaround rate limiting"), false);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["status"], "dry_run", "status should be dry_run");
    let matches = json["matches"]
        .as_array()
        .expect("matches should be an array");
    assert!(
        !matches.is_empty(),
        "dry_run should return matched candidates"
    );

    // Node should still exist in the persisted graph
    let graph = load_graph(dir.path());
    assert_eq!(
        graph.nodes.len(),
        1,
        "node should still exist after dry-run"
    );
}

// ---------------------------------------------------------------------------
// Test 7: forget with confirm=true deletes the node
// ---------------------------------------------------------------------------

#[test]
fn forget_confirm_deletes() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let created = server.call_memorize_insight(root, "delete me please", None, None, None, None);
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let node_id = created_json["id"].as_str().unwrap().to_string();

    // Confirm delete by id
    let result = server.call_forget_insight(root, Some(&node_id), None, true);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["status"], "deleted", "status should be 'deleted'");
    assert_eq!(json["removedCount"], 1, "removedCount should be 1");

    // Node should be gone from the persisted graph
    let graph = load_graph(dir.path());
    assert!(
        graph.nodes.is_empty(),
        "nodes should be empty after confirmed delete"
    );
}

// #35: the 8-character id `repolens memory save` prints works here.
#[test]
fn forget_accepts_a_unique_id_prefix_and_rejects_an_ambiguous_one() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();
    let mut ids = Vec::new();
    for content in ["first note", "second note", "third note"] {
        let created = server.call_memorize_insight(root, content, None, None, None, None);
        let json: serde_json::Value = serde_json::from_str(&created).unwrap();
        ids.push(json["id"].as_str().unwrap().to_string());
    }
    // Give the second and third notes a shared 8-character prefix.
    let mut graph = load_graph(dir.path());
    let shared = |id: &str| format!("abcd1234{}", &id[8..]);
    for n in graph.nodes.iter_mut().filter(|n| n.content != "first note") {
        n.id = shared(&n.id);
    }
    fs::write(
        memory_path(dir.path()),
        serde_json::to_string(&graph).unwrap(),
    )
    .unwrap();

    // Unique prefix: dry run lists the note, confirm deletes it.
    let short = &ids[0][..8];
    let dry: serde_json::Value =
        serde_json::from_str(&server.call_forget_insight(root, Some(short), None, false)).unwrap();
    assert_no_tool_error(&dry);
    assert_eq!(dry["matches"].as_array().unwrap().len(), 1, "{dry:#}");
    assert_eq!(dry["matches"][0]["id"], ids[0].as_str());
    let del: serde_json::Value =
        serde_json::from_str(&server.call_forget_insight(root, Some(short), None, true)).unwrap();
    assert_eq!(del["removedCount"], 1, "{del:#}");
    assert_eq!(del["removedIds"][0], ids[0].as_str());

    // Ambiguous prefix: a clear error for dry run and confirm; nothing deleted.
    for confirm in [false, true] {
        let json: serde_json::Value = serde_json::from_str(&server.call_forget_insight(
            root,
            Some("abcd1234"),
            None,
            confirm,
        ))
        .unwrap();
        let err = json["error"].as_str().unwrap_or_default();
        assert!(
            err.starts_with("abcd1234 matches 2 memories; use a longer prefix."),
            "{json:#}"
        );
        assert!(err.contains(&shared(&ids[1])), "{json:#}");
    }
    assert_eq!(load_graph(dir.path()).nodes.len(), 2);

    // A longer prefix picks one; the full id still works.
    let one = shared(&ids[2]);
    let json: serde_json::Value =
        serde_json::from_str(&server.call_forget_insight(root, Some(&one[..20]), None, false))
            .unwrap();
    assert_eq!(json["matches"][0]["id"], one.as_str(), "{json:#}");
    let json: serde_json::Value =
        serde_json::from_str(&server.call_forget_insight(root, Some(&one), None, false)).unwrap();
    assert_eq!(json["matches"].as_array().unwrap().len(), 1, "{json:#}");
}

// #35 follow-up: ids are hex, so an uppercase prefix finds the same note.
#[test]
fn forget_id_prefix_ignores_case() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();
    let created = server.call_memorize_insight(root, "case note", None, None, None, None);
    let json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = json["id"].as_str().unwrap().to_string();
    let call = |id: &str, confirm: bool| -> serde_json::Value {
        serde_json::from_str(&server.call_forget_insight(root, Some(id), None, confirm)).unwrap()
    };

    // Uppercase prefixes and the uppercase full id match the same note.
    for upper in [
        id[..4].to_uppercase(),
        id[..8].to_uppercase(),
        id.to_uppercase(),
    ] {
        let json = call(&upper, false);
        assert_no_tool_error(&json);
        assert_eq!(json["matches"][0]["id"], id.as_str(), "{upper}: {json:#}");
    }
    let json = call(&id[..8].to_uppercase(), true);
    assert_eq!(json["removedIds"][0], id.as_str(), "{json:#}");
    assert!(load_graph(dir.path()).nodes.is_empty());
}

// #81: an id prefix needs at least 4 characters, for a dry run as well as
// a confirmed delete (the same rule as `repolens memory forget`); a refused
// or unknown id writes nothing, not even the memory directory.
#[test]
fn forget_needs_a_four_character_prefix() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();
    let created = server.call_memorize_insight(root, "short prefix note", None, None, None, None);
    let json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = json["id"].as_str().unwrap().to_string();
    let call = |id: &str, confirm: bool| -> serde_json::Value {
        serde_json::from_str(&server.call_forget_insight(root, Some(id), None, confirm)).unwrap()
    };

    for confirm in [false, true] {
        let refused = call(&id[..3], confirm);
        assert_eq!(
            refused["error"], "Use at least 4 characters of the id.",
            "{refused:#}"
        );
        assert!(refused.get("matches").is_none(), "{refused:#}");
        assert_eq!(load_graph(dir.path()).nodes.len(), 1);
    }
    let dry = call(&id[..4], false);
    assert_eq!(dry["matches"][0]["id"], id.as_str(), "{dry:#}");
    let done = call(&id[..4], true);
    assert_eq!(done["removedIds"][0], id.as_str(), "{done:#}");

    let fresh = tempfile::tempdir().unwrap();
    let fresh_root = fresh.path().to_str().unwrap();
    for q in ["abcd1234", "ab"] {
        for confirm in [false, true] {
            server.call_forget_insight(fresh_root, Some(q), None, confirm);
        }
    }
    server.call_forget_insight(fresh_root, None, Some("anything"), true);
    assert!(
        !fresh.path().join(".repolens").exists(),
        "forget created files"
    );
}

#[test]
fn concurrent_forget_deletes_all_requested_nodes() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap().to_string();
    let workers = 16;

    let node_ids: Vec<String> = (0..workers)
        .map(|i| {
            let created = server.call_memorize_insight(
                &root,
                &format!("delete concurrently {i}"),
                None,
                None,
                None,
                None,
            );
            let json: serde_json::Value = serde_json::from_str(&created).unwrap();
            assert_no_tool_error(&json);
            json["id"].as_str().unwrap().to_string()
        })
        .collect();

    let root = Arc::new(root);
    let barrier = Arc::new(Barrier::new(workers));
    let handles: Vec<_> = node_ids
        .iter()
        .map(|node_id| {
            // Per-iteration clone: each id is moved into its worker thread, and
            // `node_ids` is reused by the assertion loop further down.
            let node_id = node_id.clone();
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let server = RepoLensMcpServer::new();
                let result = server.call_forget_insight(&root, Some(&node_id), None, true);
                serde_json::from_str::<serde_json::Value>(&result).expect("valid JSON")
            })
        })
        .collect();

    let responses: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("worker should not panic"))
        .collect();
    for json in &responses {
        assert_no_tool_error(json);
        assert_eq!(json["status"], "deleted", "unexpected response: {json:#}");
        assert_eq!(json["removedCount"], 1, "unexpected response: {json:#}");
    }

    let graph = load_graph(dir.path());
    for node_id in node_ids {
        assert!(
            !graph.nodes.iter().any(|n| n.id == node_id),
            "node {node_id} should have been deleted"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 8: full lifecycle — memorize → recall → forget → recall empty
// ---------------------------------------------------------------------------

#[test]
fn full_lifecycle_memorize_recall_forget() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Step 1: memorize
    let created = server.call_memorize_insight(
        root,
        "use feature flags for gradual rollout",
        Some("PATTERN"),
        Some(0.7),
        Some(vec!["deployment", "rollout"]),
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    assert_eq!(created_json["status"], "created");
    let node_id = created_json["id"].as_str().unwrap().to_string();

    // Step 2: recall — should find 1 result
    let recall1 = server.call_recall_longterm(root, "feature flags rollout", Some(10));
    let r1: serde_json::Value = serde_json::from_str(&recall1).unwrap();
    assert_eq!(
        r1["results"].as_array().unwrap().len(),
        1,
        "should find exactly 1 memory after memorize"
    );

    // Step 3: forget with confirm
    let forget = server.call_forget_insight(root, Some(&node_id), None, true);
    let f: serde_json::Value = serde_json::from_str(&forget).unwrap();
    assert_eq!(f["status"], "deleted");
    assert_eq!(f["removedCount"], 1);

    // Step 4: recall again — should return 0 results
    let recall2 = server.call_recall_longterm(root, "feature flags rollout", Some(10));
    let r2: serde_json::Value = serde_json::from_str(&recall2).unwrap();
    assert_eq!(
        r2["results"].as_array().unwrap().len(),
        0,
        "should find 0 memories after forget"
    );
}

// ---------------------------------------------------------------------------
// Test 9: grounding CONFIRMED when anchor exists in pack
// ---------------------------------------------------------------------------

#[test]
fn recall_grounding_confirmed_when_anchor_exists() {
    let (dir, server) = setup_server();
    let root = dir.path();

    create_pack_with_files(root, &["src/auth.rs", "src/main.rs"]);
    memorize_with_anchors(
        root,
        "authentication login flow",
        CoreMemoryType::Convention,
        vec!["src/auth.rs"],
    );

    let result = server.call_recall_longterm(root.to_str().unwrap(), "authentication login", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(!results.is_empty(), "should find at least one result");
    assert_eq!(results[0]["groundingStatus"], "CONFIRMED");
    assert_eq!(
        results[0]["foundAnchors"],
        serde_json::json!(["src/auth.rs"])
    );
    assert_eq!(results[0]["missingAnchors"], serde_json::json!([]));
}

// ---------------------------------------------------------------------------
// Test 10: grounding CONTRADICTED when anchor file is missing from pack
// ---------------------------------------------------------------------------

#[test]
fn recall_grounding_contradicted_when_anchor_missing() {
    let (dir, server) = setup_server();
    let root = dir.path();

    create_pack_with_files(root, &["src/main.rs"]);
    memorize_with_anchors(
        root,
        "deleted module auth handler",
        CoreMemoryType::Decision,
        vec!["src/deleted_auth.rs"],
    );

    let result = server.call_recall_longterm(root.to_str().unwrap(), "deleted module auth", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(!results.is_empty(), "should find at least one result");
    assert_eq!(results[0]["groundingStatus"], "CONTRADICTED");
    assert_eq!(
        results[0]["missingAnchors"],
        serde_json::json!(["src/deleted_auth.rs"])
    );
}

// ---------------------------------------------------------------------------
// Test 11: grounding UNVERIFIABLE when no pack exists
// ---------------------------------------------------------------------------

#[test]
fn recall_grounding_unverifiable_without_pack() {
    let (dir, server) = setup_server();
    let root = dir.path();

    // No pack created — only memory
    memorize_with_anchors(
        root,
        "anchored memory without pack",
        CoreMemoryType::Pattern,
        vec!["src/something.rs"],
    );

    let result = server.call_recall_longterm(root.to_str().unwrap(), "anchored memory pack", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(!results.is_empty(), "should find at least one result");
    assert_eq!(results[0]["groundingStatus"], "UNVERIFIABLE");
}

// ---------------------------------------------------------------------------
// Test 12: grounding UNCHECKED for bare name anchor (no / or .)
// ---------------------------------------------------------------------------

#[test]
fn recall_grounding_unchecked_for_bare_name_anchor() {
    let (dir, server) = setup_server();
    let root = dir.path();

    create_pack_with_files(root, &["src/auth.rs"]);
    memorize_with_anchors(
        root,
        "AuthService entry point handler",
        CoreMemoryType::CodeAnchor,
        vec!["AuthService"],
    );

    let result =
        server.call_recall_longterm(root.to_str().unwrap(), "AuthService entry point", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(!results.is_empty(), "should find at least one result");
    assert_eq!(results[0]["groundingStatus"], "UNCHECKED");
}

// ---------------------------------------------------------------------------
// Test 13: enum wire values use SCREAMING_SNAKE_CASE
// ---------------------------------------------------------------------------

#[test]
fn recall_grounding_enum_wire_values() {
    let (dir, server) = setup_server();
    let root = dir.path();

    create_pack_with_files(root, &["src/auth.rs"]);
    memorize_with_anchors(
        root,
        "trade off between speed and safety",
        CoreMemoryType::TradeOff,
        vec!["src/auth.rs"],
    );

    let result =
        server.call_recall_longterm(root.to_str().unwrap(), "trade off speed safety", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(!results.is_empty(), "should find at least one result");
    assert_eq!(
        results[0]["type"], "TRADE_OFF",
        "MemoryType should serialize as TRADE_OFF not TRADEOFF"
    );
    assert_eq!(results[0]["groundingStatus"], "CONFIRMED");
}

// ---------------------------------------------------------------------------
// Grounding → decay coupling: recall persists the grounding snapshot onto the
// node so the decay engine can consume it on its next run.
// ---------------------------------------------------------------------------

#[test]
fn recall_persists_contradicted_when_anchors_missing() {
    let (dir, server) = setup_server();
    let root = dir.path();

    // Pack does not contain the anchored path → grounding should contradict.
    create_pack_with_files(root, &["src/main.rs"]);
    let id = memorize_with_anchors(
        root,
        "deleted module auth handler",
        CoreMemoryType::Decision,
        vec!["src/deleted_auth.rs"],
    );

    server.call_recall_longterm(root.to_str().unwrap(), "deleted module auth", None);

    // Reload from disk — the snapshot must be persisted, not just returned.
    let graph = load_graph(root);
    let node = graph
        .nodes
        .iter()
        .find(|n| n.id == id)
        .expect("node should still exist");
    assert_eq!(
        node.last_grounding,
        Some(GroundingStatus::Contradicted),
        "recall should persist a Contradicted grounding snapshot"
    );
    assert!(
        node.last_grounded_at.is_some(),
        "last_grounded_at provenance should be stamped"
    );
}

#[test]
fn recall_persists_confirmed_when_anchors_resolve() {
    let (dir, server) = setup_server();
    let root = dir.path();

    create_pack_with_files(root, &["src/auth.rs", "src/main.rs"]);
    let id = memorize_with_anchors(
        root,
        "authentication login flow",
        CoreMemoryType::Convention,
        vec!["src/auth.rs"],
    );

    server.call_recall_longterm(root.to_str().unwrap(), "authentication login", None);

    let graph = load_graph(root);
    let node = graph
        .nodes
        .iter()
        .find(|n| n.id == id)
        .expect("node should still exist");
    assert_eq!(
        node.last_grounding,
        Some(GroundingStatus::Confirmed),
        "recall should persist a Confirmed grounding snapshot"
    );
    assert!(node.last_grounded_at.is_some());
}

#[test]
fn re_recall_is_stable() {
    let (dir, server) = setup_server();
    let root = dir.path();

    create_pack_with_files(root, &["src/auth.rs"]);
    let id = memorize_with_anchors(
        root,
        "authentication login flow",
        CoreMemoryType::Convention,
        vec!["src/auth.rs"],
    );

    server.call_recall_longterm(root.to_str().unwrap(), "authentication login", None);
    let after_first = load_graph(root);
    let importance_after_first = after_first
        .nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .importance;

    server.call_recall_longterm(root.to_str().unwrap(), "authentication login", None);
    let after_second = load_graph(root);

    // Exactly one node, snapshot unchanged — record_grounding overwrites, never
    // accumulates, so repeated recall does not distort the graph.
    assert_eq!(
        after_second.nodes.len(),
        1,
        "no node duplication on re-recall"
    );
    let node = after_second.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(node.last_grounding, Some(GroundingStatus::Confirmed));
    // Snapshot overwrite carries no importance side-effect, and the second
    // recall is within the 1h decay window so no extra decay applies —
    // importance is stable across recalls.
    assert!(
        (node.importance - importance_after_first).abs() < 1e-4,
        "importance should be stable across recalls: {} vs {}",
        importance_after_first,
        node.importance
    );
}

// --- grounding write-back on get_summary (scan+summary grounding feature) ---

#[test]
fn get_summary_persists_confirmed_for_resolving_insight() {
    let (dir, server) = setup_server();
    let root = dir.path();
    create_pack_with_files(root, &["src/auth.rs"]);
    let id = memorize_with_anchors(
        root,
        "auth login flow",
        CoreMemoryType::Convention,
        vec!["src/auth.rs"],
    );

    server.call_get_summary(root.to_str().unwrap(), "compact");

    let graph = load_graph(root);
    let n = graph.nodes.iter().find(|x| x.id == id).unwrap();
    assert_eq!(n.last_grounding, Some(GroundingStatus::Confirmed));
    assert!(n.last_grounded_at.is_some());
}

#[test]
fn get_summary_full_tier_persists_contradicted() {
    let (dir, server) = setup_server();
    let root = dir.path();
    // Anchor missing from the pack → Contradicted (all anchors missing →
    // resolved=false → surfaced only at the full tier).
    create_pack_with_files(root, &["src/main.rs"]);
    let id = memorize_with_anchors(
        root,
        "deleted module handler",
        CoreMemoryType::Decision,
        vec!["src/deleted.rs"],
    );

    server.call_get_summary(root.to_str().unwrap(), "full");

    let graph = load_graph(root);
    let n = graph.nodes.iter().find(|x| x.id == id).unwrap();
    assert_eq!(n.last_grounding, Some(GroundingStatus::Contradicted));
}

#[test]
fn get_summary_full_tier_stamps_only_selected_20() {
    let (dir, server) = setup_server();
    let root = dir.path();
    create_pack_with_files(root, &["src/auth.rs"]);
    // 21 resolving memories (distinct content so none dedupe); full caps at 20.
    for i in 0..21 {
        memorize_with_anchors(
            root,
            &format!("insight number {i}"),
            CoreMemoryType::Pattern,
            vec!["src/auth.rs"],
        );
    }

    server.call_get_summary(root.to_str().unwrap(), "full");

    let graph = load_graph(root);
    let stamped = graph
        .nodes
        .iter()
        .filter(|n| n.last_grounding.is_some())
        .count();
    assert_eq!(
        stamped, 20,
        "full tier stamps only the 20 surfaced insights"
    );
}

#[test]
fn get_summary_skip_tier_writes_no_grounding() {
    let (dir, _server) = setup_server();
    let root = dir.path();
    create_pack_with_files(root, &["src/auth.rs"]);
    let id = memorize_with_anchors(
        root,
        "auth login flow",
        CoreMemoryType::Convention,
        vec!["src/auth.rs"],
    );

    // get_summary never uses Skip; exercise the Skip path directly (this is what
    // compare_summaries uses) to prove it does NOT write a grounding snapshot.
    let pack_dir = root.join(".repolens/pack-20260405-120000");
    repolens_app::summary::SummaryService::summarize(
        &pack_dir,
        Some(root),
        repolens_app::summary::SummaryMemoryTier::Skip,
    )
    .expect("summarize skip");

    let graph = load_graph(root);
    let n = graph.nodes.iter().find(|x| x.id == id).unwrap();
    assert_eq!(n.last_grounding, None, "Skip tier must not stamp grounding");
}

// #56: a forget query of only stop words matches nothing (#35), even with
// confirm; a single stop word ("IT") is searched like any other word.
#[test]
fn forget_query_of_only_stop_words_matches_nothing() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();
    for note in ["the build of a release", "IT owns the VPN certificates"] {
        server.call_memorize_insight(root, note, None, None, None, None);
    }
    for q in ["the a of", "The, A; OF"] {
        for confirm in [false, true] {
            let json: serde_json::Value =
                serde_json::from_str(&server.call_forget_insight(root, None, Some(q), confirm))
                    .unwrap();
            let key = if confirm { "removedIds" } else { "matches" };
            assert_eq!(json[key].as_array().map(Vec::len), Some(0), "{q}: {json:#}");
            assert_eq!(load_graph(dir.path()).nodes.len(), 2, "{q} deleted notes");
        }
    }
    let json: serde_json::Value =
        serde_json::from_str(&server.call_forget_insight(root, None, Some("IT"), false)).unwrap();
    let matches = json["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1, "{json:#}");
    assert_eq!(matches[0]["content"], "IT owns the VPN certificates");
}
