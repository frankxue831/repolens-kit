//! #24: Go has no file-level imports. Every file in a package folder is
//! compiled together, so a file is not unused just because no file imports
//! it by name. Shaped like spf13/cobra: the module path itself is the root
//! package, imported by a subpackage.

use std::collections::HashSet;
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

fn cobra_like(root: &Path) {
    write(root, "go.mod", "module github.com/spf13/cobra\n\ngo 1.21\n");
    write(
        root,
        "command.go",
        "package cobra\n\ntype Command struct{}\n\nfunc (c *Command) Execute() error { return nil }\n",
    );
    write(
        root,
        "active_help.go",
        "package cobra\n\nfunc AppendActiveHelp(s []string, h string) []string { return s }\n",
    );
    write(
        root,
        "doc/man_docs.go",
        "package doc\n\nimport \"github.com/spf13/cobra\"\n\nfunc GenMan(c *cobra.Command) {}\n",
    );
    write(
        root,
        "doc/util.go",
        "package doc\n\nfunc hasSeeAlso() bool { return false }\n",
    );
    // A `package main` folder: main.go is the entry point, flags.go is in
    // the same package.
    write(
        root,
        "cmd/app/main.go",
        "package main\n\nimport \"github.com/spf13/cobra/doc\"\n\nfunc main() { doc.GenMan(nil) }\n",
    );
    write(
        root,
        "cmd/app/flags.go",
        "package main\n\nvar verbose bool\n",
    );
    // A package nothing imports stays reported.
    write(
        root,
        "internal/legacy/legacy.go",
        "package legacy\n\nfunc Old() {}\n",
    );
    write(
        root,
        "internal/legacy/more.go",
        "package legacy\n\nfunc Older() {}\n",
    );
}

#[test]
fn go_files_in_an_imported_package_are_not_orphans() {
    let dir = tempfile::tempdir().unwrap();
    cobra_like(dir.path());
    let g = graph(dir.path());

    // The module path names the root package.
    assert!(
        g.edges.iter().any(|e| e.source == "doc/man_docs.go"
            && e.target.ends_with(".go")
            && !e.target.contains('/')
            && e.edge_type == "imports"),
        "doc/man_docs.go should import a root-package file; edges: {:?}",
        g.edges
    );

    let entry_points: HashSet<String> = ["cmd/app/main.go".to_string()].into();
    let orphans: HashSet<String> = GraphAnalyzer::find_orphans(&g, &entry_points)
        .orphan_files
        .into_iter()
        .collect();
    for used in [
        "command.go",
        "active_help.go",
        "doc/man_docs.go",
        "doc/util.go",
        "cmd/app/flags.go",
    ] {
        assert!(
            !orphans.contains(used),
            "{used} is in a package that is imported or is main; orphans: {orphans:?}"
        );
    }
    assert!(orphans.contains("internal/legacy/legacy.go"), "{orphans:?}");
    assert!(orphans.contains("internal/legacy/more.go"), "{orphans:?}");
}

#[test]
fn go_package_import_resolves_to_a_non_test_file() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "go.mod", "module example.com/m\n\ngo 1.21\n");
    write(
        dir.path(),
        "pkg/a_test.go",
        "package pkg\n\nfunc TestA() {}\n",
    );
    write(dir.path(), "pkg/b.go", "package pkg\n\nfunc B() {}\n");
    write(
        dir.path(),
        "main.go",
        "package main\n\nimport \"example.com/m/pkg\"\n\nfunc main() { pkg.B() }\n",
    );
    let g = graph(dir.path());
    let targets: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.source == "main.go" && e.edge_type == "imports")
        .map(|e| e.target.as_str())
        .collect();
    assert_eq!(targets, vec!["pkg/b.go"], "edges: {:?}", g.edges);
}

#[test]
fn go_package_import_prefers_the_file_named_after_the_package() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "go.mod", "module example.com/m\n\ngo 1.21\n");
    write(dir.path(), "pkg/a.go", "package pkg\n\nfunc A() {}\n");
    write(dir.path(), "pkg/pkg.go", "package pkg\n\nfunc B() {}\n");
    write(
        dir.path(),
        "main.go",
        "package main\n\nimport \"example.com/m/pkg\"\n\nfunc main() { pkg.B() }\n",
    );
    let g = graph(dir.path());
    let targets: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.source == "main.go" && e.edge_type == "imports")
        .map(|e| e.target.as_str())
        .collect();
    assert_eq!(targets, vec!["pkg/pkg.go"], "edges: {:?}", g.edges);
}
