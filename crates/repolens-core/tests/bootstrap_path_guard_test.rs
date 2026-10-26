use repolens_core::bootstrap_paths::BootstrapPathGuard;
use std::fs;

#[test]
fn rejects_parent_escape_patterns() {
    let dir = tempfile::tempdir().unwrap();
    let err = BootstrapPathGuard::compile(dir.path(), &["../Cargo.toml".to_string()])
        .expect_err("pattern should be rejected");
    assert!(err.to_string().contains("repo-relative"));
}

#[test]
fn rejects_absolute_patterns() {
    let dir = tempfile::tempdir().unwrap();
    let err = BootstrapPathGuard::compile(dir.path(), &["/tmp/file".to_string()])
        .expect_err("pattern should be rejected");
    assert!(err.to_string().contains("repo-relative"));
}

#[test]
fn allows_recursive_directory_glob_for_direct_child() {
    let dir = tempfile::tempdir().unwrap();
    let guard = BootstrapPathGuard::compile(dir.path(), &["docs/**".to_string()]).unwrap();

    guard
        .assert_changed_path_allowed("docs/runtime.md")
        .unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape_at_finish() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::os::unix::fs::symlink("/tmp", dir.path().join("docs/outside")).unwrap();

    let guard = BootstrapPathGuard::compile(dir.path(), &["docs/**".to_string()]).unwrap();
    let err = guard
        .assert_changed_path_allowed("docs/outside/payload.txt")
        .expect_err("symlink escape should fail");
    assert!(err.to_string().contains("symlink"));
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape_when_child_not_yet_created() {
    // Regression: the walked path includes an intermediate symlink pointing
    // outside the repo root, and the trailing component is a file that has
    // not been created yet (no stat entry). The guard must still reject.
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("docs")).unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.path().join("docs/link")).unwrap();

    let guard = BootstrapPathGuard::compile(repo.path(), &["docs/**".to_string()]).unwrap();
    let err = guard
        .assert_changed_path_allowed("docs/link/unborn.txt")
        .expect_err("intermediate symlink escape should fail");
    assert!(err.to_string().contains("symlink"));
}

#[test]
fn shared_path_matching_handles_recursive_and_root_only_patterns() {
    let dir = tempfile::tempdir().unwrap();

    let recursive_guard =
        BootstrapPathGuard::compile(dir.path(), &["docs/**".to_string()]).unwrap();
    assert!(repolens_core::bootstrap_paths::path_matches_any_pattern(
        &["docs/**".to_string()],
        "docs/runtime.md"
    )
    .unwrap());
    recursive_guard
        .assert_changed_path_allowed("docs/runtime.md")
        .unwrap();

    let root_only_guard =
        BootstrapPathGuard::compile(dir.path(), &["Cargo.toml".to_string()]).unwrap();
    assert!(repolens_core::bootstrap_paths::path_matches_any_pattern(
        &["Cargo.toml".to_string()],
        "Cargo.toml"
    )
    .unwrap());
    assert!(!repolens_core::bootstrap_paths::path_matches_any_pattern(
        &["Cargo.toml".to_string()],
        "vendor/Cargo.toml"
    )
    .unwrap());
    root_only_guard
        .assert_changed_path_allowed("Cargo.toml")
        .unwrap();
    let err = root_only_guard
        .assert_changed_path_allowed("vendor/Cargo.toml")
        .expect_err("nested file should be rejected");
    assert!(err
        .to_string()
        .contains("outside the approved bootstrap boundary"));
}

#[test]
fn rejection_message_names_edited_file_and_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let guard = BootstrapPathGuard::compile(
        dir.path(),
        &["docs/**".to_string(), "README.md".to_string()],
    )
    .unwrap();
    let err = guard
        .assert_changed_path_allowed(".repolens/known-constraints.md")
        .expect_err("outside boundary");
    assert_eq!(
        err.to_string(),
        "These files changed outside the approved bootstrap boundary:\n  .repolens/known-constraints.md\nAllowed: docs/**, README.md"
    );
}

// #58 item 5: the boundary message layout and its 80-column rules.

use repolens_core::bootstrap_paths::{boundary_report, display_cols, BOUNDARY_WIDTH};

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn boundary_report_lists_each_path_then_allowed() {
    let shown = boundary_report(
        &strings(&["out.txt", "src/lib.rs"]),
        &strings(&[".repolens/**", "docs/**"]),
    );
    assert_eq!(
        shown,
        "These files changed outside the approved bootstrap boundary:\n  \
         out.txt\n  src/lib.rs\nAllowed: .repolens/**, docs/**"
    );
}

#[test]
fn boundary_report_caps_paths_at_ten() {
    let paths: Vec<String> = (0..13).map(|i| format!("out{i:02}.txt")).collect();
    let shown = boundary_report(&paths, &strings(&["docs/**"]));
    let lines: Vec<&str> = shown.lines().collect();
    assert_eq!(lines.len(), 1 + 10 + 1 + 1, "{shown}");
    for (i, line) in lines[1..11].iter().enumerate() {
        assert_eq!(*line, format!("  out{i:02}.txt"));
    }
    assert_eq!(lines[11], "  …and 3 more");
    assert_eq!(lines[12], "Allowed: docs/**");
    assert!(!shown.contains("out10.txt"), "{shown}");

    // Exactly ten: no "more" line.
    let shown = boundary_report(&paths[..10], &strings(&["docs/**"]));
    assert!(!shown.contains("more"), "{shown}");
}

/// Wrapped `Allowed:` lines: every line fits, breaks fall after a comma,
/// continuation lines are indented 2, and no glob is lost.
fn assert_allowed_wraps(globs: &[String]) {
    let shown = boundary_report(&strings(&["out.txt"]), globs);
    let allowed: Vec<&str> = shown
        .lines()
        .skip_while(|l| !l.starts_with("Allowed: "))
        .collect();
    assert!(allowed.len() > 1, "expected a wrap: {shown}");
    for (i, line) in allowed.iter().enumerate() {
        assert!(
            display_cols(line) <= BOUNDARY_WIDTH,
            "{line:?} is {} cols",
            display_cols(line)
        );
        if i > 0 {
            assert!(
                line.starts_with("  ") && !line.starts_with("   "),
                "{line:?}"
            );
        }
        if i + 1 < allowed.len() {
            assert!(line.ends_with(','), "{line:?}");
        }
    }
    let rejoined = allowed
        .iter()
        .map(|l| l.trim_start_matches("Allowed:").trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(rejoined, globs.join(", "));
}

#[test]
fn allowed_line_wraps_at_a_comma_within_80_columns() {
    let globs: Vec<String> = (0..12)
        .map(|i| format!("crates/module-{i:02}/src/**"))
        .collect();
    assert_allowed_wraps(&globs);
}

#[test]
fn allowed_line_counts_cjk_and_ellipsis_as_two_columns() {
    assert_eq!(display_cols("a"), 1);
    assert_eq!(display_cols("文"), 2);
    assert_eq!(display_cols("…"), 2);
    assert_eq!(display_cols("  …and 3 more"), 14);

    // Short CJK globs that fit by columns stay on one line.
    let globs: Vec<String> = (0..6).map(|i| format!("文档{i}/**")).collect();
    let flat = format!("Allowed: {}", globs.join(", "));
    assert!(display_cols(&flat) <= BOUNDARY_WIDTH, "{flat}");
    let shown = boundary_report(&strings(&["out.txt"]), &globs);
    assert_eq!(shown.lines().last(), Some(flat.as_str()), "{shown}");

    // 67 chars but 97 columns: must wrap.
    let globs: Vec<String> = (0..5).map(|i| format!("设计文档目录{i}/**")).collect();
    let flat = format!("Allowed: {}", globs.join(", "));
    assert!(
        flat.chars().count() <= BOUNDARY_WIDTH,
        "fits by chars: {flat}"
    );
    assert!(
        display_cols(&flat) > BOUNDARY_WIDTH,
        "not by columns: {flat}"
    );
    assert_allowed_wraps(&globs);

    // "…" counts 2: a line that fits only if "…" were 1 column wraps.
    let mut globs: Vec<String> = vec!["a".repeat(64), "b…/**".to_string()];
    let flat = format!("Allowed: {}", globs.join(", "));
    assert_eq!(flat.chars().count(), BOUNDARY_WIDTH);
    assert_eq!(display_cols(&flat), BOUNDARY_WIDTH + 1);
    assert_allowed_wraps(&globs);

    // ...and exactly 80 columns stays on one line.
    globs[1] = "b/**".to_string();
    globs[0].push('a');
    let shown = boundary_report(&strings(&["out.txt"]), &globs);
    let last = shown.lines().last().unwrap();
    assert_eq!(display_cols(last), BOUNDARY_WIDTH, "{shown}");
    assert!(last.starts_with("Allowed: "), "{shown}");
}

#[test]
fn only_a_long_path_line_may_exceed_80_columns() {
    let long = format!("very/deep/{}/file.rs", "nested-folder/".repeat(8));
    let cjk = format!("文档/{}/说明.md", "目录".repeat(20));
    let globs: Vec<String> = (0..12)
        .map(|i| format!("crates/module-{i:02}/src/**"))
        .collect();
    let shown = boundary_report(&[long.clone(), "out.txt".into(), cjk.clone()], &globs);
    for line in shown.lines() {
        if line == format!("  {long}") || line == format!("  {cjk}") {
            assert!(display_cols(line) > BOUNDARY_WIDTH, "{line}");
        } else {
            assert!(display_cols(line) <= BOUNDARY_WIDTH, "{line:?}");
        }
    }
    assert!(shown.lines().any(|l| l == format!("  {long}")), "{shown}");
}
