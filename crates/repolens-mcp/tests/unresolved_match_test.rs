//! #42: which unresolved imports count toward a graph answer.
//!
//! - Match first, then cap: with many other broken imports in the repo,
//!   the one that matters for the file asked about is still found.
//! - A crate's `[lib] name` is its module name too.
//! - `wasm-pack --out-name X` makes `./pkg/X.js` (and `X_bg`) that crate's.
//! - Any other import whose target isn't in the repo is not matched to a
//!   caller by name: only the importing file's answer is "derived".
//! - Python relative imports are shown as written (`.nope_mod`).

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

/// A Rust crate with a JS file importing `import` from its build output.
fn crate_with_js_import(root: &Path, cargo_toml: &str, import: &str) {
    write(root, "Cargo.toml", cargo_toml);
    write(
        root,
        "src/lib.rs",
        "pub fn sm4_encrypt() -> u8 {\n    1\n}\n",
    );
    write(
        root,
        "www/app.js",
        &format!("import * as g from '{import}';\nexport const run = () => g.sm4_encrypt();\n"),
    );
    write(root, "www/i18n.js", "export const t = (k) => k;\n");
}

const GM_WEB: &str =
    "[package]\nname = \"gm-web\"\nversion = \"0.1.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n";

/// The original #31 case under 60 other broken imports that sort first:
/// `./pkg/gm_web.js` used to fall past the 50-entry cap before the match
/// ran, and callers of `src/lib.rs` came back "verified" with no note.
#[test]
fn match_runs_before_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate_with_js_import(root, GM_WEB, "./pkg/gm_web.js");
    for i in 0..60 {
        write(
            root,
            &format!("a/broken_{i:02}.js"),
            &format!("import {{ x }} from './missing_{i:02}.js';\nexport const y{i} = x;\n"),
        );
    }
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert_eq!(env["data"]["unresolvedImports"]["count"], 1, "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l == "1 import couldn't be resolved: ./pkg/gm_web.js in www/app.js"),
        "{env}"
    );
    // A file none of the 61 can be about stays verified and quiet.
    let env = server.call_get_callers_envelope(&pack, "www/i18n.js");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
    assert!(env["data"].get("unresolvedImports").is_none(), "{env}");
}

/// More than 50 matching imports: the list shown is capped, the count is
/// exact.
#[test]
fn shown_list_is_capped_but_the_count_is_exact() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate_with_js_import(root, GM_WEB, "./pkg/gm_web.js");
    for i in 0..59 {
        write(
            root,
            &format!("www/a_{i:02}.js"),
            &format!("import {{ f }} from './pkg/gm_web.js';\nexport const a{i} = f;\n"),
        );
    }
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    let u = &env["data"]["unresolvedImports"];
    assert_eq!(u["count"], 60, "{env}");
    assert_eq!(u["sample"].as_array().unwrap().len(), 50, "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l.starts_with("60 imports couldn't be resolved, e.g. ./pkg/gm_web.js in ")),
        "{env}"
    );
}

/// #42 product call: an import whose target isn't in the repo (a build
/// output, a wrong relative path, a renamed output no build command
/// names) is not matched to a caller by file name. Only the importing
/// file's answer is "derived", with a note naming the import; files that
/// merely share the name stay "verified".
#[test]
fn missing_targets_mark_only_the_importing_file() {
    for (import, decoy) in [
        ("../dist/bundle.js", "src/bundle.js"),
        ("../components", "src/components.js"),
        ("./pkg/index.js", "src/index.js"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate_with_js_import(root, GM_WEB, import);
        write(root, decoy, "export const d = 1;\n");
        write(
            root,
            "www/main.js",
            "import { t } from './i18n.js';\nexport const m = t;\n",
        );
        let pack = scan(root);
        let server = RepoLensMcpServer::new();

        let deps = server.call_get_dependencies_envelope(&pack, "www/app.js");
        assert_eq!(deps["evidenceGrade"], "derived", "{import}: {deps}");
        let want = format!("1 import couldn't be resolved: {import} in www/app.js");
        assert!(notes(&deps["data"]).contains(&want), "{import}: {deps}");

        for file in [decoy, "src/lib.rs", "www/i18n.js"] {
            let env = server.call_get_callers_envelope(&pack, file);
            assert_eq!(env["evidenceGrade"], "verified", "{import} / {file}: {env}");
            assert!(
                env["data"].get("unresolvedImports").is_none(),
                "{import} / {file}: {env}"
            );
        }
        let deps = server.call_get_dependencies_envelope(&pack, "www/main.js");
        assert_eq!(deps["evidenceGrade"], "verified", "{import}: {deps}");
        let impact: serde_json::Value =
            serde_json::from_str(&server.call_get_impact_file(&pack, decoy)).unwrap();
        assert!(
            impact.get("unresolvedImports").is_none(),
            "{import}: {impact}"
        );
    }
}

/// A package.json `name` doesn't name a bundle file, so `./dist/web.js`
/// isn't tied to the package called `web`.
#[test]
fn package_json_name_is_not_a_build_output_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        "{\"name\": \"web\", \"scripts\": {\"build\": \"vite build\"}}\n",
    );
    write(root, "src/web.js", "export const w = 1;\n");
    write(root, "src/other.js", "export const o = 1;\n");
    write(
        root,
        "app/main.js",
        "import { w } from './dist/web.js';\nexport const m = w;\n",
    );
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    for file in ["src/web.js", "src/other.js"] {
        let env = server.call_get_callers_envelope(&pack, file);
        assert_eq!(env["evidenceGrade"], "verified", "{file}: {env}");
    }
    let deps = server.call_get_dependencies_envelope(&pack, "app/main.js");
    assert_eq!(deps["evidenceGrade"], "derived", "{deps}");
}

/// #42 item 3: unresolved Python relative imports are shown as written,
/// and `from . import missing` is reported. A name the package's
/// `__init__.py` defines is a symbol, not a missing module.
#[test]
fn python_relative_imports_are_shown_as_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "pkg/__init__.py", "from .b import Thing\n");
    write(root, "pkg/b.py", "class Thing:\n    pass\n");
    write(root, "pkg/sub/__init__.py", "");
    write(
        root,
        "pkg/a.py",
        "from .nope_mod import x\nfrom . import missing\nfrom . import Thing\nfrom . import b\n",
    );
    write(
        root,
        "pkg/sub/c.py",
        "from .. import gone\nfrom ..core.base import Base\n",
    );
    let pack = scan(root);
    let server = RepoLensMcpServer::new();

    let imports = |file: &str| -> Vec<String> {
        let env = server.call_get_dependencies_envelope(&pack, file);
        assert_eq!(env["evidenceGrade"], "derived", "{file}: {env}");
        env["data"]["unresolvedImports"]["sample"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["import"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(imports("pkg/a.py"), [".missing", ".nope_mod"]);
    assert_eq!(imports("pkg/sub/c.py"), ["..core.base", "..gone"]);
    let env = server.call_get_dependencies_envelope(&pack, "pkg/b.py");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
}

/// `[lib] name = "gm_wasm"` renames the module wasm-pack writes.
#[test]
fn lib_name_counts_as_the_crates_module_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let toml = "[package]\nname = \"gm-web\"\nversion = \"0.1.0\"\n\n[lib]\nname = \"gm_wasm\"\ncrate-type = [\"cdylib\"]\n";
    crate_with_js_import(root, toml, "./pkg/gm_wasm.js");
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l.contains("./pkg/gm_wasm.js")),
        "{env}"
    );
    let env = server.call_get_callers_envelope(&pack, "www/i18n.js");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
}

/// `wasm-pack build --out-name index` in a package.json script.
#[test]
fn wasm_pack_out_name_in_package_json_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate_with_js_import(root, GM_WEB, "./pkg/index.js");
    write(
        root,
        "package.json",
        "{\n  \"name\": \"site\",\n  \"scripts\": {\n    \"build:wasm\": \"wasm-pack build --target web --out-name index\"\n  }\n}\n",
    );
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l.contains("./pkg/index.js")),
        "{env}"
    );
    let env = server.call_get_callers_envelope(&pack, "www/i18n.js");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
}

/// `wasm-pack build <crate dir> --out-name=X` in a CI workflow, with the
/// `_bg` module: only that crate's files get the note.
#[test]
fn wasm_pack_out_name_in_ci_applies_to_the_named_crate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/web\", \"crates/other\"]\n",
    );
    write(
        root,
        "crates/web/Cargo.toml",
        "[package]\nname = \"web\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        "crates/web/src/lib.rs",
        "pub fn f() -> u8 {\n    1\n}\n",
    );
    write(
        root,
        "crates/other/Cargo.toml",
        "[package]\nname = \"other\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        "crates/other/src/lib.rs",
        "pub fn g() -> u8 {\n    2\n}\n",
    );
    write(
        root,
        "www/app.js",
        "import * as b from './pkg/bundle_bg.js';\nexport const run = () => b.f();\n",
    );
    write(
        root,
        ".github/workflows/ci.yml",
        "on: push\njobs:\n  wasm:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: wasm-pack build crates/web --release --out-name=bundle --out-dir ../../www/pkg\n",
    );
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "crates/web/src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l.contains("./pkg/bundle_bg.js")),
        "{env}"
    );
    let env = server.call_get_callers_envelope(&pack, "crates/other/src/lib.rs");
    assert_eq!(env["evidenceGrade"], "verified", "{env}");
}

/// #42 L1: trailing TOML comments on `[lib]` and on `name`.
#[test]
fn lib_name_is_read_past_trailing_toml_comments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let toml = "[package] # crate\nname = \"gm-web\" # published\nversion = \"0.1.0\"\n\n[lib] # wasm build\nname = \"gm_wasm\" # what wasm-pack writes\ncrate-type = [\"cdylib\"]\n";
    crate_with_js_import(root, toml, "./pkg/gm_wasm.js");
    let pack = scan(root);
    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert!(
        notes(&env["data"])
            .iter()
            .any(|l| l.contains("./pkg/gm_wasm.js")),
        "{env}"
    );
}

/// #42 L3: a pack written before #42 kept only 50 imports. The rest can't
/// be ruled out, and the note names the rescan that stores them all.
#[test]
fn older_pack_with_50_stored_names_the_rescan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate_with_js_import(root, GM_WEB, "./lib_ok.js");
    write(root, "www/lib_ok.js", "export const k = 1;\n");
    for i in 0..60 {
        write(
            root,
            &format!("a/broken_{i:02}.js"),
            &format!("import {{ x }} from './missing_{i:02}.js';\nexport const y{i} = x;\n"),
        );
    }
    let pack = scan(root);
    // Rewrite the metrics the way an older build stored them.
    let path = Path::new(&pack).join("graph-metrics.json");
    let mut m: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(m["unresolvedImports"]["count"], 60, "{m}");
    m["unresolvedImports"]["sample"]
        .as_array_mut()
        .unwrap()
        .truncate(50);
    fs::write(&path, serde_json::to_string_pretty(&m).unwrap()).unwrap();

    let server = RepoLensMcpServer::new();
    let env = server.call_get_callers_envelope(&pack, "src/lib.rs");
    assert_eq!(env["evidenceGrade"], "derived", "{env}");
    assert_eq!(env["data"]["unresolvedImports"]["count"], 10, "{env}");
    assert!(
        notes(&env["data"]).iter().any(|l| l
            == "This pack stores only 50 unresolved imports. Run `repolens scan` to check all."),
        "{env}"
    );
}
