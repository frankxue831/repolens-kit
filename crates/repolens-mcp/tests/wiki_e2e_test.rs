//! End-to-end test exercising the full wiki workflow against the real repolens-rs repo.
//!
//! Requires a scan pack to exist. Run `cargo run -- scan --root <repo>` first.

use repolens_app::discover::PackResolver;
use repolens_mcp::RepoLensMcpServer;

fn repo_root() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

#[test]
#[ignore] // requires a scan pack: cargo run -- scan --root <repo>
fn wiki_end_to_end_workflow() {
    let root = repo_root();
    let server = RepoLensMcpServer::with_resolver(PackResolver::new());

    // Clean up any leftover wiki state from previous runs
    let wiki_dir = std::path::Path::new(&root).join(".repolens").join("wiki");
    if wiki_dir.exists() {
        std::fs::remove_dir_all(&wiki_dir).ok();
    }

    println!("\n================================================================");
    println!("WIKI E2E TEST - repo: {root}");
    println!("================================================================\n");

    // ---------------------------------------------------------------
    // Step 1: Compile article with real code anchor (FRESH)
    // ---------------------------------------------------------------
    println!("--- Step 1: Compile article about Memory Decay Engine ---");
    let result = server.call_wiki_compile_full(
        &root,
        "Memory Decay Engine",
        "ENTITY",
        "# Memory Decay Engine\n\nThe decay engine applies a 30-day half-life formula to memory node importance.\n\n## Key Properties\n\n- Half-life: 30 days\n- Access floor: min(0.3, access_count * 0.02)\n- Prune threshold: 0.05",
        "Applies 30-day half-life decay to memory node importance",
        Some(vec!["crates/repolens-memory/src/decay.rs"]),
        None,
        Some(vec!["memory", "decay", "architecture"]),
        None,
        None,
    );
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  {}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["status"], "created");
    let decay_id = v["id"].as_str().unwrap().to_string();
    println!("  -> ID: {decay_id}, Staleness: {}", v["staleness"]);
    assert_eq!(v["staleness"], "FRESH", "decay.rs is in the pack");

    // ---------------------------------------------------------------
    // Step 2: Compile concept article with multiple anchors
    // ---------------------------------------------------------------
    println!("\n--- Step 2: Compile concept about Three Truth Layers ---");
    let result = server.call_wiki_compile_full(
        &root,
        "Three Truth Layers",
        "CONCEPT",
        "# Three Truth Layers\n\n1. **Packs** - structural truth\n2. **Memory** - interpretive truth\n3. **Wiki** - compiled truth",
        "RepoLens three-layer truth model",
        Some(vec![
            "crates/repolens-core/src/model/wiki.rs",
            "crates/repolens-memory/src/store.rs",
        ]),
        None,
        Some(vec!["architecture", "philosophy"]),
        Some(0.9),
        None,
    );
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  {}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["status"], "created");
    assert_eq!(v["staleness"], "FRESH", "both anchored files exist in pack");

    // ---------------------------------------------------------------
    // Step 3: Compile article with stale (non-existent) anchor
    // ---------------------------------------------------------------
    println!("\n--- Step 3: Compile article with stale anchor ---");
    let result = server.call_wiki_compile_full(
        &root,
        "Legacy Auth Module",
        "ENTITY",
        "# Legacy Auth Module\n\nRemoved during v2 refactor.",
        "Documentation of the removed auth module",
        Some(vec!["src/auth/legacy_handler.rs"]),
        None,
        Some(vec!["legacy"]),
        Some(0.5),
        None,
    );
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  {}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["status"], "created");
    assert_eq!(v["staleness"], "STALE", "non-existent file should be STALE");
    let stale_id = v["id"].as_str().unwrap().to_string();

    // ---------------------------------------------------------------
    // Step 4: Read article back
    // ---------------------------------------------------------------
    println!("\n--- Step 4: Read the decay engine article ---");
    let result = server.call_wiki_read(&root, &decay_id);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  title: {}", v["article"]["title"]);
    println!("  type:  {}", v["article"]["articleType"]);
    println!("  path:  {}", v["article"]["path"]);
    let body = v["body"].as_str().unwrap();
    println!("  body:  {}...", &body[..body.len().min(60)]);
    assert!(body.contains("30-day half-life"));

    // ---------------------------------------------------------------
    // Step 5: Search the wiki
    // ---------------------------------------------------------------
    println!("\n--- Step 5: Search for 'decay memory' ---");
    let result = server.call_wiki_search(&root, "decay memory", None, None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    let results = v["results"].as_array().unwrap();
    println!("  Found {} results", results.len());
    assert!(!results.is_empty());
    for r in results {
        println!("    - {} (score: {})", r["article"]["title"], r["score"]);
    }

    // ---------------------------------------------------------------
    // Step 6: Search with type filter
    // ---------------------------------------------------------------
    println!("\n--- Step 6: Search for 'truth' filtered to CONCEPT ---");
    let result = server.call_wiki_search(&root, "truth layers", Some("CONCEPT"), None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    let results = v["results"].as_array().unwrap();
    println!("  Found {} concept results", results.len());
    assert!(!results.is_empty());

    // ---------------------------------------------------------------
    // Step 7: Browse the full index
    // ---------------------------------------------------------------
    println!("\n--- Step 7: Browse wiki index ---");
    let result = server.call_wiki_index(&root, None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  {}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["total"], 3);

    // ---------------------------------------------------------------
    // Step 8: Browse index filtered by type
    // ---------------------------------------------------------------
    println!("\n--- Step 8: Index filtered to ENTITY ---");
    let result = server.call_wiki_index(&root, Some("ENTITY"));
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["total"], 2);
    println!("  Entity articles: {}", v["total"]);

    // ---------------------------------------------------------------
    // Step 9: Lint the wiki
    // ---------------------------------------------------------------
    println!("\n--- Step 9: Lint the wiki ---");
    let result = server.call_wiki_lint(&root);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  {}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(v["packAvailable"], true);
    let stale = v["staleArticles"].as_array().unwrap();
    assert_eq!(stale.len(), 1, "should have 1 stale article");
    println!("  Stale: {} article(s)", stale.len());
    println!(
        "  Broken refs: {} ref(s)",
        v["brokenMemoryRefs"].as_array().unwrap().len()
    );

    // ---------------------------------------------------------------
    // Step 10: Update an article
    // ---------------------------------------------------------------
    println!("\n--- Step 10: Update decay engine article ---");
    let result = server.call_wiki_compile_full(
        &root,
        "Memory Decay Engine",
        "ENTITY",
        "# Memory Decay Engine\n\nUpdated: now with wiki staleness detection.\n\n- Half-life: 30 days\n- Prune threshold: 0.05",
        "Memory decay engine - updated with wiki notes",
        Some(vec!["crates/repolens-memory/src/decay.rs"]),
        None,
        Some(vec!["memory", "decay"]),
        None,
        Some(&decay_id),
    );
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["status"], "updated");
    println!("  Updated successfully");

    let result = server.call_wiki_read(&root, &decay_id);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(v["body"]
        .as_str()
        .unwrap()
        .contains("wiki staleness detection"));
    println!("  Body verified");

    // ---------------------------------------------------------------
    // Step 11: Delete (dry run)
    // ---------------------------------------------------------------
    println!("\n--- Step 11: Delete stale article (dry run) ---");
    let result = server.call_wiki_delete(&root, &stale_id, false);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["status"], "dry_run");
    println!("  Dry run OK");

    let result = server.call_wiki_index(&root, None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["total"], 3, "article should still exist");

    // ---------------------------------------------------------------
    // Step 12: Delete (confirmed)
    // ---------------------------------------------------------------
    println!("\n--- Step 12: Delete stale article (confirmed) ---");
    let result = server.call_wiki_delete(&root, &stale_id, true);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["status"], "deleted");

    let result = server.call_wiki_index(&root, None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["total"], 2);
    println!("  Deleted - 2 articles remaining");

    // ---------------------------------------------------------------
    // Step 13: Final state
    // ---------------------------------------------------------------
    println!("\n--- Step 13: Final wiki state ---");
    let result = server.call_wiki_index(&root, None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("  {}", serde_json::to_string_pretty(&v).unwrap());

    // Clean up wiki state
    let wiki_dir = std::path::Path::new(&root).join(".repolens").join("wiki");
    if wiki_dir.exists() {
        std::fs::remove_dir_all(&wiki_dir).ok();
    }

    println!("\n================================================================");
    println!("WIKI E2E TEST PASSED - all 13 steps verified");
    println!("================================================================");
}
