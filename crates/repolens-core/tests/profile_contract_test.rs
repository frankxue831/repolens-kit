//! Fixture contract checks for captured `project-profile.json` scenarios.
//! Related invariant docs:
//! - `docs/superpowers/specs/2026-04-14-language-and-profile-catalogue.md`
//! - `docs/superpowers/specs/2026-04-15-profile-signal-catalogue.md`
//! - `docs/superpowers/specs/2026-04-15-loc-family-catalogue.md`
//!
//! These assertions are broader than the live Rust LOC and profile-aggregation
//! invariants. Captured fixture profiles may contain `Unknown` language buckets
//! and a root `.` module, while the Rust `ProfileAnalyzer` drops `Unknown`
//! from `languageDistribution` and skips root files when building `modules`.
//! Treat these as fixture-shape regressions, not as the primary invariant
//! evidence for the live producer semantics.
//!
//! Lockstep maintenance rule: any rename, deletion, or `#[ignore]`-ing of
//! the `profile_*_deserializes` tests in this file MUST be accompanied by
//! a tier demotion of the affected Project Profile Name invariant in
//! `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`
//! §5 and a matching update to the catalogue specs listed above. The rule
//! is co-located here so a `grep` from the test file surfaces it.

use repolens_core::model::profile::{ProjectProfile, ProjectType};
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn scenarios_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repolens-rs root
    p.push("fixtures");
    p.push("scenarios");
    p
}

fn load_profile(scenario: &str) -> ProjectProfile {
    let path = scenarios_dir()
        .join(scenario)
        .join("pack")
        .join("project-profile.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse profile for {}: {}", scenario, e))
}

// ---------------------------------------------------------------------------
// Deserialization — java-small
// ---------------------------------------------------------------------------

#[test]
fn profile_java_small_deserializes() {
    let p = load_profile("java-small");

    assert_eq!(p.name, "java-small");
    assert_eq!(p.project_type, ProjectType::Cli);

    // build system must be present and have null details
    let bs = p
        .build_system
        .as_ref()
        .expect("build_system must not be null");
    assert_eq!(bs.name, "Maven");
    assert!(bs.details.is_none(), "details should be null in java-small");
    assert_eq!(bs.config_file, "pom.xml");

    // frameworks list is empty
    assert!(p.frameworks.is_empty());

    // one entry point
    assert_eq!(p.entry_points.len(), 1);
    assert_eq!(p.entry_points[0].entry_type, "main");
    assert_eq!(p.entry_points[0].path, "src/main/java/com/example/App.java");

    // description is present
    assert!(p.description.is_some());

    // language distribution contains Java
    assert!(
        p.language_distribution.contains_key("Java"),
        "expected Java in languageDistribution"
    );
    assert_eq!(p.language_distribution["Java"], 31);

    // modules non-empty
    assert!(!p.modules.is_empty());
    let src_module = p
        .modules
        .iter()
        .find(|m| m.name == "src")
        .expect("expected 'src' module");
    assert_eq!(src_module.file_count, 2);
    assert_eq!(src_module.loc, 31);
    assert_eq!(src_module.primary_language, "Java");
}

// ---------------------------------------------------------------------------
// Deserialization — mixed-small
// ---------------------------------------------------------------------------

#[test]
fn profile_mixed_small_deserializes() {
    let p = load_profile("mixed-small");

    assert_eq!(p.name, "mixed-small");
    assert_eq!(p.project_type, ProjectType::Cli);

    let bs = p
        .build_system
        .as_ref()
        .expect("build_system must not be null");
    assert_eq!(bs.name, "npm");
    assert!(bs.details.is_none());
    assert_eq!(bs.config_file, "package.json");

    // description is null
    assert!(
        p.description.is_none(),
        "description should be null for mixed-small"
    );

    // multiple languages present
    assert!(p.language_distribution.contains_key("Python"));
    assert!(p.language_distribution.contains_key("Go"));
    assert!(p.language_distribution.contains_key("TypeScript"));
    assert_eq!(p.language_distribution["Python"], 16);
    assert_eq!(p.language_distribution["Go"], 11);
    assert_eq!(p.language_distribution["TypeScript"], 8);
    assert_eq!(p.language_distribution["JavaScript"], 8);
    assert_eq!(p.language_distribution["JSON"], 4);
    assert_eq!(p.language_distribution["Unknown"], 3);

    // modules: src, py, go, .
    assert!(!p.modules.is_empty());
    let src_module = p
        .modules
        .iter()
        .find(|m| m.name == "src")
        .expect("expected 'src' module");
    assert_eq!(src_module.loc, 16);
    assert_eq!(src_module.primary_language, "TypeScript");

    let py_module = p
        .modules
        .iter()
        .find(|m| m.name == "py")
        .expect("expected 'py' module");
    assert_eq!(py_module.loc, 16);
    assert_eq!(py_module.primary_language, "Python");

    let go_module = p
        .modules
        .iter()
        .find(|m| m.name == "go")
        .expect("expected 'go' module");
    assert_eq!(go_module.loc, 14);
    assert_eq!(go_module.primary_language, "Go");

    let dot_module = p
        .modules
        .iter()
        .find(|m| m.name == ".")
        .expect("expected '.' module");
    assert_eq!(dot_module.loc, 4);
}

// ---------------------------------------------------------------------------
// Deserialization — excluded-small (null buildSystem, UNKNOWN project type)
// ---------------------------------------------------------------------------

#[test]
fn profile_excluded_small_deserializes() {
    let p = load_profile("excluded-small");

    assert_eq!(p.name, "excluded-small");
    assert_eq!(p.project_type, ProjectType::Unknown);

    // build system is null
    assert!(
        p.build_system.is_none(),
        "buildSystem must be null in excluded-small"
    );

    // description is null
    assert!(p.description.is_none());

    // no entry points
    assert!(p.entry_points.is_empty());

    // some language distribution
    assert!(p.language_distribution.contains_key("Python"));
    assert_eq!(p.language_distribution["Python"], 10);
    assert_eq!(p.language_distribution["Unknown"], 3);

    let src_module = p
        .modules
        .iter()
        .find(|m| m.name == "src")
        .expect("expected 'src' module");
    assert_eq!(src_module.loc, 10);

    let dot_module = p
        .modules
        .iter()
        .find(|m| m.name == ".")
        .expect("expected '.' module");
    assert_eq!(dot_module.loc, 3);
}

// ---------------------------------------------------------------------------
// Snapshot — java-small re-serialization
// ---------------------------------------------------------------------------

#[test]
fn snapshot_java_small_profile_reserialization() {
    let p = load_profile("java-small");
    let json = serde_json::to_string_pretty(&p).expect("re-serialization must succeed");
    insta::assert_snapshot!("java_small_profile", json);
}

// ---------------------------------------------------------------------------
// MCP fixture smoke tests
// ---------------------------------------------------------------------------
//
// The mcp/ directory may not exist for any scenario (fixtures were skipped
// during capture). We skip gracefully if absent.

/// Returns the list of expected MCP tool names whose response fixtures
/// we would expect to find if the mcp/ directory exists.
fn expected_tool_names() -> &'static [&'static str] {
    &[
        "get_summary",
        "get_profile",
        "get_graph",
        "analyze_health",
        "search_code",
    ]
}

fn check_mcp_fixtures_for_scenario(scenario: &str) {
    let mcp_dir = scenarios_dir().join(scenario).join("mcp");

    if !mcp_dir.exists() {
        // No MCP fixtures captured for this scenario — skip gracefully.
        return;
    }

    // Every .json file in the directory must parse as valid JSON.
    let entries = fs::read_dir(&mcp_dir)
        .unwrap_or_else(|e| panic!("failed to read mcp dir for {}: {}", scenario, e));

    let mut json_files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();

    json_files.sort();

    for path in &json_files {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
        let _: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {}", path.display(), e));
    }

    // Check that expected tool fixtures are present (warn if missing, don't fail).
    let file_names: Vec<String> = json_files
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .collect();

    for tool in expected_tool_names() {
        if !file_names.iter().any(|n| n.contains(tool)) {
            // Fixture missing — acceptable since capture was skipped. Just note it.
            eprintln!(
                "info: mcp fixture for tool '{}' not found in scenario '{}' (skipped)",
                tool, scenario
            );
        }
    }
}

#[test]
fn mcp_fixtures_java_small_valid_json() {
    check_mcp_fixtures_for_scenario("java-small");
}

#[test]
fn mcp_fixtures_mixed_small_valid_json() {
    check_mcp_fixtures_for_scenario("mixed-small");
}

#[test]
fn mcp_fixtures_excluded_small_valid_json() {
    check_mcp_fixtures_for_scenario("excluded-small");
}
