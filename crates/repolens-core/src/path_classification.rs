//! Path-classification predicates shared by the evidence-finding emitter
//! and any other consumer that needs to filter root configuration, policy,
//! or docs files from "source-shaped" claims.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` §B/§I.

const ROOT_FILE_NAMES: &[&str] = &[
    "cargo.toml",
    "cargo.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "go.mod",
    "go.sum",
    "pyproject.toml",
    "poetry.lock",
    "pipfile",
    "pipfile.lock",
    "setup.py",
    "setup.cfg",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "license",
    "license.md",
    "license.txt",
    "license-mit",
    "license-apache",
    "readme.md",
    "readme.rst",
    "readme.txt",
    "readme",
    "changelog.md",
    "changelog",
    "contributing.md",
    "code_of_conduct.md",
    "security.md",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".rustfmt.toml",
    "rustfmt.toml",
    "clippy.toml",
    ".clippy.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    "deny.toml",
    ".dockerignore",
];

const POLICY_DIR_PREFIXES: &[&str] = &[
    ".github/", ".gitlab/", "docs/", "doc/",
    // Editor / agent configuration directories. Files inside these are
    // tool inputs, not source the import graph would link to.
    ".vscode/", ".idea/", ".cursor/", ".claude/",
];

/// Filenames that count as "not source" wherever they appear in the tree
/// (e.g. workspace member manifests like `crates/foo/Cargo.toml`).
/// Distinct from `ROOT_FILE_NAMES`, which only filter at the repo root
/// (top-level dotfiles, READMEs without a directory prefix).
const ANYWHERE_BASENAMES: &[&str] = &[
    // Manifests
    "cargo.toml",
    "package.json",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "pipfile",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "go.mod",
    // Lockfiles
    "cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "go.sum",
    "poetry.lock",
    "pipfile.lock",
    // Documentation / policy at any depth
    "readme.md",
    "readme.rst",
    "readme.txt",
    "readme",
    "changelog.md",
    "changelog",
    "contributing.md",
    "code_of_conduct.md",
    "security.md",
    "license",
    "license.md",
    "license.txt",
    "license-mit",
    "license-apache",
];

/// True if a path is a root manifest, lockfile, license, README, gitconfig,
/// CI config directory, or a policy/docs directory entry. These files are
/// not treated as source for orphan analysis or "dead code" claims.
pub fn is_root_config_or_policy(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    // Root-level exact matches (dotfiles, top-level README without a path).
    if ROOT_FILE_NAMES.contains(&lower.as_str()) {
        return true;
    }
    if POLICY_DIR_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    // Top-level markdown files are also treated as docs/policy.
    if lower.ends_with(".md") && !lower.contains('/') {
        return true;
    }
    // Manifests, lockfiles, and policy docs at any depth (e.g. workspace
    // member manifests like `crates/foo/Cargo.toml` or `packages/app/
    // package.json`). The basename is checked against ANYWHERE_BASENAMES.
    let basename = lower.rsplit('/').next().unwrap_or(&lower);
    ANYWHERE_BASENAMES.contains(&basename)
}

/// True if a lower-cased path is a `.rs` file directly inside a Cargo
/// runner directory (`tests/`, `benches/`, `examples/`) at a crate root —
/// the exact set Cargo treats as independent target files. Deeper paths
/// like `tests/common/mod.rs` are helper modules (Cargo doesn't invoke
/// them; another target file imports them) and must NOT be classified
/// here, otherwise unreferenced helpers escape dead-code detection.
///
/// "Direct child of `tests/`/`benches/`/`examples/` at a crate root"
/// means: after the last `tests/`/`benches/`/`examples/` segment in the
/// path, the remainder is a single filename with no further `/`.
/// Also rejects paths nested under `src/` (those are normal source
/// modules, not target files).
fn is_cargo_runner_target_direct_child(lower: &str) -> bool {
    const RUNNER_DIRS: &[&str] = &["/tests/", "/benches/", "/examples/"];

    // Crate-root case. The substring after the OUTERMOST runner dir
    // (closest to the crate root) must be either:
    //   (a) a single filename (`tests/foo.rs`) — file-style target, OR
    //   (b) `<dir>/main.rs` (`tests/api/main.rs`) — directory-style
    //       target (Cargo discovers `<dir>/main.rs` under each runner
    //       directory as a named target).
    // Use `find` (first occurrence), NOT `rfind`: for a pathological
    // case like `crates/foo/tests/common/tests/helper.rs`, `rfind`
    // would pick the inner `/tests/` and incorrectly classify the file
    // as a target. Cargo only treats files under the outermost runner
    // dir (the sibling of `src/`) as targets.
    //
    // Additionally, runner dirs live as SIBLINGS of `src/` at a crate
    // root. A runner segment that sits UNDER any of `src/`, `tests/`,
    // `benches/`, or `examples/` is a nested helper module, not a
    // Cargo target. Test that by splitting the prefix-before-the-
    // runner-dir into segments and rejecting if any earlier segment is
    // one of those ancestor names.
    //
    // - `crates/foo/src/tests/helper.rs`        → ancestor `src` rejects
    // - `crates/foo/tests/common/benches/helper.rs` → ancestor `tests` rejects
    // - `crates/foo/tests/common/tests/helper.rs`   → ancestor `tests` rejects
    //
    // This deliberately keeps directory-style targets NAMED with any
    // of these words (e.g. `tests/src/main.rs`, `tests/tests/main.rs`)
    // classified — the matching word appears AFTER the OUTERMOST
    // runner dir, so it's the target name, not an ancestor.
    //
    // `fixtures`, `fixture`, and `testdata` are also rejected as
    // ancestors. A path like `crates/foo/fixtures/tests/helper.rs` is
    // inside a fixtures tree, NOT a Cargo target — Cargo discovers
    // targets only under crate-root `tests/`/`benches/`/`examples/`,
    // never under fixture/testdata subtrees.
    const SOURCE_TREE_SEGMENTS: &[&str] = &[
        "src", "tests", "benches", "examples", "fixtures", "fixture", "testdata",
    ];
    for dir in RUNNER_DIRS {
        if let Some(idx) = lower.find(dir) {
            let before = &lower[..idx];
            if before
                .split('/')
                .any(|seg| SOURCE_TREE_SEGMENTS.contains(&seg))
            {
                continue;
            }
            let after = &lower[idx + dir.len()..];
            if is_runner_target_remainder(after) {
                return true;
            }
        }
    }
    // Top-level case: `tests/<file>.rs`, `benches/<file>.rs`,
    // `examples/<file>.rs`. Same direct-child OR directory-style rule.
    // No "before" segments to check — a top-level runner dir can't be
    // nested under `src/` by definition.
    for prefix in &["tests/", "benches/", "examples/"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if is_runner_target_remainder(rest) {
                return true;
            }
        }
    }
    false
}

/// Given the path remainder after a Cargo runner dir (e.g.
/// `tests/`), true if it names a runner target:
///   - file-style: `<file>.rs` directly under the runner dir
///   - directory-style: `<dir>/main.rs` exactly (Cargo's named-target
///     convention; helpers like `<dir>/helper.rs` are NOT targets).
fn is_runner_target_remainder(remainder: &str) -> bool {
    if remainder.is_empty() {
        return false;
    }
    // File-style: no `/` in the remainder.
    if !remainder.contains('/') {
        return true;
    }
    // Directory-style: exactly one `/`, ends with `/main.rs`.
    if remainder.ends_with("/main.rs") && remainder.matches('/').count() == 1 {
        return true;
    }
    false
}

/// True if a file path is an "external runner target" — source that is
/// executed by a framework/tool runner rather than imported by other source
/// files. Cargo integration tests, benches, examples, and build scripts all
/// fit this shape: `cargo test` / `cargo bench` / `cargo run --example` /
/// the build-script protocol invokes them directly, so they have zero
/// inbound structural import edges by design. Without this filter they
/// dominate the orphan-files finding on every Cargo workspace.
///
/// Test-data files (yaml/json/toml under `**/fixtures/**`) are also
/// recognised here: they are read at test runtime, never imported, and
/// flagging them as orphans is pure noise.
pub fn is_external_runner_target(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();

    // Cargo build script — ONLY the literal top-level `build.rs`.
    // Cargo gives build-script semantics to `build.rs` only when it
    // sits next to `Cargo.toml`. From the path alone we cannot tell
    // which nested directories contain a `Cargo.toml`, so the
    // conservative rule is to match the unambiguous top-level case
    // and let nested `build.rs` files (e.g. `tools/build.rs`,
    // `crates/foo/src/build.rs`) remain visible to orphan / dead-code
    // analysis. Workspace-member-root build scripts are a known false
    // negative under this rule; the alternative is hiding real dead
    // code, which is worse.
    if lower == "build.rs" {
        return true;
    }

    // Cargo target dirs that the test/bench/example runners pick up by
    // file path. Cargo's rule: ONLY `.rs` files DIRECTLY inside `tests/`,
    // `benches/`, or `examples/` are independent target files. Helper
    // modules at deeper paths (e.g. `tests/common/mod.rs`) are ordinary
    // modules imported by another target — if no target imports them
    // they ARE dead code and must remain visible to F-ORPHAN-FILES /
    // find_dead_code.
    //
    // Additionally these target dirs live as SIBLINGS of `src/` under a
    // crate root; a `mod tests {}` submodule nested under `src/` is
    // also normal source. The nested-under-src guard rejects both the
    // `crates/foo/src/...` shape and the top-level `src/...` shape.
    if lower.ends_with(".rs") && is_cargo_runner_target_direct_child(&lower) {
        return true;
    }

    // Test-data fixtures (yaml/json/toml/txt files under any `fixtures/`
    // directory). These are inputs to tests, not source.
    let is_fixture_data = lower.contains("/fixtures/") || lower.starts_with("fixtures/");
    if is_fixture_data
        && (lower.ends_with(".yaml")
            || lower.ends_with(".yml")
            || lower.ends_with(".json")
            || lower.ends_with(".toml")
            || lower.ends_with(".txt"))
    {
        return true;
    }

    // `insta` snapshot files. Generated by the testing framework, consumed
    // by `cargo insta review` — never imported by source.
    if lower.ends_with(".snap") || lower.ends_with(".snap.new") {
        return true;
    }

    false
}

/// True if a file path is a library entry point — a crate-root `lib.rs`
/// at the top of a Cargo crate (whether top-level or workspace-member).
/// These files are the public interface to their crate and Cargo treats
/// them as the library target's entry point; they have no inbound
/// structural import edges by design.
///
/// Distinct from [`is_indicator_tagged_module_path`], which also matches
/// hub modules like `-core/` / `-util/` workspace members. Those legitimately
/// need imports; only the entry-point subset belongs in this predicate.
pub fn is_library_entry_point(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "src/lib.rs" || lower.ends_with("/src/lib.rs")
}

/// Combined predicate used by orphan / dead-code analysis: true if a path
/// should be excluded from "files with no inbound structural import edges"
/// claims. Unions three classes:
///   1. Non-source artefacts (manifests, docs, policy, editor configs) —
///      [`is_root_config_or_policy`].
///   2. External-runner targets (Cargo tests/benches/examples/build scripts,
///      fixture data, insta snapshots) — [`is_external_runner_target`].
///   3. Library entry points (crate-root `lib.rs`) — [`is_library_entry_point`].
///
/// Every consumer that flags "orphan" / "dead code" files based on
/// zero-inbound-edges MUST use this predicate to avoid false positives
/// against the same classes. Used by `F-ORPHAN-FILES` (evidence findings)
/// and `find_dead_code` (graph tool).
pub fn is_non_orphan_source(path: &str) -> bool {
    is_root_config_or_policy(path)
        || is_license_file(path)
        || is_external_runner_target(path)
        || is_library_entry_point(path)
        || is_test_fixture_path(path)
        || is_harness_run_file(path)
}

/// True for files a test runner, compiler or tool loads by naming
/// convention rather than by import (#26), so "nothing imports it" says
/// nothing about whether they are used:
///   - Python test modules (`test_*.py`, `*_test.py`, `conftest.py`) and
///     `__init__.py` package markers under a `test`/`tests` folder;
///   - JUnit-style classes under `src/test/` (`*Test`, `*Tests`, `Test*`,
///     `*TestCase`, `*IT`) and Java `package-info.java` / `module-info.java`;
///   - Go `*_test.go`;
///   - Jest/Vitest/Mocha specs (`*.test.*`, `*.spec.*`, `__tests__/`);
///   - tool configs (`*.config.{js,ts,...}`, `.*rc.{js,cjs}`);
///   - non-Rust files under an example or benchmark folder. Rust keeps
///     Cargo's own target rules in [`is_external_runner_target`].
pub fn is_harness_run_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let segments: Vec<&str> = lower.split('/').collect();
    let (dirs, name) = segments.split_at(segments.len() - 1);
    let name = name[0];
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let in_dir = |names: &[&str]| dirs.iter().any(|d| names.contains(d));

    match ext {
        "py" => {
            if stem.starts_with("test_") || stem.ends_with("_test") || stem == "conftest" {
                return true;
            }
            if stem == "__init__" && in_dir(&["test", "tests"]) {
                return true;
            }
        }
        "java" | "kt" | "groovy" | "scala" => {
            if ext == "java" && (stem == "package-info" || stem == "module-info") {
                return true;
            }
            let under_src_test = dirs.windows(2).any(|w| w == ["src", "test"]);
            // Case matters for JUnit class-name patterns: use the original.
            let orig = path.rsplit('/').next().unwrap_or(path);
            let orig_stem = orig.rsplit_once('.').map(|(s, _)| s).unwrap_or(orig);
            let junit_name = orig_stem.ends_with("Test")
                || orig_stem.ends_with("Tests")
                || orig_stem.ends_with("TestCase")
                || orig_stem.ends_with("IT")
                || (orig_stem.starts_with("Test")
                    && orig_stem[4..].starts_with(|c: char| c.is_ascii_uppercase()));
            if under_src_test && junit_name {
                return true;
            }
        }
        "go" if stem.ends_with("_test") => return true,
        _ => {}
    }

    let is_js = matches!(
        ext,
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
    );
    if is_js {
        let inner = stem.rsplit_once('.').map(|(_, s)| s);
        if matches!(inner, Some("test" | "spec" | "config")) || in_dir(&["__tests__"]) {
            return true;
        }
        if stem.starts_with('.') && stem.ends_with("rc") {
            return true;
        }
    }

    if ext != "rs"
        // Plural only: `example` is also a common Java package name
        // (`com/example/App.java`).
        && in_dir(&[
            "examples",
            "bench",
            "benches",
            "benchmark",
            "benchmarks",
        ])
    {
        return true;
    }
    false
}

/// True for any file under a test-fixture or test-data directory
/// (`fixtures/`, `__fixtures__/`, `testdata/`, `test-data/`, at any depth),
/// whatever its extension. Such trees hold sample inputs (#12: RepoLens's
/// own `fixtures/inputs/**` are sample `.rs`/`.js`/`.py` projects) that
/// nothing imports by design, so they are never "unused source". This is
/// deliberately separate from [`is_external_runner_target`]: fixture
/// helpers are still ordinary source for runner-target classification.
pub fn is_test_fixture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    ["fixtures", "__fixtures__", "testdata", "test-data"]
        .iter()
        .any(|d| lower.starts_with(&format!("{d}/")) || lower.contains(&format!("/{d}/")))
}

/// License / notice files anywhere in the tree: `LICENSE*`, `LICENCE*`,
/// `COPYING*`, `UNLICENSE`, `NOTICE*`.
pub fn is_license_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    ["license", "licence", "copying", "unlicense", "notice"]
        .iter()
        .any(|p| name.starts_with(p))
}

/// File extensions RepoLens extracts imports from (Rust, Go, Java,
/// JavaScript/TypeScript, Python). Only these files can be "not imported by
/// anything" in a meaningful sense; data files, scripts in other languages
/// and extension-less files have no import edges by construction.
const IMPORT_GRAPH_EXTENSIONS: &[&str] = &[
    ".rs", ".go", ".java", ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".mts", ".cts", ".py",
];

/// True if `path` is a source file in a language with import extraction.
pub fn is_import_graph_source(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    IMPORT_GRAPH_EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

/// True if a file path looks like a hub module — paths that legitimately
/// have very high inbound graph fan-in by design (lib.rs, mod.rs,
/// prelude.rs, or a workspace member named `*-core`/`*-util`/etc.).
pub fn is_indicator_tagged_module_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with("/lib.rs")
        || lower.ends_with("/mod.rs")
        || lower.ends_with("/prelude.rs")
        || lower == "src/lib.rs"
    {
        return true;
    }
    const STEMS: &[&str] = &[
        "-core/", "-common/", "-util/", "-utils/", "-lib/", "-shared/", "-types/", "-base/",
    ];
    STEMS.iter().any(|s| lower.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #26: files a test runner or tool loads by naming convention are
    /// never "unused source": pytest/unittest modules, JUnit classes under
    /// `src/test/`, `go test` files, Jest/Vitest specs, tool configs, Java
    /// package/module descriptors, and non-Rust example/benchmark folders.
    #[test]
    fn harness_run_files_are_not_orphan_sources() {
        for p in [
            // pytest / unittest (requests, ManimSimpleRender)
            "tests/conftest.py",
            "tests/test_utils.py",
            "tests/explainer_tests/test_diagram_codegen.py",
            "pkg/util_test.py",
            "tests/e2e/__init__.py",
            "test/__init__.py",
            // JUnit (gson)
            "gson/src/test/java/com/google/gson/CommentsTest.java",
            "extras/src/test/java/com/google/gson/interceptors/InterceptorTest.java",
            "src/test/java/a/TestTypes.java",
            "src/test/java/a/FooTests.java",
            "src/test/java/a/FooIT.java",
            "src/test/kotlin/a/FooTest.kt",
            // Java descriptors read by the compiler / javadoc
            "gson/src/main/java/com/google/gson/package-info.java",
            "gson/src/main/java/module-info.java",
            // go test
            "args_test.go",
            "internal/x/y_test.go",
            // Jest / Vitest / Mocha
            "src/a.test.ts",
            "packages/zod/src/v4/x.spec.tsx",
            "web/__tests__/app.js",
            // Tool configs loaded by the tool, not imported
            ".configs/rollup.config.js",
            "vitest.config.ts",
            "packages/x/eslint.config.mjs",
            ".eslintrc.cjs",
            // Example and benchmark folders (non-Rust)
            "examples/scenes/simple.py",
            "examples/scenes/test_debugging_workflow.py",
            "packages/bench/array.ts",
            "benchmarks/run.py",
        ] {
            assert!(is_non_orphan_source(p), "{p}");
        }
        for p in [
            // Ordinary modules, including helpers in test folders.
            "requests/utils.py",
            "tests/utils.py",
            "src/__init__.py",
            "app/__init__.py",
            "gson/src/main/java/com/google/gson/Gson.java",
            "src/test/java/a/Helpers.java",
            "src/main/java/a/FooTest.java",
            "cobra.go",
            "src/config.ts",
            "src/testing.ts",
            "src/latest.py",
            "src/contest.py",
            "src/main/java/com/example/App.java",
            // Rust keeps Cargo's precise target rules.
            "crates/foo/examples/shared/helper.rs",
            "crates/foo/benches/utils/inner.rs",
        ] {
            assert!(!is_non_orphan_source(p), "{p}");
        }
    }

    #[test]
    fn fixture_trees_are_not_orphan_sources() {
        for p in [
            "fixtures/inputs/rust-small/src/main.rs",
            "fixtures/scenarios/js/app.js",
            "crates/x/tests/fixtures/sample.py",
            "go/pkg/testdata/a.go",
            "web/__fixtures__/x.ts",
            "fixtures/test-data/cyclic-graph/graph.json",
        ] {
            assert!(is_non_orphan_source(p), "{p}");
        }
        for p in ["src/fixtures.rs", "src/my_fixtures_x/a.rs"] {
            assert!(!is_non_orphan_source(p), "{p}");
        }
    }

    #[test]
    fn license_files_and_non_code_are_not_orphan_sources() {
        for p in [
            "COPYING",
            "UNLICENSE",
            "LICENSE-MIT",
            "crates/x/LICENSE-APACHE",
            "NOTICE",
        ] {
            assert!(is_non_orphan_source(p), "{p}");
        }
        for p in [
            "benchsuite/runs/a/raw.csv",
            "benchsuite/benchsuite",
            ".nvim.lua",
            "COPYING",
        ] {
            assert!(!is_import_graph_source(p), "{p}");
        }
        for p in [
            "src/a.rs",
            "web/x.tsx",
            "tool.py",
            "Main.java",
            "cmd/main.go",
        ] {
            assert!(is_import_graph_source(p), "{p}");
        }
    }

    #[test]
    fn manifests_and_lockfiles_classified() {
        assert!(is_root_config_or_policy("Cargo.toml"));
        assert!(is_root_config_or_policy("Cargo.lock"));
        assert!(is_root_config_or_policy("package.json"));
        assert!(is_root_config_or_policy("go.mod"));
        assert!(is_root_config_or_policy("pyproject.toml"));
        assert!(is_root_config_or_policy("pom.xml"));
    }

    #[test]
    fn licenses_and_readmes_classified() {
        assert!(is_root_config_or_policy("LICENSE"));
        assert!(is_root_config_or_policy("LICENSE.md"));
        assert!(is_root_config_or_policy("LICENSE-MIT"));
        assert!(is_root_config_or_policy("README.md"));
        assert!(is_root_config_or_policy("README.rst"));
    }

    #[test]
    fn dotfiles_and_lints_classified() {
        assert!(is_root_config_or_policy(".gitignore"));
        assert!(is_root_config_or_policy(".editorconfig"));
        assert!(is_root_config_or_policy("rustfmt.toml"));
        assert!(is_root_config_or_policy("clippy.toml"));
        assert!(is_root_config_or_policy("deny.toml"));
    }

    #[test]
    fn policy_directory_prefixes_classified() {
        assert!(is_root_config_or_policy(".github/workflows/ci.yml"));
        assert!(is_root_config_or_policy(".gitlab/issue_templates/bug.md"));
        assert!(is_root_config_or_policy("docs/architecture.md"));
        assert!(is_root_config_or_policy("doc/internal.md"));
    }

    #[test]
    fn top_level_markdown_and_nested_policy_docs_classified() {
        assert!(is_root_config_or_policy("CHANGELOG.md"));
        assert!(is_root_config_or_policy("CONTRIBUTING.md"));
        // Nested README/CHANGELOG/LICENSE files are still docs/policy
        // wherever they live (e.g. workspace member crates).
        assert!(is_root_config_or_policy("crates/foo/README.md"));
        assert!(is_root_config_or_policy("packages/web/LICENSE"));
        // Nested arbitrary .md inside a source tree is NOT classified
        // (it might be embedded help, docs-as-code).
        assert!(!is_root_config_or_policy("src/help.md"));
        assert!(!is_root_config_or_policy("docs-internal/notes/x.md"));
    }

    #[test]
    fn nested_workspace_manifests_classified_anywhere() {
        // Workspace member manifests must not be reported as orphan
        // source files even though they live deep in the tree.
        assert!(is_root_config_or_policy("crates/foo/Cargo.toml"));
        assert!(is_root_config_or_policy("packages/app/package.json"));
        assert!(is_root_config_or_policy("services/api/pom.xml"));
        assert!(is_root_config_or_policy("workspaces/db/go.mod"));
        // Lockfiles too.
        assert!(is_root_config_or_policy("crates/foo/Cargo.lock"));
        assert!(is_root_config_or_policy("packages/app/package-lock.json"));
    }

    #[test]
    fn source_files_not_classified() {
        assert!(!is_root_config_or_policy("src/lib.rs"));
        assert!(!is_root_config_or_policy("src/main.rs"));
        assert!(!is_root_config_or_policy("crates/foo/src/lib.rs"));
        assert!(!is_root_config_or_policy("internal/util.go"));
    }

    /// Codex P2 round 5 (2026-05-14): only the literal top-level
    /// `build.rs` is classified as a Cargo build script. From the path
    /// alone we cannot tell which nested directories are Cargo package
    /// roots, so workspace-member-root build scripts
    /// (`crates/foo/build.rs`) are a deliberate false negative —
    /// preferable to hiding real dead code at paths like
    /// `tools/build.rs` or `src/build.rs`.
    #[test]
    fn only_root_build_rs_classified_as_build_script() {
        // Literal top-level.
        assert!(is_external_runner_target("build.rs"));
        // Nested cases: NOT classified (might be helper / dead code).
        assert!(!is_external_runner_target("crates/foo/build.rs"));
        assert!(!is_external_runner_target("packages/api/build.rs"));
        assert!(!is_external_runner_target("tools/build.rs"));
        assert!(!is_external_runner_target("src/build.rs"));
        assert!(!is_external_runner_target("crates/foo/src/build.rs"));
        assert!(!is_external_runner_target("crates/foo/src/sub/build.rs"));
    }

    #[test]
    fn cargo_runner_targets_classified() {
        // Workspace-member integration tests.
        assert!(is_external_runner_target(
            "crates/repolens-cli/tests/scan_cli_test.rs"
        ));
        assert!(is_external_runner_target(
            "crates/repolens-mcp/tests/integration_test.rs"
        ));
        // Top-level integration tests.
        assert!(is_external_runner_target("tests/contract_test.rs"));
        // Benches and examples.
        assert!(is_external_runner_target(
            "crates/foo/benches/throughput.rs"
        ));
        assert!(is_external_runner_target("benches/throughput.rs"));
        assert!(is_external_runner_target("crates/foo/examples/demo.rs"));
        assert!(is_external_runner_target("examples/demo.rs"));
        // Build scripts — only the literal top-level is classified.
        // Workspace-member-root build scripts are a deliberate false
        // negative; see `only_root_build_rs_classified_as_build_script`.
        assert!(is_external_runner_target("build.rs"));
    }

    #[test]
    fn insta_snapshot_files_classified_as_runner_target() {
        assert!(is_external_runner_target(
            "crates/foo/tests/snapshots/render__output.snap"
        ));
        assert!(is_external_runner_target(
            "crates/foo/tests/snapshots/render__output.snap.new"
        ));
        // Standalone snap at top-level (some repos store them this way).
        assert!(is_external_runner_target("snapshots/render.snap"));
    }

    #[test]
    fn editor_and_agent_config_dirs_classified_as_policy() {
        assert!(is_root_config_or_policy(".vscode/settings.json"));
        assert!(is_root_config_or_policy(
            ".idea/inspectionProfiles/Project.xml"
        ));
        assert!(is_root_config_or_policy(".cursor/rules.md"));
        assert!(is_root_config_or_policy(".claude/settings.local.json"));
    }

    #[test]
    fn library_entry_point_classified() {
        assert!(is_library_entry_point("src/lib.rs"));
        assert!(is_library_entry_point("crates/foo/src/lib.rs"));
        assert!(is_library_entry_point("packages/api/src/lib.rs"));
        // Non-lib.rs source files are not library entry points.
        assert!(!is_library_entry_point("src/main.rs"));
        assert!(!is_library_entry_point("crates/foo/src/mod.rs"));
        assert!(!is_library_entry_point("crates/foo/src/something.rs"));
    }

    #[test]
    fn cargo_test_data_fixtures_classified() {
        assert!(is_external_runner_target(
            "crates/repolens-core/fixtures/bad-bootstrap-missing-version.yaml"
        ));
        assert!(is_external_runner_target(
            "fixtures/scenarios/scenario.json"
        ));
        assert!(is_external_runner_target("crates/foo/fixtures/input.toml"));
    }

    /// Codex P3 round 8 (2026-05-14): a directory-style runner target
    /// can legally be named `src` (e.g. `tests/src/main.rs`). The
    /// nested-under-src guard must reject only paths where `src/`
    /// appears as a segment BEFORE the runner dir, not the target
    /// name AFTER the runner dir.
    #[test]
    fn directory_style_target_named_src_classified() {
        // Target named "src" — IS classified (the `src` is AFTER `tests/`).
        assert!(is_external_runner_target("tests/src/main.rs"));
        assert!(is_external_runner_target("crates/foo/tests/src/main.rs"));
        assert!(is_external_runner_target("crates/foo/examples/src/main.rs"));
        // Sanity: `src/` ancestor before the runner dir still rejects.
        assert!(!is_external_runner_target("crates/foo/src/tests/helper.rs"));
        assert!(!is_external_runner_target("src/tests/helper.rs"));
    }

    /// Codex P2 round 7 (2026-05-14): Cargo discovers directory-style
    /// runner targets at `<runner-dir>/<name>/main.rs`. Those ARE Cargo
    /// targets and must be filtered. Helpers in the same subdirectory
    /// (`<name>/helper.rs`) are NOT — only `main.rs`.
    #[test]
    fn directory_style_cargo_runner_targets_classified() {
        // Directory-style: `<dir>/main.rs` IS a target.
        assert!(is_external_runner_target("crates/foo/tests/api/main.rs"));
        assert!(is_external_runner_target("crates/foo/benches/crit/main.rs"));
        assert!(is_external_runner_target(
            "crates/foo/examples/demo/main.rs"
        ));
        // Top-level directory-style.
        assert!(is_external_runner_target("tests/api/main.rs"));
        assert!(is_external_runner_target("benches/crit/main.rs"));
        assert!(is_external_runner_target("examples/demo/main.rs"));

        // Helpers in the same subdirectory are NOT targets — only main.rs.
        assert!(!is_external_runner_target("crates/foo/tests/api/helper.rs"));
        assert!(!is_external_runner_target(
            "crates/foo/examples/demo/utils.rs"
        ));
        // Deeper nesting still rejected.
        assert!(!is_external_runner_target(
            "crates/foo/tests/api/sub/main.rs"
        ));
    }

    /// Codex P2 round 10 (2026-05-14): paths inside a fixtures/testdata
    /// tree that happen to contain a runner-named segment must NOT be
    /// classified as Cargo runner targets. Cargo discovers targets only
    /// under crate-root `tests/`/`benches/`/`examples/`, never under
    /// fixture subtrees.
    #[test]
    fn fixtures_tree_runner_paths_not_classified() {
        // Helper source under a fixtures tree's tests/ subdir — NOT a target.
        assert!(!is_external_runner_target(
            "crates/foo/fixtures/tests/helper.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/fixtures/benches/inner.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/fixture/examples/demo.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/testdata/tests/helper.rs"
        ));
        // Even directory-style paths inside fixtures don't classify.
        assert!(!is_external_runner_target(
            "crates/foo/fixtures/tests/api/main.rs"
        ));
    }

    /// Codex P2 round 9 (2026-05-14): a helper module nested under one
    /// runner tree that contains another runner-named segment (across
    /// runner-dir kinds) must NOT be classified. E.g.
    /// `crates/foo/tests/common/benches/helper.rs` — the find-loop for
    /// `/benches/` would otherwise see `helper.rs` as a direct child.
    /// The ancestor-segment guard rejects any of {src, tests, benches,
    /// examples} appearing before the matched runner dir.
    #[test]
    fn cross_runner_dir_nesting_not_classified() {
        assert!(!is_external_runner_target(
            "crates/foo/tests/common/benches/helper.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/benches/common/tests/helper.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/examples/shared/tests/helper.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/tests/inner/examples/helper.rs"
        ));
    }

    /// Codex P2 round 6 (2026-05-14): a helper module nested under a runner
    /// directory that itself contains another `tests/`/`benches/`/`examples/`
    /// segment must NOT be classified as a runner target. Cargo only treats
    /// files directly under the OUTERMOST runner dir (the sibling of `src/`)
    /// as independent targets — never inner re-occurrences.
    #[test]
    fn doubly_nested_runner_dir_path_not_classified() {
        // Helper module nested under a `tests/` dir that itself contains
        // another `tests/` segment.
        assert!(!is_external_runner_target(
            "crates/foo/tests/common/tests/helper.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/benches/utils/benches/inner.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/examples/shared/examples/helper.rs"
        ));
    }

    /// Codex P2 round 3 (2026-05-14): Cargo treats only `.rs` files DIRECTLY
    /// under `tests/` / `benches/` / `examples/` as independent target files.
    /// Helper modules at deeper paths (e.g. `tests/common/mod.rs`,
    /// `benches/utils/inner.rs`) are ordinary modules imported by another
    /// target; if no target imports them they ARE dead code and must remain
    /// visible to F-ORPHAN-FILES / `find_dead_code`.
    #[test]
    fn helper_modules_under_runner_dirs_not_classified_as_runner_target() {
        // Helper modules under `tests/`/`benches/`/`examples/` are real
        // source — not Cargo target files.
        assert!(!is_external_runner_target("crates/foo/tests/common/mod.rs"));
        assert!(!is_external_runner_target("tests/common/mod.rs"));
        assert!(!is_external_runner_target(
            "crates/foo/benches/utils/inner.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/examples/shared/helper.rs"
        ));
        // Sanity: direct children of these dirs remain runner targets.
        assert!(is_external_runner_target(
            "crates/foo/tests/integration_test.rs"
        ));
        assert!(is_external_runner_target("tests/api_test.rs"));
        assert!(is_external_runner_target("crates/foo/benches/perf.rs"));
        assert!(is_external_runner_target("benches/perf.rs"));
        assert!(is_external_runner_target("crates/foo/examples/demo.rs"));
        assert!(is_external_runner_target("examples/demo.rs"));
    }

    /// Codex P2 (2026-05-14): `*/src/tests/*.rs`, `*/src/benches/*.rs`,
    /// `*/src/examples/*.rs` are ordinary Rust source modules (a `mod
    /// tests {}` / `mod examples {}` submodule nested under `src/`),
    /// NOT Cargo runner targets. Cargo's `tests/`/`benches/`/`examples/`
    /// directories live as siblings of `src/`, not descendants.
    #[test]
    fn nested_under_src_not_classified_as_runner() {
        assert!(!is_external_runner_target("crates/foo/src/tests/helper.rs"));
        assert!(!is_external_runner_target("src/tests/helper.rs"));
        assert!(!is_external_runner_target(
            "crates/foo/src/benches/inner.rs"
        ));
        assert!(!is_external_runner_target(
            "crates/foo/src/examples/inline_demo.rs"
        ));
        // Sanity: the genuine crate-root pattern still matches.
        assert!(is_external_runner_target(
            "crates/foo/tests/integration_test.rs"
        ));
        assert!(is_external_runner_target(
            "crates/foo/benches/throughput.rs"
        ));
        assert!(is_external_runner_target("crates/foo/examples/demo.rs"));
    }

    #[test]
    fn non_runner_source_not_classified_as_runner() {
        // src/ files under crates are normal source.
        assert!(!is_external_runner_target("crates/repolens-app/src/lib.rs"));
        assert!(!is_external_runner_target("src/main.rs"));
        // A `.rs` file under a directory that just happens to be named
        // "tests" inside src/ is still source. Keep behaviour permissive
        // about that edge case — only match the canonical Cargo layouts.
        assert!(!is_external_runner_target("crates/foo/src/something.rs"));
        // Non-data fixture files (.rs, .py) under fixtures/ are source —
        // probably fixture *helpers*, not test data.
        assert!(!is_external_runner_target("crates/foo/fixtures/helper.rs"));
    }

    #[test]
    fn hub_module_paths_classified() {
        assert!(is_indicator_tagged_module_path("src/lib.rs"));
        assert!(is_indicator_tagged_module_path("crates/foo/src/lib.rs"));
        assert!(is_indicator_tagged_module_path("crates/foo/src/mod.rs"));
        assert!(is_indicator_tagged_module_path("crates/bar/src/prelude.rs"));
        assert!(is_indicator_tagged_module_path("crates/foo-core/src/x.rs"));
        assert!(is_indicator_tagged_module_path(
            "crates/repolens-util/src/x.rs"
        ));
    }

    #[test]
    fn leaf_module_paths_not_classified() {
        assert!(!is_indicator_tagged_module_path("src/leaf.rs"));
        assert!(!is_indicator_tagged_module_path("src/foo.rs"));
        assert!(!is_indicator_tagged_module_path("crates/foo/src/x.rs"));
    }
}
