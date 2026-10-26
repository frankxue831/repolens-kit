//! End-to-end test for the `## Runtime Freshness` section: a hand-written
//! `.repolens/session.json` names a newer active pack than the human-edited
//! `repo-overview.md` cites, and the rendered brief must surface the active
//! pack and warn about the stale citation.

use std::fs;
use std::process::Command;

fn repolens_bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

#[test]
fn brief_runtime_freshness_surfaces_active_pack_and_warns_on_stale_citation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let repolens = root.join(".repolens");
    fs::create_dir_all(&repolens).unwrap();

    // Human-edited repo overview cites an OLD pack id.
    fs::write(
        repolens.join("repo-overview.md"),
        "# Repo Overview\n\n**Last scan:** pack-20260101-000000 \
         (`.repolens/pack-20260101-000000/`)\n",
    )
    .unwrap();

    // session.json names a NEWER active pack (the authoritative source).
    let active = repolens.join("pack-20260516-015206");
    let session = serde_json::json!({
        "schemaVersion": "1.0",
        "sessionId": "test-session",
        "repoRoot": root.to_str().unwrap(),
        "activePackPath": active.to_str().unwrap(),
        "activeReportPath": null,
        "updatedAt": "2026-05-16T01:52:06.000000Z",
        "lastAccessed": "2026-05-16T01:52:06.000000Z",
        "lastScanMode": "incremental",
        "reprocessedFiles": 2,
        "deletedFiles": 0,
        "carriedForwardFiles": 448,
    });
    fs::write(
        repolens.join("session.json"),
        serde_json::to_string_pretty(&session).unwrap(),
    )
    .unwrap();

    let out = Command::new(repolens_bin())
        .args([
            "brief",
            "--task",
            "freshness",
            "--root",
            root.to_str().unwrap(),
        ])
        .output()
        .expect("failed to invoke repolens brief");
    assert!(
        out.status.success(),
        "brief failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let brief = fs::read_to_string(repolens.join("current-agent-brief.md")).unwrap();
    assert!(brief.contains("## Runtime Freshness"), "brief=\n{brief}");
    assert!(
        brief.contains("pack-20260516-015206"),
        "active pack from session.json must appear in the freshness table; brief=\n{brief}"
    );
    assert!(
        brief.contains("cites stale pack `pack-20260101-000000`"),
        "stale-pack warning missing; brief=\n{brief}"
    );
    assert!(
        brief.contains("active: `pack-20260516-015206`"),
        "stale-pack warning must cite the active pack; brief=\n{brief}"
    );
    assert!(
        brief.contains("incremental"),
        "last scan mode should be surfaced; brief=\n{brief}"
    );
}
