use repolens_core::rules::RulesEngine;
use std::path::Path;

#[test]
fn included_by_default_when_no_rules() {
    let engine = RulesEngine::new(Path::new("/tmp/repo"), &[]);
    let decision = engine.evaluate(Path::new("/tmp/repo/src/main.rs"), false);
    assert!(decision.included);
    assert_eq!(decision.reason, "included");
}

#[test]
fn gitignore_excludes_matching_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    std::fs::write(root.join("app.log"), "log data").unwrap();

    let engine = RulesEngine::load(root, &[]).unwrap();
    let decision = engine.evaluate(Path::new(&root.join("app.log")), false);
    assert!(!decision.included);
    assert_eq!(decision.reason, "excluded:gitignore");
}

#[test]
fn gitignore_does_not_exclude_unmatched_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();

    let engine = RulesEngine::load(root, &[]).unwrap();
    let decision = engine.evaluate(Path::new(&root.join("app.rs")), false);
    assert!(decision.included);
    assert_eq!(decision.reason, "included");
}

#[test]
fn gitignore_directory_pattern_excludes_children() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "assets/\n").unwrap();
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("assets/image.png"), "fake").unwrap();

    let engine = RulesEngine::load(root, &[]).unwrap();
    let decision = engine.evaluate(Path::new(&root.join("assets/image.png")), false);
    assert!(!decision.included);
    assert_eq!(decision.reason, "excluded:gitignore");
}

#[test]
fn negated_gitignore_pattern_re_includes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "*.env\n!.env.example\n").unwrap();

    let engine = RulesEngine::load(root, &[]).unwrap();

    let excluded = engine.evaluate(Path::new(&root.join("secrets.env")), false);
    assert!(!excluded.included);

    let included = engine.evaluate(Path::new(&root.join(".env.example")), false);
    assert!(included.included);
}

#[test]
fn reposnapshot_rules_override_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "").unwrap();
    let rules_path = root.join(".reposnapshot.rules");
    std::fs::write(&rules_path, "*.md\n").unwrap();

    let engine = RulesEngine::load(root, &[rules_path]).unwrap();
    let decision = engine.evaluate(Path::new(&root.join("README.md")), false);
    assert!(!decision.included);
    assert_eq!(decision.reason, "excluded:rules");
}

#[test]
fn nested_gitignore_in_subdirectory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/.gitignore"), "*.tmp\n").unwrap();

    let engine = RulesEngine::load(root, &[]).unwrap();

    let decision = engine.evaluate(Path::new(&root.join("sub/data.tmp")), false);
    assert!(!decision.included);

    let decision = engine.evaluate(Path::new(&root.join("data.tmp")), false);
    assert!(decision.included);
}

#[test]
fn nested_gitignore_inside_ignored_dir_does_not_re_include() {
    // git-aligned semantics: the root .gitignore excludes build/, and a
    // build/.gitignore CANNOT re-include files under it ("it is not possible to
    // re-include a file if a parent directory of that file is excluded").
    // build_gitignores prunes the ignored subtree, so build/.gitignore is never
    // collected. The supported way to re-include under an ignored dir is
    // `.reposnapshot.rules` (see reposnapshot_rules_override_gitignore).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join("build/.gitignore"), "!keep.txt\n").unwrap();
    std::fs::write(root.join("build/keep.txt"), "important").unwrap();
    std::fs::write(root.join("build/output.bin"), "junk").unwrap();

    let engine = RulesEngine::load(root, &[]).unwrap();

    let output = engine.evaluate(&root.join("build/output.bin"), false);
    assert!(!output.included, "build/output.bin stays excluded");

    let keep = engine.evaluate(&root.join("build/keep.txt"), false);
    assert!(
        !keep.included,
        "build/keep.txt must NOT be re-included by a .gitignore inside the ignored \
         build/ — a parent exclusion cannot be undone by a nested .gitignore"
    );
}

#[test]
fn excluded_small_gitignore_matches_fixture() {
    let mut inputs = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    inputs.pop();
    inputs.pop();
    inputs.push("fixtures");
    inputs.push("inputs");
    inputs.push("excluded-small");

    let engine = RulesEngine::load(&inputs, &[]).unwrap();

    let env_decision = engine.evaluate(&inputs.join("ignored.env"), false);
    assert!(!env_decision.included, "*.env should be excluded");
    assert_eq!(env_decision.reason, "excluded:gitignore");

    let log_decision = engine.evaluate(&inputs.join("ignored.log"), false);
    assert!(!log_decision.included, "*.log should be excluded");

    let bin_decision = engine.evaluate(&inputs.join("assets/blob.bin"), false);
    assert!(!bin_decision.included, "assets/ should be excluded");

    let py_decision = engine.evaluate(&inputs.join("src/main.py"), false);
    assert!(py_decision.included, "src/main.py should be included");
}
