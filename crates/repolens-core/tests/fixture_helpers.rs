/// Cross-scenario contract gate — Phase 0 + Phase 1
///
/// This file is a standalone integration test binary (Cargo treats every file
/// directly under `tests/` as a separate test binary).  It exercises the
/// three shared helpers that every contract test relies on:
///
/// 1. `all_scenarios()` — enumerate every directory under `fixtures/scenarios/`
/// 2. `load_fixture(scenario, rel_path)` — read a JSON file within a scenario
/// 3. Placeholder-aware path assertions — verify that paths in fixture JSON
///    contain `__REPO_ROOT__` or `__PACK_DIR__` instead of real absolute paths
///
/// Running `cargo test -p repolens-core` compiles and executes ALL five test
/// binaries (this one + the four contract test files), giving a single command
/// that gates the entire Phase-0/Phase-1 contract surface.
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Shared helpers (copy-free — each contract test binary is self-contained;
// shared logic lives here as the canonical reference implementation)
// ---------------------------------------------------------------------------

/// Returns the `fixtures/scenarios/` directory, resolving from the crate root
/// via `CARGO_MANIFEST_DIR`.
///
/// `CARGO_MANIFEST_DIR` = `<workspace>/crates/repolens-core`
/// We pop twice to reach `<workspace>` and then descend.
fn fixtures_scenarios_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repolens-rs workspace root
    p.push("fixtures");
    p.push("scenarios");
    p
}

/// Enumerate all scenario directory paths under `fixtures/scenarios/`.
///
/// Returns absolute [`PathBuf`]s, sorted alphabetically by directory name.
/// Only entries that are actually directories are included.
fn all_scenarios() -> Vec<PathBuf> {
    let base = fixtures_scenarios_dir();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&base)
        .unwrap_or_else(|e| panic!("failed to read scenarios dir {}: {}", base.display(), e))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    dirs.sort();
    dirs
}

/// Return just the scenario *names* (directory names) in sorted order.
fn all_scenario_names() -> Vec<String> {
    all_scenarios()
        .into_iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

/// Load a JSON document by scenario name and a relative path within that
/// scenario directory.
///
/// # Example
/// ```text
/// let v = load_fixture("java-small", "pack/manifest.json");
/// ```
fn load_fixture(scenario: &str, rel_path: &str) -> serde_json::Value {
    let path = fixtures_scenarios_dir().join(scenario).join(rel_path);
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("load_fixture({scenario}, {rel_path}) failed: {e}"));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("load_fixture: invalid JSON at {}: {e}", path.display()))
}

/// Returns `true` when the string value *either* is a placeholder itself or
/// contains a known placeholder token.
///
/// Recognised placeholders:
/// - `__REPO_ROOT__`
/// - `__PACK_DIR__`
fn is_placeholder_or_relative(s: &str) -> bool {
    s.contains("__REPO_ROOT__") || s.contains("__PACK_DIR__")
}

/// Assert that every string value inside `v` that *looks like an absolute path*
/// (starts with `/` on Unix, or a drive letter + `:\` on Windows) also
/// contains a known placeholder token.
///
/// This enforces that fixtures are portable: no developer-specific absolute
/// paths are baked into the JSON.
fn assert_no_raw_absolute_paths(context: &str, v: &serde_json::Value) {
    match v {
        serde_json::Value::String(s) => {
            let looks_absolute = s.starts_with('/') || (s.len() > 2 && s.as_bytes()[1] == b':');
            if looks_absolute {
                assert!(
                    is_placeholder_or_relative(s),
                    "{context}: raw absolute path found in fixture JSON: {s:?}\n\
                     Paths must use __REPO_ROOT__ or __PACK_DIR__ placeholders instead."
                );
            }
        }
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                assert_no_raw_absolute_paths(&format!("{context}.{key}"), val);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                assert_no_raw_absolute_paths(&format!("{context}[{i}]"), val);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Gate: all_scenarios()
// ---------------------------------------------------------------------------

#[test]
fn all_scenarios_returns_three_directories() {
    let names = all_scenario_names();
    assert_eq!(
        names.len(),
        3,
        "expected exactly 3 scenarios, got: {names:?}"
    );
    assert!(names.contains(&"java-small".to_string()));
    assert!(names.contains(&"mixed-small".to_string()));
    assert!(names.contains(&"excluded-small".to_string()));
}

#[test]
fn all_scenarios_are_sorted() {
    let names = all_scenario_names();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "all_scenarios() must return sorted names");
}

// ---------------------------------------------------------------------------
// Gate: load_fixture()
// ---------------------------------------------------------------------------

#[test]
fn load_fixture_manifest_parses_as_json_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "pack/manifest.json");
        assert!(
            v.is_object(),
            "manifest.json for {name} must be a JSON object"
        );
        assert!(
            v.get("schemaVersion").is_some(),
            "manifest.json for {name} must have schemaVersion"
        );
    }
}

#[test]
fn load_fixture_session_parses_as_json_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "repo/session.json");
        assert!(
            v.is_object(),
            "session.json for {name} must be a JSON object"
        );
        assert!(
            v.get("schemaVersion").is_some(),
            "session.json for {name} must have schemaVersion"
        );
    }
}

#[test]
fn load_fixture_graph_parses_as_json_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "pack/graph.json");
        assert!(v.is_object(), "graph.json for {name} must be a JSON object");
        assert!(
            v.get("nodes").is_some(),
            "graph.json for {name} must have nodes"
        );
        assert!(
            v.get("edges").is_some(),
            "graph.json for {name} must have edges"
        );
    }
}

#[test]
fn load_fixture_profile_parses_as_json_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "pack/project-profile.json");
        assert!(
            v.is_object(),
            "project-profile.json for {name} must be a JSON object"
        );
        assert!(
            v.get("name").is_some(),
            "project-profile.json for {name} must have name"
        );
    }
}

// ---------------------------------------------------------------------------
// Gate: placeholder-aware path assertions
// ---------------------------------------------------------------------------

#[test]
fn is_placeholder_detects_repo_root_token() {
    assert!(is_placeholder_or_relative("__REPO_ROOT__"));
    assert!(is_placeholder_or_relative(
        "__REPO_ROOT__/.repolens/pack-20260329-030041"
    ));
    assert!(!is_placeholder_or_relative("/home/user/repos/my-project"));
    assert!(!is_placeholder_or_relative("relative/path/no/slash"));
}

#[test]
fn is_placeholder_detects_pack_dir_token() {
    assert!(is_placeholder_or_relative("__PACK_DIR__"));
    assert!(is_placeholder_or_relative("__PACK_DIR__/manifest.json"));
}

/// The most important gate: every fixture file in every scenario must not
/// contain raw absolute paths (i.e. all path-like fields must use placeholders).
#[test]
fn no_raw_absolute_paths_in_any_fixture() {
    let files_to_check = [
        "repo/session.json",
        "pack/manifest.json",
        "pack/graph.json",
        "pack/graph-metrics.json",
        "pack/project-profile.json",
    ];

    for scenario in all_scenario_names() {
        for rel in &files_to_check {
            let path = fixtures_scenarios_dir().join(&scenario).join(rel);
            if !path.exists() {
                continue;
            }
            let v = load_fixture(&scenario, rel);
            assert_no_raw_absolute_paths(&format!("{scenario}/{rel}"), &v);
        }
    }
}

#[test]
fn session_json_uses_repo_root_placeholder_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "repo/session.json");
        let repo_root = v
            .get("repoRoot")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("session.json for {name} must have repoRoot"));

        assert_eq!(
            repo_root, "__REPO_ROOT__",
            "session.json for {name}: repoRoot must be exactly __REPO_ROOT__, got: {repo_root:?}"
        );
    }
}

#[test]
fn session_json_active_pack_path_uses_placeholder_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "repo/session.json");
        if let Some(pack_path) = v.get("activePackPath").and_then(|v| v.as_str()) {
            assert!(
                is_placeholder_or_relative(pack_path),
                "session.json for {name}: activePackPath must use a placeholder, got: {pack_path:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Gate: cross-scenario consistency
// ---------------------------------------------------------------------------

/// Every manifest must reference at least one chunk file, and every referenced
/// chunk file must actually exist on disk.
#[test]
fn manifest_chunks_exist_on_disk_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "pack/manifest.json");
        let chunks = v
            .get("chunks")
            .and_then(|c| c.as_array())
            .unwrap_or_else(|| panic!("manifest.json for {name} must have a chunks array"));

        assert!(
            !chunks.is_empty(),
            "manifest.json for {name}: chunks array must not be empty"
        );

        for chunk in chunks {
            let chunk_name = chunk
                .as_str()
                .unwrap_or_else(|| panic!("chunk entry in {name} must be a string"));
            let chunk_path = fixtures_scenarios_dir()
                .join(&name)
                .join("pack")
                .join(chunk_name);
            assert!(
                chunk_path.exists(),
                "manifest.json for {name}: referenced chunk {chunk_name:?} not found at {}",
                chunk_path.display()
            );
        }
    }
}

/// Every scenario must have a `repo/session.json`.
#[test]
fn all_scenarios_have_session_json() {
    for name in all_scenario_names() {
        let path = fixtures_scenarios_dir()
            .join(&name)
            .join("repo")
            .join("session.json");
        assert!(
            path.exists(),
            "scenario {name}: missing repo/session.json at {}",
            path.display()
        );
    }
}

/// Every scenario must have a `pack/` subdirectory.
#[test]
fn all_scenarios_have_pack_dir() {
    for name in all_scenario_names() {
        let path = fixtures_scenarios_dir().join(&name).join("pack");
        assert!(
            path.is_dir(),
            "scenario {name}: missing pack/ directory at {}",
            path.display()
        );
    }
}

/// All chunk files referenced by manifests must be valid JSON arrays.
#[test]
fn all_chunk_files_are_valid_json_arrays() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "pack/manifest.json");
        let chunks = v
            .get("chunks")
            .and_then(|c| c.as_array())
            .unwrap_or_else(|| panic!("manifest.json for {name} must have chunks"));

        for chunk in chunks {
            let chunk_name = chunk.as_str().unwrap();
            let chunk_val = load_fixture(&name, &format!("pack/{chunk_name}"));
            assert!(
                chunk_val.is_array(),
                "pack/{chunk_name} for {name} must be a JSON array of file entries"
            );
        }
    }
}

/// `graph.json` `nodes` must be a non-empty array for every scenario (at
/// minimum one file was scanned), and `edges` must be an array (may be empty).
#[test]
fn graph_nodes_non_empty_edges_array_for_all_scenarios() {
    for name in all_scenario_names() {
        let v = load_fixture(&name, "pack/graph.json");

        let nodes = v
            .get("nodes")
            .and_then(|n| n.as_array())
            .unwrap_or_else(|| panic!("graph.json for {name} must have a nodes array"));
        assert!(
            !nodes.is_empty(),
            "graph.json for {name}: nodes must not be empty"
        );

        let _edges = v
            .get("edges")
            .and_then(|e| e.as_array())
            .unwrap_or_else(|| panic!("graph.json for {name} must have an edges array"));
    }
}
