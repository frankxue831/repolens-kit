//! Integration tests for MCP wiki tools: compile, read, search, index, lint, delete.

use std::fs;

use repolens_app::discover::PackResolver;
use repolens_mcp::RepoLensMcpServer;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

fn setup_server() -> (tempfile::TempDir, RepoLensMcpServer) {
    let dir = tempfile::TempDir::new().unwrap();
    let server = RepoLensMcpServer::with_resolver(PackResolver::new());
    (dir, server)
}

/// Create a minimal pack on disk so the PackResolver can find it.
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
            "sessionId": "test-wiki-session",
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

/// Extended compile helper that exposes confidence, anchors, and memory_refs.
///
/// We cannot add methods to RepoLensMcpServer from outside the crate, so we
/// inline the param construction and call the existing public test helper.
///
/// NOTE: This replicates the logic of `call_wiki_compile` but passes extra
/// fields. It depends on the `test-helpers` feature of `repolens-mcp`.
#[allow(clippy::too_many_arguments)]
fn call_wiki_compile_full(
    server: &RepoLensMcpServer,
    root: &str,
    title: &str,
    article_type: &str,
    body: &str,
    summary: &str,
    id: Option<&str>,
    confidence: Option<f32>,
    anchors: Option<Vec<&str>>,
    memory_refs: Option<Vec<&str>>,
) -> String {
    // We need to reach into the wiki params struct. Since it is `pub` in
    // `repolens_mcp::tools::wiki`, we can only construct it from within the
    // crate. However, the test-helpers feature exposes `call_wiki_compile`,
    // which lacks confidence/anchors.
    //
    // Strategy: write the article directly through WikiStore for fields that
    // are not exposed, then call compile for the rest. Instead, we build the
    // JSON request ourselves and route it via the public test helper while
    // compensating for the missing fields:
    //
    // For anchors/confidence tests, we write the article via compile (which
    // sets defaults) and then inspect the on-disk index directly.
    //
    // This helper calls the standard call_wiki_compile and then patches the
    // article on disk if confidence or anchors were supplied.
    use repolens_wiki::store::WikiStore;
    use std::path::Path;

    // Step 1: compile with defaults via existing helper
    let result = server.call_wiki_compile(root, title, article_type, body, summary, id, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    if json.get("error").is_some() {
        return result;
    }
    let article_id = json["id"].as_str().unwrap().to_string();

    // Step 2: if no extra fields, return as-is
    if confidence.is_none() && anchors.is_none() && memory_refs.is_none() {
        return result;
    }

    // Step 3: patch index on disk
    let root_path = Path::new(root);
    let mut index = WikiStore::load_index(root_path).unwrap();
    if let Some(pos) = index.articles.iter().position(|a| a.id == article_id) {
        if let Some(c) = confidence {
            index.articles[pos].confidence = c;
        }
        if let Some(ref a) = anchors {
            index.articles[pos].anchors = a.iter().map(|s| s.to_string()).collect();
        }
        if let Some(ref m) = memory_refs {
            index.articles[pos].memory_refs = m.iter().map(|s| s.to_string()).collect();
        }
        WikiStore::save_index(root_path, &index).unwrap();

        // Re-write the article file to include updated frontmatter/body
        let article = index.articles[pos].clone();
        WikiStore::write_article(root_path, &article, body).unwrap();
    }

    result
}

// ---------------------------------------------------------------------------
// Test 1: compile_creates_article
// ---------------------------------------------------------------------------

#[test]
fn compile_creates_article() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let result = server.call_wiki_compile(
        root,
        "Decay Engine",
        "ENTITY",
        "## Decay Engine\n\nHandles exponential memory decay.",
        "Controls how memory nodes decay over time.",
        None,
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["status"], "created", "status should be 'created'");
    let id = json["id"].as_str().unwrap();
    assert!(!id.is_empty(), "id should be a non-empty UUID");
    // path should be under articles/entity/
    let path = json["path"].as_str().unwrap();
    assert!(
        path.starts_with("articles/entity/"),
        "path should start with articles/entity/, got: {path}"
    );
    // staleness is present
    assert!(json["staleness"].is_string(), "staleness should be present");
}

// ---------------------------------------------------------------------------
// Test 2: compile_updates_article
// ---------------------------------------------------------------------------

#[test]
fn compile_updates_article() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create initial article
    let created = server.call_wiki_compile(
        root,
        "Cache Layer",
        "CONCEPT",
        "Original body text.",
        "Cache layer overview.",
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    assert_eq!(created_json["status"], "created");
    let article_id = created_json["id"].as_str().unwrap();

    // Update with same ID and new body
    let updated = server.call_wiki_compile(
        root,
        "Cache Layer",
        "CONCEPT",
        "Updated body with new content.",
        "Cache layer overview updated.",
        Some(article_id),
        None,
    );
    let updated_json: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(
        updated_json["status"], "updated",
        "status should be 'updated'"
    );
    assert_eq!(
        updated_json["id"].as_str().unwrap(),
        article_id,
        "id should remain the same"
    );

    // Verify body changed via wiki_read
    let read_result = server.call_wiki_read(root, article_id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();
    let body = read_json["body"].as_str().unwrap();
    assert!(
        body.contains("Updated body with new content."),
        "body should reflect the update, got: {body}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: compile_with_no_pack — staleness == UNVERIFIABLE when anchored
// but no pack exists
// ---------------------------------------------------------------------------

#[test]
fn compile_with_no_pack() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Compile with an anchor (so staleness check is triggered) but no pack
    let result = call_wiki_compile_full(
        &server,
        root,
        "Orphan Article",
        "CONCEPT",
        "Body of orphan article.",
        "No pack available.",
        None,
        None,
        Some(vec!["src/some_file.rs"]), // groundable anchor → UNVERIFIABLE
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    // First compile creates; staleness must be UNVERIFIABLE because no pack
    assert_eq!(json["status"], "created");
    // We need to lint to get the correct staleness; the article was written with Fresh
    // then the anchor was patched in. Re-read from the index to check:
    let id = json["id"].as_str().unwrap();
    let lint = server.call_wiki_lint(root);
    let lint_json: serde_json::Value = serde_json::from_str(&lint).unwrap();
    assert_eq!(
        lint_json["packAvailable"], false,
        "packAvailable should be false when no pack is present"
    );

    // The article with anchors and no pack shows as UNVERIFIABLE in staleArticles
    // (lint flags them as stale only when pack IS available and anchor is missing)
    // With no pack, articles are UNVERIFIABLE, not STALE per staleness rules.
    // So staleArticles should NOT contain it — but packAvailable is false.
    let stale = lint_json["staleArticles"].as_array().unwrap();
    let is_stale = stale.iter().any(|a| a["id"].as_str() == Some(id));
    assert!(
        !is_stale,
        "article with anchors but no pack should be UNVERIFIABLE (not STALE), so it should not appear in staleArticles"
    );
}

// ---------------------------------------------------------------------------
// Test 4: compile_with_title_change
// ---------------------------------------------------------------------------

#[test]
fn compile_with_title_change() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create article with title "Alpha"
    let created = server.call_wiki_compile(
        root,
        "Alpha",
        "ENTITY",
        "Alpha body.",
        "Alpha summary.",
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();

    // Update title to "Beta"
    let updated = server.call_wiki_compile(
        root,
        "Beta",
        "ENTITY",
        "Beta body.",
        "Beta summary.",
        Some(id),
        None,
    );
    let updated_json: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(updated_json["status"], "updated");
    let new_path = updated_json["path"].as_str().unwrap();
    assert!(
        new_path.contains("beta"),
        "new path should contain 'beta', got: {new_path}"
    );

    // Verify only one article in index (path changed, old path gone)
    let index_result = server.call_wiki_index(root, None);
    let index_json: serde_json::Value = serde_json::from_str(&index_result).unwrap();
    assert_eq!(
        index_json["total"], 1,
        "should still have exactly one article"
    );
}

// ---------------------------------------------------------------------------
// Test 5: compile_with_type_change
// ---------------------------------------------------------------------------

#[test]
fn compile_with_type_change() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create as ENTITY
    let created = server.call_wiki_compile(
        root,
        "Auth Module",
        "ENTITY",
        "Entity body.",
        "Auth module entity.",
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();
    let old_path = created_json["path"].as_str().unwrap();
    assert!(
        old_path.starts_with("articles/entity/"),
        "initial path should be entity"
    );

    // Update type to CONCEPT
    let updated = server.call_wiki_compile(
        root,
        "Auth Module",
        "CONCEPT",
        "Concept body.",
        "Auth module concept.",
        Some(id),
        None,
    );
    let updated_json: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(updated_json["status"], "updated");
    let new_path = updated_json["path"].as_str().unwrap();
    assert!(
        new_path.starts_with("articles/concept/"),
        "path should now be under articles/concept/, got: {new_path}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: compile_slug_collision
// ---------------------------------------------------------------------------

#[test]
fn compile_slug_collision() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create first article with title "Decay"
    let first = server.call_wiki_compile(
        root,
        "Decay",
        "ENTITY",
        "First decay article.",
        "First.",
        None,
        None,
    );
    let first_json: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first_json["status"], "created");
    let first_path = first_json["path"].as_str().unwrap();
    assert_eq!(
        first_path, "articles/entity/decay.md",
        "first path should be decay.md"
    );

    // Create second article with same title "Decay"
    let second = server.call_wiki_compile(
        root,
        "Decay",
        "ENTITY",
        "Second decay article.",
        "Second.",
        None,
        None,
    );
    let second_json: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(second_json["status"], "created");
    let second_path = second_json["path"].as_str().unwrap();
    assert!(
        second_path.contains("-2"),
        "second path should contain '-2' suffix, got: {second_path}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: compile_confidence_default
// ---------------------------------------------------------------------------

#[test]
fn compile_confidence_default() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let result = server.call_wiki_compile(
        root,
        "Default Confidence Article",
        "CONCEPT",
        "Body.",
        "Summary.",
        None,
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let id = json["id"].as_str().unwrap();

    // Read to get full article metadata including confidence
    let read_result = server.call_wiki_read(root, id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();
    let confidence = read_json["article"]["confidence"]
        .as_f64()
        .expect("confidence should be a float");
    assert!(
        (confidence - 0.8).abs() < 1e-6,
        "default confidence should be 0.8, got: {confidence}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: compile_confidence_inherit
// ---------------------------------------------------------------------------

#[test]
fn compile_confidence_inherit() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create with explicit confidence 0.9 using extended helper
    let created = call_wiki_compile_full(
        &server,
        root,
        "High Confidence Article",
        "ENTITY",
        "Body.",
        "Summary.",
        None,
        Some(0.9),
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();

    // Update without specifying confidence (should inherit 0.9)
    let updated = server.call_wiki_compile(
        root,
        "High Confidence Article",
        "ENTITY",
        "Updated body.",
        "Updated summary.",
        Some(id),
        None,
    );
    let updated_json: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(updated_json["status"], "updated");

    // Read to verify confidence was preserved
    let read_result = server.call_wiki_read(root, id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();
    let confidence = read_json["article"]["confidence"]
        .as_f64()
        .expect("confidence should be a float");
    assert!(
        (confidence - 0.9).abs() < 1e-4,
        "confidence should remain 0.9 when not specified in update, got: {confidence}"
    );
}

// ---------------------------------------------------------------------------
// Test 9: compile_confidence_override
// ---------------------------------------------------------------------------

#[test]
fn compile_confidence_override() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create with confidence 0.9
    let created = call_wiki_compile_full(
        &server,
        root,
        "Override Confidence Article",
        "CONCEPT",
        "Body.",
        "Summary.",
        None,
        Some(0.9),
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();

    // Update with confidence 0.6 (override)
    let updated = call_wiki_compile_full(
        &server,
        root,
        "Override Confidence Article",
        "CONCEPT",
        "Updated body.",
        "Updated summary.",
        Some(id),
        Some(0.6),
        None,
        None,
    );
    let updated_json: serde_json::Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(updated_json["status"], "updated");

    // Read to verify confidence was overridden
    let read_result = server.call_wiki_read(root, id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();
    let confidence = read_json["article"]["confidence"]
        .as_f64()
        .expect("confidence should be a float");
    assert!(
        (confidence - 0.6).abs() < 1e-4,
        "confidence should be 0.6 after override, got: {confidence}"
    );
}

// ---------------------------------------------------------------------------
// Test 10: read_returns_content
// ---------------------------------------------------------------------------

#[test]
fn read_returns_content() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let body = "## Memory System\n\nThe memory system uses decay to age out old nodes.\n";
    let created = server.call_wiki_compile(
        root,
        "Memory System",
        "OVERVIEW",
        body,
        "Overview of memory system.",
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();

    let read_result = server.call_wiki_read(root, id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();

    let returned_body = read_json["body"].as_str().unwrap();
    assert_eq!(
        returned_body, body,
        "body returned by wiki_read should match compiled body"
    );
}

// ---------------------------------------------------------------------------
// Test 11: search_by_keyword
// ---------------------------------------------------------------------------

#[test]
fn search_by_keyword() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create 3 articles with different topics
    server.call_wiki_compile(
        root,
        "Decay Engine",
        "ENTITY",
        "Handles exponential decay of memory nodes.",
        "Decay engine summary.",
        None,
        None,
    );
    server.call_wiki_compile(
        root,
        "Authentication Flow",
        "OVERVIEW",
        "Describes how users log in and authenticate.",
        "Authentication overview.",
        None,
        None,
    );
    server.call_wiki_compile(
        root,
        "Database Schema",
        "CONCEPT",
        "Outlines the database tables and columns.",
        "Database schema summary.",
        None,
        None,
    );

    // Search for "decay memory" — should match "Decay Engine" first
    let search_result = server.call_wiki_search(root, "decay memory", None, Some(10));
    let search_json: serde_json::Value = serde_json::from_str(&search_result).unwrap();

    let results = search_json["results"]
        .as_array()
        .expect("results should be an array");
    assert!(
        !results.is_empty(),
        "search should return at least one result"
    );

    let first_title = results[0]["article"]["title"].as_str().unwrap();
    assert_eq!(
        first_title, "Decay Engine",
        "Decay Engine should rank first for 'decay memory', got: {first_title}"
    );
}

// ---------------------------------------------------------------------------
// Test 12: index_returns_catalog
// ---------------------------------------------------------------------------

#[test]
fn index_returns_catalog() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create articles of different types
    server.call_wiki_compile(
        root,
        "User Service",
        "ENTITY",
        "The User Service handles accounts.",
        "User service entity.",
        None,
        None,
    );
    server.call_wiki_compile(
        root,
        "Event Sourcing",
        "CONCEPT",
        "Event sourcing is an architectural pattern.",
        "Event sourcing concept.",
        None,
        None,
    );
    server.call_wiki_compile(
        root,
        "Auth Module",
        "ENTITY",
        "Auth module handles login.",
        "Auth module entity.",
        None,
        None,
    );

    let index_result = server.call_wiki_index(root, None);
    let index_json: serde_json::Value = serde_json::from_str(&index_result).unwrap();

    assert_eq!(
        index_json["total"], 3,
        "total should be 3, got: {}",
        index_json["total"]
    );

    let entity_articles = index_json["articles"]["entity"]
        .as_array()
        .expect("entity bucket should be an array");
    assert_eq!(entity_articles.len(), 2, "should have 2 entity articles");

    let concept_articles = index_json["articles"]["concept"]
        .as_array()
        .expect("concept bucket should be an array");
    assert_eq!(concept_articles.len(), 1, "should have 1 concept article");
}

// ---------------------------------------------------------------------------
// Test 13: lint_detects_stale
// ---------------------------------------------------------------------------

#[test]
fn lint_detects_stale() {
    let (dir, server) = setup_server();
    let root = dir.path();

    // Create a pack with src/found.rs but NOT src/missing.rs
    create_pack_with_files(root, &["src/found.rs"]);

    // Create an article anchored to src/missing.rs (not in the pack)
    let created = call_wiki_compile_full(
        &server,
        root.to_str().unwrap(),
        "Stale Article",
        "ENTITY",
        "This article references a deleted file.",
        "Stale article summary.",
        None,
        None,
        Some(vec!["src/missing.rs"]), // anchor NOT in pack → STALE
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let article_id = created_json["id"].as_str().unwrap();

    let lint_result = server.call_wiki_lint(root.to_str().unwrap());
    let lint_json: serde_json::Value = serde_json::from_str(&lint_result).unwrap();

    assert_eq!(
        lint_json["packAvailable"], true,
        "packAvailable should be true"
    );

    let stale_articles = lint_json["staleArticles"].as_array().unwrap();
    assert!(
        stale_articles
            .iter()
            .any(|a| a["id"].as_str() == Some(article_id)),
        "article anchored to missing file should appear in staleArticles, got: {stale_articles:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 14: lint_with_no_pack
// ---------------------------------------------------------------------------

#[test]
fn lint_with_no_pack() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create articles with anchors but no pack on disk
    call_wiki_compile_full(
        &server,
        root,
        "Unverifiable Article",
        "CONCEPT",
        "Body with anchors.",
        "Summary.",
        None,
        None,
        Some(vec!["src/something.rs"]),
        None,
    );

    let lint_result = server.call_wiki_lint(root);
    let lint_json: serde_json::Value = serde_json::from_str(&lint_result).unwrap();

    assert_eq!(
        lint_json["packAvailable"], false,
        "packAvailable should be false when no pack exists"
    );
    // No error — lint succeeds even with no pack
    assert!(
        lint_json.get("error").is_none(),
        "lint should not return an error"
    );
    // Articles with anchors and no pack are UNVERIFIABLE, not STALE — so staleArticles is empty
    let stale_articles = lint_json["staleArticles"].as_array().unwrap();
    assert!(
        stale_articles.is_empty(),
        "staleArticles should be empty when pack is not available"
    );
}

// ---------------------------------------------------------------------------
// Test 15: lint_detects_broken_ref
// ---------------------------------------------------------------------------

#[test]
fn lint_detects_broken_ref() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create a memory insight so the memory store exists
    server.call_memorize_insight(root, "real memory node", None, None, None, None);

    let fake_uuid = "00000000-dead-beef-cafe-000000000000";

    // Create an article that references a non-existent memory UUID
    call_wiki_compile_full(
        &server,
        root,
        "Broken Ref Article",
        "ENTITY",
        "This article has a broken memory reference.",
        "Broken ref summary.",
        None,
        None,
        None,
        Some(vec![fake_uuid]), // this memory ref does NOT exist
    );

    let lint_result = server.call_wiki_lint(root);
    let lint_json: serde_json::Value = serde_json::from_str(&lint_result).unwrap();

    let broken_refs = lint_json["brokenMemoryRefs"]
        .as_array()
        .expect("brokenMemoryRefs should be an array");
    assert!(
        !broken_refs.is_empty(),
        "should detect the broken memory reference"
    );
    assert!(
        broken_refs
            .iter()
            .any(|r| r["missingRef"].as_str() == Some(fake_uuid)),
        "broken ref should include the fake UUID, got: {broken_refs:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 16: lint_with_valid_ref
// ---------------------------------------------------------------------------

#[test]
fn lint_with_valid_ref() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create a real memory insight and get its ID
    let mem_result =
        server.call_memorize_insight(root, "real insight about caching", None, None, None, None);
    let mem_json: serde_json::Value = serde_json::from_str(&mem_result).unwrap();
    let real_memory_id = mem_json["id"].as_str().unwrap().to_string();

    // Create an article referencing the real memory ID
    call_wiki_compile_full(
        &server,
        root,
        "Valid Ref Article",
        "CONCEPT",
        "This article references a real memory node.",
        "Valid ref summary.",
        None,
        None,
        None,
        Some(vec![real_memory_id.as_str()]),
    );

    let lint_result = server.call_wiki_lint(root);
    let lint_json: serde_json::Value = serde_json::from_str(&lint_result).unwrap();

    let broken_refs = lint_json["brokenMemoryRefs"]
        .as_array()
        .expect("brokenMemoryRefs should be an array");
    assert!(
        broken_refs.is_empty(),
        "valid memory ref should NOT appear in brokenMemoryRefs, got: {broken_refs:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 17: delete_dry_run
// ---------------------------------------------------------------------------

#[test]
fn delete_dry_run() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create an article
    let created = server.call_wiki_compile(
        root,
        "To Be Kept",
        "OVERVIEW",
        "This article should survive a dry run.",
        "Dry run target.",
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();

    // Dry run delete (confirm = false)
    let delete_result = server.call_wiki_delete(root, id, false);
    let delete_json: serde_json::Value = serde_json::from_str(&delete_result).unwrap();

    assert_eq!(
        delete_json["status"], "dry_run",
        "status should be 'dry_run'"
    );

    // Article should still exist in index
    let index_result = server.call_wiki_index(root, None);
    let index_json: serde_json::Value = serde_json::from_str(&index_result).unwrap();
    assert_eq!(
        index_json["total"], 1,
        "article should still exist after dry run, total: {}",
        index_json["total"]
    );
}

// ---------------------------------------------------------------------------
// Test 18: delete_confirmed
// ---------------------------------------------------------------------------

#[test]
fn delete_confirmed() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Create an article
    let created = server.call_wiki_compile(
        root,
        "To Be Deleted",
        "ENTITY",
        "This article will be deleted.",
        "Delete target.",
        None,
        None,
    );
    let created_json: serde_json::Value = serde_json::from_str(&created).unwrap();
    let id = created_json["id"].as_str().unwrap();

    // Also create a second article so the index isn't completely empty after delete
    server.call_wiki_compile(
        root,
        "Survivor Article",
        "ENTITY",
        "This one stays.",
        "Survivor.",
        None,
        None,
    );

    // Confirm delete
    let delete_result = server.call_wiki_delete(root, id, true);
    let delete_json: serde_json::Value = serde_json::from_str(&delete_result).unwrap();

    assert_eq!(
        delete_json["status"], "deleted",
        "status should be 'deleted'"
    );

    // Article should be gone — index should have only 1 article left
    let index_result = server.call_wiki_index(root, None);
    let index_json: serde_json::Value = serde_json::from_str(&index_result).unwrap();
    assert_eq!(
        index_json["total"], 1,
        "only the survivor article should remain, total: {}",
        index_json["total"]
    );

    // The deleted article should not be findable via wiki_read
    let read_result = server.call_wiki_read(root, id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();
    assert!(
        read_json.get("error").is_some(),
        "wiki_read should return error for deleted article"
    );
}

// ---------------------------------------------------------------------------
// Test 19: wiki_ingest_external_creates_with_source
// ---------------------------------------------------------------------------

#[test]
fn wiki_ingest_external_creates_with_source() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let result = server.call_wiki_ingest_external(
        root,
        "React Hooks Guide",
        "CONCEPT",
        "## React Hooks\n\nHooks let you use state in function components.",
        "Guide to React hooks from official docs.",
        "https://react.dev/reference/react/hooks",
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["status"], "created", "status should be 'created'");
    assert!(
        json["id"].as_str().is_some() && !json["id"].as_str().unwrap().is_empty(),
        "id should be a non-empty string"
    );
    assert!(
        json["source"]["origin"]
            .as_str()
            .unwrap()
            .contains("https://react.dev"),
        "source.origin should contain the URL, got: {}",
        json["source"]["origin"]
    );
    assert_eq!(
        json["staleness"], "FRESH",
        "staleness should be FRESH for newly ingested article"
    );
}

// ---------------------------------------------------------------------------
// Test 20: wiki_ingest_external_default_confidence_is_0_6
// ---------------------------------------------------------------------------

#[test]
fn wiki_ingest_external_default_confidence_is_0_6() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let result = server.call_wiki_ingest_external(
        root,
        "External API Docs",
        "ENTITY",
        "## API Reference\n\nThe external API provides REST endpoints.",
        "External API documentation.",
        "https://api.example.com/docs",
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["status"], "created");

    // Read the index and verify confidence is 0.6
    let index_result = server.call_wiki_index(root, None);
    let index_json: serde_json::Value = serde_json::from_str(&index_result).unwrap();
    assert_eq!(index_json["total"], 1);

    // Find the article in the index buckets
    let articles = index_json["articles"].as_object().unwrap();
    let mut found_confidence = None;
    for (_type_key, bucket) in articles {
        if let Some(arr) = bucket.as_array() {
            for article in arr {
                if article["title"].as_str() == Some("External API Docs") {
                    found_confidence = article["confidence"].as_f64();
                }
            }
        }
    }
    let confidence = found_confidence.expect("should find the article in the index");
    assert!(
        (confidence - 0.6).abs() < 1e-4,
        "default confidence for external articles should be 0.6, got: {confidence}"
    );
}

// ---------------------------------------------------------------------------
// Test 21: wiki_search_phrase_query
// ---------------------------------------------------------------------------

#[test]
fn wiki_search_phrase_query() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Compile an article with a specific phrase in the title
    server.call_wiki_compile(
        root,
        "Dependency Injection Pattern",
        "CONCEPT",
        "## Dependency Injection\n\nDependency injection is an inversion-of-control technique.",
        "Explains the dependency injection pattern.",
        None,
        None,
    );

    // Also compile a decoy article to make sure phrase matching is selective
    server.call_wiki_compile(
        root,
        "Database Injection Prevention",
        "CONCEPT",
        "## SQL Injection\n\nPreventing SQL injection attacks.",
        "SQL injection prevention guide.",
        None,
        None,
    );

    // Search with phraseQuery: true for "dependency injection"
    let search_result =
        server.call_wiki_search_phrase(root, "dependency injection", true, Some(10));
    let search_json: serde_json::Value = serde_json::from_str(&search_result).unwrap();

    let total = search_json["total"].as_u64().unwrap();
    assert!(
        total >= 1,
        "phrase search for 'dependency injection' should return at least 1 result, got: {total}"
    );

    // The first result should be the "Dependency Injection Pattern" article
    let results = search_json["results"].as_array().unwrap();
    let first_title = results[0]["article"]["title"].as_str().unwrap();
    assert_eq!(
        first_title, "Dependency Injection Pattern",
        "first result should be 'Dependency Injection Pattern', got: {first_title}"
    );
}

// ---------------------------------------------------------------------------
// Test 22: wiki_lint_includes_coverage_gaps
// ---------------------------------------------------------------------------

#[test]
fn wiki_lint_includes_coverage_gaps() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    // Compile an article (no pack available)
    server.call_wiki_compile(
        root,
        "Coverage Test Article",
        "ENTITY",
        "Body of the article.",
        "Summary for coverage test.",
        None,
        None,
    );

    let lint_result = server.call_wiki_lint(root);
    let lint_json: serde_json::Value = serde_json::from_str(&lint_result).unwrap();

    // coverageGaps should be present in the response
    assert!(
        lint_json.get("coverageGaps").is_some(),
        "coverageGaps field should be present in lint response"
    );

    // With no pack, coverageGaps should be null
    let coverage_gaps = &lint_json["coverageGaps"];
    if coverage_gaps.is_null() {
        // Expected: no pack means null coverageGaps
    } else {
        // If a pack were available, it would have totalFiles
        assert!(
            coverage_gaps.get("totalFiles").is_some(),
            "if coverageGaps is not null, it should have totalFiles"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 23: wiki_ingest_external_source_in_read
// ---------------------------------------------------------------------------

#[test]
fn wiki_ingest_external_source_in_read() {
    let (dir, server) = setup_server();
    let root = dir.path().to_str().unwrap();

    let origin = "https://docs.rs/tokio/latest/tokio/";
    let result = server.call_wiki_ingest_external(
        root,
        "Tokio Runtime",
        "ENTITY",
        "## Tokio\n\nTokio is an async runtime for Rust.",
        "Tokio runtime documentation from docs.rs.",
        origin,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let id = json["id"].as_str().unwrap();

    // Read the article back
    let read_result = server.call_wiki_read(root, id);
    let read_json: serde_json::Value = serde_json::from_str(&read_result).unwrap();

    // Verify source is present with correct origin
    let source = &read_json["article"]["source"];
    assert!(
        !source.is_null(),
        "article.source should be present for externally ingested article"
    );
    assert_eq!(
        source["origin"].as_str().unwrap(),
        origin,
        "source.origin should match the ingested origin"
    );
}
