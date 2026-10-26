//! Integration test: scan a temp repo, then call MCP tool methods directly.

use std::fs;
use std::path::Path;

use repolens_app::scan::ScanService;
use repolens_mcp::RepoLensMcpServer;

fn setup_scanned_repo(root: &Path) -> String {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/App.java"),
        "package com.example;\n\npublic class App {\n    public void run() {\n        System.out.println(\"hello world\");\n    }\n}\n",
    ).unwrap();
    fs::write(root.join("README.md"), "# My Project\n\nSome docs here.\n").unwrap();

    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

/// Creates a 3-node Java graph: App → Helper → Base (import direction).
/// Impact direction (reverse): changing Base affects Helper (depth 1), then App (depth 2).
fn setup_scanned_repo_with_graph(root: &Path) -> String {
    fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
    fs::write(
        root.join("src/main/java/com/example/Base.java"),
        "package com.example;\n\npublic class Base {\n    public String value() { return \"base\"; }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main/java/com/example/Helper.java"),
        "package com.example;\n\nimport com.example.Base;\n\npublic class Helper {\n    public String help() {\n        Base b = new Base();\n        return b.value();\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main/java/com/example/App.java"),
        "package com.example;\n\nimport com.example.Helper;\n\npublic class App {\n    public void run() {\n        Helper h = new Helper();\n        System.out.println(h.help());\n    }\n}\n",
    )
    .unwrap();
    fs::write(root.join("README.md"), "# My Project\n\nSome docs here.\n").unwrap();

    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

/// Creates a Rust module tree reproducing the dogfood-round-2 declares
/// inflation shape (docs/evals/runs/dogfood-round2-gm-crypto-rs.md, finding 4):
///
/// - `lib.rs` declares `m` and `user` (declares edges)
/// - `m/mod.rs` declares `child_a` and `child_b` (declares edges)
/// - `child_b.rs` imports+calls `crate::m::shared` (real edges → mod.rs)
/// - `user.rs` imports+calls `crate::m::child_a::alpha` (real edges → child_a.rs)
///
/// Seeding impact at `child_a.rs`: the only honest dependent is `user.rs`.
/// Traversing declares edges would admit `m/mod.rs` (container), then pull in
/// `lib.rs` and the unrelated sibling `child_b.rs` through it.
fn setup_rust_module_tree_repo(root: &Path) -> String {
    fs::create_dir_all(root.join("src/m")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub mod m;\npub mod user;\n").unwrap();
    fs::write(
        root.join("src/m/mod.rs"),
        "pub mod child_a;\npub mod child_b;\n\npub fn shared() -> i32 {\n    7\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/m/child_a.rs"),
        "pub fn alpha() -> i32 {\n    1\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/m/child_b.rs"),
        "use crate::m::shared;\n\npub fn beta() -> i32 {\n    shared() + 2\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/user.rs"),
        "use crate::m::child_a::alpha;\n\npub fn use_alpha() -> i32 {\n    alpha()\n}\n",
    )
    .unwrap();

    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

/// Characterization test for `wiki_suggest` — the one MCP tool with no prior
/// test coverage. Pins both the data shape (ranked suggestions + coverage) and
/// the epistemic envelope it emits via the 4-arg `ok_epistemic` shim.
#[test]
fn wiki_suggest_ranks_uncovered_files_and_emits_epistemic_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Real scan -> pack with graph.json. Base <- Helper <- App gives Base
    // inbound centrality, so the ranking has something to order by.
    setup_scanned_repo_with_graph(root);
    let root_str = root.to_str().unwrap();

    let server = RepoLensMcpServer::new();

    // No wiki articles compiled => every scanned file is an uncovered gap.
    let data: serde_json::Value =
        serde_json::from_str(&server.call_wiki_suggest(root_str, None, None)).unwrap();

    let suggestions = data["suggestions"]
        .as_array()
        .expect("suggestions must be an array");
    assert!(
        !suggestions.is_empty(),
        "empty wiki over a scanned repo should yield suggestions; got {data}"
    );
    for s in suggestions {
        for key in [
            "filePath",
            "importance",
            "reason",
            "suggestedType",
            "suggestedTitle",
            "symbols",
        ] {
            assert!(s.get(key).is_some(), "suggestion missing `{key}`: {s}");
        }
    }
    // Suggestions are ranked by importance, descending.
    let importances: Vec<f64> = suggestions
        .iter()
        .map(|s| s["importance"].as_f64().expect("importance is a number"))
        .collect();
    assert!(
        importances.windows(2).all(|w| w[0] >= w[1]),
        "suggestions must be sorted by importance desc; got {importances:?}"
    );

    let coverage = &data["coverage"];
    assert!(coverage["uncoveredFiles"].is_number());
    assert!(coverage["totalFiles"].is_number());
    // No articles => 0% coverage.
    assert_eq!(
        coverage["coveragePercent"].as_f64().unwrap(),
        0.0,
        "an empty wiki must report 0% coverage; got {coverage}"
    );

    // Envelope: wiki_suggest uses the 4-arg `ok_epistemic` shim, so it carries a
    // multi-layer knowledgeLayer, Variable confidence, the default `unaudited`
    // grade, and two provenance sources (wiki index + pack extraction).
    let env = server.call_wiki_suggest_envelope(root_str, None, None);
    assert_eq!(
        env["knowledgeLayer"],
        serde_json::json!(["observation", "interpretation"])
    );
    assert_eq!(env["confidence"], "variable");
    assert_eq!(env["evidenceGrade"], "unaudited");
    let provenance = env["provenance"]
        .as_array()
        .expect("provenance must be an array");
    assert_eq!(
        provenance.len(),
        2,
        "expected wiki-index + pack-extraction provenance; got {provenance:?}"
    );
    assert!(env["data"]["suggestions"].is_array());
}

#[test]
fn discover_packs_finds_scanned_pack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_discover_packs(root.to_str().unwrap());
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["latestPack"].as_str().unwrap(), pack_path);
    // `sessionId` was retired from the response shape in the 2026-05-13
    // discover_packs expansion — no MCP tool consumed it. The session is
    // still used internally by `PackResolver::discover` to pick the active
    // pack; only the response field is gone.
    assert!(
        json.get("sessionId").is_none(),
        "sessionId was retired from the response; got {:?}",
        json.get("sessionId")
    );
    // `latestPackId` and `availablePacks` are the new ergonomic surfaces.
    let latest_id = json["latestPackId"]
        .as_str()
        .expect("latestPackId must be present");
    assert!(
        latest_id.starts_with("pack-"),
        "latestPackId must be the canonical pack-YYYYMMDD-HHMMSS form, got {latest_id}"
    );
    let available = json["availablePacks"]
        .as_array()
        .expect("availablePacks must be present");
    assert_eq!(
        available.len(),
        1,
        "single-pack repo must surface exactly one entry"
    );
    let entry = &available[0];
    assert_eq!(entry["packId"].as_str().unwrap(), latest_id);
    assert_eq!(entry["path"].as_str().unwrap(), pack_path);
}

#[test]
fn discover_packs_envelope_grade_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_discover_packs_envelope(root.to_str().unwrap());

    // Pack-directory enumeration is a direct filesystem read with no
    // heuristics — promoted to `verified` in the 2026-05-13 discover_packs
    // expansion. The legacy `unaudited` default applied when the tool used
    // `ok_epistemic` (the 4-arg shim) instead of `ok_epistemic_graded`.
    assert_eq!(envelope["evidenceGrade"], "verified");
}

#[test]
fn discover_packs_lists_every_pack_dir_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Scan twice to produce two packs in `.repolens/`.
    let pack1 = setup_scanned_repo(root);
    // Mutate the working tree so the second scan produces a distinct pack.
    fs::write(
        root.join("src/Other.java"),
        "package com.example;\npublic class Other {}\n",
    )
    .unwrap();
    // Sleep briefly so the second pack's timestamp dir name sorts after
    // the first. ScanService names packs as `pack-YYYYMMDD-HHMMSS`.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let result2 = ScanService::new().run(root, false, 100).unwrap();
    let pack2 = result2.pack_dir.to_string_lossy().to_string();

    let server = RepoLensMcpServer::new();
    let result = server.call_discover_packs(root.to_str().unwrap());
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let available = json["availablePacks"].as_array().unwrap();
    assert_eq!(available.len(), 2, "two scans must produce two entries");
    // Sorted ascending by packId.
    let paths: Vec<&str> = available
        .iter()
        .map(|e| e.get("path").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(paths, vec![pack1.as_str(), pack2.as_str()]);
}

#[test]
fn compare_summaries_accepts_pack_id_alias_with_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack1 = setup_scanned_repo(root);

    // Produce a second pack via rescan.
    fs::write(
        root.join("src/Other.java"),
        "package com.example;\npublic class Other {}\n",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let result2 = ScanService::new().run(root, false, 100).unwrap();
    let pack2 = result2.pack_dir.to_string_lossy().to_string();

    // Extract pack-ids from the absolute paths.
    let pack1_id = Path::new(&pack1)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let pack2_id = Path::new(&pack2)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let server = RepoLensMcpServer::new();
    // Call compare_summaries with packId aliases — same response as
    // calling with absolute paths.
    let env = server.call_compare_summaries_with_root_envelope(
        &pack1_id,
        &pack2_id,
        root.to_str().unwrap(),
    );
    // Sanity: a successful compare emits the triage envelope on data.
    assert_eq!(
        env["data"]["triageHeader"]["reportKind"],
        "structuralTriage"
    );
    // The triage header packId is the after-pack's canonical id.
    assert_eq!(env["data"]["triageHeader"]["scope"]["packId"], pack2_id);
}

#[test]
fn compare_summaries_rejects_pack_id_without_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);
    fs::write(
        root.join("src/Other.java"),
        "package com.example;\npublic class Other {}\n",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    ScanService::new().run(root, false, 100).unwrap();

    let server = RepoLensMcpServer::new();
    // Without `root`, packIds cannot be resolved — must be a structured
    // error, not a silent path-pass-through that crashes at pack-read time.
    let err =
        server.try_call_compare_summaries("pack-20260101-000000", "pack-20260102-000000", None);
    let msg = err.expect_err("compare_summaries with packId aliases and no root must error");
    assert!(
        msg.contains("packId") && msg.contains("root"),
        "error must explain the missing root requirement, got: {msg}"
    );
}

#[test]
fn read_file_content_returns_full_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content(&pack_path, "src/App.java", None, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["path"], "src/App.java");
    assert_eq!(json["language"], "Java");
    assert_eq!(json["totalLines"], 7);
    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("public class App"));
}

#[test]
fn read_file_content_with_line_range() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content(&pack_path, "src/App.java", Some(3), Some(4));
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["totalLines"], 7);
    let content = json["content"].as_str().unwrap();
    assert!(
        content.contains("public class App"),
        "line 3 should have class declaration"
    );
    assert!(
        !content.contains("package"),
        "line 1 (package) should not be included"
    );
}

#[test]
fn read_file_content_missing_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content(&pack_path, "nonexistent.java", None, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json["error"].as_str().unwrap().contains("File not found"));
}

#[test]
fn read_file_content_rejects_oversized_packed_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);
    let pack_dir = Path::new(&pack_path);

    fs::write(
        pack_dir.join("files/src/App.java"),
        "x".repeat((repolens_app::scan_inputs::DEFAULT_MAX_FILE_BYTES as usize) + 1),
    )
    .unwrap();

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content(&pack_path, "src/App.java", None, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("exceeds configured size limit"));
}

#[test]
fn read_file_content_outline_returns_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);
    fs::write(
        root.join("src/App.java"),
        "package com.example;\n\npublic class App {\n    public void run() {\n        System.out.println(\"hello world\");\n    }\n}\n// mutated after scan\n",
    )
    .unwrap();

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content_detail(
        &pack_path,
        "src/App.java",
        None,
        None,
        Some("outline"),
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["path"], "src/App.java");
    assert_eq!(json["language"], "Java");
    assert_eq!(json["detail"], "outline");
    assert_eq!(json["loc"].as_i64().unwrap(), 7);

    let symbols = json["symbols"].as_array().unwrap();
    assert!(!symbols.is_empty(), "should have symbols");

    // Should have App class and run method
    assert!(
        symbols.iter().any(|s| s["name"] == "App"),
        "should have App class symbol: {:?}",
        symbols
    );

    // Should NOT have file content
    assert!(
        json.get("content").is_none(),
        "outline should not include content"
    );
}

#[test]
fn read_file_content_signatures_strips_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content_detail(
        &pack_path,
        "src/App.java",
        None,
        None,
        Some("signatures"),
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["path"], "src/App.java");
    assert_eq!(json["detail"], "signatures");

    let content = json["content"].as_str().unwrap();

    // Should keep package and class declaration
    assert!(
        content.contains("package com.example"),
        "should keep package"
    );
    assert!(
        content.contains("public class App"),
        "should keep class decl"
    );
    assert!(
        content.contains("public void run()"),
        "should keep method sig"
    );

    // Should be shorter than full content
    let full_result =
        server.call_read_file_content_detail(&pack_path, "src/App.java", None, None, Some("full"));
    let full_json: serde_json::Value = serde_json::from_str(&full_result).unwrap();
    let full_content = full_json["content"].as_str().unwrap();

    assert!(
        content.len() < full_content.len(),
        "signatures ({}) should be shorter than full ({})",
        content.len(),
        full_content.len()
    );
}

#[test]
fn single_hop_graph_tools_emit_verified_evidence_grade() {
    // Regression guard for tools whose response is a single-hop projection
    // of verified graph.json data OR a directly-catalogued output:
    //
    // - get_callers:     filter edges by target == file (trivial projection)
    // - get_dependencies: filter edges by source == file (trivial projection)
    // - find_cycles:     CycleReport is directly covered by the graph cycles
    //                    family's named invariants (I-GRAPH-CYCLE-REPORT-SHAPE
    //                    plus the three exclude-* invariants).
    //
    // These three handlers may emit `evidenceGrade: "verified"`. Tools that
    // run BFS or transitive-closure traversals are deliberately NOT in this
    // test — see `bfs_graph_tools_emit_derived_evidence_grade`.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();

    let callers =
        server.call_get_callers_envelope(&pack_path, "src/main/java/com/example/Base.java");
    assert_eq!(callers["evidenceGrade"], "verified");
    assert!(callers["data"]["callers"].is_array());

    let deps =
        server.call_get_dependencies_envelope(&pack_path, "src/main/java/com/example/App.java");
    assert_eq!(deps["evidenceGrade"], "verified");
    assert!(deps["data"]["dependencies"].is_array());

    let cycles = server.call_find_cycles_envelope(&pack_path);
    assert_eq!(cycles["evidenceGrade"], "verified");
    assert!(cycles["data"]["cycles"].is_array());
}

#[test]
fn bfs_graph_tools_emit_derived_evidence_grade() {
    // Counter-guard against over-promotion. `get_graph` (when `file` is set)
    // runs a bidirectional BFS neighbour walk; `get_impact` runs a reverse-BFS
    // transitive closure. The inputs (edges) are covered by the import-edges
    // and declares/cycles/orphans catalogues, but the traversal operations
    // themselves have no invariant suite on their output. Per the envelope
    // spec's "verified inputs + uncaught derivation = derived" rule, these
    // must emit `evidenceGrade: "derived"`, not "verified".
    //
    // `get_graph`'s no-file branch returns graph.json verbatim (which would
    // be Verified in isolation), but the handler is one piece of code that
    // also hosts the BFS branch, so the lowest grade applies to the whole
    // tool per the mixed-response rule.
    //
    // A regression to "verified" here means someone over-promoted without
    // adding an invariant catalogue for the BFS operation's output.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();

    let full_graph = server.call_get_graph_envelope(&pack_path, None);
    assert_eq!(full_graph["evidenceGrade"], "derived");
    assert!(full_graph["data"]["nodes"].is_array());

    let impact = server.call_get_impact_envelope(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        Some(10),
    );
    assert_eq!(impact["evidenceGrade"], "derived");
    assert!(impact["data"]["impactedFiles"].is_array());
}

#[test]
fn read_file_content_outline_emits_unaudited_evidence_grade() {
    // Raw `loc` is catalogued now, but outline mode still includes
    // uncatalogued raw symbol fields such as `name`, `type`, and
    // `visibility`. Mixed responses take the lowest evidence grade
    // present, so this payload remains `unaudited` until the outline
    // payload is narrowed or those fields gain their own catalogue.
    //
    // Supersedes the `derived` assertion introduced in commit 071aa25,
    // which pinned `loc` as the sole uncatalogued field. That analysis
    // treated raw symbol fields as covered by the exact-qualified-symbols
    // catalogue, but that catalogue only covers the qualified-name
    // identity of a symbol, not the `type`/`visibility` metadata this
    // payload exposes. Closing the `loc` gap did not promote the grade
    // because the symbol-field gap remains.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_read_file_content_outline_envelope(&pack_path, "src/App.java");
    assert_eq!(envelope["evidenceGrade"], "unaudited");
    assert_eq!(envelope["data"]["detail"], "outline");
    assert!(envelope["data"]["symbols"].is_array());
}

#[test]
fn default_tools_still_emit_unaudited_evidence_grade() {
    // Counter-test: tools that are NOT yet promoted must continue to emit
    // `evidenceGrade: "unaudited"` via the `ok_epistemic` shim. This guards
    // against accidentally bulk-promoting via a type-level default change.
    // `read_file_content` in full mode performs line-slicing — a derivation
    // without invariant coverage — so it must remain unaudited.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_read_file_content_full_envelope(&pack_path, "src/App.java");
    assert_eq!(envelope["evidenceGrade"], "unaudited");
    assert!(envelope["data"]["content"].is_string());
}

#[test]
fn search_code_content_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_search_code(&pack_path, "hello world", "content", 10, false);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["mode"], "content");
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0]["path"], "src/App.java");
}

#[test]
fn search_code_symbol_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_search_code(&pack_path, "App", "symbol", 10, false);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["mode"], "symbol");
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0]["symbolName"], "App");
    assert_eq!(results[0]["symbolType"], "class");
}

#[test]
fn search_code_path_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_search_code(&pack_path, "README", "path", 10, false);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["mode"], "path");
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["path"], "README.md");
}

/// Minimal Rust repo for line-anchor tests: alpha() on line 1, Beta on line 3.
fn setup_scanned_rust_repo(root: &Path) -> String {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() {}\n\npub struct Beta {\n    pub x: u8,\n}\n",
    )
    .unwrap();
    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

#[test]
fn outline_includes_line_for_rust_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_rust_repo(root);

    let server = RepoLensMcpServer::new();
    let result =
        server.call_read_file_content_detail(&pack_path, "src/lib.rs", None, None, Some("outline"));
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let symbols = json["symbols"].as_array().unwrap();
    let alpha = symbols
        .iter()
        .find(|s| s["name"] == "alpha")
        .expect("alpha symbol present");
    assert_eq!(alpha["line"], 1, "1-based declaration line: {alpha}");
    let beta = symbols
        .iter()
        .find(|s| s["name"] == "Beta")
        .expect("Beta symbol present");
    assert_eq!(beta["line"], 3, "1-based declaration line: {beta}");
}

#[test]
fn outline_omits_line_key_for_unpositioned_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Java extractor does not capture lines yet: the key must be ABSENT from
    // outline symbols, not null — matching the pack wire convention.
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_read_file_content_detail(
        &pack_path,
        "src/App.java",
        None,
        None,
        Some("outline"),
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let symbols = json["symbols"].as_array().unwrap();
    assert!(!symbols.is_empty());
    for s in symbols {
        assert!(
            s.get("line").is_none(),
            "unpositioned symbol must omit line, not emit null: {s}"
        );
    }
}

#[test]
fn get_callers_returns_calls_edges_for_rust() {
    // End-to-end: Rust scoped call -> resolved calls edge -> get_callers
    // entry with edgeType "calls" and a calls bucket in the breakdown.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.rs"),
        "mod helper;\n\nfn main() {\n    helper::run();\n}\n",
    )
    .unwrap();
    fs::write(root.join("src/helper.rs"), "pub fn run() {}\n").unwrap();
    let pack = ScanService::new()
        .run(root, false, 100)
        .unwrap()
        .pack_dir
        .to_string_lossy()
        .to_string();

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value =
        serde_json::from_str(&server.call_get_callers(&pack, "src/helper.rs", None)).unwrap();

    let callers = json["callers"].as_array().unwrap();
    assert!(
        callers
            .iter()
            .any(|c| c["source"] == "src/main.rs" && c["edgeType"] == "calls"),
        "main.rs should be a calls-typed caller of helper.rs: {json}"
    );
    assert!(
        json["edgeTypeBreakdown"]["calls"].as_u64().unwrap_or(0) >= 1,
        "breakdown must include the calls bucket: {json}"
    );
}

#[test]
fn search_code_symbol_mode_includes_line_for_rust() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_rust_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_search_code(&pack_path, "alpha", "symbol", 10, false);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0]["symbolName"], "alpha");
    assert_eq!(
        results[0]["line"], 1,
        "symbol search carries the line anchor"
    );
}

#[test]
fn search_code_invalid_regex_is_error_shaped_not_graded_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_search_code_envelope(&pack_path, "[unclosed", "content", 10, true);

    // A failed search must be distinguishable from a verified empty result by
    // shape alone: bare error object, no epistemic envelope around it.
    assert!(
        envelope.get("knowledgeLayer").is_none(),
        "a failed search must not be wrapped in a graded envelope: {envelope}"
    );
    assert!(
        envelope["error"]
            .as_str()
            .expect("error must be a string")
            .contains("Invalid regex"),
        "error should name the regex failure: {envelope}"
    );
}

#[test]
fn search_code_unknown_mode_is_error_shaped_not_graded_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_search_code_envelope(&pack_path, "x", "garbage", 10, false);

    assert!(
        envelope.get("knowledgeLayer").is_none(),
        "an unknown-mode failure must not be wrapped in a graded envelope: {envelope}"
    );
    assert!(
        envelope["error"]
            .as_str()
            .expect("error must be a string")
            .contains("Unknown search mode"),
        "error should name the unknown mode: {envelope}"
    );
}

#[test]
fn search_code_valid_empty_keeps_graded_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope =
        server.call_search_code_envelope(&pack_path, "zzz_no_such_token_zzz", "content", 10, false);

    // The contrast half of the contract: a search that ran and found nothing
    // is a graded observation with empty results — not an error.
    assert!(
        envelope.get("knowledgeLayer").is_some(),
        "a valid empty search keeps its graded envelope: {envelope}"
    );
    assert!(
        envelope["data"]["results"]
            .as_array()
            .expect("results must be an array")
            .is_empty(),
        "no matches expected: {envelope}"
    );
}

#[test]
fn get_profile_returns_build_system_and_language() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_profile(&pack_path);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json.get("languageDistribution").is_some());
    assert!(json["languageDistribution"]["Java"].as_i64().unwrap() > 0);
}

#[test]
fn get_profile_emits_derived_evidence_grade() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_get_profile_envelope(&pack_path);

    assert_eq!(envelope["evidenceGrade"], "derived");
    assert_eq!(envelope["data"]["name"], "unknown");
    assert!(envelope["data"]["languageDistribution"].is_object());
}

#[test]
fn analyze_health_returns_structure_and_evidence_findings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_analyze_health(&pack_path);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json.get("structure").is_some());
    assert!(json.get("evidenceFindings").is_some());
    // `signals` was retired in the 2026-05-12 Phase 4B Step 2 amendment.
    // `dependencyRisk` and `graphHealth` were retired in the 2026-05-13
    // Phase 4B Step 4 amendment — the cycles, orphans, lockfile state, and
    // test-inventory ratio they previously surfaced now live in
    // `evidenceFindings` (F-DEPENDENCY-CYCLES, F-ORPHAN-FILES,
    // F-CARGO-LOCK-STATE / F-LOCKFILE-MISSING-NON-CARGO,
    // F-TEST-INVENTORY-RATIO) with per-finding calibration.
    for retired in ["signals", "dependencyRisk", "graphHealth"] {
        assert!(
            json.get(retired).is_none(),
            "{retired} was retired; expected absent, got {:?}",
            json.get(retired)
        );
    }
}

#[test]
fn analyze_health_envelope_grade_promoted_to_derived() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_analyze_health_envelope(&pack_path);

    // Phase 4B Step 4 (2026-05-13): envelope grade promoted from
    // `unaudited` to `derived` once the excluded-tier `dependencyRisk` and
    // raw-counter `graphHealth.*` keys were retired from the response. The
    // surviving payload (`structure` + `evidenceFindings`) is fully
    // catalogue-backed via doc 06 §5.
    assert_eq!(envelope["evidenceGrade"], "derived");

    // The calibration block is preserved but `legacyFields` is now empty —
    // every former legacy field has been retired in the Phase 4B sequence.
    let cal = &envelope["data"]["calibration"];
    assert_eq!(cal["preferred"], "evidenceFindings");
    let legacy: Vec<&str> = cal["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        legacy.is_empty(),
        "after Phase 4B Step 4 no legacy fields remain on analyze_health; got {legacy:?}"
    );
}

/// Phase 4B Step 4 (2026-05-13) regression: the envelope grade is computed
/// as the minimum over per-finding grades. A non-Cargo project missing its
/// lockfile fires `F-LOCKFILE-MISSING-NON-CARGO`, which is intentionally
/// `evidence_grade: Unaudited` until the cross-language lockfile-state
/// catalogue lands (see comment in
/// `crates/repolens-app/src/evidence_findings.rs` near
/// `lockfile_missing_non_cargo`). The envelope must fall back to
/// `unaudited` accordingly — promoting it to `derived` would overclaim.
#[test]
fn analyze_health_envelope_falls_back_when_unaudited_finding_present() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // npm application (Express makes it a web app; libraries get no
    // lockfile finding since #25) with package.json but no
    // package-lock.json triggers F-LOCKFILE-MISSING-NON-CARGO. The build-system detector recognises
    // `package.json` (see `repolens-profile::profile::ProfileAnalyzer`).
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"non-cargo-sample","version":"0.0.0","dependencies":{"express":"^4.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("src/index.js"),
        "module.exports = function () { return 'hello'; };\n",
    )
    .unwrap();

    let result = ScanService::new().run(root, false, 100).unwrap();
    let pack_path = result.pack_dir.to_string_lossy().to_string();

    let server = RepoLensMcpServer::new();
    let envelope = server.call_analyze_health_envelope(&pack_path);

    // Sanity: confirm F-LOCKFILE-MISSING-NON-CARGO actually fired so the
    // fallback path is exercised. If this stops firing (e.g., the rule is
    // promoted), the test should switch to a different unaudited finding.
    let findings = envelope["data"]["evidenceFindings"]
        .as_array()
        .expect("evidenceFindings present");
    let has_unaudited_finding = findings.iter().any(|f| {
        f["id"].as_str() == Some("F-LOCKFILE-MISSING-NON-CARGO")
            && f["evidenceGrade"].as_str() == Some("unaudited")
    });
    assert!(
        has_unaudited_finding,
        "expected F-LOCKFILE-MISSING-NON-CARGO at unaudited grade in findings: {findings:#?}"
    );

    // Envelope grade falls back to the lowest grade in the payload.
    assert_eq!(
        envelope["evidenceGrade"], "unaudited",
        "envelope must fall back to unaudited when an unaudited finding is present; \
         got envelope={}",
        envelope["evidenceGrade"]
    );
}

#[test]
fn get_graph_returns_full_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_graph(&pack_path, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json.get("nodes").is_some());
    assert!(json.get("edges").is_some());
}

#[test]
fn get_conventions_reads_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("CLAUDE.md"), "# Project Rules\n\nBe nice.\n").unwrap();
    let rules_dir = root.join(".claude/rules");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("style.md"), "Use tabs.\n").unwrap();

    let server = RepoLensMcpServer::new();
    let result = server.call_get_conventions(root.to_str().unwrap());
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let sources = json["sources"].as_array().unwrap();
    let claude = sources.iter().find(|s| s["path"] == "CLAUDE.md").unwrap();
    assert_eq!(claude["present"], true);
    assert_eq!(claude["kind"], "file");
    assert!(claude["content"]
        .as_str()
        .unwrap()
        .contains("Project Rules"));

    let style = sources
        .iter()
        .find(|s| s["path"] == ".claude/rules/style.md")
        .unwrap();
    assert_eq!(style["kind"], "rule");
    assert_eq!(style["present"], true);
    assert_eq!(style["name"], "style.md");

    assert_eq!(json["_meta"]["fixed"]["found"], 1);
    assert_eq!(json["_meta"]["rules"]["found"], 1);
}

#[test]
fn get_conventions_multi_source_reports_found_and_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("CLAUDE.md"), "c\n").unwrap();
    fs::write(root.join("AGENTS.md"), "a\n").unwrap();
    fs::write(root.join("CONTRIBUTING.md"), "contrib\n").unwrap();

    let server = RepoLensMcpServer::new();
    let result = server.call_get_conventions(root.to_str().unwrap());
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["_meta"]["fixed"]["considered"], 8);
    assert_eq!(json["_meta"]["fixed"]["found"], 3);
    assert_eq!(json["_meta"]["fixed"]["missing"], 5);
    assert_eq!(json["_meta"]["rules"]["found"], 0);

    let sources = json["sources"].as_array().unwrap();
    let gemini = sources.iter().find(|s| s["path"] == "GEMINI.md").unwrap();
    assert_eq!(gemini["present"], false);
    assert!(gemini["content"].is_null());
}

#[test]
fn get_conventions_empty_repo_reports_all_fixed_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let server = RepoLensMcpServer::new();
    let result = server.call_get_conventions(root.to_str().unwrap());
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let sources = json["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 8, "exactly the 8 fixed entries");
    assert!(sources
        .iter()
        .all(|s| s["present"] == false && s["content"].is_null()));
    assert_eq!(json["_meta"]["fixed"]["found"], 0);
    assert_eq!(json["_meta"]["rules"]["found"], 0);
}

#[test]
fn get_conventions_provenance_lists_only_read_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("CLAUDE.md"), "ok\n").unwrap();
    // Present but unreadable (non-utf8): counts as found, excluded from provenance.
    fs::write(root.join("CONTRIBUTING.md"), [0xFF, 0xFE]).unwrap();

    let server = RepoLensMcpServer::new();
    let env = server.call_get_conventions_envelope(root.to_str().unwrap());

    let sources = env["data"]["sources"].as_array().unwrap();
    let contrib = sources
        .iter()
        .find(|s| s["path"] == "CONTRIBUTING.md")
        .unwrap();
    assert_eq!(contrib["present"], true);
    assert!(contrib["content"].is_null());
    // Both CLAUDE.md and CONTRIBUTING.md are "found" (present), even though one is unreadable.
    assert_eq!(env["data"]["_meta"]["fixed"]["found"], 2);

    // Provenance is Multiple -> array; lists only the successfully-read path.
    let arr = env["provenance"]
        .as_array()
        .expect("multiple provenance serializes as an array");
    let read: Vec<&str> = arr.iter().map(|p| p["source"].as_str().unwrap()).collect();
    assert_eq!(read, vec!["CLAUDE.md"]);
}

#[test]
fn get_architecture_overview() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_architecture(&pack_path, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["overview"], true);
    assert!(json["totalFiles"].as_i64().unwrap() > 0);
    assert!(json.get("directories").is_some());
}

/// Writes a repo whose extraction yields more than the 20-symbol top-N cap,
/// so `_meta.totalSymbols` can be distinguished from `top_symbols.len()`.
fn setup_repo_with_many_symbols(root: &Path) -> String {
    fs::create_dir_all(root.join("src")).unwrap();
    let mut body = String::new();
    for i in 0..30 {
        body.push_str(&format!("pub fn f{i}() -> i32 {{ {i} }}\n"));
    }
    fs::write(root.join("src/lib.rs"), body).unwrap();

    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

/// Regression: `_meta.totalSymbols` must be the repo-wide symbol inventory,
/// not `top_symbols.len()` (hard-capped at 20). Before the fix every tier
/// reported 20 regardless of the real count, undercounting any non-trivial
/// repo. The capped sample size is surfaced separately as `topSymbolCount`.
#[test]
fn get_summary_total_symbols_reflects_real_count_not_top_n_cap() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_repo_with_many_symbols(root);

    // Ground truth: sum symbols across included pack entries.
    let index = repolens_core::pack_reader::PackReader::read(Path::new(&pack_path)).unwrap();
    let real_total: usize = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.symbols.len())
        .sum();
    assert!(
        real_total > 20,
        "fixture must extract >20 symbols to exercise the cap; got {real_total}"
    );

    let server = RepoLensMcpServer::new();
    for detail in ["compact", "standard", "full"] {
        let result = server.call_get_summary(root.to_str().unwrap(), detail);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let meta = &json["_meta"];

        assert_eq!(
            meta["totalSymbols"].as_u64().unwrap() as usize,
            real_total,
            "{detail}: _meta.totalSymbols must equal the real pack symbol count, not the top-N cap"
        );
        let top_symbol_count = meta["topSymbolCount"].as_u64().unwrap() as usize;
        assert!(
            top_symbol_count <= 20,
            "{detail}: topSymbolCount is the ranked sample, capped at 20; got {top_symbol_count}"
        );
        assert!(
            (meta["totalSymbols"].as_u64().unwrap() as usize) >= top_symbol_count,
            "{detail}: totalSymbols must be >= the capped sample"
        );
    }
}

#[test]
fn get_summary_compact() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_summary(root.to_str().unwrap(), "compact");
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json.get("project").is_some());
    assert!(json.get("stats").is_some());
    assert!(json.get("languages").is_some());
    assert_eq!(json["_meta"]["detail"], "compact");
}

#[test]
fn get_summary_full() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_summary(root.to_str().unwrap(), "full");
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["_meta"]["detail"], "full");
    assert!(json.get("health").is_some());
}

#[test]
fn get_summary_full_detail_omits_retired_legacy_keys() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_get_summary_envelope(root.to_str().unwrap(), "full");
    let data = &envelope["data"];

    // Phase 4B retirement schedule:
    //   Step 1 (2026-05-11): `fitnessScore`
    //   Step 2 (2026-05-12): `health.signals`
    //   Step 3 (2026-05-13): `recommendations`
    //   Step 4 (2026-05-13): `health.dependencyRisk`, `health.graphHealth`
    // All retired keys must be absent from the response payload.
    assert!(data.get("fitnessScore").is_none());
    assert!(data.get("recommendations").is_none());
    assert!(data["health"]["signals"].is_null());
    assert!(data["health"]["dependencyRisk"].is_null());
    assert!(data["health"]["graphHealth"].is_null());

    // The calibration block is preserved but `legacyFields` is now empty.
    let cal = &data["calibration"];
    assert_eq!(cal["preferred"], "evidenceFindings");
    let legacy: Vec<&str> = cal["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        legacy.is_empty(),
        "after Phase 4B Step 4 no legacy fields remain on get_summary at any detail tier; got {legacy:?}"
    );
}

/// Phase 4B Step 4 (2026-05-13): at every detail tier, the calibration
/// `legacyFields` list is empty — every former legacy field has been
/// retired in the Phase 4B sequence (`fitnessScore` Step 1, `signals` Step 2,
/// `recommendations` Step 3, `dependencyRisk` / `graphHealth` Step 4).
#[test]
fn get_summary_standard_detail_omits_retired_legacy_keys() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_get_summary_envelope(root.to_str().unwrap(), "standard");
    let data = &envelope["data"];

    // Retired keys absent from response.
    assert!(data.get("fitnessScore").is_none());
    assert!(data.get("recommendations").is_none());
    assert!(data["health"]["signals"].is_null());
    assert!(data["health"]["dependencyRisk"].is_null());
    assert!(data["health"]["graphHealth"].is_null());

    let legacy: Vec<&str> = data["calibration"]["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        legacy.is_empty(),
        "after Phase 4B Step 4 no legacy fields remain on get_summary at standard; got {legacy:?}"
    );
}

#[test]
fn get_summary_compact_detail_omits_retired_legacy_keys() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    let envelope = server.call_get_summary_envelope(root.to_str().unwrap(), "compact");
    let data = &envelope["data"];

    // Compact omits the health block entirely (no `health` key in this tier).
    assert!(data.get("health").is_none() || data["health"].is_null());
    assert!(data.get("fitnessScore").is_none());
    assert!(data.get("recommendations").is_none());

    let legacy: Vec<&str> = data["calibration"]["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        legacy.is_empty(),
        "after Phase 4B Step 4 no legacy fields remain on get_summary at compact; got {legacy:?}"
    );
}

#[test]
fn get_summary_envelope_grade_promoted_to_derived() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_scanned_repo(root);

    let server = RepoLensMcpServer::new();
    for detail in ["compact", "standard", "full"] {
        let envelope = server.call_get_summary_envelope(root.to_str().unwrap(), detail);
        // Phase 4B Step 4 (2026-05-13): envelope promoted from `unaudited`
        // to `derived` once `dependencyRisk` and `graphHealth.*` were
        // retired from the response shape.
        assert_eq!(
            envelope["evidenceGrade"], "derived",
            "get_summary {detail} envelope must be derived after Phase 4B Step 4"
        );
    }
}

#[test]
fn compare_summaries_detects_delta() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack1 = setup_scanned_repo(root);

    // Add more files and rescan
    fs::write(
        root.join("src/Util.java"),
        "package com.example;\n\npublic class Util {\n    public String help() { return \"help\"; }\n}\n",
    ).unwrap();
    let result2 = ScanService::new().run(root, false, 100).unwrap();
    let pack2 = result2.pack_dir.to_string_lossy().to_string();

    let server = RepoLensMcpServer::new();
    let result = server.call_compare_summaries(&pack1, &pack2);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    // `beforeFitness`/`afterFitness`/`deltaFitness` were retired in the
    // 2026-05-11 Phase 4B Step 1 amendment. `metrics` (raw catalogued
    // counts) and `evidenceFindings` (calibrated per-metric deltas) are
    // the surviving channels.
    assert!(json.get("beforeFitness").is_none());
    assert!(json.get("afterFitness").is_none());
    assert!(json.get("deltaFitness").is_none());
    assert!(json.get("metrics").is_some());
}

/// Phase 2 of the 2026-05-11 evidence-aware-findings spec: `compare_summaries`
/// must emit a triage envelope. Per-metric `evidenceFindings` carry the
/// calibrated view that consumers should prefer.
///
/// Phase 4B Step 4 (2026-05-13) retired the last legacy field (`metrics`
/// was demoted from legacy to verified-observation after `trend` was
/// dropped) and promoted the envelope grade from `unaudited` to `derived`.
#[test]
fn compare_summaries_emits_triage_envelope_and_calibration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack1 = setup_scanned_repo(root);

    fs::write(
        root.join("src/Util.java"),
        "package com.example;\n\npublic class Util {\n    public String help() { return \"help\"; }\n}\n",
    )
    .unwrap();
    let result2 = ScanService::new().run(root, false, 100).unwrap();
    let pack2 = result2.pack_dir.to_string_lossy().to_string();

    let server = RepoLensMcpServer::new();
    let env = server.call_compare_summaries_envelope(&pack1, &pack2);
    let data = &env["data"];

    // Triage header pinned to the after-pack scope.
    assert_eq!(data["triageHeader"]["reportKind"], "structuralTriage");
    assert!(data["triageHeader"]["scope"]["packId"]
        .as_str()
        .unwrap()
        .starts_with("pack-"));

    // Phase 4B Step 4 (2026-05-13): every former legacy field has been
    // retired. `deltaFitness` (Step 1), `recommendations` (Step 3), and
    // `metrics.trend` (Step 4) are gone; the surviving `metrics` block is
    // verified observation, not legacy interpretive content.
    let cal = &data["calibration"];
    assert_eq!(cal["preferred"], "evidenceFindings");
    let legacy: Vec<&str> = cal["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        legacy.is_empty(),
        "after Phase 4B Step 4 no legacy fields remain on compare_summaries; got {legacy:?}"
    );

    // notChecked includes always-shown codes.
    let nc_codes: Vec<&str> = data["notChecked"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["code"].as_str())
        .collect();
    assert!(nc_codes.contains(&"RUNTIME_BEHAVIOR"));

    // Envelope grade promoted to derived.
    assert_eq!(env["evidenceGrade"], "derived");
}

/// PR A (2026-05-12 dogfood D1): when the comparison has no delta findings
/// (sub-threshold or identical packs) `evidenceFindings` must still appear
/// as an empty array. Previously the field was omitted via
/// `skip_serializing_if = "Vec::is_empty"`, leaving the calibration block's
/// `preferred: "evidenceFindings"` claim dangling.
#[test]
fn compare_summaries_emits_empty_evidence_findings_when_no_deltas() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo(root);

    // Comparing a pack against itself produces zero deltas — no F-DELTA-*
    // finding will be emitted.
    let server = RepoLensMcpServer::new();
    let env = server.call_compare_summaries_envelope(&pack, &pack);
    let data = &env["data"];

    let findings = data["evidenceFindings"].as_array().unwrap_or_else(|| {
        panic!("evidenceFindings must be present (as []) when no deltas fire; got {data:?}")
    });
    assert!(
        findings.is_empty(),
        "self-compare should produce no findings; got {findings:?}"
    );

    // Calibration still references it as preferred — the claim is now
    // non-dangling because the empty array is in the payload.
    assert_eq!(data["calibration"]["preferred"], "evidenceFindings");
}

#[test]
fn get_callers_returns_incoming_edges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_callers(&pack_path, "src/main/java/com/example/Helper.java", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["file"], "src/main/java/com/example/Helper.java");
    let callers = json["callers"].as_array().unwrap();
    assert!(
        callers
            .iter()
            .any(|c| c["source"] == "src/main/java/com/example/App.java"),
        "App.java should be a caller of Helper.java, callers: {callers:?}"
    );
    assert_eq!(json["totalCallers"], callers.len());
}

#[test]
fn get_callers_returns_empty_for_no_callers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_callers(&pack_path, "src/main/java/com/example/App.java", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["totalCallers"], 0);
    assert!(json["callers"].as_array().unwrap().is_empty());
}

#[test]
fn get_callers_returns_empty_for_unknown_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_callers(&pack_path, "nonexistent.rs", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["totalCallers"], 0);
}

#[test]
fn get_callers_default_limit_reports_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_callers(&pack_path, "src/main/java/com/example/Helper.java", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let callers = json["callers"].as_array().unwrap();
    assert!(!callers.is_empty(), "fixture must yield callers: {json}");
    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalCallers"]);
    assert_eq!(json["returned"].as_u64().unwrap() as usize, callers.len());
}

#[test]
fn get_callers_respects_limit_and_flags_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();

    // Callers count EDGES, not distinct caller files: one importing-and-calling
    // Java file emits up to 3 edges to its target (imports + calls + type_ref;
    // dedup key is source|target|edge_type). Observe the real unlimited total
    // first, then pin the limited call against it relationally.
    let unlimited: serde_json::Value = serde_json::from_str(&server.call_get_callers(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        None,
    ))
    .unwrap();
    let total = unlimited["totalCallers"].as_u64().unwrap();
    assert!(
        total >= 2,
        "fixture must give Helper.java at least 2 inbound edges for a meaningful truncation test, got {total}"
    );

    let limited: serde_json::Value = serde_json::from_str(&server.call_get_callers(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        Some(1),
    ))
    .unwrap();

    assert_eq!(limited["callers"].as_array().unwrap().len(), 1);
    assert_eq!(limited["returned"], 1);
    assert_eq!(limited["totalCallers"], total);
    assert_eq!(limited["truncated"], true);
    // Deterministic order: sort is by source first (App.java is Helper's only
    // caller file), then edge_type — the limit takes a stable prefix.
    assert_eq!(
        limited["callers"][0]["source"],
        "src/main/java/com/example/App.java"
    );
}

#[test]
fn get_callers_discloses_edge_types_and_call_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value = serde_json::from_str(&server.call_get_callers(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        None,
    ))
    .unwrap();

    // Edge-type composition is disclosed and accounts for every returned entry.
    let breakdown = json["edgeTypeBreakdown"]
        .as_object()
        .expect("edgeTypeBreakdown must be an object");
    let sum: u64 = breakdown.values().map(|v| v.as_u64().unwrap()).sum();
    assert_eq!(sum, json["returned"].as_u64().unwrap());

    // Call-semantics coverage is disclosed: only the Java extractor emits
    // calls-typed edges, so an agent reading non-Java results knows it got
    // imports/type references, not call sites.
    assert_eq!(json["callEdgeCoverage"], "java-and-rust");
    assert!(
        json["callEdgeCoverageDetail"]["rust"]
            .as_str()
            .unwrap()
            .contains("receiver-method-calls-excluded"),
        "structured coverage detail must state the Rust v1 boundary: {json}"
    );

    // The flat callers array is unchanged (disclosure is additive, no grouping).
    assert!(json["callers"].is_array());

    // Breakdown is computed over the RETURNED (post-truncation) entries, so a
    // limited response's breakdown sums to the limit, not the total.
    let limited: serde_json::Value = serde_json::from_str(&server.call_get_callers(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        Some(1),
    ))
    .unwrap();
    let limited_sum: u64 = limited["edgeTypeBreakdown"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(limited_sum, 1);
}

#[test]
fn get_callers_oversized_limit_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    // The 200 cap is not observable on a small fixture; this is an acceptance
    // smoke proving an oversized limit is clamped, not rejected.
    let result = server.call_get_callers(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        Some(100_000),
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalCallers"]);
}

#[test]
fn get_dependencies_returns_outgoing_edges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result =
        server.call_get_dependencies(&pack_path, "src/main/java/com/example/App.java", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["file"], "src/main/java/com/example/App.java");
    let deps = json["dependencies"].as_array().unwrap();
    assert!(
        deps.iter()
            .any(|d| d["target"] == "src/main/java/com/example/Helper.java"),
        "App.java should depend on Helper.java, deps: {deps:?}"
    );
    assert_eq!(json["totalDependencies"], deps.len());
}

#[test]
fn get_dependencies_returns_empty_for_leaf_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    // Base.java is the leaf — it doesn't import anything
    let result =
        server.call_get_dependencies(&pack_path, "src/main/java/com/example/Base.java", None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["totalDependencies"], 0);
    assert!(json["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn get_dependencies_default_limit_reports_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value = serde_json::from_str(&server.call_get_dependencies(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        None,
    ))
    .unwrap();

    let deps = json["dependencies"].as_array().unwrap();
    assert!(!deps.is_empty(), "fixture must yield dependencies: {json}");
    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalDependencies"]);
    assert_eq!(json["returned"].as_u64().unwrap() as usize, deps.len());
}

#[test]
fn get_dependencies_respects_limit_and_flags_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();

    // Dependencies count EDGES (a file pair can appear once per edge_type).
    // Observe the unlimited total first, then pin the limited call against it.
    let unlimited: serde_json::Value = serde_json::from_str(&server.call_get_dependencies(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        None,
    ))
    .unwrap();
    let total = unlimited["totalDependencies"].as_u64().unwrap();
    assert!(
        total >= 2,
        "fixture must give Helper.java >= 2 outgoing edges for a meaningful truncation test, got {total}"
    );

    let limited: serde_json::Value = serde_json::from_str(&server.call_get_dependencies(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        Some(1),
    ))
    .unwrap();

    assert_eq!(limited["dependencies"].as_array().unwrap().len(), 1);
    assert_eq!(limited["returned"], 1);
    assert_eq!(limited["totalDependencies"], total);
    assert_eq!(limited["truncated"], true);
}

#[test]
fn get_dependencies_oversized_limit_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value = serde_json::from_str(&server.call_get_dependencies(
        &pack_path,
        "src/main/java/com/example/Helper.java",
        Some(100_000),
    ))
    .unwrap();

    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalDependencies"]);
}

#[test]
fn get_impact_finds_transitive_dependents() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Helper.java"],
        None,
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(
        json["seedFiles"][0],
        "src/main/java/com/example/Helper.java"
    );
    let impacted = json["impactedFiles"].as_array().unwrap();
    assert!(
        impacted
            .iter()
            .any(|i| i["file"] == "src/main/java/com/example/App.java"),
        "App.java should be impacted when Helper.java changes, impacted: {impacted:?}"
    );
    assert_eq!(json["totalImpacted"], impacted.len());
    assert_eq!(json["maxDepthReached"], false);

    let app_entry = impacted
        .iter()
        .find(|i| i["file"] == "src/main/java/com/example/App.java")
        .unwrap();
    assert_eq!(app_entry["depth"], 1);
    let path = app_entry["path"].as_array().unwrap();
    assert_eq!(path[0], "src/main/java/com/example/Helper.java");
    assert_eq!(path[1], "src/main/java/com/example/App.java");
}

#[test]
fn get_impact_default_limit_reports_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value = serde_json::from_str(&server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        None,
        None,
    ))
    .unwrap();

    let impacted = json["impactedFiles"].as_array().unwrap();
    assert!(!impacted.is_empty(), "fixture must yield impact: {json}");
    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalImpacted"]);
    assert_eq!(json["returned"].as_u64().unwrap() as usize, impacted.len());
}

#[test]
fn get_impact_respects_limit_and_flags_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();

    // Base <- Helper (d1) <- App (d2): unlimited total is 2 impacted files.
    let unlimited: serde_json::Value = serde_json::from_str(&server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        None,
        None,
    ))
    .unwrap();
    let total = unlimited["totalImpacted"].as_u64().unwrap();
    assert!(
        total >= 2,
        "fixture must yield >= 2 impacted files, got {total}"
    );

    let limited: serde_json::Value = serde_json::from_str(&server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        None,
        Some(1),
    ))
    .unwrap();

    let impacted = limited["impactedFiles"].as_array().unwrap();
    assert_eq!(impacted.len(), 1);
    assert_eq!(limited["returned"], 1);
    // totalImpacted keeps pre-truncation semantics; truncated is the COUNT
    // cap signal, distinct from the depth-cap signal maxDepthReached.
    assert_eq!(limited["totalImpacted"], total);
    assert_eq!(limited["truncated"], true);
    // Results are sorted (depth, then file) BEFORE the cap, so the limit
    // takes the shallowest entries first — the depth-1 dependent survives.
    assert_eq!(impacted[0]["depth"], 1);
    assert_eq!(impacted[0]["file"], "src/main/java/com/example/Helper.java");
}

#[test]
fn get_impact_oversized_limit_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value = serde_json::from_str(&server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        None,
        Some(100_000),
    ))
    .unwrap();

    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalImpacted"]);
}

#[test]
fn get_impact_excludes_seed_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Helper.java"],
        None,
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let impacted = json["impactedFiles"].as_array().unwrap();
    assert!(
        !impacted
            .iter()
            .any(|i| i["file"] == "src/main/java/com/example/Helper.java"),
        "seed file should not appear in impactedFiles"
    );
}

#[test]
fn get_impact_does_not_traverse_declares_edges() {
    // Dogfood round 2 finding 4: traversing `declares` edges routes the BFS
    // through the module container and reports every sibling as impacted.
    // `declares` is module-tree containment, not a dependency — the same
    // rationale that already excludes it from cycle detection
    // (DEPENDENCY_EDGE_TYPES in repolens-app graph_analysis).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_rust_module_tree_repo(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(&pack_path, vec!["src/m/child_a.rs"], None, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let impacted = json["impactedFiles"].as_array().unwrap();
    let files: Vec<&str> = impacted
        .iter()
        .map(|i| i["file"].as_str().unwrap())
        .collect();

    assert!(
        files.contains(&"src/user.rs"),
        "user.rs imports+calls child_a::alpha and must stay impacted: {files:?}"
    );
    for containment_only in ["src/m/mod.rs", "src/lib.rs", "src/m/child_b.rs"] {
        assert!(
            !files.contains(&containment_only),
            "{containment_only} is reachable only via declares (containment) \
             and must not be reported as impacted: {files:?}"
        );
    }
    // Additive disclosure: the payload names the edge types the BFS skips.
    assert_eq!(
        json["excludedEdgeTypes"],
        serde_json::json!(["declares"]),
        "payload must disclose the excluded edge types: {json}"
    );
}

#[test]
fn get_impact_singular_file_alias_behaves_like_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let via_files: serde_json::Value = serde_json::from_str(&server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Helper.java"],
        None,
        None,
    ))
    .unwrap();
    let via_file: serde_json::Value = serde_json::from_str(
        &server.call_get_impact_file(&pack_path, "src/main/java/com/example/Helper.java"),
    )
    .unwrap();

    assert_eq!(
        via_file["impactedFiles"], via_files["impactedFiles"],
        "singular `file` alias must produce the same blast radius as `files`"
    );
    assert_eq!(via_file["seedFiles"], via_files["seedFiles"]);
}

#[test]
fn get_impact_with_no_seed_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(&pack_path, vec![], None, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let err = json["error"]
        .as_str()
        .expect("empty seed set must be an error, not a misleading empty impact");
    assert!(
        err.contains("file") && err.contains("files"),
        "error must name both accepted params, got: {err}"
    );
}

#[test]
fn get_impact_returns_empty_for_leaf_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/App.java"],
        None,
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(json["totalImpacted"], 0);
    assert!(json["impactedFiles"].as_array().unwrap().is_empty());
}

#[test]
fn get_impact_normalizes_zero_depth() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Helper.java"],
        Some(0),
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    // maxDepth=0 normalizes to default (10), should still find impacted files
    let impacted = json["impactedFiles"].as_array().unwrap();
    assert!(
        !impacted.is_empty(),
        "maxDepth=0 should normalize to default and still find impacted files"
    );
}

#[test]
fn get_impact_empty_files_returns_empty() {
    // Contract change (paper-cut 1): an empty seed set used to return a
    // misleading "0 impacted" success; it is now a self-documenting error.
    // The error shape itself is asserted by get_impact_with_no_seed_is_an_error;
    // this test pins that no impact payload fields leak into the error case.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let result = server.call_get_impact(&pack_path, vec![], None, None);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert!(json["error"].is_string(), "empty seed must error: {json}");
    assert!(json.get("totalImpacted").is_none());
    assert!(json.get("impactedFiles").is_none());
}

#[test]
fn get_impact_max_depth_reached() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Graph chain: App → Helper → Base (import direction)
    // Impact direction: Base change → Helper (depth 1) → App (depth 2)
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    // maxDepth=1: should find Helper but NOT App, and set maxDepthReached=true
    let result = server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        Some(1),
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let impacted = json["impactedFiles"].as_array().unwrap();
    assert!(
        impacted
            .iter()
            .any(|i| i["file"] == "src/main/java/com/example/Helper.java"),
        "Helper.java should be impacted at depth 1, impacted: {impacted:?}"
    );
    assert!(
        !impacted
            .iter()
            .any(|i| i["file"] == "src/main/java/com/example/App.java"),
        "App.java should NOT be impacted at depth 1 (needs depth 2)"
    );
    assert_eq!(json["maxDepthReached"], true);
}

#[test]
fn get_impact_full_depth_finds_all() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    // Default depth (10): should find both Helper and App
    let result = server.call_get_impact(
        &pack_path,
        vec!["src/main/java/com/example/Base.java"],
        None,
        None,
    );
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();

    let impacted = json["impactedFiles"].as_array().unwrap();
    assert!(
        impacted
            .iter()
            .any(|i| i["file"] == "src/main/java/com/example/Helper.java"),
        "Helper.java should be impacted"
    );
    assert!(
        impacted
            .iter()
            .any(|i| i["file"] == "src/main/java/com/example/App.java"),
        "App.java should be transitively impacted"
    );
    assert_eq!(json["maxDepthReached"], false);

    // Verify propagation path for App goes through Helper
    let app_entry = impacted
        .iter()
        .find(|i| i["file"] == "src/main/java/com/example/App.java")
        .unwrap();
    assert_eq!(app_entry["depth"], 2);
    let path = app_entry["path"].as_array().unwrap();
    assert_eq!(path.len(), 3);
    assert_eq!(path[0], "src/main/java/com/example/Base.java");
    assert_eq!(path[2], "src/main/java/com/example/App.java");
}

// ---------------------------------------------------------------------------
// find_dead_code
// ---------------------------------------------------------------------------

#[test]
fn find_dead_code_returns_orphan_files() {
    // App → Helper → Base (chain). App has no inbound edges → dead code (orphan).
    // README.md also has no inbound edges.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let raw = server.call_find_dead_code(&pack, None, None, None);
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let dead_files = json["deadCodeFiles"].as_array().unwrap();
    let total = json["totalDeadCodeFiles"].as_u64().unwrap();

    // App.java is at the root of the import chain (nothing imports it) → orphan
    assert!(
        dead_files
            .iter()
            .any(|f| f["file"] == "src/main/java/com/example/App.java"),
        "App.java should be dead code (no inbound imports), got: {:?}",
        dead_files
    );

    assert!(total > 0, "should have at least one dead code file");
    assert!(json["totalFiles"].as_u64().unwrap() > 0);
    assert!(json["percentDead"].as_f64().is_some());
}

#[test]
fn find_dead_code_flags_truncation_when_limit_caps_results() {
    // Two unreferenced source files -> at least 2 orphans, so limit 1 truncates.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/Alpha.java"),
        "package com.example;\n\npublic class Alpha {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/Beta.java"),
        "package com.example;\n\npublic class Beta {}\n",
    )
    .unwrap();
    let pack = ScanService::new()
        .run(root, false, 100)
        .unwrap()
        .pack_dir
        .to_string_lossy()
        .to_string();

    let server = RepoLensMcpServer::new();

    let unlimited: serde_json::Value =
        serde_json::from_str(&server.call_find_dead_code(&pack, None, None, None)).unwrap();
    let total = unlimited["totalDeadCodeFiles"].as_u64().unwrap();
    assert!(total >= 2, "fixture must yield >= 2 orphans, got {total}");
    assert_eq!(unlimited["truncated"], false);
    assert_eq!(unlimited["returned"], total);

    let limited: serde_json::Value =
        serde_json::from_str(&server.call_find_dead_code(&pack, None, None, Some(1))).unwrap();
    assert_eq!(limited["deadCodeFiles"].as_array().unwrap().len(), 1);
    assert_eq!(limited["returned"], 1);
    // totalDeadCodeFiles keeps pre-truncation semantics (back-compat).
    assert_eq!(limited["totalDeadCodeFiles"], total);
    assert_eq!(limited["truncated"], true);
}

/// Two independent 2-file import cycles: a/One <-> a/Two and b/Three <-> b/Four.
fn setup_scanned_repo_with_two_cycles(root: &Path) -> String {
    fs::create_dir_all(root.join("src/a")).unwrap();
    fs::create_dir_all(root.join("src/b")).unwrap();
    fs::write(
        root.join("src/a/One.java"),
        "package a;\n\nimport a.Two;\n\npublic class One {\n    Two t;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/a/Two.java"),
        "package a;\n\nimport a.One;\n\npublic class Two {\n    One o;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/b/Three.java"),
        "package b;\n\nimport b.Four;\n\npublic class Three {\n    Four f;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/b/Four.java"),
        "package b;\n\nimport b.Three;\n\npublic class Four {\n    Three t;\n}\n",
    )
    .unwrap();
    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

#[test]
fn find_cycles_default_limit_reports_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_two_cycles(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value =
        serde_json::from_str(&server.call_find_cycles(&pack, None, None)).unwrap();

    let cycles = json["cycles"].as_array().unwrap();
    assert!(cycles.len() >= 2, "fixture must yield >= 2 cycles: {json}");
    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalCycles"]);
}

#[test]
fn find_cycles_respects_limit_and_flags_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_two_cycles(root);

    let server = RepoLensMcpServer::new();

    let unlimited: serde_json::Value =
        serde_json::from_str(&server.call_find_cycles(&pack, None, None)).unwrap();
    let total = unlimited["totalCycles"].as_u64().unwrap();
    assert!(total >= 2, "fixture must yield >= 2 cycles, got {total}");

    let limited: serde_json::Value =
        serde_json::from_str(&server.call_find_cycles(&pack, None, Some(1))).unwrap();

    let cycles = limited["cycles"].as_array().unwrap();
    assert_eq!(cycles.len(), 1);
    assert_eq!(limited["returned"], 1);
    assert_eq!(limited["totalCycles"], total);
    assert_eq!(limited["truncated"], true);
    // The cap drops whole CYCLES, never files within one: the surviving SCC
    // is complete (both files of the pair present).
    assert_eq!(cycles[0]["files"].as_array().unwrap().len(), 2);
    assert_eq!(cycles[0]["size"], 2);
    // Analyzer order is deterministic (size desc, then first file): the
    // a/ pair sorts before the b/ pair.
    assert!(
        cycles[0]["files"][0].as_str().unwrap().contains("src/a/"),
        "deterministic survivor should be the a/ cycle: {cycles:?}"
    );
}

#[test]
fn find_cycles_oversized_limit_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_two_cycles(root);

    let server = RepoLensMcpServer::new();
    let json: serde_json::Value =
        serde_json::from_str(&server.call_find_cycles(&pack, None, Some(100_000))).unwrap();

    assert_eq!(json["truncated"], false);
    assert_eq!(json["returned"], json["totalCycles"]);
}

#[test]
fn find_dead_code_filters_by_language() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();

    // Filter to Java only
    let raw = server.call_find_dead_code(&pack, Some("Java"), None, None);
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let dead_files = json["deadCodeFiles"].as_array().unwrap();

    for f in dead_files {
        assert_eq!(
            f["language"], "Java",
            "all results should be Java when filtered"
        );
    }

    // Filter to Markdown — README.md must NOT appear: as of the
    // 2026-05-11 evidence-aware spec, root-level READMEs are classified
    // as docs/policy and filtered out of the dead-code list (see
    // `repolens-core::path_classification::is_root_config_or_policy`).
    let raw = server.call_find_dead_code(&pack, Some("Markdown"), None, None);
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let dead_files = json["deadCodeFiles"].as_array().unwrap();
    assert!(
        !dead_files.iter().any(|f| f["file"] == "README.md"),
        "README.md must not be reported as dead code (root docs are filtered)"
    );
}

/// Phase 2 of the 2026-05-11 evidence-aware-findings spec: `find_dead_code`
/// must emit a triage envelope (triageHeader / notChecked / calibration)
/// and surface `evidenceFindings` for the orphan family alongside the
/// legacy `deadCodeFiles` field.
#[test]
fn find_dead_code_emits_triage_envelope_and_evidence_findings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let env = server.call_find_dead_code_envelope(&pack);
    let data = &env["data"];

    // Triage envelope: required keys present.
    assert_eq!(data["triageHeader"]["reportKind"], "structuralTriage");
    assert!(data["notChecked"].is_array());
    let nc_codes: Vec<&str> = data["notChecked"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["code"].as_str())
        .collect();
    // Always-shown codes must be present regardless of repo shape.
    for must_have in [
        "RUNTIME_BEHAVIOR",
        "RELEASE_READINESS",
        "SECURITY_AUDIT_GENERIC",
    ] {
        assert!(
            nc_codes.contains(&must_have),
            "missing always-shown notChecked code: {must_have}"
        );
    }

    // Calibration block: dead-code legacy fields named explicitly.
    let cal = &data["calibration"];
    assert_eq!(cal["preferred"], "evidenceFindings");
    let legacy: Vec<&str> = cal["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        legacy.contains(&"deadCodeFiles") && legacy.contains(&"totalDeadCodeFiles"),
        "calibration must call out deadCodeFiles + totalDeadCodeFiles, got {legacy:?}"
    );

    // evidenceFindings array exists.
    assert!(data["evidenceFindings"].is_array());
    // Phase 3 promotion (2026-05-11): every metric in the substantive payload
    // is backed by a doc-06 §5 catalogue (orphan detection + included file
    // presence). The legacy deadCodeFiles/totalDeadCodeFiles/percentDead keys
    // are direct derivations of those catalogued families. The orphan-ratio
    // threshold inside F-ORPHAN-FILES is what motivates `derived` rather
    // than `verified`. See docs/superpowers/specs/2026-05-11-evidence-aware-findings.md
    // §"Phase 3 amendments".
    assert_eq!(env["evidenceGrade"], "derived");
}

/// Phase 3 envelope-grade promotion, locked at the entry point. Separate
/// from the triage-envelope-shape test above so a future grade flip is
/// visible by failing this dedicated test by name.
#[test]
fn find_dead_code_envelope_grade_promoted_to_derived() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let env = server.call_find_dead_code_envelope(&pack);
    assert_eq!(
        env["evidenceGrade"], "derived",
        "find_dead_code envelope grade was promoted from 'unaudited' to 'derived' \
         in the 2026-05-11 Phase-3 amendment. If you saw this test fail, the spec \
         and doc 06 §5 catalogue coverage need to be re-reviewed before relaxing it."
    );
    // Layers, confidence, and provenance shape are unchanged by the promotion.
    let layers = env["knowledgeLayer"].as_array().unwrap();
    let layer_strs: Vec<&str> = layers.iter().filter_map(|v| v.as_str()).collect();
    assert!(layer_strs.contains(&"observation"));
    assert!(layer_strs.contains(&"heuristic"));
    assert_eq!(env["confidence"], "codified");
}

/// Phase 3 grade gating: the `find_dead_code` `Derived` envelope grade
/// applies only when `graph.json` was actually loaded. A pack missing or
/// with an unreadable graph falls back to a synthesized empty graph and
/// returns "zero dead-code files" — but that response has not actually
/// observed any catalogued evidence, so the envelope grade must stay
/// `unaudited`. Regression for the 2026-05-11 second Codex pass on PR #50.
#[test]
fn find_dead_code_envelope_stays_unaudited_when_graph_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_graph(root);
    // Delete graph.json so read_graph returns None and the handler falls
    // back to a synthesized empty graph.
    let graph_path = std::path::Path::new(&pack).join("graph.json");
    assert!(graph_path.exists(), "fixture should have graph.json");
    std::fs::remove_file(&graph_path).expect("remove graph.json");

    let server = RepoLensMcpServer::new();
    let env = server.call_find_dead_code_envelope(&pack);
    assert_eq!(
        env["evidenceGrade"], "unaudited",
        "find_dead_code must NOT promote to 'derived' when graph.json could \
         not be read — without a real graph there is no catalogued \
         observation to derive from."
    );
}

/// End-to-end test for Phase 3 of the get_summary memory-population plan.
///
/// Verifies that when the repo's memory graph is populated:
/// - compact surfaces tier-selected memoryInsights (cap 3, anchor-filter);
/// - standard caps at 5 under the same filter;
/// - full includes up to 20 including unresolved entries under the bonus rule;
/// - each returned insight carries groundingStatus from the grounding module;
/// - the envelope's knowledgeLayer includes "interpretation" for every tier
///   that surfaces memory — including compact.
#[test]
fn get_summary_tiers_surface_grounded_memory_insights() {
    use repolens_core::model::memory::{MemoryGraph, MemoryNode, MemorySource, MemoryType};
    use repolens_memory::store::MemoryStore;
    use time::OffsetDateTime;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // setup_scanned_repo writes src/App.java + README.md and scans.
    let _pack = setup_scanned_repo(root);

    // Build a memory graph with 25 anchor-resolving + unresolved entries.
    let now = OffsetDateTime::now_utc();
    let mk_node = |id: String, importance: f32, anchors: Vec<String>| MemoryNode {
        id,
        content: "insight".to_string(),
        memory_type: MemoryType::Convention,
        importance,
        created: now,
        last_accessed: now,
        access_count: 1,
        anchors,
        tags: vec![],
        source: MemorySource::Explicit,
        provenance: "test".to_string(),
        last_grounding: None,
        last_grounded_at: None,
        last_decayed_at: None,
        importance_before_grounding: None,
        anchor_sha256: Default::default(),
    };
    let mut nodes = Vec::new();
    for i in 0..15 {
        // Resolving anchors against the included file, varying importance.
        nodes.push(mk_node(
            format!("resolved-{i:02}"),
            0.5 + (i as f32) * 0.01,
            vec!["src/App.java".to_string()],
        ));
    }
    for i in 0..10 {
        // Unresolved (ghost file) — would contradict under grounding.
        nodes.push(mk_node(
            format!("ghost-{i:02}"),
            0.8 + (i as f32) * 0.01,
            vec![format!("src/ghost{i}.java")],
        ));
    }
    let graph = MemoryGraph {
        schema_version: "1.0".to_string(),
        repo_root: root.to_string_lossy().into_owned(),
        nodes,
        edges: vec![],
        last_decay_run: Some(now),
        updated_at: now,
    };
    MemoryStore::save(root, &graph).expect("memory save");

    let server = RepoLensMcpServer::new();
    let root_str = root.to_str().unwrap();

    // Compact: cap 3, only resolving anchors surface.
    let envelope_c = server.call_get_summary_envelope(root_str, "compact");
    let data_c = &envelope_c["data"];
    let insights_c = data_c["memoryInsights"].as_array().unwrap();
    assert_eq!(
        insights_c.len(),
        3,
        "compact tier should cap at 3, got {}",
        insights_c.len()
    );
    for ins in insights_c {
        let status = ins["groundingStatus"].as_str().unwrap();
        assert_eq!(
            status, "CONFIRMED",
            "compact filter excludes non-resolving anchors; all entries should resolve"
        );
    }
    // knowledgeLayer must include interpretation in compact when memory shows up.
    let layers_c = &envelope_c["knowledgeLayer"];
    let layer_json = serde_json::to_string(layers_c).unwrap();
    assert!(
        layer_json.contains("interpretation"),
        "compact envelope should include 'interpretation' when memoryInsights non-empty, got {}",
        layer_json
    );

    // Standard: cap 5, same filter as compact.
    let envelope_s = server.call_get_summary_envelope(root_str, "standard");
    let data_s = &envelope_s["data"];
    let insights_s = data_s["memoryInsights"].as_array().unwrap();
    assert_eq!(insights_s.len(), 5, "standard tier should cap at 5");
    for ins in insights_s {
        assert_eq!(ins["groundingStatus"].as_str().unwrap(), "CONFIRMED");
    }

    // Full: cap 20, unresolved entries eligible (they outrank some resolved
    // because their raw importance (0.8-0.89) exceeds most resolved (0.5-0.64)
    // even after the 1.5x bonus lifts resolved ones to 0.75-0.96).
    let envelope_f = server.call_get_summary_envelope(root_str, "full");
    let data_f = &envelope_f["data"];
    let insights_f = data_f["memoryInsights"].as_array().unwrap();
    assert!(
        insights_f.len() <= 20,
        "full tier cap at 20, got {}",
        insights_f.len()
    );
    assert!(
        insights_f.len() >= 15,
        "full tier with 25 memories should surface at least 15"
    );
    let has_contradicted = insights_f
        .iter()
        .any(|i| i["groundingStatus"].as_str() == Some("CONTRADICTED"));
    assert!(
        has_contradicted,
        "full tier should include at least one CONTRADICTED entry (unresolved ghost)"
    );
}

#[test]
fn find_dead_code_filters_by_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack = setup_scanned_repo_with_graph(root);

    let server = RepoLensMcpServer::new();
    let raw = server.call_find_dead_code(&pack, None, Some("src/main"), None);
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let dead_files = json["deadCodeFiles"].as_array().unwrap();

    for f in dead_files {
        let path = f["file"].as_str().unwrap();
        assert!(
            path.starts_with("src/main"),
            "all results should be under src/main, got: {}",
            path
        );
    }
}
