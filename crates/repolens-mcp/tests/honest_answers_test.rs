//! #31: dependency, impact and fan-in answers say what was skipped.
//!
//! - An import that couldn't be resolved (gm-crypto-web's
//!   `www/app.js` -> `./pkg/gm_web.js`, the WASM build of `src/lib.rs`) is
//!   named in the answer, and the empty answer is not graded `verified`.
//! - `get_impact` says that Rust integration tests aren't in the graph.

use std::fs;
use std::path::Path;

use repolens_app::scan::ScanService;
use repolens_mcp::RepoLensMcpServer;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn scan(root: &Path) -> String {
    let result = ScanService::new().run(root, false, 100).unwrap();
    result.pack_dir.to_string_lossy().to_string()
}

/// gm-crypto-web's shape: a Rust crate built to WASM, loaded by a JS file
/// through a build output folder that isn't in the repo.
fn wasm_repo(root: &Path) -> String {
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"gm-web\"\nversion = \"0.1.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
    );
    write(
        root,
        "src/lib.rs",
        "pub fn sm4_encrypt() -> u8 {\n    1\n}\n",
    );
    write(
        root,
        "www/app.js",
        "import init, * as g from './pkg/gm_web.js';\n\nexport function run() {\n  return g.sm4_encrypt();\n}\n",
    );
    scan(root)
}

fn notes(data: &serde_json::Value) -> Vec<String> {
    data["notes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn callers_of_lib_rs_name_the_unresolved_import() {
    let dir = tempfile::tempdir().unwrap();
    let pack = wasm_repo(dir.path());
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    let data = &env["data"];
    assert_eq!(data["totalCallers"], 0, "{env}");
    assert_ne!(env["evidenceGrade"], "verified", "{env}");
    assert_eq!(data["unresolvedImports"]["count"], 1, "{env}");
    assert_eq!(
        data["unresolvedImports"]["sample"][0]["source"],
        "www/app.js"
    );
    assert_eq!(
        data["unresolvedImports"]["sample"][0]["import"],
        "./pkg/gm_web.js"
    );
    let n = notes(data);
    assert!(
        n.iter()
            .any(|l| l == "1 import couldn't be resolved: ./pkg/gm_web.js in www/app.js"),
        "{n:?}"
    );
}

#[test]
fn dependencies_and_impact_also_name_the_unresolved_import() {
    let dir = tempfile::tempdir().unwrap();
    let pack = wasm_repo(dir.path());
    let server = RepoLensMcpServer::new();

    let deps = server.call_get_dependencies_envelope(&pack, "www/app.js");
    assert_ne!(deps["evidenceGrade"], "verified", "{deps}");
    assert_eq!(deps["data"]["unresolvedImports"]["count"], 1, "{deps}");

    let impact: serde_json::Value =
        serde_json::from_str(&server.call_get_impact_file(&pack, "src/lib.rs")).unwrap();
    let n = notes(&impact);
    assert!(n.iter().any(|l| l.contains("./pkg/gm_web.js")), "{impact}");
}

#[test]
fn answers_without_unresolved_imports_stay_verified_and_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "src/a.js",
        "import { b } from './b.js';\nexport const a = b;\n",
    );
    write(root, "src/b.js", "export const b = 1;\n");
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/b.js");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
    assert!(env["data"].get("unresolvedImports").is_none(), "{env}");
    assert!(notes(&env["data"]).is_empty(), "{env}");
}

#[test]
fn impact_says_rust_integration_tests_are_not_covered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "crates/demo-core/Cargo.toml",
        "[package]\nname = \"demo-core\"\nversion = \"0.1.0\"\n",
    );
    write(root, "crates/demo-core/src/lib.rs", "pub mod curve;\n");
    write(
        root,
        "crates/demo-core/src/curve.rs",
        "pub fn double(x: u32) -> u32 {\n    x * 2\n}\n",
    );
    write(
        root,
        "crates/demo-core/tests/curve_test.rs",
        "use demo_core::curve::double;\n\n#[test]\nfn doubles() {\n    assert_eq!(double(2), 4);\n}\n",
    );
    write(
        root,
        "crates/demo-core/tests/other_test.rs",
        "#[test]\nfn other() {}\n",
    );
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let impact: serde_json::Value =
        serde_json::from_str(&server.call_get_impact_file(&pack, "crates/demo-core/src/curve.rs"))
            .unwrap();
    let listed = impact["impactedFiles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["file"] == "crates/demo-core/tests/curve_test.rs");
    let n = notes(&impact);
    assert!(
        listed
            || n.iter().any(|l| l
                == "Tests aren't covered: 2 Rust test files in tests/ folders aren't in the import graph (2 in crates/demo-core/tests/)"),
        "{impact}"
    );
}

/// QA bug 3 on #41: the note is scoped to answers it can affect. In the
/// gm-crypto-web shape, `www/i18n.js` is a JS file that resolves fine;
/// the unresolved `./pkg/gm_web.js` (a build of the Rust crate `gm-web`)
/// can't be one of its callers, so its answers stay verified and quiet.
#[test]
fn unrelated_files_stay_verified_without_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "www/i18n.js",
        "export function t(k) {\n  return k;\n}\n",
    );
    write(
        root,
        "www/main.js",
        "import { t } from './i18n.js';\nexport const x = t('a');\n",
    );
    let pack = wasm_repo(root);
    let server = RepoLensMcpServer::new();

    let env = server.call_get_callers_envelope(&pack, "www/i18n.js");
    assert_eq!(env["data"]["totalCallers"], 1, "{env}");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
    assert!(env["data"].get("unresolvedImports").is_none(), "{env}");
    assert!(notes(&env["data"]).is_empty(), "{env}");

    let deps = server.call_get_dependencies_envelope(&pack, "www/main.js");
    assert_eq!(deps["evidenceGrade"], "verified", "{deps}");
    assert!(deps["data"].get("unresolvedImports").is_none(), "{deps}");

    let impact: serde_json::Value =
        serde_json::from_str(&server.call_get_impact_file(&pack, "www/i18n.js")).unwrap();
    assert!(impact.get("unresolvedImports").is_none(), "{impact}");
    assert!(
        !notes(&impact).iter().any(|l| l.contains("gm_web")),
        "{impact}"
    );

    // The crate's own files still get the note: lib.rs is what
    // ./pkg/gm_web.js is built from.
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_ne!(env["evidenceGrade"], "verified", "{env}");
    assert_eq!(env["data"]["unresolvedImports"]["count"], 1, "{env}");
}

/// #42 product call: an unresolved import with the queried file's name
/// (`./vendor/util.js` vs `lib/util.js`) is not guessed to be that file.
/// The importing file's dependencies answer names it instead.
#[test]
fn unresolved_import_with_the_queried_files_name_is_not_a_guessed_caller() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "lib/util.js", "export const u = 1;\n");
    write(
        root,
        "src/a.js",
        "import { u } from './vendor/util.js';\nexport const a = u;\n",
    );
    write(root, "src/b.js", "export const b = 2;\n");
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    for file in ["lib/util.js", "src/b.js"] {
        let env = server.call_get_callers_envelope(&pack, file);
        assert_eq!(env["evidenceGrade"], "verified", "{file}: {env}");
        assert!(env["data"].get("unresolvedImports").is_none(), "{env}");
    }
    let env = server.call_get_dependencies_envelope(&pack, "src/a.js");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert_eq!(env["data"]["unresolvedImports"]["count"], 1, "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l == "1 import couldn't be resolved: ./vendor/util.js in src/a.js"),
        "{env}"
    );
}

/// #42 N1: over the 5,000 stored unresolved imports, `get_callers`,
/// `get_dependencies` and `get_impact` each carry exactly one line saying
/// results may be incomplete, within 80 columns, and none says "callers".
#[test]
fn over_cap_answers_say_results_may_be_incomplete_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/target.js", "export const t = 1;\n");
    write(
        root,
        "src/app.js",
        "import { t } from './target.js';\nexport const a = t + 1;\n",
    );
    for f in 0..51 {
        let body: String = (0..100)
            .map(|i| format!("import './missing/m{:05}.js';\n", f * 100 + i))
            .chain(std::iter::once(format!("export const g{f} = {f};\n")))
            .collect();
        write(root, &format!("gen/g{f:03}.js"), &body);
    }
    write(root, "gen/extra.js", "import './missing/last.js';\n");
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let want = "5,101 unresolved imports (101 not stored); results may be incomplete.";

    let callers = server.call_get_callers_envelope(&pack, "src/target.js");
    let deps = server.call_get_dependencies_envelope(&pack, "src/app.js");
    let impact: serde_json::Value =
        serde_json::from_str(&server.call_get_impact_file(&pack, "src/target.js")).unwrap();
    for (tool, data) in [
        ("get_callers", &callers["data"]),
        ("get_dependencies", &deps["data"]),
        ("get_impact", &impact),
    ] {
        let n = notes(data);
        assert_eq!(
            n.iter().filter(|l| l.as_str() == want).count(),
            1,
            "{tool}: {n:?}"
        );
        assert_eq!(
            n.iter().filter(|l| l.contains("not stored")).count(),
            1,
            "{tool}: {n:?}"
        );
        assert!(
            n.iter().all(|l| !l.contains("callers may be missing")),
            "{tool}: {n:?}"
        );
        assert!(n.iter().all(|l| l.chars().count() <= 80), "{tool}: {n:?}");
    }
}
