use std::process::Command;

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

fn run_help(args: &[&str]) -> String {
    let out = Command::new(repolens_bin())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke repolens {args:?}: {e}"));
    assert!(
        out.status.success(),
        "help command should exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn top_level_help_describes_state_as_persistence_inventory() {
    let stdout = run_help(&["--help"]);

    assert!(
        stdout.contains("state"),
        "top-level help must list state; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("scan packs, typed memory, and session handoff"),
        "state description should name the three persistence surfaces; stdout=\n{stdout}"
    );
}

#[test]
fn state_help_names_surfaces_and_read_only_contract() {
    let stdout = run_help(&["state", "--help"]);

    for expected in [
        "read-only",
        "scan packs",
        "typed memory graph",
        "session handoff markdown",
        "does not scan, call MCP, or modify files",
    ] {
        assert!(
            stdout.contains(expected),
            "state --help missing {expected:?}; stdout=\n{stdout}"
        );
    }
}

#[test]
fn remember_help_clarifies_markdown_only_boundary() {
    let stdout = run_help(&["remember", "--help"]);

    for expected in [
        "workspace markdown only",
        ".repolens/session-memory.md",
        ".repolens/memory/memory.json",
        "not modified",
        "repolens state",
    ] {
        assert!(
            stdout.contains(expected),
            "remember --help missing {expected:?}; stdout=\n{stdout}"
        );
    }
}
