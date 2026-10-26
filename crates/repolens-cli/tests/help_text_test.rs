//! `--help` speaks plain words, not internal project jargon.

use std::process::Command;

fn help(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_repolens"))
        .args(args)
        .arg("--help")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn help_has_no_internal_jargon() {
    for args in [
        vec![],
        vec!["init"],
        vec!["eval"],
        vec!["eval", "list"],
        vec!["eval", "start"],
        vec!["findings"],
    ] {
        let h = help(&args);
        for bad in [
            "Round 1",
            "ACE eval",
            "ACE cases",
            "ACE catalogue",
            "calibrated",
            "Phase ",
        ] {
            assert!(!h.contains(bad), "{args:?} --help mentions {bad:?}:\n{h}");
        }
    }
    assert!(help(&[]).contains("Set up the `.repolens/` workspace files"));
    assert!(help(&["findings"]).contains("--verbose"));
}
