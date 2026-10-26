//! Snapshot tests for the triage envelope (`triageHeader`, `notChecked`,
//! `calibration`) added to the `data` payload of MCP tools that emit
//! mixed-grade content. Locks down the consumer-visible contract.

use repolens_core::model::evidence::RepoContextTag;
use repolens_mcp::triage;
use serde_json::json;

/// Input data includes `signals` and `dependencyRisk` so PR A's
/// presence-filter retains them in the calibration block. The snapshot
/// pins the consumer-visible envelope shape for a pure Rust library
/// context (no crypto codes, MSRV included).
#[test]
fn triage_envelope_for_pure_rust_library_has_no_crypto_codes() {
    let data = json!({
        "structure": { "moduleCount": 1 },
        "signals": [],
        "dependencyRisk": {}
    });
    let augmented = triage::augment_data(
        data,
        "pack-test",
        "2026-05-11T00:00:00Z",
        &[RepoContextTag::RustLibrary],
        &["signals", "dependencyRisk"],
    );

    let json = serde_json::to_string_pretty(&augmented).unwrap();
    insta::assert_snapshot!("triage_envelope_rust_library", json);
}

#[test]
fn triage_envelope_for_crypto_indicator_repo_includes_crypto_codes() {
    let data = json!({
        "structure": { "moduleCount": 1 },
        "signals": [],
        "dependencyRisk": {}
    });
    let augmented = triage::augment_data(
        data,
        "pack-test",
        "2026-05-11T00:00:00Z",
        &[
            RepoContextTag::RustLibrary,
            RepoContextTag::CryptoIndicators,
        ],
        &["signals", "dependencyRisk"],
    );

    let json = serde_json::to_string_pretty(&augmented).unwrap();
    insta::assert_snapshot!("triage_envelope_crypto_indicators", json);
}

#[test]
fn triage_envelope_for_non_rust_repo_omits_msrv() {
    let data = json!({ "structure": { "moduleCount": 1 } });
    let augmented = triage::augment_data(
        data,
        "pack-test",
        "2026-05-11T00:00:00Z",
        &[RepoContextTag::JavaService],
        &["signals", "dependencyRisk"],
    );

    let nc = augmented["notChecked"]
        .as_array()
        .expect("notChecked array");
    let codes: Vec<&str> = nc.iter().filter_map(|v| v["code"].as_str()).collect();
    assert!(!codes.contains(&"MSRV"));
}

/// PR A (2026-05-12 dogfood D2): the calibration block must only list
/// legacy fields actually present in the payload. Locks in the filter
/// invariant — a present field stays, an absent field is dropped.
#[test]
fn triage_envelope_calibration_block_only_lists_present_legacy_fields() {
    // `metrics` and `health.dependencyRisk` are present; `fitnessScore` and
    // `health.graphHealth.cycleCount` are not. Order of present fields is
    // preserved. (`recommendations` was retired in 2026-05-13 Phase 4B
    // Step 3, `signals` in Step 2.)
    let data = json!({
        "metrics": [],
        "health": { "dependencyRisk": { "riskLevel": "low" } }
    });
    let augmented = triage::augment_data(
        data,
        "pack-test",
        "2026-05-13T00:00:00Z",
        &[],
        &[
            "metrics",
            "fitnessScore",
            "health.dependencyRisk",
            "health.graphHealth.cycleCount",
        ],
    );
    let cal = &augmented["calibration"];
    assert_eq!(cal["preferred"], "evidenceFindings");
    let legacy: Vec<&str> = cal["legacyFields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(legacy, vec!["metrics", "health.dependencyRisk"]);
}
