//! #29: the same Java class can exist in two source trees, e.g. a module
//! and a copy of it under fixtures. Each copy's references must land on the
//! copy in its own tree, and a name that is still ambiguous stays
//! unresolved.

use std::fs;
use std::path::Path;

use repolens_app::graph::GraphBuilder;
use repolens_app::scan::ScanService;
use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::graph::FileGraph;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn graph(root: &Path) -> FileGraph {
    let scan = ScanService::new().run(root, false, 100).unwrap();
    let mut chunks: Vec<_> = fs::read_dir(&scan.pack_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("files-"))
        .map(|e| e.path())
        .collect();
    chunks.sort();
    let entries: Vec<FileEntry> = chunks
        .iter()
        .flat_map(|c| {
            serde_json::from_str::<Vec<FileEntry>>(&fs::read_to_string(c).unwrap()).unwrap()
        })
        .collect();
    GraphBuilder::new()
        .build(&entries, &scan.pack_dir, Some(root))
        .graph
}

const HELPER: &str = "package com.example;\n\npublic class Helper {\n    public String greet(String n) { return n; }\n}\n";
const APP: &str = "package com.example;\n\nimport com.example.Helper;\n\npublic class App {\n    public String run(String n) {\n        Helper h = new Helper();\n        return h.greet(n);\n    }\n}\n";
const SAME_PKG_APP: &str = "package com.example;\n\npublic class Other {\n    public String run(String n) {\n        Helper h = new Helper();\n        return h.greet(n);\n    }\n}\n";

fn targets<'a>(g: &'a FileGraph, source: &str) -> Vec<&'a str> {
    let mut t: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.source == source)
        .map(|e| e.target.as_str())
        .collect();
    t.sort();
    t.dedup();
    t
}

#[test]
fn each_copy_of_a_class_resolves_within_its_own_tree() {
    let dir = tempfile::tempdir().unwrap();
    let a = "fixtures/inputs/java-small/src/main/java/com/example";
    let b = "fixtures/scenarios/java-small/pack/files/src/main/java/com/example";
    for root in [a, b] {
        write(dir.path(), &format!("{root}/Helper.java"), HELPER);
        write(dir.path(), &format!("{root}/App.java"), APP);
        write(dir.path(), &format!("{root}/Other.java"), SAME_PKG_APP);
    }
    let g = graph(dir.path());
    for root in [a, b] {
        let helper = format!("{root}/Helper.java");
        for src in ["App.java", "Other.java"] {
            let src = format!("{root}/{src}");
            assert_eq!(
                targets(&g, &src),
                vec![helper.as_str()],
                "{src} should only reach its own Helper; edges: {:?}",
                g.edges
            );
        }
    }
}

#[test]
fn main_and_test_code_of_one_module_share_it() {
    let dir = tempfile::tempdir().unwrap();
    for m in ["mod-a", "mod-b"] {
        write(
            dir.path(),
            &format!("{m}/src/main/java/com/example/Helper.java"),
            HELPER,
        );
        write(
            dir.path(),
            &format!("{m}/src/test/java/com/example/HelperTest.java"),
            "package com.example;\n\nimport com.example.Helper;\n\npublic class HelperTest {\n    void t() { new Helper().greet(\"x\"); }\n}\n",
        );
    }
    let g = graph(dir.path());
    for m in ["mod-a", "mod-b"] {
        assert_eq!(
            targets(
                &g,
                &format!("{m}/src/test/java/com/example/HelperTest.java")
            ),
            vec![format!("{m}/src/main/java/com/example/Helper.java").as_str()],
            "edges: {:?}",
            g.edges
        );
    }
}

#[test]
fn a_class_in_two_other_trees_stays_unresolved() {
    let dir = tempfile::tempdir().unwrap();
    for m in ["lib-a", "lib-b"] {
        write(
            dir.path(),
            &format!("{m}/src/main/java/com/example/Helper.java"),
            HELPER,
        );
    }
    write(dir.path(), "app/src/main/java/com/example/App.java", APP);
    let g = graph(dir.path());
    assert!(
        targets(&g, "app/src/main/java/com/example/App.java").is_empty(),
        "an ambiguous Helper must not be guessed; edges: {:?}",
        g.edges
    );
}

#[test]
fn a_class_declared_once_still_resolves_across_modules() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "lib/src/main/java/com/example/Helper.java",
        HELPER,
    );
    write(dir.path(), "app/src/main/java/com/example/App.java", APP);
    let g = graph(dir.path());
    assert_eq!(
        targets(&g, "app/src/main/java/com/example/App.java"),
        vec!["lib/src/main/java/com/example/Helper.java"],
        "edges: {:?}",
        g.edges
    );
}
