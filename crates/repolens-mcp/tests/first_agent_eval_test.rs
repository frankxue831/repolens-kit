use std::fs;
use std::path::Path;

use repolens_app::scan::ScanService;
use repolens_mcp::RepoLensMcpServer;

fn setup_scanned_rust_profile_repo(root: &Path) -> String {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"profile-demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub mod profile;\n").unwrap();
    fs::write(
        root.join("src/profile.rs"),
        "pub struct ProfileAnalyzer;\n\nimpl ProfileAnalyzer {\n    pub fn analyze() -> usize {\n        1\n    }\n}\n",
    )
    .unwrap();

    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

fn symbol_results_for(
    server: &RepoLensMcpServer,
    pack_path: &str,
    query: &str,
) -> Vec<serde_json::Value> {
    let result = server.call_search_code(pack_path, query, "symbol", 10, false);
    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    json["results"].as_array().unwrap().clone()
}

#[test]
fn search_code_symbol_mode_keeps_qualified_profile_analyzer_known_fail() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pack_path = setup_scanned_rust_profile_repo(root);

    let server = RepoLensMcpServer::new();

    let type_results = symbol_results_for(&server, &pack_path, "ProfileAnalyzer");
    assert!(!type_results.is_empty());
    assert_eq!(type_results[0]["symbolName"], "ProfileAnalyzer");

    // Extracted Rust qualified names use `.` as the separator, but agents
    // naturally write `::` — that asymmetry is the miss FAC-005 locks in.
    let method_results = symbol_results_for(&server, &pack_path, "analyze");
    assert!(
        method_results.iter().any(|result| {
            result["symbolName"] == "analyze" && result["qualified"] == "ProfileAnalyzer.analyze"
        }),
        "expected extracted Rust method symbol to be searchable by bare name"
    );

    let qualified_results = symbol_results_for(&server, &pack_path, "ProfileAnalyzer::analyze");
    assert!(
        qualified_results.is_empty(),
        "known fail: search_code symbol mode does not support Rust-style qualified lookups"
    );
}
