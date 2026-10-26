//! Triage-envelope helpers for MCP tools that emit mixed-grade content.
//!
//! Adds three top-level keys to a tool's `data` payload:
//! - `triageHeader`: identifies the report as structural triage and pins
//!   the scope (pack id + commit + generated-at timestamp).
//! - `notChecked`: list of `NotCheckedArea` entries, with general entries
//!   shown on every report and indicator-conditional entries shown only
//!   when the matching `RepoContextTag` is detected.
//! - `calibration`: tells consumers that legacy fields exist and that
//!   `evidenceFindings` is the preferred view.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` §B/§E.

use serde_json::{json, Value};

use repolens_core::model::evidence::{
    not_checked_codes, NotCheckedArea, NotCheckedTrigger, RepoContextTag,
};

/// Build the standard "What This Scan Cannot Prove" set, filtering
/// indicator-conditional entries by which `RepoContextTag`s are present.
pub fn build_not_checked(repo_context: &[RepoContextTag]) -> Vec<NotCheckedArea> {
    let mut out: Vec<NotCheckedArea> = vec![
        always(
            not_checked_codes::RUNTIME_BEHAVIOR,
            "RepoLens does not execute the code; runtime behaviour is out of scope.",
        ),
        always(
            not_checked_codes::TEST_PASS_FAIL,
            "RepoLens does not run tests. Whether tests pass, fail, or compile is out of scope.",
        ),
        always(
            not_checked_codes::RELEASE_READINESS,
            "RepoLens does not assess release readiness, deployment safety, or rollout suitability.",
        ),
        always(
            not_checked_codes::SECURITY_AUDIT_GENERIC,
            "RepoLens is not a security audit. Vulnerabilities, misconfiguration, and threat \
             modelling are out of scope.",
        ),
        always(
            not_checked_codes::LICENSE_COMPATIBILITY,
            "RepoLens does not analyse license compatibility across the dependency tree.",
        ),
        always(
            not_checked_codes::EXTERNAL_DEPENDENCY_VERSIONS,
            "RepoLens does not check external dependency versions, vulnerabilities, or yanked \
             status.",
        ),
        always(
            not_checked_codes::LOCKFILE_FRESHNESS,
            "RepoLens does not verify whether lockfiles are current with respect to the manifest \
             they pin.",
        ),
        always(
            not_checked_codes::LOCKFILE_GENERATION_OUT_OF_SCOPE,
            "RepoLens does not run cargo, npm, pip, or any package manager. Lockfile generation \
             is out of scope.",
        ),
        always(
            not_checked_codes::CHANGE_SAFETY,
            "RepoLens does not predict the safety of code changes; refactoring impact must be \
             assessed by the agent or developer.",
        ),
        always(
            not_checked_codes::MANIFEST_AND_LOCKFILE_LIVE_ROOT_DERIVED,
            "Manifest dependency names (Cargo.toml, package.json, etc.) and lockfile state are \
             read from the live working tree at report time, not from the pack snapshot. When \
             summarizing or comparing older packs, indicator-tag-driven findings and the \
             cargo-lock state may diverge from what was originally scanned.",
        ),
    ];

    let has = |tag: &RepoContextTag| repo_context.contains(tag);
    let any_rust = repo_context.iter().any(|t| t.is_rust());

    if has(&RepoContextTag::CryptoIndicators) || has(&RepoContextTag::SecuritySensitiveIndicators) {
        // Pick the trigger tag that actually appears in `repo_context`
        // so consumers can verify the gating tag is present in the
        // report (e.g. a SecuritySensitive-only repo gets the
        // securitySensitiveIndicators trigger, not cryptoIndicators).
        let trigger_tag = if has(&RepoContextTag::CryptoIndicators) {
            RepoContextTag::CryptoIndicators
        } else {
            RepoContextTag::SecuritySensitiveIndicators
        };
        out.push(when(
            not_checked_codes::CRYPTO_CORRECTNESS,
            "RepoLens does not verify cryptographic correctness, primitive choice, or known-bad \
             construction patterns.",
            trigger_tag,
        ));
    }
    if has(&RepoContextTag::CryptoIndicators) {
        out.push(when(
            not_checked_codes::CONSTANT_TIME,
            "RepoLens does not perform constant-time analysis on cryptographic code paths.",
            RepoContextTag::CryptoIndicators,
        ));
        out.push(when(
            not_checked_codes::TIMING_LEAKAGE,
            "RepoLens does not detect timing side-channels in cryptographic code paths.",
            RepoContextTag::CryptoIndicators,
        ));
    }
    if has(&RepoContextTag::CryptoIndicators)
        || has(&RepoContextTag::CompilerIndicators)
        || has(&RepoContextTag::DatabaseIndicators)
    {
        let trigger_tag = if has(&RepoContextTag::CryptoIndicators) {
            RepoContextTag::CryptoIndicators
        } else if has(&RepoContextTag::CompilerIndicators) {
            RepoContextTag::CompilerIndicators
        } else {
            RepoContextTag::DatabaseIndicators
        };
        out.push(when(
            not_checked_codes::FUZZING_RESULTS,
            "RepoLens does not run fuzzers. Fuzzing campaign results, corpus coverage, and \
             discovered crashes are out of scope.",
            trigger_tag,
        ));
    }
    if any_rust {
        // Use whichever Rust tag is actually present so consumers can
        // verify the gating tag in the same envelope. Order: prefer
        // RustWorkspace, then RustLibrary, then RustBinary.
        let rust_tag = if has(&RepoContextTag::RustWorkspace) {
            RepoContextTag::RustWorkspace
        } else if has(&RepoContextTag::RustLibrary) {
            RepoContextTag::RustLibrary
        } else {
            RepoContextTag::RustBinary
        };
        out.push(when(
            not_checked_codes::MSRV,
            "RepoLens does not verify the minimum supported Rust version (MSRV) declared in \
             Cargo.toml against actual code or dependencies.",
            rust_tag,
        ));
    }
    if has(&RepoContextTag::NoStdIndicators) {
        out.push(when(
            not_checked_codes::NO_STD_TARGET_VALIDATION,
            "RepoLens does not validate `no_std` compatibility across all code paths or against \
             specific embedded targets.",
            RepoContextTag::NoStdIndicators,
        ));
    }
    if has(&RepoContextTag::DatabaseIndicators) {
        out.push(when(
            not_checked_codes::MIGRATION_SAFETY,
            "RepoLens does not analyse database migration safety, ordering, or compatibility \
             with production data.",
            RepoContextTag::DatabaseIndicators,
        ));
    }
    if has(&RepoContextTag::CompilerIndicators) {
        out.push(when(
            not_checked_codes::ABI_STABILITY,
            "RepoLens does not analyse ABI stability for compiler/codegen artefacts.",
            RepoContextTag::CompilerIndicators,
        ));
    }

    out
}

fn always(code: &str, detail: &str) -> NotCheckedArea {
    NotCheckedArea {
        code: code.to_string(),
        detail: detail.to_string(),
        trigger: NotCheckedTrigger::Always,
    }
}

fn when(code: &str, detail: &str, tag: RepoContextTag) -> NotCheckedArea {
    NotCheckedArea {
        code: code.to_string(),
        detail: detail.to_string(),
        trigger: NotCheckedTrigger::WhenIndicator { tag },
    }
}

/// Build the `triageHeader` block for a tool response.
pub fn triage_header(pack_id: &str, generated_at: &str) -> Value {
    json!({
        "reportKind": "structuralTriage",
        "scope": {
            "packId": pack_id,
            "generatedAt": generated_at,
        },
    })
}

/// Build the `calibration` block listing legacy fields and pointing
/// consumers at `evidenceFindings`.
///
/// `legacy_fields` is taken as-is — the caller is responsible for filtering
/// to fields actually present in the payload. `augment_data` does that
/// filtering centrally; direct callers (e.g., tests) should pre-filter.
pub fn calibration_block(legacy_fields: &[&str]) -> Value {
    json!({
        "preferred": "evidenceFindings",
        "legacyFields": legacy_fields,
        "guidance": "evidenceFindings carry per-finding evidence grade, knowledge layer, \
                     inference, limitations, and repo context. Legacy fields lack per-finding \
                     calibration and may overclaim. Prefer evidenceFindings when present."
    })
}

/// Returns `true` if `path` (a dotted key path like `health.dependencyRisk`
/// or `metrics`) resolves to a key that exists in `root`. Empty
/// arrays/objects count as present — the key is there even if the value is
/// empty, and the calibration warning still applies. Non-object containers
/// along the path short-circuit to `false`.
fn dotted_path_present(root: &serde_json::Map<String, Value>, path: &str) -> bool {
    let mut segments = path.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let Some(mut cursor) = root.get(first) else {
        return false;
    };
    for segment in segments {
        match cursor {
            Value::Object(map) => match map.get(segment) {
                Some(v) => cursor = v,
                None => return false,
            },
            _ => return false,
        }
    }
    true
}

/// Augment a tool's `data` JSON object with `triageHeader`, `notChecked`,
/// and `calibration` keys. `legacy_fields` is filtered against `data` so the
/// calibration block only references keys actually present in the response
/// — preventing dangling pointers like `health.signals` at `detail: standard`
/// where the field is absent. If `data` is not a JSON object the function
/// is a no-op (degenerate case; preserves the existing payload).
pub fn augment_data(
    mut data: Value,
    pack_id: &str,
    generated_at: &str,
    repo_context: &[RepoContextTag],
    legacy_fields: &[&str],
) -> Value {
    let Value::Object(ref mut map) = data else {
        return data;
    };
    // Filter against the inbound payload (before we inject the envelope
    // keys), so envelope keys can never accidentally satisfy a legacy-field
    // claim.
    let present_legacy_fields: Vec<&str> = legacy_fields
        .iter()
        .copied()
        .filter(|p| dotted_path_present(map, p))
        .collect();
    map.insert("triageHeader".into(), triage_header(pack_id, generated_at));
    let nc = build_not_checked(repo_context);
    map.insert(
        "notChecked".into(),
        serde_json::to_value(&nc).unwrap_or_else(|_| json!([])),
    );
    map.insert(
        "calibration".into(),
        calibration_block(&present_legacy_fields),
    );
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn always_shown_codes_present_for_empty_context() {
        let nc = build_not_checked(&[]);
        let codes: Vec<&str> = nc.iter().map(|n| n.code.as_str()).collect();
        for must_have in [
            not_checked_codes::RUNTIME_BEHAVIOR,
            not_checked_codes::TEST_PASS_FAIL,
            not_checked_codes::RELEASE_READINESS,
            not_checked_codes::SECURITY_AUDIT_GENERIC,
            not_checked_codes::LICENSE_COMPATIBILITY,
            not_checked_codes::EXTERNAL_DEPENDENCY_VERSIONS,
            not_checked_codes::LOCKFILE_FRESHNESS,
            not_checked_codes::LOCKFILE_GENERATION_OUT_OF_SCOPE,
            not_checked_codes::CHANGE_SAFETY,
        ] {
            assert!(
                codes.contains(&must_have),
                "missing always-shown code: {must_have}"
            );
        }
    }

    #[test]
    fn no_crypto_or_timing_codes_for_csv_parser_repo() {
        let nc = build_not_checked(&[RepoContextTag::RustLibrary]);
        let codes: Vec<&str> = nc.iter().map(|n| n.code.as_str()).collect();
        assert!(!codes.contains(&not_checked_codes::CRYPTO_CORRECTNESS));
        assert!(!codes.contains(&not_checked_codes::CONSTANT_TIME));
        assert!(!codes.contains(&not_checked_codes::TIMING_LEAKAGE));
        // MSRV fires for any Rust repo.
        assert!(codes.contains(&not_checked_codes::MSRV));
    }

    #[test]
    fn crypto_codes_fire_when_indicators_present() {
        let nc = build_not_checked(&[
            RepoContextTag::RustLibrary,
            RepoContextTag::CryptoIndicators,
        ]);
        let codes: Vec<&str> = nc.iter().map(|n| n.code.as_str()).collect();
        assert!(codes.contains(&not_checked_codes::CRYPTO_CORRECTNESS));
        assert!(codes.contains(&not_checked_codes::CONSTANT_TIME));
        assert!(codes.contains(&not_checked_codes::TIMING_LEAKAGE));
        assert!(codes.contains(&not_checked_codes::FUZZING_RESULTS));
    }

    #[test]
    fn msrv_trigger_tag_uses_actually_present_rust_tag() {
        // RustBinary repo: trigger should be RustBinary, not RustLibrary.
        let nc = build_not_checked(&[RepoContextTag::RustBinary]);
        let entry = nc
            .iter()
            .find(|n| n.code == not_checked_codes::MSRV)
            .expect("MSRV must fire for RustBinary");
        match &entry.trigger {
            NotCheckedTrigger::WhenIndicator { tag } => {
                assert_eq!(*tag, RepoContextTag::RustBinary);
            }
            _ => panic!("expected whenIndicator trigger, got {:?}", entry.trigger),
        }

        // RustWorkspace repo: trigger should be RustWorkspace.
        let nc = build_not_checked(&[RepoContextTag::RustWorkspace]);
        let entry = nc
            .iter()
            .find(|n| n.code == not_checked_codes::MSRV)
            .expect("MSRV must fire for RustWorkspace");
        match &entry.trigger {
            NotCheckedTrigger::WhenIndicator { tag } => {
                assert_eq!(*tag, RepoContextTag::RustWorkspace);
            }
            _ => panic!("expected whenIndicator trigger, got {:?}", entry.trigger),
        }
    }

    #[test]
    fn crypto_correctness_trigger_tag_matches_actually_present_indicator() {
        // When only SecuritySensitiveIndicators is present, the
        // CRYPTO_CORRECTNESS notChecked entry must use that tag as its
        // trigger — not the absent CryptoIndicators tag.
        let nc = build_not_checked(&[
            RepoContextTag::RustLibrary,
            RepoContextTag::SecuritySensitiveIndicators,
        ]);
        let entry = nc
            .iter()
            .find(|n| n.code == not_checked_codes::CRYPTO_CORRECTNESS)
            .expect("CRYPTO_CORRECTNESS must fire under SecuritySensitiveIndicators");
        match &entry.trigger {
            NotCheckedTrigger::WhenIndicator { tag } => {
                assert_eq!(*tag, RepoContextTag::SecuritySensitiveIndicators);
            }
            _ => panic!("expected whenIndicator trigger, got {:?}", entry.trigger),
        }

        // When CryptoIndicators is present, the trigger should be that.
        let nc = build_not_checked(&[
            RepoContextTag::RustLibrary,
            RepoContextTag::CryptoIndicators,
        ]);
        let entry = nc
            .iter()
            .find(|n| n.code == not_checked_codes::CRYPTO_CORRECTNESS)
            .expect("CRYPTO_CORRECTNESS must fire under CryptoIndicators");
        match &entry.trigger {
            NotCheckedTrigger::WhenIndicator { tag } => {
                assert_eq!(*tag, RepoContextTag::CryptoIndicators);
            }
            _ => panic!("expected whenIndicator trigger, got {:?}", entry.trigger),
        }
    }

    #[test]
    fn no_std_target_validation_fires_only_for_no_std_indicators() {
        let without = build_not_checked(&[RepoContextTag::RustLibrary]);
        let with =
            build_not_checked(&[RepoContextTag::RustLibrary, RepoContextTag::NoStdIndicators]);
        assert!(!without
            .iter()
            .any(|n| n.code == not_checked_codes::NO_STD_TARGET_VALIDATION));
        assert!(with
            .iter()
            .any(|n| n.code == not_checked_codes::NO_STD_TARGET_VALIDATION));
    }

    #[test]
    fn msrv_does_not_fire_for_non_rust_repos() {
        let nc = build_not_checked(&[RepoContextTag::JavaService]);
        let codes: Vec<&str> = nc.iter().map(|n| n.code.as_str()).collect();
        assert!(!codes.contains(&not_checked_codes::MSRV));
    }

    #[test]
    fn augment_data_adds_three_top_level_keys() {
        // Input includes legacy fields so the presence-filter keeps them in
        // the calibration block.
        let original = json!({
            "structure": { "moduleCount": 1 },
            "metrics": [],
            "health": { "dependencyRisk": { "riskLevel": "low" } },
        });
        let out = augment_data(
            original,
            "pack-test",
            "2026-05-11T00:00:00Z",
            &[RepoContextTag::RustLibrary],
            &["metrics", "health.dependencyRisk"],
        );
        assert_eq!(out["structure"]["moduleCount"], 1);
        assert_eq!(out["triageHeader"]["reportKind"], "structuralTriage");
        assert_eq!(out["triageHeader"]["scope"]["packId"], "pack-test");
        assert!(out["notChecked"].is_array());
        assert_eq!(out["calibration"]["preferred"], "evidenceFindings");
        assert!(out["calibration"]["legacyFields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "metrics"));
    }

    #[test]
    fn augment_data_is_no_op_for_non_object_data() {
        let original = Value::String("not an object".into());
        let out = augment_data(original.clone(), "pack-test", "x", &[], &[]);
        assert_eq!(out, Value::String("not an object".into()));
    }

    #[test]
    fn triage_header_serializes_camel_case_keys() {
        let v = triage_header("pack-x", "2026-05-11T00:00:00Z");
        assert!(v.get("reportKind").is_some());
        assert!(v["scope"].get("packId").is_some());
        assert!(v["scope"].get("generatedAt").is_some());
    }

    // -------------------------------------------------------------------
    // PR A (2026-05-12 dogfood D2): `dotted_path_present` powers the
    // calibration-field filter inside `augment_data`. The function must
    // resolve nested keys honestly so the calibration block only names
    // fields actually present in the payload.
    // -------------------------------------------------------------------

    fn obj(v: Value) -> serde_json::Map<String, Value> {
        match v {
            Value::Object(m) => m,
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn dotted_path_present_top_level_key() {
        let m = obj(json!({ "metrics": [] }));
        assert!(dotted_path_present(&m, "metrics"));
        assert!(!dotted_path_present(&m, "absent"));
    }

    #[test]
    fn dotted_path_present_nested_path() {
        let m = obj(json!({
            "health": {
                "graphHealth": {
                    "orphanCount": 7
                }
            }
        }));
        assert!(dotted_path_present(&m, "health.graphHealth.orphanCount"));
        assert!(dotted_path_present(&m, "health.graphHealth"));
        assert!(dotted_path_present(&m, "health"));
    }

    #[test]
    fn dotted_path_present_missing_nested_key() {
        // `health.signals` is the dogfood D2 case: `health` is present at
        // standard detail but `signals` only appears at full.
        let m = obj(json!({
            "health": { "dependencyRisk": { "riskLevel": "low" } }
        }));
        assert!(!dotted_path_present(&m, "health.signals"));
        assert!(!dotted_path_present(&m, "health.graphHealth.orphanCount"));
        assert!(dotted_path_present(&m, "health.dependencyRisk.riskLevel"));
    }

    #[test]
    fn dotted_path_present_short_circuits_on_non_object_segment() {
        // Trying to walk into a scalar/array mid-path is treated as absent.
        let m = obj(json!({
            "metrics": [],
            "stats": { "fileCount": 3 }
        }));
        assert!(!dotted_path_present(&m, "metrics.firstItem"));
        assert!(!dotted_path_present(&m, "stats.fileCount.value"));
    }

    #[test]
    fn dotted_path_present_empty_path_is_absent() {
        let m = obj(json!({ "anything": 1 }));
        assert!(!dotted_path_present(&m, ""));
    }

    #[test]
    fn augment_data_filters_absent_legacy_fields() {
        // Standard-detail get_summary shape: no `health.graphHealth.*`. The
        // handler passes the union list; the filter must drop the absent ones.
        let original = json!({
            "stats": { "testToSourceRatio": 0.1 },
            "health": { "dependencyRisk": { "riskLevel": "low" } }
        });
        let out = augment_data(
            original,
            "pack-test",
            "2026-05-12T00:00:00Z",
            &[RepoContextTag::RustLibrary],
            &[
                "stats.testToSourceRatio",
                "health.dependencyRisk",
                "health.graphHealth.testToSourceRatio",
                "health.graphHealth.orphanCount",
                "health.graphHealth.orphanFiles",
            ],
        );
        let legacy: Vec<&str> = out["calibration"]["legacyFields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            legacy,
            vec!["stats.testToSourceRatio", "health.dependencyRisk"],
            "filter must preserve order and drop absent paths; got {legacy:?}"
        );
    }
}
