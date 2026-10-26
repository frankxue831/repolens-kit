//! End-to-end coverage for `repolens findings`. Scans a tiny real repo so
//! `SummaryService::summarize` produces actual `evidenceFindings`, then asserts
//! the command resolves the active pack, renders the calibration-first markdown
//! shape, and emits a known finding id in JSON. The renderer's per-variant
//! shape is unit-tested in `src/findings.rs`; this proves the live
//! scan -> summarize -> render path (not just the empty case).

use std::fs;
use std::path::Path;
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

/// Write a minimal two-file Rust crate. A single `src/` module with all LOC
/// and no `Cargo.lock` deterministically yields the `F-DOMINANT-MODULE` and
/// `F-CARGO-LOCK-STATE` findings.
fn write_tiny_repo(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"hi\"); }\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() -> i32 { 1 }\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
}

fn scan(root: &Path) {
    let out = Command::new(binary())
        .args(["scan", "--root", root.to_str().unwrap()])
        .output()
        .expect("invoke scan");
    assert!(
        out.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn findings_verbose_renders_full_markdown() {
    let dir = tempfile::tempdir().unwrap();
    write_tiny_repo(dir.path());
    scan(dir.path());

    let out = Command::new(binary())
        .args([
            "findings",
            "--verbose",
            "--root",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("invoke findings");
    assert!(
        out.status.success(),
        "findings failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let md = String::from_utf8_lossy(&out.stdout);
    assert!(md.contains("# Evidence Findings — pack-"), "md=\n{md}");
    assert!(md.contains("### "), "expected a finding heading; md=\n{md}");
    assert!(md.contains("grade="), "expected the tag strip; md=\n{md}");
    assert!(md.contains("**Inference:**"), "md=\n{md}");
    assert!(md.contains("**Action:**"), "md=\n{md}");
}

fn findings(dir: &std::path::Path, extra: &[&str]) -> String {
    let mut args = vec!["findings", "--root", dir.to_str().unwrap()];
    args.extend_from_slice(extra);
    let out = Command::new(binary()).args(&args).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn findings_default_is_one_plain_line_per_finding() {
    let dir = tempfile::tempdir().unwrap();
    write_tiny_repo(dir.path());
    scan(dir.path());
    let text = findings(dir.path(), &[]);
    let json: serde_json::Value = serde_json::from_str(&findings(dir.path(), &["--json"])).unwrap();
    let n = json.as_array().unwrap().len();

    assert!(text.starts_with("The last scan (pack-"), "{text}");
    let rows: Vec<&str> = text
        .lines()
        .filter(|l| {
            ["  High  ", "  Med   ", "  Low   ", "  Info  "]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect();
    assert_eq!(rows.len(), n, "one line per finding:\n{text}");
    // No jargon or raw JSON in the default view.
    for jargon in [
        "grade=",
        "confidence=",
        "layer=",
        "Codified",
        "Derived",
        "Heuristic",
        "{\"",
        "**",
    ] {
        assert!(!text.contains(jargon), "found {jargon:?} in:\n{text}");
    }
    assert!(text.contains("findings --verbose"), "{text}");
    // Most severe first.
    let rank = |l: &str| {
        ["  High", "  Med ", "  Low ", "  Info"]
            .iter()
            .position(|p| l.starts_with(p))
            .unwrap()
    };
    let ranks: Vec<usize> = rows.iter().map(|l| rank(l)).collect();
    let mut sorted = ranks.clone();
    sorted.sort();
    assert_eq!(ranks, sorted);
}

#[test]
fn findings_json_flag_matches_format_json() {
    let dir = tempfile::tempdir().unwrap();
    write_tiny_repo(dir.path());
    scan(dir.path());
    assert_eq!(
        findings(dir.path(), &["--json"]),
        findings(dir.path(), &["--format", "json"])
    );
}

#[test]
fn findings_json_emits_known_finding_ids() {
    let dir = tempfile::tempdir().unwrap();
    write_tiny_repo(dir.path());
    scan(dir.path());

    let out = Command::new(binary())
        .args([
            "findings",
            "--root",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("invoke findings json");
    assert!(
        out.status.success(),
        "findings json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("output is valid JSON");
    let arr = v.as_array().expect("findings JSON must be an array");
    assert!(!arr.is_empty(), "expected at least one finding");
    let ids: Vec<&str> = arr.iter().filter_map(|f| f["id"].as_str()).collect();
    assert!(
        ids.contains(&"F-DOMINANT-MODULE"),
        "expected F-DOMINANT-MODULE in {ids:?}"
    );
}

#[test]
fn findings_errors_without_packs() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".repolens")).unwrap();
    let out = Command::new(binary())
        .args(["findings", "--root", dir.path().to_str().unwrap()])
        .output()
        .expect("invoke findings");
    assert!(
        !out.status.success(),
        "expected non-zero exit with no packs"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Run 'repolens scan' first"),
        "stderr=\n{stderr}"
    );
}

#[test]
fn orphan_sample_lists_only_source_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("benchsuite/runs/2016")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "mod used;\nfn main() { used::f(); }\n",
    )
    .unwrap();
    std::fs::write(root.join("src/used.rs"), "pub fn f() {}\n").unwrap();
    for i in 0..3 {
        std::fs::write(root.join(format!("src/unused{i}.rs")), "pub fn g() {}\n").unwrap();
    }
    for f in [
        "COPYING",
        "UNLICENSE",
        "LICENSE-MIT",
        "data.csv",
        "benchsuite/benchsuite",
    ] {
        std::fs::write(root.join(f), "x\n").unwrap();
    }
    std::fs::write(root.join("benchsuite/runs/2016/raw.csv"), "a,b\n").unwrap();
    std::fs::write(root.join("benchsuite/runs/2016/summary"), "s\n").unwrap();
    scan(root);

    let v: serde_json::Value = serde_json::from_str(&findings(root, &["--json"])).unwrap();
    let orphan = v
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "F-ORPHAN-FILES")
        .expect("orphan finding");
    let sample: Vec<&str> = orphan["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["metric"] == "graph.orphanFiles")
        .flat_map(|e| e["observedValue"]["sample"].as_array().unwrap())
        .filter_map(|p| p.as_str())
        .collect();
    assert!(!sample.is_empty(), "{orphan}");
    for p in &sample {
        assert!(
            p.ends_with(".rs"),
            "non-source file in orphan sample: {p} ({sample:?})"
        );
    }
    assert!(sample.contains(&"src/unused0.rs"), "{sample:?}");
}

/// ripgrep-like layout: a crate under `crates/`, a deeply nested, widely
/// imported error module, an orphan with a long path, and a module loop.
/// Every default `findings` line must fit in 80 columns.
#[test]
fn default_findings_lines_fit_80_columns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let a = "a_rather_long_directory_name_for_matching";
    let b = "another_fairly_long_subdirectory_name";
    let krate = root.join("crates/searcher_with_long_name");
    let deep = krate.join("src").join(a).join(b);
    fs::create_dir_all(&deep).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/searcher_with_long_name\"]\n",
    )
    .unwrap();
    fs::write(
        krate.join("Cargo.toml"),
        "[package]\nname = \"searcher_with_long_name\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let mut lib = format!("pub mod {a};\n");
    for i in 0..14 {
        lib.push_str(&format!("pub mod m{i};\n"));
        let body = if i < 2 {
            format!(
                "use crate::{a}::{b}::error_definitions_module::Error;\n\
                 use crate::m{}::f{};\npub fn f{i}() -> Option<Error> {{ None }}\n",
                1 - i,
                1 - i
            )
        } else {
            format!(
                "use crate::{a}::{b}::error_definitions_module::Error;\n\
                 pub fn f{i}() -> Option<Error> {{ None }}\n"
            )
        };
        fs::write(krate.join(format!("src/m{i}.rs")), body).unwrap();
    }
    fs::write(krate.join("src/lib.rs"), lib).unwrap();
    fs::write(
        krate.join("src").join(a).join("mod.rs"),
        format!("pub mod {b};\n"),
    )
    .unwrap();
    fs::write(deep.join("mod.rs"), "pub mod error_definitions_module;\n").unwrap();
    fs::write(
        deep.join("error_definitions_module.rs"),
        "pub struct Error;\n",
    )
    .unwrap();
    fs::write(
        deep.join("an_unused_and_very_long_orphaned_source_file_name.rs"),
        "pub fn unused() {}\n",
    )
    .unwrap();
    scan(root);

    let text = findings(root, &[]);
    for line in text.lines() {
        assert!(
            line.chars().count() <= 80,
            "{} columns: {line:?}\n{text}",
            line.chars().count()
        );
    }
    assert!(text.contains("widely imported file"), "{text}");
    assert!(
        text.contains("Some files may be unused (imports only)"),
        "{text}"
    );
}

/// A large cycle wholly inside one crate is ordinary module structure and is
/// rated Low (the thresholds alone would say High for 8 files).
#[test]
fn cycle_within_one_crate_is_low() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"loopy\"\n").unwrap();
    let mut lib = String::new();
    for i in 0..8 {
        lib.push_str(&format!("pub mod m{i};\n"));
        let next = (i + 1) % 8;
        fs::write(
            root.join(format!("src/m{i}.rs")),
            format!("use crate::m{next}::T{next};\npub struct T{i}(Option<Box<T{next}>>);\n"),
        )
        .unwrap();
    }
    fs::write(root.join("src/lib.rs"), lib).unwrap();
    scan(root);
    let v: serde_json::Value = serde_json::from_str(&findings(root, &["--json"])).unwrap();
    let f = v
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "F-DEPENDENCY-CYCLES")
        .expect("cycle finding");
    assert_eq!(f["severity"], "low", "{f}");
}

/// #46: Next.js route and convention files are loaded by name, so they are
/// counted under "Left out" instead of listed as possibly unused, but only
/// in a package whose `package.json` depends on `next`.
#[test]
fn nextjs_route_files_are_left_out_only_with_a_next_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let w = |p: &str, body: &str| {
        let f = root.join(p);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, body).unwrap();
    };
    w(
        "package.json",
        r#"{"name":"nx","private":true,"dependencies":{"next":"14.2.0"}}"#,
    );
    w("lib/dead.ts", "export const neverUsed = 1;\n");
    let routes = [
        "app/layout.tsx",
        "app/page.tsx",
        "app/not-found.tsx",
        "app/global-not-found.tsx",
        "app/(marketing)/about/page.tsx",
        "app/blog/[slug]/page.tsx",
        "app/docs/[...parts]/page.tsx",
        "app/shop/[[...cat]]/page.tsx",
        "app/dashboard/@analytics/page.tsx",
        "app/api/x/route.ts",
        "pages/_app.tsx",
        "pages/_document.tsx",
        "pages/api/hello.ts",
        "middleware.ts",
        "src/middleware.ts",
        "mdx-components.tsx",
        "src/mdx-components.ts",
        "packages/docs/app/page.tsx",
        "packages/docs/mdx-components.jsx",
        "apps/site/src/app/page.tsx",
    ];
    for r in routes {
        w(r, "export default function P() { return null; }\n");
    }
    w(
        "packages/docs/package.json",
        r#"{"name":"docs","devDependencies":{"next":"15.0.0"}}"#,
    );
    w(
        "apps/site/package.json",
        r#"{"name":"site","dependencies":{"next":"14.2.0"}}"#,
    );
    w("packages/plain/package.json", r#"{"name":"plain"}"#);
    w("packages/plain/app/page.ts", "export const notNext = 1;\n");
    w(
        "packages/plain/middleware.ts",
        "export const alsoNotNext = 1;\n",
    );
    w(
        "packages/plain/mdx-components.tsx",
        "export const plainMdx = 1;\n",
    );
    w(
        "packages/plain/app/global-not-found.tsx",
        "export const plainNotFound = 1;\n",
    );
    scan(root);

    let v: serde_json::Value = serde_json::from_str(&findings(root, &["--json"])).unwrap();
    let orphan = v
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "F-ORPHAN-FILES")
        .expect("orphan finding");
    let ev = |metric: &str| {
        orphan["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["metric"] == metric)
            .map(|e| e["observedValue"].clone())
            .unwrap_or_else(|| panic!("no {metric}: {orphan}"))
    };
    let sample: Vec<String> = ev("graph.orphanFiles")["sample"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p.as_str().map(String::from))
        .collect();
    for r in routes {
        assert!(!sample.iter().any(|p| p == r), "{r} listed: {sample:?}");
    }
    for p in [
        "lib/dead.ts",
        "packages/plain/app/page.ts",
        "packages/plain/middleware.ts",
        "packages/plain/mdx-components.tsx",
        "packages/plain/app/global-not-found.tsx",
    ] {
        assert!(sample.iter().any(|s| s == p), "{p} not listed: {sample:?}");
    }
    let counts = ev("graph.orphanCount");
    assert_eq!(counts["sourceOrphanCount"], 5, "{counts}");
    assert!(
        counts["loadedByNameCount"].as_u64().unwrap() >= routes.len() as u64,
        "{counts}"
    );
}
