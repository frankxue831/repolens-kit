//! Regression: Rust import edges stay inside a crate unless Cargo.toml
//! declares the dependency (ripgrep's false 44-file cross-crate cycle).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use repolens_app::graph::GraphBuilder;
use repolens_app::graph_analysis::GraphAnalyzer;
use repolens_app::scan::ScanService;
use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::graph::FileGraph;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// Workspace modelled on ripgrep:
/// - root package `app` with `[[bin]] path = "crates/core/main.rs"`
///   (no src/), depending on `searcher` and `printer`;
/// - `grep-searcher` in crates/searcher, no workspace deps;
/// - `grep-printer` in crates/printer, depends on `grep-searcher`.
///   Both library crates have a module named `util` and each uses
///   `crate::util` / `super::` internally. A doc comment shows
///   `use grep_printer::Standard;` in searcher.
fn fixture(root: &Path) {
    write(
        root,
        "Cargo.toml",
        r#"[package]
name = "app"

[[bin]]
name = "app"
path = "crates/core/main.rs"

[workspace]
members = ["crates/searcher", "crates/printer"]

[dependencies]
grep-searcher = { path = "crates/searcher" }
printer = { package = "grep-printer", path = "crates/printer" }
"#,
    );
    write(root, "crates/core/main.rs", "mod flags;\nuse crate::flags::Args;\nuse grep_searcher::Searcher;\nuse printer::Standard;\nfn main() { flags::parse(); }\n");
    write(
        root,
        "crates/core/flags/mod.rs",
        "pub struct Args;\npub fn parse() { let _ = crate::flags::Args; }\n",
    );

    write(
        root,
        "crates/searcher/Cargo.toml",
        "[package]\nname = \"grep-searcher\"\n\n[dependencies]\nmemchr = \"2\"\n",
    );
    write(
        root,
        "crates/searcher/src/lib.rs",
        "/*!\nExample:\n\n```\nuse grep_printer::Standard;\n```\n*/\n\n//! use grep_printer::sink;\n\nmod util;\nmod sink;\npub use crate::sink::Sink;\npub struct Searcher;\n",
    );
    write(
        root,
        "crates/searcher/src/util.rs",
        "pub fn searcher_only_helper() {}\n",
    );
    write(root, "crates/searcher/src/sink.rs", "use crate::util::searcher_only_helper;\npub struct Sink;\npub fn run() { searcher_only_helper(); printer_only_helper(); }\n");

    write(root, "crates/printer/Cargo.toml", "[package]\nname = \"grep-printer\"\n\n[dependencies]\ngrep-searcher = { path = \"../searcher\" }\n");
    write(
        root,
        "crates/printer/src/lib.rs",
        "mod util;\nmod standard;\npub use crate::standard::Standard;\n",
    );
    write(
        root,
        "crates/printer/src/util.rs",
        "use grep_searcher::Sink;\npub fn printer_only_helper() {}\n",
    );
    write(
        root,
        "crates/printer/src/standard.rs",
        "use super::util::printer_only_helper;\npub struct Standard;\n",
    );
}

fn read_entries(pack_dir: &Path) -> Vec<FileEntry> {
    let mut chunks: Vec<_> = fs::read_dir(pack_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("files-"))
        .map(|e| e.path())
        .collect();
    chunks.sort();
    chunks
        .iter()
        .flat_map(|c| {
            serde_json::from_str::<Vec<FileEntry>>(&fs::read_to_string(c).unwrap()).unwrap()
        })
        .collect()
}

fn build(root: &Path) -> FileGraph {
    let scan = ScanService::new().run(root, false, 100).unwrap();
    let entries = read_entries(&scan.pack_dir);
    GraphBuilder::new()
        .build(&entries, &scan.pack_dir, Some(root))
        .graph
}

fn crate_of(path: &str) -> &'static str {
    if path.starts_with("crates/searcher/") {
        "searcher"
    } else if path.starts_with("crates/printer/") {
        "printer"
    } else {
        "app"
    }
}

fn edges(g: &FileGraph) -> HashSet<(String, String)> {
    g.edges
        .iter()
        .filter(|e| e.edge_type == "imports")
        .map(|e| (e.source.clone(), e.target.clone()))
        .collect()
}

#[test]
fn crate_paths_resolve_within_the_owning_crate() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let g = build(dir.path());
    let e = edges(&g);
    let has = |a: &str, b: &str| e.contains(&(a.to_string(), b.to_string()));

    // crate:: in a [[bin]] crate whose root is crates/core (no src/).
    assert!(
        has("crates/core/main.rs", "crates/core/flags/mod.rs"),
        "{e:?}"
    );
    // crate:: and super:: stay in their own crate even though both crates
    // have a `util` module.
    assert!(
        has("crates/searcher/src/sink.rs", "crates/searcher/src/util.rs"),
        "{e:?}"
    );
    assert!(
        has(
            "crates/printer/src/standard.rs",
            "crates/printer/src/util.rs"
        ),
        "{e:?}"
    );
    assert!(!has(
        "crates/searcher/src/sink.rs",
        "crates/printer/src/util.rs"
    ));
    assert!(!has(
        "crates/printer/src/standard.rs",
        "crates/searcher/src/util.rs"
    ));
    // Declared dependencies (incl. a renamed one) still produce edges.
    assert!(
        has("crates/printer/src/util.rs", "crates/searcher/src/sink.rs"),
        "{e:?}"
    );
    assert!(
        e.iter()
            .any(|(s, t)| s == "crates/core/main.rs" && t.starts_with("crates/printer/")),
        "renamed dep `printer` -> grep-printer: {e:?}"
    );
}

#[test]
fn no_edge_crosses_an_undeclared_crate_boundary() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let g = build(dir.path());
    let deps: HashMap<&str, BTreeSet<&str>> = [
        ("app", ["searcher", "printer"].into()),
        ("searcher", BTreeSet::new()),
        ("printer", ["searcher"].into()),
    ]
    .into();
    for edge in &g.edges {
        let (s, t) = (crate_of(&edge.source), crate_of(&edge.target));
        if edge.source.ends_with(".rs") && edge.target.ends_with(".rs") && s != t {
            assert!(
                deps[s].contains(t),
                "undeclared cross-crate edge {} -> {} ({})",
                edge.source,
                edge.target,
                edge.edge_type
            );
        }
    }
    // searcher's doc comment mentions grep_printer; that must not count.
    assert!(!g.edges.iter().any(
        |e| e.source == "crates/searcher/src/lib.rs" && e.target.starts_with("crates/printer/")
    ));
}

#[test]
fn no_cycle_spans_crates() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let g = build(dir.path());
    let report = GraphAnalyzer::find_cycles(&g);
    for cycle in &report.cycles {
        let crates: BTreeSet<&str> = cycle.files.iter().map(|p| crate_of(p)).collect();
        assert_eq!(crates.len(), 1, "cycle spans crates: {cycle:?}");
    }
}

/// Edges that exist only in test code (`#[cfg(test)]` items, files declared
/// by `#[cfg(test)] mod x;`, and `tests/` dirs) are not part of the graph,
/// so they cannot close a cycle.
#[test]
fn test_only_code_adds_no_edges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "Cargo.toml", "[package]\nname = \"demo\"\n");
    write(
        root,
        "src/lib.rs",
        "mod a;\nmod b;\n#[cfg(test)]\nmod testutil;\n",
    );
    write(
        root,
        "src/a.rs",
        "use crate::b::B;\n#[cfg(test)]\nuse crate::testutil::fixture;\npub struct A(B);\n",
    );
    write(
        root,
        "src/b.rs",
        "pub struct B;\n\n#[cfg(test)]\nmod tests {\n    use crate::a::A;\n    #[test]\n    fn t() {}\n}\n",
    );
    write(
        root,
        "src/testutil.rs",
        "use crate::a::A;\npub fn fixture() -> Option<A> { None }\n",
    );
    write(
        root,
        "tests/it.rs",
        "use demo::a::A;\n#[test]\nfn it() {}\n",
    );
    let g = build(root);
    let e = edges(&g);
    let has = |a: &str, b: &str| e.contains(&(a.to_string(), b.to_string()));
    assert!(has("src/a.rs", "src/b.rs"), "{e:?}");
    assert!(!has("src/b.rs", "src/a.rs"), "cfg(test) mod body: {e:?}");
    assert!(!has("src/a.rs", "src/testutil.rs"), "cfg(test) use: {e:?}");
    assert!(!has("src/testutil.rs", "src/a.rs"), "cfg(test) file: {e:?}");
    assert!(
        !g.edges
            .iter()
            .any(|x| x.source == "tests/it.rs" && x.edge_type != "declares"),
        "tests/ dir: {:?}",
        g.edges
    );
    assert!(GraphAnalyzer::find_cycles(&g).cycles.is_empty());
}
