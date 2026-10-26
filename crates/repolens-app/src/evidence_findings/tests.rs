use super::*;
use repolens_core::model::graph::{FileGraph, GraphEdge, GraphNode};
use repolens_core::model::profile::{BuildSystem, EntryPoint, ModuleInfo, ProjectProfile};
use repolens_profile::repo_context::{RepoContext, RepoContextDetector};
use std::collections::BTreeMap;

fn rust_library_profile(modules: Vec<ModuleInfo>) -> ProjectProfile {
    ProjectProfile {
        name: "demo".into(),
        build_system: Some(BuildSystem {
            name: "cargo".into(),
            details: None,
            config_file: "Cargo.toml".into(),
        }),
        frameworks: vec![],
        project_type: ProjectType::Library,
        entry_points: vec![],
        description: None,
        language_distribution: BTreeMap::from([("rust".into(), 1)]),
        modules,
    }
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

fn rust_lib_ctx(profile: &ProjectProfile, file_paths: &[&str]) -> RepoContext {
    RepoContextDetector::detect(profile, &paths(file_paths))
}

fn builder<'a>(
    profile: &'a ProjectProfile,
    ctx: &'a RepoContext,
    graph: Option<&'a FileGraph>,
    graph_health: Option<&'a GraphHealthInput>,
    scan: Option<&'a ScanSummary>,
    lockfile_state: Option<&'a CargoLockState>,
) -> EvidenceFindingsBuilder<'a> {
    EvidenceFindingsBuilder {
        pack_id: "pack-test",
        commit: None,
        profile,
        repo_context: ctx,
        graph,
        graph_health,
        scan,
        lockfile_state,
        cargo_has_binary: None,
        referenced_files: None,
        dependency_risk: None,
    }
}

fn builder_with_risk<'a>(
    profile: &'a ProjectProfile,
    ctx: &'a RepoContext,
    risk: &'a repolens_core::model::health::DependencyRisk,
) -> EvidenceFindingsBuilder<'a> {
    EvidenceFindingsBuilder {
        pack_id: "pack-test",
        commit: None,
        profile,
        repo_context: ctx,
        graph: None,
        graph_health: None,
        scan: None,
        lockfile_state: None,
        cargo_has_binary: None,
        referenced_files: None,
        dependency_risk: Some(risk),
    }
}

// -- 0. Per-finding evidence_grade (Phase 3) ----------------------------

/// Per-finding `evidence_grade` for the four findings whose underlying
/// metrics are backed by doc-06 §5 catalogues (Phase 3, 2026-05-11).
/// The two findings whose metrics are still uncatalogued
/// (F-CARGO-LOCK-STATE — lockfile state; F-TEST-INVENTORY-RATIO —
/// test-file detection) stay at `Unaudited` until Phase 4 lands their
/// invariant catalogues.
#[test]
fn phase3_per_finding_evidence_grades() {
    let profile = rust_library_profile(vec![
        ModuleInfo {
            name: "demo".into(),
            path: "crates/demo".into(),
            file_count: 10,
            loc: 100,
            primary_language: "rust".into(),
        },
        ModuleInfo {
            name: "other".into(),
            path: "crates/other".into(),
            file_count: 2,
            loc: 5,
            primary_language: "rust".into(),
        },
    ]);
    let ctx = rust_lib_ctx(&profile, &["crates/demo/src/leaf.rs"]);
    // Fan-in target must be non-indicator (post-2026-05-14 calibration:
    // indicator-tagged hub modules are filtered before F-HIGH-FAN-IN-FILE
    // emits). Use a leaf file so the finding fires.
    let graph = fan_in_test_graph("crates/demo/src/leaf.rs", 10);
    // GraphHealthInput with non-zero orphans so F-ORPHAN-FILES fires.
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 3,
        orphan_files: paths(&["src/a.rs", "src/b.rs", "src/c.rs"]),
        test_to_source_ratio: 0.2,
    };
    let b = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None);
    let findings = b.build();

    // All catalogued findings carry `Derived` per-finding evidence_grade.
    for fid in [
        F_NO_ENTRY_POINTS,
        F_ORPHAN_FILES,
        F_DOMINANT_MODULE,
        F_HIGH_FAN_IN_FILE,
    ] {
        let f = findings
            .iter()
            .find(|f| f.id.as_str() == fid)
            .unwrap_or_else(|| panic!("missing finding {fid}"));
        assert_eq!(
            f.evidence_grade,
            EvidenceGrade::Derived,
            "{fid} should carry per-finding evidence_grade=Derived; got {:?}",
            f.evidence_grade
        );
    }
}

/// Phase 4 (2026-05-11): F-CARGO-LOCK-STATE promotes to `Derived` once the
/// lockfile-state invariant catalogue lands. Pin the new grade.
#[test]
fn phase4_cargo_lock_evidence_grade_derived() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["Cargo.toml", "src/lib.rs"]);
    let absent = CargoLockState::AbsentFromRepo;
    let b = builder(&profile, &ctx, None, None, None, Some(&absent));
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_CARGO_LOCK_STATE)
        .expect("cargo lock finding");
    assert_eq!(
        f.evidence_grade,
        EvidenceGrade::Derived,
        "F-CARGO-LOCK-STATE was promoted to Derived in Phase 4 (2026-05-11) \
         via the lockfile-state invariant catalogue"
    );
}

/// Phase 4 (2026-05-11): F-TEST-INVENTORY-RATIO promotes to `Derived` once
/// the test-file detection invariant catalogue lands.
#[test]
fn phase4_test_inventory_evidence_grade_derived() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "demo".into(),
        path: "src".into(),
        file_count: 10,
        loc: 200,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 1 test file, 10 source files = 0.1 ratio, just at threshold.
    // Make it slightly worse to fire: 1/15 = ~0.067.
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 16,
        included_count: 16,
        total_loc: 800,
        test_file_count: 1,
        source_file_count: 15,
        test_to_source_ratio: 1.0 / 15.0,
    };
    let b = builder(&profile, &ctx, None, None, Some(&scan), None);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_TEST_INVENTORY_RATIO)
        .expect("test inventory finding");
    assert_eq!(
        f.evidence_grade,
        EvidenceGrade::Derived,
        "F-TEST-INVENTORY-RATIO was promoted to Derived in Phase 4 (2026-05-11) \
         via the test-file detection invariant catalogue"
    );
}

// -- 1. F-NO-ENTRY-POINTS ------------------------------------------------

#[test]
fn rust_library_no_entry_points_classified_expected() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "demo".into(),
        path: "src".into(),
        file_count: 1,
        loc: 10,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    let entry = findings
        .iter()
        .find(|f| f.id.as_str() == F_NO_ENTRY_POINTS)
        .expect("expected entry-points finding");
    assert_eq!(entry.classification, FindingClassification::Expected);
    assert_eq!(entry.severity, FindingSeverity::None);
    assert!(entry.repo_context.contains(&RepoContextTag::RustLibrary));
    // Action is NoAction → inference.does_not_imply must be non-empty.
    assert!(matches!(entry.action, RecommendedAction::NoAction));
    assert!(!entry.inference.does_not_imply.is_empty());
}

#[test]
fn rust_binary_no_entry_points_classified_informational() {
    let mut profile = rust_library_profile(vec![]);
    profile.project_type = ProjectType::Cli;
    // entry_points still empty — the test is what happens when project_type
    // says CLI but extractors found no main.
    let ctx = rust_lib_ctx(&profile, &["src/main.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    let entry = findings
        .iter()
        .find(|f| f.id.as_str() == F_NO_ENTRY_POINTS)
        .expect("expected finding");
    assert_eq!(entry.classification, FindingClassification::Informational);
    assert!(matches!(
        entry.action,
        RecommendedAction::ReviewIfMaintainerPain
    ));
}

// -- 2. F-ORPHAN-FILES ---------------------------------------------------

#[test]
fn root_config_and_policy_files_filtered_from_orphans() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 5,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 4,
        orphan_files: paths(&["Cargo.toml", "LICENSE", "README.md", ".gitignore"]),
        test_to_source_ratio: 0.0,
    };
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 50,
        included_count: 50,
        total_loc: 5000,
        test_file_count: 5,
        source_file_count: 45,
        test_to_source_ratio: 0.11,
    };
    let b = builder(&profile, &ctx, None, Some(&gh), Some(&scan), None);
    let findings = b.build();
    let orph = findings
        .iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    // All orphans filtered → Informational, severity None.
    assert_eq!(orph.classification, FindingClassification::Informational);
    assert_eq!(orph.severity, FindingSeverity::None);
    assert!(matches!(orph.action, RecommendedAction::NoAction));
    // Limitation must explicitly note that configs/docs are filtered.
    assert!(orph
        .limitations
        .iter()
        .any(|l| l.code == limitation_codes::DOCS_AND_CONFIG_NOT_TREATED_AS_SOURCE));
}

#[test]
fn orphan_source_files_after_filter_classified_actionable_above_threshold() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 5,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 5 orphan source files out of 50 total = 10% > 5% threshold.
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 5,
        orphan_files: paths(&[
            "src/unused1.rs",
            "src/unused2.rs",
            "src/unused3.rs",
            "src/unused4.rs",
            "src/unused5.rs",
        ]),
        test_to_source_ratio: 0.2,
    };
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 50,
        included_count: 50,
        total_loc: 5000,
        test_file_count: 10,
        source_file_count: 40,
        test_to_source_ratio: 0.25,
    };
    let b = builder(&profile, &ctx, None, Some(&gh), Some(&scan), None);
    let findings = b.build();
    let orph = findings
        .iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    assert_eq!(orph.classification, FindingClassification::Actionable);
    assert_eq!(orph.severity, FindingSeverity::Low);
    // Iteration 4: actionable orphans carry a concrete, calibrated action
    // (what reachability to check + a conditional remedy), not the vague
    // reviewIfMaintainerPain.
    match &orph.action {
        RecommendedAction::ConcreteAction { steps } => {
            assert!(!steps.is_empty(), "orphan action must carry steps");
            let joined = steps.join(" ").to_ascii_lowercase();
            assert!(
                joined.contains("reach") || joined.contains("entry") || joined.contains("dynamic"),
                "orphan action should tell the agent what reachability to check; got {steps:?}"
            );
        }
        other => panic!("expected ConcreteAction for actionable orphans, got {other:?}"),
    }
    // Heuristic judgment must appear in evidence.
    assert!(orph
        .evidence
        .iter()
        .any(|ev| matches!(ev, EvidenceItem::HeuristicJudgment { .. })));
}

/// N1 calibration (2026-05-14): Cargo integration tests, benches, examples,
/// build scripts, and fixture-data files are filtered from the orphan list
/// even though they have zero inbound structural import edges. The 2026-05-14
/// dogfood on repolens-rs surfaced 16 of these as false-positive orphans.
#[test]
fn orphan_filter_excludes_cargo_runner_targets_and_fixtures() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 20,
        loc: 2000,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // Mix of (a) Cargo runner targets that the filter should exclude
    // and (b) genuine source orphans that should remain.
    let orphans = vec![
        // Filtered: Cargo integration tests.
        "crates/foo/tests/integration_test.rs".to_string(),
        "crates/bar/tests/api_test.rs".to_string(),
        // Filtered: benches & examples.
        "crates/foo/benches/throughput.rs".to_string(),
        "crates/foo/examples/demo.rs".to_string(),
        // Filtered: top-level build script.
        "build.rs".to_string(),
        // Filtered: fixture data.
        "crates/foo/fixtures/input.yaml".to_string(),
        "crates/foo/fixtures/golden.json".to_string(),
        // KEPT: real source orphan.
        "crates/foo/src/dead_module.rs".to_string(),
        // KEPT: nested build.rs is NOT a Cargo build script —
        // codex round-5 narrowed this to literal root only.
        "tools/build.rs".to_string(),
    ];
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: orphans.len(),
        orphan_files: orphans,
        test_to_source_ratio: 0.2,
    };
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 50,
        included_count: 50,
        total_loc: 5000,
        test_file_count: 10,
        source_file_count: 40,
        test_to_source_ratio: 0.25,
    };
    let b = builder(&profile, &ctx, None, Some(&gh), Some(&scan), None);
    let orph = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding must fire");
    let paths: Vec<String> = orph.scope.paths.clone();
    // Real source orphans remain.
    for kept in &["crates/foo/src/dead_module.rs", "tools/build.rs"] {
        assert!(
            paths.iter().any(|p| p == kept),
            "real source orphan {kept} must remain in scope.paths: {paths:?}"
        );
    }
    // Filtered classes are absent.
    for excluded in &[
        "crates/foo/tests/integration_test.rs",
        "crates/bar/tests/api_test.rs",
        "crates/foo/benches/throughput.rs",
        "crates/foo/examples/demo.rs",
        "build.rs",
        "crates/foo/fixtures/input.yaml",
        "crates/foo/fixtures/golden.json",
    ] {
        assert!(
            !paths.iter().any(|p| p == excluded),
            "filtered path {excluded} must not appear in scope.paths: {paths:?}"
        );
    }
    // The depends_on tag reflects the broader filter set.
    assert!(
        orph.inference
            .depends_on
            .iter()
            .any(|d| d == "filter:nonOrphanSource"),
        "depends_on must reference filter:nonOrphanSource: {:?}",
        orph.inference.depends_on
    );
}

// -- 3. F-DOMINANT-MODULE ------------------------------------------------

#[test]
fn dominant_module_in_single_crate_library_classified_expected() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 5,
        loc: 1000,
        primary_language: "rust".into(),
    }]);
    // Single-crate library → no nested Cargo.toml entries.
    let ctx = rust_lib_ctx(&profile, &["Cargo.toml", "src/lib.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    let dom = findings
        .iter()
        .find(|f| f.id.as_str() == F_DOMINANT_MODULE)
        .expect("dominant-module finding");
    assert_eq!(dom.classification, FindingClassification::Expected);
    assert_eq!(dom.severity, FindingSeverity::None);
    assert!(matches!(dom.action, RecommendedAction::NoAction));
    assert!(dom
        .inference
        .depends_on
        .iter()
        .any(|s| s == "projectShape=singleCrateLibrary"));
}

#[test]
fn dominant_module_in_multi_module_repo_classified_informational() {
    let profile = rust_library_profile(vec![
        ModuleInfo {
            name: "core".into(),
            path: "src/core".into(),
            file_count: 10,
            loc: 950,
            primary_language: "rust".into(),
        },
        ModuleInfo {
            name: "util".into(),
            path: "src/util".into(),
            file_count: 1,
            loc: 50,
            primary_language: "rust".into(),
        },
    ]);
    let ctx = rust_lib_ctx(&profile, &["Cargo.toml", "src/core/mod.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    let dom = findings
        .iter()
        .find(|f| f.id.as_str() == F_DOMINANT_MODULE)
        .expect("dominant-module finding");
    assert_eq!(dom.classification, FindingClassification::Informational);
}

// -- 4. F-HIGH-FAN-IN-FILE ----------------------------------------------

/// Build a fan-in test graph: one hub + `leaf_count` leaves, all leaves
/// import the hub. Leaves a/b/c import the hub; the rest are disconnected
/// padding so the graph meets the `HIGH_FAN_IN_MIN_GRAPH_NODES` floor
/// without changing the inbound count on the hub.
fn fan_in_test_graph(hub: &str, leaf_count: usize) -> FileGraph {
    let mut nodes = vec![GraphNode {
        id: hub.into(),
        symbols: vec![],
    }];
    let leaf_names: Vec<String> = (0..leaf_count)
        .map(|i| format!("src/leaf_{i:02}.rs"))
        .collect();
    for name in &leaf_names {
        nodes.push(GraphNode {
            id: name.clone(),
            symbols: vec![],
        });
    }
    // First three leaves import the hub — three inbound edges on the hub.
    let edges = leaf_names
        .iter()
        .take(3)
        .map(|leaf| GraphEdge {
            source: leaf.clone(),
            target: hub.into(),
            edge_type: "imports".into(),
            detail: None,
        })
        .collect();
    FileGraph { nodes, edges }
}

/// Inbound-edge observation of a fan-in finding.
fn fan_in_observation(f: &EvidenceFinding) -> serde_json::Value {
    f.evidence
        .iter()
        .find_map(|e| match e {
            EvidenceItem::Observation {
                metric,
                observed_value,
                ..
            } if metric == metric_names::GRAPH_INBOUND_EDGES => Some(observed_value.clone()),
            _ => None,
        })
        .expect("graph.inboundEdges observation")
}

/// When the only above-threshold file is an indicator-tagged hub
/// (lib.rs, mod.rs, prelude), F-HIGH-FAN-IN-FILE lists nothing and is a
/// no-action note, but it is kept and counts the hub as left out (#31:
/// nothing over the threshold disappears silently).
#[test]
fn high_fan_in_to_lib_rs_is_a_no_action_note_that_counts_it() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 1 hub + 10 leaves = 11 nodes, meeting the min-nodes guard. Three
    // leaves import lib.rs → 3 inbound on the hub, above the absolute
    // floor and above 2× the (low) mean.
    let graph = fan_in_test_graph("src/lib.rs", 10);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("indicator-only fan-in is reported, not dropped");
    assert_eq!(fan.severity, FindingSeverity::None);
    assert_eq!(fan.classification, FindingClassification::Expected);
    assert!(fan.scope.paths.is_empty(), "{:?}", fan.scope.paths);
    let obs = fan_in_observation(&fan);
    assert_eq!(obs["sample"].as_array().unwrap().len(), 0, "{obs}");
    assert_eq!(obs["moduleHubFilesLeftOut"]["count"], 1, "{obs}");
    assert_eq!(
        obs["moduleHubFilesLeftOut"]["sample"][0]["path"],
        "src/lib.rs"
    );
    assert!(fan.title.contains("none listed"), "{}", fan.title);
}

/// #31 / Design call 3: when every file over the threshold sits in a
/// shared `*-core` crate that is not the main crate, the finding stays
/// (no-action) and counts those files, instead of vanishing.
#[test]
fn high_fan_in_all_in_shared_crate_is_kept_and_counted() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["crates/app/src/a00.rs"]);
    // repo-core (6 files) is smaller than app (20), so it is not main.
    let graph = shared_crate_graph("repo-core", 6, 20);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding kept when everything is filtered");
    assert_eq!(fan.severity, FindingSeverity::None);
    let obs = fan_in_observation(&fan);
    assert_eq!(obs["sample"].as_array().unwrap().len(), 0, "{obs}");
    assert_eq!(obs["sharedCrateFilesLeftOut"]["count"], 1, "{obs}");
    assert!(obs.get("moduleHubFilesLeftOut").is_none(), "{obs}");
}

#[test]
fn high_fan_in_to_leaf_module_classified_actionable() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/leaf.rs"]);
    let graph = fan_in_test_graph("src/leaf.rs", 10);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let findings = b.build();
    let fan = findings
        .iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding");
    assert_eq!(fan.classification, FindingClassification::Actionable);
    assert_eq!(fan.severity, FindingSeverity::Medium);
    // Iteration 4: actionable fan-in carries a concrete, calibrated action
    // (judge intentional-vs-accidental + a conditional remedy), not the vague
    // reviewIfMaintainerPain.
    match &fan.action {
        RecommendedAction::ConcreteAction { steps } => {
            assert!(!steps.is_empty(), "fan-in action must carry steps");
            let joined = steps.join(" ").to_ascii_lowercase();
            assert!(
                joined.contains("intentional")
                    || joined.contains("coupl")
                    || joined.contains("api"),
                "fan-in action should help judge intentional-vs-accidental; got {steps:?}"
            );
        }
        other => panic!("expected ConcreteAction for actionable fan-in, got {other:?}"),
    }
}

/// The min-nodes guard fires before any expected/actionable classification.
/// Small graphs (the typical fixture-test or scratch repo case) should
/// produce zero F-HIGH-FAN-IN-FILE findings even when a file has multiple
/// inbound edges — the mean-multiplier statistic isn't meaningful below
/// the threshold.
#[test]
fn high_fan_in_suppressed_on_tiny_graph() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/hub.rs"]);
    let graph = fan_in_test_graph("src/hub.rs", 3);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let findings = b.build();
    assert!(
        !findings.iter().any(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE),
        "F-HIGH-FAN-IN-FILE must not fire on graphs below the min-nodes guard"
    );
}

/// F-HIGH-FAN-IN-FILE caps the number of paths reported in the aggregate
/// finding's sample list. When the cap fires, the finding carries the
/// FAN_IN_FINDINGS_CAPPED limitation disclosing how many other files were
/// above-threshold but unreported.
#[test]
fn high_fan_in_caps_findings_and_discloses_omission() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/hub_00.rs"]);
    // 15 hubs, each with 5 importers → 15 files above the absolute floor.
    // The cap is 10, so 5 should be omitted.
    let mut nodes: Vec<GraphNode> = (0..15)
        .map(|i| GraphNode {
            id: format!("src/hub_{i:02}.rs"),
            symbols: vec![],
        })
        .collect();
    // Padding so we comfortably exceed HIGH_FAN_IN_MIN_GRAPH_NODES and the
    // overall mean stays low (most padding nodes have zero inbound).
    for i in 0..20 {
        nodes.push(GraphNode {
            id: format!("src/leaf_{i:02}.rs"),
            symbols: vec![],
        });
    }
    let mut edges = Vec::new();
    for hub in 0..15 {
        for leaf in 0..5 {
            edges.push(GraphEdge {
                source: format!("src/leaf_{leaf:02}.rs"),
                target: format!("src/hub_{hub:02}.rs"),
                edge_type: "imports".into(),
                detail: None,
            });
        }
    }
    let graph = FileGraph { nodes, edges };
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("aggregate fan-in finding");
    assert_eq!(
        fan.scope.paths.len(),
        10,
        "cap should hold the sample to 10 files"
    );
    let codes: Vec<&str> = fan.limitations.iter().map(|l| l.code.as_str()).collect();
    assert!(
        codes.contains(&"FAN_IN_FINDINGS_CAPPED"),
        "capped finding must carry FAN_IN_FINDINGS_CAPPED limitation; got {codes:?}"
    );
    let cap_lim = fan
        .limitations
        .iter()
        .find(|l| l.code == "FAN_IN_FINDINGS_CAPPED")
        .unwrap();
    assert!(
        cap_lim.detail.contains("5 additional"),
        "cap limitation should disclose 5 omitted; got: {}",
        cap_lim.detail
    );
}

/// F-HIGH-FAN-IN-FILE aggregates: a workspace with multiple non-indicator
/// hub paths produces ONE finding listing all of them in `scope.paths`,
/// not N separate findings differing only by path.
#[test]
fn high_fan_in_aggregates_multiple_files_into_one_finding() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/leaf.rs"]);
    // 5 hubs, each with 3 importers → 5 paths above threshold (below cap).
    let mut nodes: Vec<GraphNode> = (0..5)
        .map(|i| GraphNode {
            id: format!("src/leaf_{i:02}.rs"),
            symbols: vec![],
        })
        .collect();
    // Padding to clear HIGH_FAN_IN_MIN_GRAPH_NODES while keeping the mean low.
    for i in 0..15 {
        nodes.push(GraphNode {
            id: format!("src/pad_{i:02}.rs"),
            symbols: vec![],
        });
    }
    let mut edges = Vec::new();
    for hub in 0..5 {
        for importer in 0..3 {
            edges.push(GraphEdge {
                source: format!("src/importer_{hub:02}_{importer}.rs"),
                target: format!("src/leaf_{hub:02}.rs"),
                edge_type: "imports".into(),
                detail: None,
            });
        }
    }
    // importer_*_* nodes must exist for the edge to count toward the
    // target's inbound degree.
    for hub in 0..5 {
        for importer in 0..3 {
            nodes.push(GraphNode {
                id: format!("src/importer_{hub:02}_{importer}.rs"),
                symbols: vec![],
            });
        }
    }
    let graph = FileGraph { nodes, edges };
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan_findings: Vec<_> = b
        .build()
        .into_iter()
        .filter(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .collect();
    assert_eq!(
        fan_findings.len(),
        1,
        "aggregate must emit exactly one finding; got {}",
        fan_findings.len()
    );
    let fan = &fan_findings[0];
    assert_eq!(
        fan.scope.paths.len(),
        5,
        "all 5 high-fan-in paths must appear in scope.paths"
    );
    // No cap → no FAN_IN_FINDINGS_CAPPED limitation.
    let codes: Vec<&str> = fan.limitations.iter().map(|l| l.code.as_str()).collect();
    assert!(
        !codes.contains(&"FAN_IN_FINDINGS_CAPPED"),
        "no cap should fire below the cap threshold; got {codes:?}"
    );
}

/// Mixed indicator + non-indicator paths: the aggregate finding is
/// Actionable / Medium, but indicator-tagged hub modules are filtered
/// out of `scope.paths` so the response only carries the non-indicator
/// signal. The filter disclosure surfaces via a limitation note carrying
/// the indicator-filtered count.
#[test]
fn high_fan_in_mixed_indicator_filters_indicator_paths_from_scope() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs", "src/leaf.rs"]);
    // Two hubs above threshold: lib.rs (indicator) and a leaf (non-indicator).
    let mut nodes: Vec<GraphNode> = vec![
        GraphNode {
            id: "src/lib.rs".into(),
            symbols: vec![],
        },
        GraphNode {
            id: "src/some_leaf.rs".into(),
            symbols: vec![],
        },
    ];
    for i in 0..15 {
        nodes.push(GraphNode {
            id: format!("src/pad_{i:02}.rs"),
            symbols: vec![],
        });
    }
    let mut edges = Vec::new();
    for importer in 0..3 {
        edges.push(GraphEdge {
            source: format!("src/imp_lib_{importer}.rs"),
            target: "src/lib.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        });
        edges.push(GraphEdge {
            source: format!("src/imp_leaf_{importer}.rs"),
            target: "src/some_leaf.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        });
    }
    for importer in 0..3 {
        nodes.push(GraphNode {
            id: format!("src/imp_lib_{importer}.rs"),
            symbols: vec![],
        });
        nodes.push(GraphNode {
            id: format!("src/imp_leaf_{importer}.rs"),
            symbols: vec![],
        });
    }
    let graph = FileGraph { nodes, edges };
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("aggregate fan-in finding");
    assert_eq!(
        fan.classification,
        FindingClassification::Actionable,
        "non-indicator path remains → Actionable"
    );
    assert_eq!(fan.severity, FindingSeverity::Medium);
    assert!(
        !fan.scope.paths.contains(&"src/lib.rs".to_string()),
        "indicator path must be filtered out of scope.paths: {:?}",
        fan.scope.paths
    );
    assert!(
        fan.scope.paths.contains(&"src/some_leaf.rs".to_string()),
        "non-indicator path must appear in scope.paths: {:?}",
        fan.scope.paths
    );
    let depends: Vec<&String> = fan.inference.depends_on.iter().collect();
    assert!(
        depends
            .iter()
            .any(|d| d.as_str() == "filter:nonIndicatorModule"),
        "depends_on must record filter:nonIndicatorModule: {depends:?}"
    );
    // The filter disclosure should appear among the limitations as a
    // FAN_IN_THRESHOLD_HEURISTIC note citing the filtered count.
    let limit_details: Vec<&String> = fan.limitations.iter().map(|l| &l.detail).collect();
    assert!(
        limit_details
            .iter()
            .any(|d| d.contains("indicator-tagged hub module")),
        "filter disclosure must surface as a limitation: {limit_details:?}"
    );
}

/// Absolute-minimum-inbound-edges guard. On a sparse graph where the
/// mean inbound is well below 1, the mean-multiplier alone would flag
/// any file with 2+ inbound edges. The absolute floor of 3 inbound
/// prevents that.
#[test]
fn high_fan_in_suppressed_when_inbound_below_absolute_floor() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/hub.rs"]);
    // 1 hub + 15 leaves = 16 nodes (above min). Only two leaves import
    // the hub → inbound=2 on the hub, below the absolute floor.
    let mut graph = fan_in_test_graph("src/hub.rs", 15);
    graph.edges.truncate(2);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let findings = b.build();
    assert!(
        !findings.iter().any(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE),
        "F-HIGH-FAN-IN-FILE must not fire when hub has only 2 inbound edges"
    );
}

// -- 5. F-CARGO-LOCK-STATE ----------------------------------------------

#[test]
fn cargo_lock_states_dispatch_table() {
    let profile = rust_library_profile(vec![]);
    let lib_ctx = rust_lib_ctx(&profile, &["Cargo.toml", "src/lib.rs"]);

    let absent = CargoLockState::AbsentFromRepo;
    let f_lib = builder(&profile, &lib_ctx, None, None, None, Some(&absent))
        .cargo_lock()
        .expect("library + absent → finding");
    assert_eq!(f_lib.classification, FindingClassification::Informational);
    assert_eq!(f_lib.severity, FindingSeverity::None);

    // Binary repo: absent → Actionable.
    let mut bin_profile = profile.clone();
    bin_profile.project_type = ProjectType::Cli;
    bin_profile.entry_points = vec![EntryPoint {
        entry_type: "binary".into(),
        path: "src/main.rs".into(),
        description: "main".into(),
    }];
    let bin_ctx = RepoContextDetector::detect(&bin_profile, &paths(&["Cargo.toml", "src/main.rs"]));
    let f_bin = builder(&bin_profile, &bin_ctx, None, None, None, Some(&absent))
        .cargo_lock()
        .expect("binary + absent → finding");
    assert_eq!(f_bin.classification, FindingClassification::Actionable);
    assert_eq!(f_bin.severity, FindingSeverity::Low);

    // Committed-and-tracked → NoAction.
    let committed = CargoLockState::CommittedAndTracked {
        path: "Cargo.lock".into(),
    };
    let f_comm = builder(&profile, &lib_ctx, None, None, None, Some(&committed))
        .cargo_lock()
        .expect("committed → finding");
    assert!(matches!(f_comm.action, RecommendedAction::NoAction));

    // Tracking-unknown → still Informational, NoAction.
    let unknown = CargoLockState::PresentTrackingUnknown {
        path: "Cargo.lock".into(),
    };
    let f_unk = builder(&profile, &lib_ctx, None, None, None, Some(&unknown))
        .cargo_lock()
        .expect("present-tracking-unknown → finding");
    assert_eq!(f_unk.classification, FindingClassification::Informational);
    // Limitation must mention partial tracking inspection.
    assert!(f_unk
        .limitations
        .iter()
        .any(|l| l.code == limitation_codes::LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL));
}

#[test]
fn cargo_lock_finding_skipped_for_non_rust_repos() {
    let mut profile = rust_library_profile(vec![]);
    profile.build_system = Some(BuildSystem {
        name: "maven".into(),
        details: None,
        config_file: "pom.xml".into(),
    });
    let ctx = RepoContextDetector::detect(&profile, &paths(&["pom.xml"]));
    let absent = CargoLockState::AbsentFromRepo;
    let f = builder(&profile, &ctx, None, None, None, Some(&absent)).cargo_lock();
    assert!(f.is_none());
}

// -- 6. F-TEST-INVENTORY-RATIO -----------------------------------------

#[test]
fn test_inventory_uses_calibrated_wording_and_inline_caveat() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 10,
        loc: 1000,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 50,
        included_count: 50,
        total_loc: 5000,
        test_file_count: 1,
        source_file_count: 40,
        test_to_source_ratio: 0.025, // < 0.1 threshold
    };
    let b = builder(&profile, &ctx, None, None, Some(&scan), None);
    let findings = b.build();
    let f = findings
        .iter()
        .find(|f| f.id.as_str() == F_TEST_INVENTORY_RATIO)
        .expect("test-inventory finding");
    // No "coverage" wording in title or inference.
    assert!(!f.title.to_ascii_lowercase().contains("coverage"));
    assert!(!f
        .inference
        .statement
        .to_ascii_lowercase()
        .contains("coverage"));
    assert!(f.title.to_ascii_lowercase().contains("test-file ratio"));
    // Both inline-tests and test-pass-fail limitations present.
    assert!(f
        .limitations
        .iter()
        .any(|l| l.code == limitation_codes::INLINE_RUST_TESTS_NOT_COUNTED));
    assert!(f
        .limitations
        .iter()
        .any(|l| l.code == limitation_codes::TEST_PASS_FAIL_NOT_RUN));
    // does_not_imply must explicitly disclaim coverage.
    assert!(f
        .inference
        .does_not_imply
        .iter()
        .any(|s| s.to_ascii_lowercase().contains("coverage")));
}

#[test]
fn test_inventory_skipped_for_tiny_repos() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 4,
        included_count: 4,
        total_loc: 100,
        test_file_count: 0,
        source_file_count: 4,
        test_to_source_ratio: 0.0,
    };
    let b = builder(&profile, &ctx, None, None, Some(&scan), None);
    let findings = b.build();
    assert!(findings
        .iter()
        .all(|f| f.id.as_str() != F_TEST_INVENTORY_RATIO));
}

// -- F-DEPENDENCY-CYCLES (Phase 4B Step 2, 2026-05-12) ------------------

fn cycle_gh(cycle_count: usize, largest: usize) -> GraphHealthInput {
    GraphHealthInput {
        cycle_count,
        largest_cycle_size: largest,
        files_in_cycles_count: cycle_count * 2,
        orphan_count: 0,
        orphan_files: vec![],
        test_to_source_ratio: 0.3,
    }
}

#[test]
fn dependency_cycles_finding_skipped_when_zero_cycles() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 1,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let gh = cycle_gh(0, 0);
    let b = builder(&profile, &ctx, None, Some(&gh), None, None);
    let findings = b.build();
    assert!(
        findings
            .iter()
            .all(|f| f.id.as_str() != F_DEPENDENCY_CYCLES),
        "no cycles → no finding"
    );
}

#[test]
fn dependency_cycles_finding_fires_with_low_severity_for_small_count() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 1,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 1 cycle, largest 2 — low severity per cycle_severity rules.
    let gh = cycle_gh(1, 2);
    let b = builder(&profile, &ctx, None, Some(&gh), None, None);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DEPENDENCY_CYCLES)
        .expect("dependency-cycles finding must fire when cycle_count > 0");
    assert_eq!(f.severity, FindingSeverity::Low);
    assert_eq!(f.classification, FindingClassification::Actionable);
    assert_eq!(f.knowledge_layer, KnowledgeLayer::Heuristic);
    assert_eq!(f.evidence_grade, EvidenceGrade::Derived);
}

#[test]
fn dependency_cycles_finding_escalates_to_high_for_many_cycles() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 1,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 7 cycles → high severity (>= 6).
    let gh = cycle_gh(7, 3);
    let b = builder(&profile, &ctx, None, Some(&gh), None, None);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DEPENDENCY_CYCLES)
        .expect("dependency-cycles finding must fire");
    assert_eq!(f.severity, FindingSeverity::High);
    // does_not_imply must disclaim compile/runtime failure — this is a
    // structural-only signal.
    let nots = &f.inference.does_not_imply;
    assert!(
        nots.iter()
            .any(|s| s.to_ascii_lowercase().contains("compile")),
        "expected does_not_imply to disclaim compile failure; got {nots:?}"
    );
    assert!(
        nots.iter()
            .any(|s| s.to_ascii_lowercase().contains("runtime")),
        "expected does_not_imply to disclaim runtime; got {nots:?}"
    );
    // Limitation must call out import-graph structural-only scope.
    assert!(f
        .limitations
        .iter()
        .any(|l| l.code == limitation_codes::IMPORT_GRAPH_STRUCTURAL_ONLY));
}

/// N4 calibration (2026-05-14): when the full graph is available, the
/// cycle finding narrows `scope.languages` to the languages of files
/// actually in cycles, not every language scanned in the repo.
#[test]
fn dependency_cycles_scope_languages_narrows_to_files_in_cycles() {
    // Profile that distributes languages broadly so the fallback would
    // otherwise emit a wide list.
    let mut profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 4,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    profile.language_distribution = BTreeMap::from([
        ("Rust".into(), 4),
        ("Markdown".into(), 8),
        ("YAML".into(), 3),
        ("JSON".into(), 2),
    ]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // Build a graph with a 2-cycle entirely in .rs files.
    let nodes = vec![
        GraphNode {
            id: "src/a.rs".into(),
            symbols: vec![],
        },
        GraphNode {
            id: "src/b.rs".into(),
            symbols: vec![],
        },
    ];
    let edges = vec![
        GraphEdge {
            source: "src/a.rs".into(),
            target: "src/b.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        },
        GraphEdge {
            source: "src/b.rs".into(),
            target: "src/a.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        },
    ];
    let graph = FileGraph { nodes, edges };
    let gh = cycle_gh(1, 2);
    let b = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DEPENDENCY_CYCLES)
        .expect("cycle finding must fire");
    // scope.languages should be ["Rust"] only — not Markdown/YAML/JSON.
    assert_eq!(
        f.scope.languages,
        vec!["Rust".to_string()],
        "scope.languages must narrow to languages of files in cycles, \
         not languages_from_profile (which would include Markdown/YAML/JSON)"
    );
}

/// Iteration 3 (2026-06-05): with the graph available, the cycle finding must
/// *name* the in-cycle files (scope.paths + a graph.cycleFiles observation) and
/// give a concrete next step — so an agent can act without a separate
/// find_cycles call. Closes the "surface lacks evidence to act" dogfood gap.
#[test]
fn dependency_cycles_names_member_files_and_gives_concrete_action() {
    let mut profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 2,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    profile.language_distribution = BTreeMap::from([("Rust".into(), 4)]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 2-file cycle: a.rs <-> b.rs.
    let nodes = vec![
        GraphNode {
            id: "src/a.rs".into(),
            symbols: vec![],
        },
        GraphNode {
            id: "src/b.rs".into(),
            symbols: vec![],
        },
    ];
    let edges = vec![
        GraphEdge {
            source: "src/a.rs".into(),
            target: "src/b.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        },
        GraphEdge {
            source: "src/b.rs".into(),
            target: "src/a.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        },
    ];
    let graph = FileGraph { nodes, edges };
    let gh = cycle_gh(1, 2);
    let f = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None)
        .dependency_cycles()
        .expect("cycle finding must fire");

    // Names the member files in scope.paths (the gap this slice closes).
    assert!(
        f.scope.paths.contains(&"src/a.rs".to_string())
            && f.scope.paths.contains(&"src/b.rs".to_string()),
        "scope.paths must name the in-cycle files; got {:?}",
        f.scope.paths
    );
    // Emits a graph.cycleFiles observation carrying the members as sourcePaths.
    let has_cycle_files = f.evidence.iter().any(|e| {
        matches!(
            e,
            EvidenceItem::Observation { metric, source_paths, .. }
                if metric == metric_names::GRAPH_CYCLE_FILES
                    && source_paths.contains(&"src/a.rs".to_string())
        )
    });
    assert!(
        has_cycle_files,
        "must emit a graph.cycleFiles observation with member sourcePaths"
    );
    assert!(metric_names::is_known(metric_names::GRAPH_CYCLE_FILES));
    // Action is now concrete (drives a plan), not the vague reviewIfMaintainerPain.
    match &f.action {
        RecommendedAction::ConcreteAction { steps } => {
            assert!(!steps.is_empty(), "concrete action must carry steps")
        }
        other => panic!("expected ConcreteAction once members are known, got {other:?}"),
    }
}

/// N4 calibration (2026-05-14): when the graph is unavailable, fall back
/// to the full language list — preserves previous behavior for callers
/// that only have the graph-health input (no FileGraph).
#[test]
fn dependency_cycles_scope_languages_fallback_when_no_graph() {
    let mut profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 1,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    profile.language_distribution = BTreeMap::from([("Rust".into(), 4), ("Markdown".into(), 8)]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let gh = cycle_gh(1, 2);
    // No graph passed.
    let b = builder(&profile, &ctx, None, Some(&gh), None, None);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DEPENDENCY_CYCLES)
        .expect("cycle finding must fire");
    // Without a graph we cannot narrow — full profile list returned.
    let mut langs = f.scope.languages.clone();
    langs.sort();
    assert_eq!(langs, vec!["Markdown".to_string(), "Rust".to_string()]);
}

#[test]
fn dependency_cycles_finding_carries_invariant_id() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 1,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let gh = cycle_gh(2, 3);
    let b = builder(&profile, &ctx, None, Some(&gh), None, None);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_DEPENDENCY_CYCLES)
        .expect("dependency-cycles finding must fire");
    let has_invariant = f.evidence.iter().any(|e| match e {
        EvidenceItem::Observation { invariant_id, .. } => invariant_id
            .as_deref()
            .map(|s| s == "I-GRAPH-CYCLES-EXCLUDE-DECLARES")
            .unwrap_or(false),
        _ => false,
    });
    assert!(
        has_invariant,
        "must reference I-GRAPH-CYCLES-EXCLUDE-DECLARES"
    );
}

// -- F-LOCKFILE-MISSING-NON-CARGO (Phase 4B Step 2, 2026-05-12) ---------
//
// Replaces the retired `no-lockfile` signal for non-Cargo build systems.
// Cargo cases are covered by F-CARGO-LOCK-STATE; this finding only fires
// when build_system != Cargo.

fn risk(
    build_system: Option<&str>,
    lockfile_present: bool,
) -> repolens_core::model::health::DependencyRisk {
    repolens_core::model::health::DependencyRisk {
        build_system: build_system.map(str::to_string),
        framework_count: 0,
        missing_version_count: 0,
        lockfile_present,
        risk_level: "low".into(),
    }
}

#[test]
fn lockfile_missing_non_cargo_fires_for_npm_without_package_lock() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &[]);
    let r = risk(Some("npm"), false);
    let b = builder_with_risk(&profile, &ctx, &r);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_LOCKFILE_MISSING_NON_CARGO)
        .expect("npm without lockfile must fire F-LOCKFILE-MISSING-NON-CARGO");
    assert_eq!(f.severity, FindingSeverity::Low);
    assert_eq!(f.classification, FindingClassification::Actionable);
    assert!(f.title.contains("npm"));
}

#[test]
fn lockfile_missing_non_cargo_skipped_for_cargo() {
    // Cargo is covered by F-CARGO-LOCK-STATE — this finding must not
    // duplicate that coverage.
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &[]);
    let r = risk(Some("Cargo"), false);
    let b = builder_with_risk(&profile, &ctx, &r);
    assert!(
        b.build()
            .iter()
            .all(|f| f.id.as_str() != F_LOCKFILE_MISSING_NON_CARGO),
        "must not duplicate F-CARGO-LOCK-STATE coverage"
    );
}

#[test]
fn lockfile_missing_non_cargo_skipped_when_lockfile_present() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &[]);
    let r = risk(Some("poetry"), true);
    let b = builder_with_risk(&profile, &ctx, &r);
    assert!(
        b.build()
            .iter()
            .all(|f| f.id.as_str() != F_LOCKFILE_MISSING_NON_CARGO),
        "must not fire when lockfile is present"
    );
}

#[test]
fn lockfile_missing_non_cargo_skipped_when_no_build_system() {
    // No build system means no expectation.
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &[]);
    let r = risk(None, false);
    let b = builder_with_risk(&profile, &ctx, &r);
    assert!(
        b.build()
            .iter()
            .all(|f| f.id.as_str() != F_LOCKFILE_MISSING_NON_CARGO),
        "must not fire when no build system detected"
    );
}

#[test]
fn lockfile_missing_non_cargo_skipped_when_no_dependency_risk_supplied() {
    // Old call sites that don't pass dependency_risk must not crash.
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &[]);
    let b = builder(&profile, &ctx, None, None, None, None);
    assert!(
        b.build()
            .iter()
            .all(|f| f.id.as_str() != F_LOCKFILE_MISSING_NON_CARGO),
        "must not fire when dependency_risk is None"
    );
}

// -- 7. Inference always distinct from action --------------------------

#[test]
fn inference_distinct_from_action_for_no_action_findings() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 1,
        loc: 100,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    for f in &findings {
        if matches!(f.action, RecommendedAction::NoAction) {
            assert!(
                !f.inference.statement.is_empty(),
                "{}: inference statement must be non-empty",
                f.id.as_str()
            );
            assert!(
                !f.inference.does_not_imply.is_empty(),
                "{}: NoAction findings must have explicit non-implications",
                f.id.as_str()
            );
        }
    }
}

// -- 8. Metric names drawn from constant table -------------------------

#[test]
fn metric_names_are_drawn_from_the_constant_table() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 5,
        loc: 1000,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 50,
        included_count: 50,
        total_loc: 5000,
        test_file_count: 1,
        source_file_count: 40,
        test_to_source_ratio: 0.025,
    };
    let absent = CargoLockState::AbsentFromRepo;
    let b = builder(&profile, &ctx, None, None, Some(&scan), Some(&absent));
    let findings = b.build();
    for f in &findings {
        for ev in &f.evidence {
            match ev {
                EvidenceItem::Observation { metric, .. } => {
                    assert!(
                        metric_names::is_known(metric),
                        "{}: unknown metric {metric}",
                        f.id.as_str()
                    );
                }
                EvidenceItem::HeuristicJudgment {
                    observed_metric, ..
                } => {
                    assert!(
                        metric_names::is_known(observed_metric),
                        "{}: unknown observed_metric {observed_metric}",
                        f.id.as_str()
                    );
                }
                EvidenceItem::ExternalSignal { .. } => {}
            }
        }
    }
}

// -- Regression tests for Codex Phase 2 review feedback --------------

/// When the orphan_files list is truncated (sample of 10 from a larger
/// set), a finding must not claim "all orphans are configs/policy" —
/// inference depends_on must record sampleComplete=false.
#[test]
fn truncated_orphan_sample_does_not_claim_all_configs() {
    let profile = rust_library_profile(vec![ModuleInfo {
        name: "src".into(),
        path: "src".into(),
        file_count: 100,
        loc: 5000,
        primary_language: "rust".into(),
    }]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 25 orphans total but graph_health_input only carries 10 paths
    // (truncation done by build_graph_health_input). All 10 happen to
    // be configs/policy; the unsampled 15 remain unknown.
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 25,
        orphan_files: vec![
            "Cargo.toml".into(),
            "LICENSE".into(),
            "README.md".into(),
            ".gitignore".into(),
            "CHANGELOG.md".into(),
            "CONTRIBUTING.md".into(),
            "docs/a.md".into(),
            "docs/b.md".into(),
            ".github/workflows/ci.yml".into(),
            ".dockerignore".into(),
        ],
        test_to_source_ratio: 0.2,
    };
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 200,
        included_count: 200,
        total_loc: 20000,
        test_file_count: 20,
        source_file_count: 180,
        test_to_source_ratio: 0.11,
    };
    let findings = builder(&profile, &ctx, None, Some(&gh), Some(&scan), None).build();
    let f = findings
        .iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    assert!(
        !f.title.contains("No unused files found"),
        "must not claim all-configs from a truncated sample, got: {}",
        f.title
    );
    // depends_on must record sampleComplete=false
    assert!(
        f.inference
            .depends_on
            .iter()
            .any(|s| s == "sampleComplete=false"),
        "inference.depends_on must include sampleComplete=false; got {:?}",
        f.inference.depends_on
    );
    // #20: plain title and statement, counts filled in.
    assert_eq!(
        f.title,
        "No unused files in the sample; the rest were not checked (imports only)"
    );
    assert_eq!(
        f.inference.statement,
        "25 files are not imported by any other file. This scan lists 10 of them, \
         and all 10 are files that tools load by name or that aren't code (tests, \
         build scripts, configs, docs, examples, fixtures, snapshots, crate-root \
         lib.rs). The other 15 were not listed, so some of them may be unused \
         source files."
    );
    assert!(format!("### {}", f.title).chars().count() <= 80);
}

/// When the orphan sample is truncated AND every sampled path is a
/// config/policy artefact, we cannot claim "all orphans are configs"
/// (the unsampled remainder is unknown) and we must NOT title the
/// finding "Orphan source files detected" — no source orphan was
/// observed.
#[test]
fn truncated_orphan_sample_with_only_configs_does_not_claim_source_orphans() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // 25 orphans total but the sample of 10 is entirely configs/docs.
    // The unsampled 15 may or may not be source files.
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 25,
        orphan_files: vec![
            "Cargo.toml".into(),
            "LICENSE".into(),
            "README.md".into(),
            ".gitignore".into(),
            "CHANGELOG.md".into(),
            "CONTRIBUTING.md".into(),
            "docs/a.md".into(),
            "docs/b.md".into(),
            ".github/workflows/ci.yml".into(),
            ".dockerignore".into(),
        ],
        test_to_source_ratio: 0.0,
    };
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 200,
        included_count: 200,
        total_loc: 20000,
        test_file_count: 0,
        source_file_count: 200,
        test_to_source_ratio: 0.0,
    };
    let f = builder(&profile, &ctx, None, Some(&gh), Some(&scan), None)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    // Must not claim source orphans.
    assert!(
        f.title != ORPHAN_TITLE,
        "must not claim source orphans from a config-only truncated sample, got: {}",
        f.title
    );
    // Inference must record sampleComplete=false.
    assert!(
        f.inference
            .depends_on
            .iter()
            .any(|s| s == "sampleComplete=false"),
        "inference.depends_on must record sampleComplete=false; got {:?}",
        f.inference.depends_on
    );
    // Severity stays None — no source-orphan observation to act on.
    assert_eq!(f.severity, FindingSeverity::None);
}

/// When the truncated `gh.orphan_files` sample contains only configs
/// but the full graph holds real source orphans, the finding must
/// reflect the source orphans (defer to `full_filtered_count`) rather
/// than fall into the "config-only" branch.
#[test]
fn config_only_sample_does_not_hide_source_orphans_when_full_graph_present() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    // Construct a graph with real source orphans and unrelated nodes
    // that have inbound edges. Three source orphans (no inbound edges)
    // plus root configs/policy paths that show up as graph nodes (also
    // no inbound edges by construction).
    let graph = FileGraph {
        nodes: vec![
            GraphNode {
                id: "src/orphan_a.rs".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "src/orphan_b.rs".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "src/orphan_c.rs".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "Cargo.toml".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "LICENSE".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "README.md".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "src/lib.rs".into(),
                symbols: vec![],
            },
            GraphNode {
                id: "src/foo.rs".into(),
                symbols: vec![],
            },
        ],
        edges: vec![GraphEdge {
            source: "src/lib.rs".into(),
            target: "src/foo.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        }],
    };
    // graph_health sample (truncated to 10 by build_graph_health_input
    // in production; here we simulate by passing only the configs/docs
    // first to mimic a sort-truncate that hid source orphans).
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        // Pretend the full count is 6 (3 source + 3 config) but the
        // sample only carries the 3 configs (sort by name puts
        // Cargo.toml/LICENSE/README first lexicographically before
        // src/orphan_*).
        orphan_count: 6,
        orphan_files: vec!["Cargo.toml".into(), "LICENSE".into(), "README.md".into()],
        test_to_source_ratio: 0.0,
    };
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 100,
        included_count: 100,
        total_loc: 5000,
        test_file_count: 10,
        source_file_count: 90,
        test_to_source_ratio: 0.11,
    };
    let f = builder(&profile, &ctx, Some(&graph), Some(&gh), Some(&scan), None)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    // Because the full-graph recomputation found 3 source orphans, we
    // must NOT report the config-only branch.
    assert!(
        !f.title.contains("No unused files found")
            && !f.title.contains("No unused files in the sample"),
        "must not hide source orphans behind a config-only sample, got: {}",
        f.title
    );
    assert!(
        f.title == ORPHAN_TITLE,
        "expected source-orphan finding, got: {}",
        f.title
    );
}

/// Orphan ratio must use `included_count` as the denominator, not
/// the total `file_count`. With many excluded entries, using the
/// total dilutes the ratio and can suppress an Actionable
/// classification.
#[test]
fn orphan_ratio_denominator_uses_included_count_not_total() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 6,
        // Source paths so the finding takes the regular branch.
        orphan_files: vec![
            "src/o1.rs".into(),
            "src/o2.rs".into(),
            "src/o3.rs".into(),
            "src/o4.rs".into(),
            "src/o5.rs".into(),
            "src/o6.rs".into(),
        ],
        test_to_source_ratio: 0.2,
    };
    // 1000 total files, but only 50 included. 6/1000 = 0.6% (below 5%
    // threshold) — but 6/50 = 12% (above threshold).
    let scan = ScanSummary {
        mode: "full".into(),
        file_count: 1000,
        included_count: 50,
        total_loc: 5000,
        test_file_count: 10,
        source_file_count: 40,
        test_to_source_ratio: 0.25,
    };
    let f = builder(&profile, &ctx, None, Some(&gh), Some(&scan), None)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    assert_eq!(
        f.classification,
        FindingClassification::Actionable,
        "orphan ratio (6/included=50 = 12%) must exceed threshold; \
         do not use total file_count (6/1000) as the denominator"
    );
}

/// A library workspace with absent Cargo.lock is the workspace's
/// problem (workspaces conventionally commit Cargo.lock), not the
/// library default. Workspace path must take precedence.
#[test]
fn library_workspace_absent_lockfile_is_informational() {
    let profile = rust_library_profile(vec![]);
    // RepoContextDetector emits BOTH RustWorkspace and RustLibrary for
    // a virtual library workspace.
    let ctx = RepoContext {
        tags: vec![RepoContextTag::RustWorkspace, RepoContextTag::RustLibrary],
        notes: vec![],
    };
    let absent = CargoLockState::AbsentFromRepo;
    let f = builder(&profile, &ctx, None, None, None, Some(&absent))
        .cargo_lock()
        .expect("cargo-lock finding");
    // #16: a library-only workspace conventionally leaves Cargo.lock out.
    assert_eq!(f.classification, FindingClassification::Informational);
    assert_eq!(f.severity, FindingSeverity::None);
    assert_eq!(f.title, "Cargo.lock absent (library-only workspace)");
    assert!(!f.title.contains("binary or workspace"));
}

#[test]
fn binary_workspace_absent_lockfile_names_the_reason() {
    let profile = rust_library_profile(vec![]);
    let ctx = RepoContext {
        tags: vec![RepoContextTag::RustWorkspace, RepoContextTag::RustBinary],
        notes: vec![],
    };
    let absent = CargoLockState::AbsentFromRepo;
    let f = builder(&profile, &ctx, None, None, None, Some(&absent))
        .cargo_lock()
        .expect("cargo-lock finding");
    assert_eq!(f.classification, FindingClassification::Actionable);
    assert_eq!(f.title, "Cargo.lock absent (has a binary target)");
    let ignored = CargoLockState::PresentButGitIgnored {
        path: "Cargo.lock".into(),
    };
    let lib_ctx = RepoContext {
        tags: vec![RepoContextTag::RustWorkspace, RepoContextTag::RustLibrary],
        notes: vec![],
    };
    let f = builder(&profile, &lib_ctx, None, None, None, Some(&ignored))
        .cargo_lock()
        .expect("cargo-lock finding");
    assert_eq!(f.classification, FindingClassification::Informational);
}

// -- F-LANGUAGE-EXTRACTION-GAP ------------------------------------------

/// Build a profile with a given language→LOC distribution. Keys are the
/// capitalized names `lang::detect` produces (e.g. "Python", "Shell"), which
/// is what a real scan writes into `language_distribution`.
fn profile_with_languages(dist: &[(&str, i32)]) -> ProjectProfile {
    let mut p = rust_library_profile(vec![]);
    p.language_distribution = dist.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    p
}

#[test]
fn language_extraction_gap_lists_unsupported_and_notes_python() {
    let profile = profile_with_languages(&[
        ("Rust", 5000),     // Ast — fully extracted
        ("Python", 200),    // Heuristic — imports read, cross-file calls/types not
        ("Shell", 1900),    // Unsupported — no extractor
        ("Ruby", 50),       // Unsupported — no extractor
        ("Markdown", 8000), // NotApplicable
        ("JSON", 100),      // NotApplicable
    ]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    let f = findings
        .iter()
        .find(|f| f.id.as_str() == F_LANGUAGE_EXTRACTION_GAP)
        .expect("F-LANGUAGE-EXTRACTION-GAP must fire when unsupported/regex langs are present");

    assert_eq!(f.classification, FindingClassification::Informational);
    assert_eq!(f.severity, FindingSeverity::Low);
    assert_eq!(f.knowledge_layer, KnowledgeLayer::Observation);

    // Observation-shaped title — no forbidden tokens (mirrors the e2e hygiene
    // check, which would otherwise reject the title if the finding ever fired
    // on its fixture).
    let title_lc = f.title.to_ascii_lowercase();
    for tok in ["coverage", "fitness", "quality", "readiness", "risk level"] {
        assert!(
            !title_lc.contains(tok),
            "title has forbidden token '{tok}': {}",
            f.title
        );
    }

    // Unsupported and regex (Python) languages are scoped; AST and
    // not-applicable languages are excluded.
    for lang in ["Python", "Shell", "Ruby"] {
        assert!(
            f.scope.languages.contains(&lang.to_string()),
            "scope.languages should contain {lang}"
        );
    }
    for lang in ["Rust", "Markdown", "JSON"] {
        assert!(
            !f.scope.languages.contains(&lang.to_string()),
            "scope.languages should not contain {lang}"
        );
    }
    assert!(f.title.contains("(3 language(s), 2150 LOC)"), "{}", f.title);
    // Python is never described as unread: only its note mentions it.
    assert!(
        f.inference
            .statement
            .contains("RepoLens extracts Python with regex only (200 LOC): imports are read"),
        "{}",
        f.inference.statement
    );
    assert!(
        !f.inference.statement.contains("Python)"),
        "Python must not be in the no-extractor list: {}",
        f.inference.statement
    );
    // The CLI line ("Imports aren't read for ...") is built from the
    // unsupported tier only; the heuristic tier carries Python's note.
    let breakdown = f
        .evidence
        .iter()
        .find_map(|ev| match ev {
            EvidenceItem::Observation {
                metric,
                observed_value,
                ..
            } if metric == metric_names::EXTRACTION_LANGUAGES_BY_FIDELITY => {
                Some(observed_value.clone())
            }
            _ => None,
        })
        .expect("languagesByFidelity observation");
    assert_eq!(
        breakdown["heuristic"],
        serde_json::json!({
            "languages": ["Python"],
            "loc": 200,
            "notes": ["Python: imports read; cross-file calls and types not yet"]
        })
    );
    assert_eq!(
        breakdown["unsupported"]["languages"],
        serde_json::json!(["Ruby", "Shell"])
    );

    // Metric emitted and registered in the constant table.
    let mut saw_metric = false;
    for ev in &f.evidence {
        if let EvidenceItem::Observation { metric, .. } = ev {
            assert!(metric_names::is_known(metric), "unknown metric {metric}");
            if metric == metric_names::EXTRACTION_LANGUAGES_BY_FIDELITY {
                saw_metric = true;
            }
        }
    }
    assert!(saw_metric, "must emit the languagesByFidelity observation");

    // Concrete, non-empty action so the finding can drive a plan.
    match &f.action {
        RecommendedAction::ConcreteAction { steps } => {
            assert!(!steps.is_empty(), "action steps must be non-empty")
        }
        other => panic!("expected ConcreteAction, got {other:?}"),
    }
    assert!(!f.inference.statement.is_empty());
}

#[test]
fn language_extraction_gap_silent_for_fully_supported_repo() {
    // Only AST languages + non-code → no gap, no finding.
    let profile = profile_with_languages(&[
        ("Rust", 5000),
        ("Go", 300),
        ("TypeScript", 200),
        ("Java", 120),
        ("JavaScript", 80),
        ("Markdown", 8000),
        ("JSON", 100),
        ("YAML", 50),
    ]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let b = builder(&profile, &ctx, None, None, None, None);
    let findings = b.build();
    assert!(
        findings
            .iter()
            .all(|f| f.id.as_str() != F_LANGUAGE_EXTRACTION_GAP),
        "no extraction-gap finding when every source language is AST-extracted"
    );
}

fn graph_with_nodes(ids: &[&str]) -> FileGraph {
    FileGraph {
        nodes: ids
            .iter()
            .map(|id| GraphNode {
                id: id.to_string(),
                symbols: vec![],
            })
            .collect(),
        edges: vec![],
    }
}

/// A Python repo gets the finding with Python's note (imports read, calls and
/// types between files not yet) and no "imports aren't read" tier.
#[test]
fn language_extraction_gap_notes_python_in_multi_file_python_repo() {
    let profile = profile_with_languages(&[("Python", 22127), ("Markdown", 900), ("TOML", 40)]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let g = graph_with_nodes(&["pkg/__init__.py", "pkg/a.py", "README.md"]);
    let b = builder(&profile, &ctx, Some(&g), None, None, None);
    let findings = b.build();
    let f = findings
        .iter()
        .find(|f| f.id.as_str() == F_LANGUAGE_EXTRACTION_GAP)
        .expect("Python with several files: finding with the partial-extraction note");
    assert_eq!(f.scope.languages, vec!["Python".to_string()]);
    assert_eq!(
        f.inference.statement,
        "RepoLens extracts Python with regex only (22127 LOC): imports are read and \
         resolved; calls and type references between files are not extracted yet."
    );
    let breakdown = f
        .evidence
        .iter()
        .find_map(|ev| match ev {
            EvidenceItem::Observation {
                metric,
                observed_value,
                ..
            } if metric == metric_names::EXTRACTION_LANGUAGES_BY_FIDELITY => {
                Some(observed_value.clone())
            }
            _ => None,
        })
        .expect("languagesByFidelity observation");
    assert!(breakdown.get("unsupported").is_none(), "{breakdown}");
    assert_eq!(
        breakdown["heuristic"]["notes"],
        serde_json::json!(["Python: imports read; cross-file calls and types not yet"])
    );
}

/// A single Python file has no cross-file calls or types to miss, so a repo
/// whose only partly-extracted source is one script (gm-crypto-web's
/// `ref_sm2enc.py`) gets no finding.
#[test]
fn language_extraction_gap_silent_for_single_python_file() {
    let profile = profile_with_languages(&[("Rust", 700), ("JavaScript", 49), ("Python", 21)]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let g = graph_with_nodes(&["src/lib.rs", "app.js", "scripts/ref.py"]);
    let b = builder(&profile, &ctx, Some(&g), None, None, None);
    let findings = b.build();
    assert!(
        findings
            .iter()
            .all(|f| f.id.as_str() != F_LANGUAGE_EXTRACTION_GAP),
        "one Python file: no extraction-gap finding expected"
    );
}

/// #12: files loaded by `<script src>` / run by CI (passed in as
/// `referenced_files`) and test fixtures are not reported as unused.
#[test]
fn referenced_and_fixture_files_are_not_reported_unused() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let node = |id: &str| GraphNode {
        id: id.into(),
        symbols: vec![],
    };
    let graph = FileGraph {
        nodes: vec![
            node("assets/js/theme.js"),
            node("pw/run.cjs"),
            node("fixtures/inputs/rust-small/src/main.rs"),
            node("fixtures/scenarios/js/app.js"),
        ],
        edges: vec![],
    };
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 4,
        orphan_files: vec![
            "assets/js/theme.js".into(),
            "fixtures/inputs/rust-small/src/main.rs".into(),
            "fixtures/scenarios/js/app.js".into(),
            "pw/run.cjs".into(),
        ],
        test_to_source_ratio: 0.0,
    };
    let referenced: std::collections::HashSet<String> =
        ["assets/js/theme.js".to_string(), "pw/run.cjs".to_string()]
            .into_iter()
            .collect();
    let mut b = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None);
    b.referenced_files = Some(&referenced);
    let f = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES);
    if let Some(f) = f {
        assert_ne!(f.title, ORPHAN_TITLE, "nothing should be flagged unused");
        assert_eq!(f.severity, FindingSeverity::None);
    }
}

/// #16: gm-crypto-rs shape. Repo-wide tags say RustBinary (Python scripts,
/// a separate fuzz workspace), but the root workspace has no binary target.
#[test]
fn root_workspace_without_binary_overrides_repo_wide_binary_tag() {
    let profile = rust_library_profile(vec![]);
    let ctx = RepoContext {
        tags: vec![RepoContextTag::RustWorkspace, RepoContextTag::RustBinary],
        notes: vec![],
    };
    let absent = CargoLockState::AbsentFromRepo;
    let mut b = builder(&profile, &ctx, None, None, None, Some(&absent));
    b.cargo_has_binary = Some(false);
    let f = b.cargo_lock().expect("finding");
    assert_eq!(f.classification, FindingClassification::Informational);
    assert_eq!(f.title, "Cargo.lock absent (library-only workspace)");
}

/// Graph for the #31 fan-in cases: a `crates/<core>` crate whose
/// `src/curve.rs` is imported by four files, plus a `crates/<other>` crate.
/// `core_files` / `other_files` set how many code files each crate holds,
/// so a test can make either crate the largest.
fn shared_crate_graph(core: &str, core_files: usize, other_files: usize) -> FileGraph {
    let mut ids: Vec<String> = vec![
        "Cargo.toml".into(),
        format!("crates/{core}/Cargo.toml"),
        format!("crates/{core}/src/curve.rs"),
        "crates/app/Cargo.toml".into(),
    ];
    for i in 0..core_files {
        ids.push(format!("crates/{core}/src/m{i:02}.rs"));
    }
    for i in 0..other_files {
        ids.push(format!("crates/app/src/a{i:02}.rs"));
    }
    let edges = (0..4)
        .map(|i| GraphEdge {
            source: format!("crates/{core}/src/m{i:02}.rs"),
            target: format!("crates/{core}/src/curve.rs"),
            edge_type: "imports".into(),
            detail: None,
        })
        .collect();
    let nodes = ids
        .into_iter()
        .map(|id| GraphNode {
            id,
            symbols: vec![],
        })
        .collect();
    FileGraph { nodes, edges }
}

/// #31 case 2 (gm-crypto-rs): when the `*-core` crate is the repo's main
/// (largest) crate, its widely imported files are listed, not dropped as
/// "indicator hubs".
#[test]
fn high_fan_in_lists_files_in_the_main_core_crate() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["crates/gmcrypto-core/src/curve.rs"]);
    let graph = shared_crate_graph("gmcrypto-core", 12, 3);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding for the main crate's hub file");
    assert!(
        fan.scope
            .paths
            .contains(&"crates/gmcrypto-core/src/curve.rs".to_string()),
        "{:?}",
        fan.scope.paths
    );
}

/// #31 case 2: a `*-core` crate that is not the main crate is still left
/// out, but the finding says so (count and example) instead of vanishing.
#[test]
fn high_fan_in_says_when_shared_crate_files_were_left_out() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["crates/app/src/a00.rs"]);
    let mut graph = shared_crate_graph("repo-core", 6, 20);
    // A second, non-core hub so the finding still fires.
    for i in 0..4 {
        graph.edges.push(GraphEdge {
            source: format!("crates/app/src/a{:02}.rs", i + 1),
            target: "crates/app/src/a00.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        });
    }
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding");
    assert!(!fan
        .scope
        .paths
        .contains(&"crates/repo-core/src/curve.rs".to_string()));
    let obs = fan
        .evidence
        .iter()
        .find_map(|e| match e {
            EvidenceItem::Observation {
                metric,
                observed_value,
                ..
            } if metric == metric_names::GRAPH_INBOUND_EDGES => Some(observed_value.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(obs["sharedCrateFilesLeftOut"]["count"], 1, "{obs}");
    assert_eq!(
        obs["sharedCrateFilesLeftOut"]["sample"][0]["path"],
        "crates/repo-core/src/curve.rs"
    );
    assert!(
        fan.limitations
            .iter()
            .any(|l| l.detail.contains("*-core") && l.detail.contains("left out")),
        "{:?}",
        fan.limitations
    );
}

/// #26 (evidence wording): when the full graph lists every source orphan,
/// `inference.dependsOn` must say `sampleComplete=true`, matching the
/// `graph.orphanFiles` evidence, even though the 10-path graph-health
/// sample was truncated.
#[test]
fn orphan_depends_on_agrees_with_listed_sample_completeness() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let graph = graph_with_nodes(&[
        "src/orphan_a.rs",
        "src/orphan_b.rs",
        "Cargo.toml",
        "LICENSE",
        "README.md",
    ]);
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 5,
        orphan_files: vec!["Cargo.toml".into(), "LICENSE".into()],
        test_to_source_ratio: 0.0,
    };
    let f = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    let listed_complete = f
        .evidence
        .iter()
        .find_map(|e| match e {
            EvidenceItem::Observation {
                metric,
                observed_value,
                ..
            } if metric == metric_names::GRAPH_ORPHAN_FILES => {
                observed_value["sampleComplete"].as_bool()
            }
            _ => None,
        })
        .expect("orphanFiles evidence");
    assert!(listed_complete);
    assert!(
        f.inference
            .depends_on
            .iter()
            .any(|d| d == "sampleComplete=true"),
        "{:?}",
        f.inference.depends_on
    );
    assert!(!f
        .inference
        .depends_on
        .iter()
        .any(|d| d == "sampleComplete=false"));
}

/// #26 (evidence wording): the fan-in judgment shows the real cutoff
/// (2 × average), not the integer it was rounded down to.
#[test]
fn high_fan_in_threshold_is_the_actual_cutoff() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/leaf.rs"]);
    // 11 nodes; 10 edges into the hub plus a 7-edge chain among leaves:
    // 17 / 11 = 1.545 average, cutoff 3.09.
    let mut graph = fan_in_test_graph("src/hub.rs", 10);
    graph.edges.clear();
    for i in 0..10 {
        graph.edges.push(GraphEdge {
            source: format!("src/leaf_{i:02}.rs"),
            target: "src/hub.rs".into(),
            edge_type: "imports".into(),
            detail: None,
        });
    }
    for i in 0..7 {
        graph.edges.push(GraphEdge {
            source: format!("src/leaf_{i:02}.rs"),
            target: format!("src/leaf_{:02}.rs", i + 1),
            edge_type: "imports".into(),
            detail: None,
        });
    }
    let fan = builder(&profile, &ctx, Some(&graph), None, None, None)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding");
    let threshold = fan
        .evidence
        .iter()
        .find_map(|e| match e {
            EvidenceItem::HeuristicJudgment { threshold, .. } => threshold.as_f64(),
            _ => None,
        })
        .expect("judgment");
    assert!((threshold - 3.09).abs() < 1e-9, "{threshold}");
}

/// #26: when the full graph was recomputed and holds no source orphans, the
/// finding covers every orphan; it must not call itself "truncated" just
/// because the 10-path graph-health sample was.
#[test]
fn full_graph_with_no_source_orphans_is_not_reported_truncated() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let graph = graph_with_nodes(&["Cargo.toml", "LICENSE", "README.md", "tests/test_a.py"]);
    let gh = GraphHealthInput {
        cycle_count: 0,
        largest_cycle_size: 0,
        files_in_cycles_count: 0,
        orphan_count: 4,
        orphan_files: vec!["Cargo.toml".into(), "LICENSE".into()],
        test_to_source_ratio: 0.0,
    };
    let f = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_ORPHAN_FILES)
        .expect("orphan finding");
    assert_eq!(f.title, "No unused files found (imports only)");
    assert!(f.inference.statement.contains("None looks unused."));
    assert!(f
        .inference
        .depends_on
        .iter()
        .any(|d| d == "sampleComplete=true"));
}

/// #20: Design's statement copy, with real singular and plural.
#[test]
fn orphan_statement_uses_design_copy_with_real_plurals() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let statement_of = |nodes: &[&str]| {
        let graph = graph_with_nodes(nodes);
        let gh = GraphHealthInput {
            cycle_count: 0,
            largest_cycle_size: 0,
            files_in_cycles_count: 0,
            orphan_count: nodes.len(),
            orphan_files: nodes.iter().map(|s| s.to_string()).collect(),
            test_to_source_ratio: 0.0,
        };
        builder(&profile, &ctx, Some(&graph), Some(&gh), None, None)
            .build()
            .into_iter()
            .find(|f| f.id.as_str() == F_ORPHAN_FILES)
            .expect("orphan finding")
            .inference
            .statement
    };
    let list = "(root manifests, policy docs, editor/agent configs, Cargo \
                test/bench/example/build-script targets, test files a test runner \
                finds by name, tool configs, example and benchmark folders, fixture \
                data, insta snapshots, crate-root lib.rs entry points)";

    let one = statement_of(&["src/unused.rs", "README.md", "Cargo.toml"]);
    assert_eq!(
        one,
        format!(
            "1 source file (of 3 graph orphans) has no inbound import edges (entry \
             points excluded), not counting files that aren't source code {list}. \
             It may still import other modules."
        )
    );

    let two = statement_of(&["src/a.rs", "src/b.rs", "README.md"]);
    assert_eq!(
        two,
        format!(
            "2 source files (of 3 graph orphans) have no inbound import edges (entry \
             points excluded), not counting files that aren't source code {list}. \
             They may still import other modules."
        )
    );
    for s in [&one, &two] {
        assert!(
            !s.contains("artefacts") && !s.contains("zero inbound"),
            "{s}"
        );
    }
}

/// Design call on #44: the files the orphan list leaves out because a
/// runner or tool loads them by name are counted in `graph.orphanCount`
/// (`loadedByNameCount`), in both the source-orphan and filtered-only
/// branches. Manifests, docs and fixtures are not part of that count.
#[test]
fn orphan_count_reports_files_loaded_by_name() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &["src/lib.rs"]);
    let count_of = |nodes: &[&str]| {
        let graph = graph_with_nodes(nodes);
        let gh = GraphHealthInput {
            cycle_count: 0,
            largest_cycle_size: 0,
            files_in_cycles_count: 0,
            orphan_count: nodes.len(),
            orphan_files: nodes.iter().take(2).map(|s| s.to_string()).collect(),
            test_to_source_ratio: 0.0,
        };
        let f = builder(&profile, &ctx, Some(&graph), Some(&gh), None, None)
            .build()
            .into_iter()
            .find(|f| f.id.as_str() == F_ORPHAN_FILES)
            .expect("orphan finding");
        f.evidence
            .iter()
            .find_map(|e| match e {
                EvidenceItem::Observation {
                    metric,
                    observed_value,
                    ..
                } if metric == metric_names::GRAPH_ORPHAN_COUNT => Some(observed_value.clone()),
                _ => None,
            })
            .expect("orphanCount")
    };
    // Filtered-only: 3 loaded by name (pytest, examples, JUnit); README,
    // Cargo.toml and fixture code are not counted.
    let v = count_of(&[
        "Cargo.toml",
        "README.md",
        "tests/test_a.py",
        "examples/scenes/simple.py",
        "src/test/java/a/FooTest.java",
        "fixtures/inputs/x/src/main.rs",
    ]);
    assert_eq!(v["sourceOrphanCount"], 0, "{v}");
    assert_eq!(v["loadedByNameCount"], 3, "{v}");
    assert_eq!(v["graphOrphanCount"], 6, "{v}");
    // With source orphans too.
    let v = count_of(&["src/unused.rs", "tests/test_a.py", "vitest.config.ts"]);
    assert_eq!(v["sourceOrphanCount"], 1, "{v}");
    assert_eq!(v["loadedByNameCount"], 2, "{v}");
}

// -- #25: lockfile advice matches the ecosystem ---------------------------

fn profile_for(build_system: &str, project_type: ProjectType) -> ProjectProfile {
    let mut p = rust_library_profile(vec![]);
    p.build_system = Some(BuildSystem {
        name: build_system.into(),
        details: None,
        config_file: "manifest".into(),
    });
    p.project_type = project_type;
    p.language_distribution = BTreeMap::new();
    p
}

fn lockfile_finding(build_system: &str, project_type: ProjectType) -> Option<EvidenceFinding> {
    lockfile_finding_with(build_system, None, project_type)
}

fn lockfile_finding_with(
    build_system: &str,
    details: Option<&str>,
    project_type: ProjectType,
) -> Option<EvidenceFinding> {
    let mut profile = profile_for(build_system, project_type);
    profile.build_system.as_mut().unwrap().details = details.map(str::to_string);
    let ctx = RepoContextDetector::detect(&profile, &paths(&[]));
    let r = risk(Some(build_system), false);
    builder_with_risk(&profile, &ctx, &r)
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_LOCKFILE_MISSING_NON_CARGO)
}

#[test]
fn lockfile_missing_skipped_for_maven_and_gradle() {
    for bs in ["Maven", "Gradle"] {
        for pt in [ProjectType::Cli, ProjectType::WebApp, ProjectType::Library] {
            assert!(
                lockfile_finding(bs, pt.clone()).is_none(),
                "{bs} has no standard lockfile"
            );
        }
    }
}

#[test]
fn lockfile_missing_skipped_for_libraries() {
    for bs in ["npm", "Python"] {
        assert!(
            lockfile_finding(bs, ProjectType::Library).is_none(),
            "{bs} library must not get a lockfile finding"
        );
    }
}

#[test]
fn lockfile_missing_lists_only_the_ecosystems_lockfiles() {
    for (tool, lock) in [
        ("poetry", "poetry.lock"),
        ("uv", "uv.lock"),
        ("pdm", "pdm.lock"),
        ("pipenv", "Pipfile.lock"),
    ] {
        let py = lockfile_finding_with("Python", Some(tool), ProjectType::Cli)
            .expect("Python app with a lock tool fires");
        let s = &py.inference.statement;
        assert!(s.contains(lock), "{s}");
        for name in ["package-lock.json", "yarn.lock", "pnpm-lock.yaml", "go.sum"] {
            assert!(!s.contains(name), "Python must not list {name}: {s}");
        }
    }

    let npm = lockfile_finding("npm", ProjectType::WebApp).expect("npm app fires");
    let s = &npm.inference.statement;
    assert!(
        s.contains("package-lock.json") && s.contains("yarn.lock"),
        "{s}"
    );
    assert!(!s.contains("poetry.lock") && !s.contains("go.sum"), "{s}");

    let go = lockfile_finding("Go", ProjectType::Cli).expect("Go app fires");
    let s = &go.inference.statement;
    assert!(
        s.contains("go.sum") && !s.contains("package-lock.json"),
        "{s}"
    );
}

#[test]
fn lockfile_missing_skipped_for_plain_pip_and_setuptools() {
    for pt in [ProjectType::Cli, ProjectType::WebApp, ProjectType::Library] {
        assert!(
            lockfile_finding_with("Python", None, pt).is_none(),
            "plain pip/setuptools has no standard lockfile"
        );
    }
}

#[test]
fn python_lockfiles_are_recognized() {
    for name in ["uv.lock", "Pipfile.lock", "poetry.lock", "pdm.lock"] {
        assert!(
            repolens_core::model::health::LOCKFILE_NAMES.contains(&name),
            "{name}"
        );
    }
}

#[test]
fn tracked_cargo_lock_is_reported_as_tracked() {
    let profile = rust_library_profile(vec![]);
    let ctx = rust_lib_ctx(&profile, &[]);
    let state = CargoLockState::CommittedAndTracked {
        path: "Cargo.lock".into(),
    };
    let f = builder(&profile, &ctx, None, None, None, Some(&state))
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_CARGO_LOCK_STATE)
        .expect("Cargo.lock finding");
    assert_eq!(f.title, "Cargo.lock is committed and tracked");
    assert!(!f.inference.statement.contains("not inspect"));
}

/// #24: a Go package's import edges land on the one file that stands for
/// it, so the fan-in entry names the package, its import path and how many
/// files import it. Non-Go entries are left as files.
#[test]
fn high_fan_in_names_the_go_package() {
    let profile = rust_library_profile(vec![]);
    let mut nodes = vec![
        GraphNode {
            id: "cobra.go".into(),
            symbols: vec![],
        },
        GraphNode {
            id: "go.mod".into(),
            symbols: vec![],
        },
    ];
    let mut edges = Vec::new();
    for i in 0..10 {
        let id = format!("doc/f_{i:02}.go");
        nodes.push(GraphNode {
            id: id.clone(),
            symbols: vec![],
        });
        if i < 3 {
            edges.push(GraphEdge {
                source: id,
                target: "cobra.go".into(),
                edge_type: "imports".into(),
                detail: Some("github.com/spf13/cobra".into()),
            });
        }
    }
    let graph = FileGraph { nodes, edges };
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let ctx = rust_lib_ctx(&profile, &ids);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding");
    let obs = fan_in_observation(&fan);
    let first = &obs["sample"][0];
    assert_eq!(first["path"], "cobra.go", "{obs}");
    assert_eq!(first["goPackage"], "cobra", "{obs}");
    assert_eq!(first["importPath"], "github.com/spf13/cobra", "{obs}");
    assert_eq!(first["importers"], 3, "{obs}");
    assert!(fan.title.starts_with("Go packages "), "{}", fan.title);
    assert!(
        fan.limitations
            .iter()
            .any(|l| l.detail.contains("stands for its whole package")),
        "{:?}",
        fan.limitations
    );

    // A Rust hub in the same shape gets no package fields.
    let graph = fan_in_test_graph("src/hub.rs", 10);
    let b = builder(&profile, &ctx, Some(&graph), None, None, None);
    let fan = b
        .build()
        .into_iter()
        .find(|f| f.id.as_str() == F_HIGH_FAN_IN_FILE)
        .expect("fan-in finding");
    let obs = fan_in_observation(&fan);
    assert!(obs["sample"][0].get("goPackage").is_none(), "{obs}");
    assert!(fan.title.starts_with("Files "), "{}", fan.title);
}
