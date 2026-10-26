//! #20: the F-ORPHAN-FILES "nothing looks unused" title is plain and the
//! same word for word in `findings --verbose`, `findings --json` and the
//! brief's Calibrated Findings.

use std::fs;
use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_repolens"))
        .args(args)
        .arg("--root")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn no_unused_files_title_is_the_same_in_every_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub mod a;\n").unwrap();
    fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(root.join("tests/it.rs"), "#[test]\nfn t() {}\n").unwrap();
    fs::write(root.join("README.md"), "# d\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    run(root, &["init"]);
    run(root, &["scan"]);

    let title = "No unused files found (imports only)";
    let md = run(root, &["findings", "--verbose"]);
    assert!(md.contains(&format!("\n### {title}\n")), "{md}");
    assert!(md.contains("None looks unused."), "{md}");

    let json: serde_json::Value =
        serde_json::from_str(&run(root, &["findings", "--json"])).unwrap();
    let f = json
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "F-ORPHAN-FILES")
        .expect("F-ORPHAN-FILES");
    assert_eq!(f["title"], title);
    let deps: Vec<&str> = f["inference"]["dependsOn"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(deps.contains(&"filter:nonOrphanSource"), "{deps:?}");
    assert!(deps.contains(&"sampleComplete=true"), "{deps:?}");

    let brief = run(root, &["brief", "--stdout"]);
    assert!(brief.contains(&format!("- **{title}**")), "{brief}");

    for out in [&md, &brief] {
        assert!(!out.contains("non-source artefacts"), "{out}");
        assert!(!out.contains("Orphan sample"), "{out}");
    }
}
