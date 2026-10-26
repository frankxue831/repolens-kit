use std::sync::Arc;
use std::time::Instant;

use schemars::JsonSchema;
use serde::Deserialize;
use tracing::info;

use repolens_app::discover::PackResolver;
use repolens_app::evidence_findings::EvidenceFindingsBuilder;
use repolens_app::summary::read_graph;
use repolens_core::model::graph::FileGraph;
use repolens_profile::health::HealthAnalyzer;
use repolens_profile::lockfile::CargoLockStateDetector;
use repolens_profile::profile::ProfileAnalyzer;
use repolens_profile::repo_context::RepoContextDetector;
use rmcp::{model::CallToolResult, ErrorData as McpError};

use crate::epistemic::*;
use crate::err_json;
use crate::triage;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetProfileParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeHealthParams {
    #[schemars(description = "Absolute path to the pack directory (optional if root is given)")]
    pub pack: Option<String>,
    #[schemars(description = "Absolute path to the repository root")]
    pub root: Option<String>,
}

pub fn handle_get_profile(
    resolver: &Arc<PackResolver>,
    params: GetProfileParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(
        pack = ?params.pack,
        root = ?params.root,
        "get_profile: entry"
    );

    let (pack_dir, index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_profile: resolve error");
            return err_json(&e.to_string());
        }
    };

    let profile = ProfileAnalyzer::analyze(&index, &pack_dir);

    let response = match serde_json::to_value(&profile) {
        Ok(v) => v,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "get_profile: serialize error");
            return err_json(&format!("Failed to serialize profile: {}", e));
        }
    };

    let pid = pack_id_from_dir(&pack_dir);
    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        project_type = ?profile.project_type,
        "get_profile: exit"
    );
    // evidenceGrade: Derived — the observation-backed profile fields are
    // catalogue-backed, but `projectType` is a heuristic classification over
    // those signals, so this mixed-layer tool is derived rather than verified.
    ok_epistemic_graded(
        KnowledgeLayerField::Multiple(vec![KnowledgeLayer::Observation, KnowledgeLayer::Heuristic]),
        EpistemicConfidence::Codified,
        EvidenceGrade::Derived,
        ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: pid.clone(),
                source: "multiple".into(),
            },
            Provenance::HeuristicRule {
                rule: "projectProfile".into(),
                threshold: "multiple".into(),
                pack_id: Some(pid),
            },
        ]),
        response,
    )
}

pub fn handle_analyze_health(
    resolver: &Arc<PackResolver>,
    params: AnalyzeHealthParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(
        pack = ?params.pack,
        root = ?params.root,
        "analyze_health: entry"
    );

    let (pack_dir, index) = match resolver.resolve(params.pack.as_deref(), params.root.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            info!(elapsed_ms = %start.elapsed().as_millis(), error = %e, "analyze_health: resolve error");
            return err_json(&e.to_string());
        }
    };

    let profile = ProfileAnalyzer::analyze(&index, &pack_dir);

    let graph: FileGraph = read_graph(&pack_dir).unwrap_or_else(|| {
        info!("analyze_health: no graph.json found, using empty graph");
        FileGraph {
            nodes: vec![],
            edges: vec![],
        }
    });

    let graph_health = repolens_app::graph_health::build_graph_health_input_in_pack(
        &index,
        &profile,
        &graph,
        Some(&pack_dir),
    );
    let mut report = HealthAnalyzer::analyze(&index, &profile, Some(&graph_health));

    // Evidence-aware findings — emitted in parallel to legacy `signals`.
    // Only included entries should drive repo-context detection so excluded
    // vendor/fixture paths cannot add indicator tags or reclassify findings.
    let file_paths: Vec<String> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.path.clone())
        .collect();
    // Resolve a usable repo root: prefer an explicit `params.root`, fall
    // back to deriving it from the pack dir layout so pack-only calls
    // (`analyze_health { pack: ... }` with no `root`) still pick up
    // manifest-driven context and F-CARGO-LOCK-STATE for Rust packs.
    let derived_root = params
        .root
        .as_deref()
        .map(std::path::Path::new)
        .or_else(|| repolens_core::storage::RepoStoragePaths::repo_root_from_pack_dir(&pack_dir));
    let manifest_deps: Vec<String> = derived_root
        .map(|root| {
            repolens_profile::repo_context::extract_cargo_dependency_names_recursive(
                root,
                &file_paths,
            )
        })
        .unwrap_or_default();
    let is_cargo_workspace = derived_root
        .map(repolens_profile::repo_context::cargo_root_is_workspace)
        .unwrap_or(false);
    let repo_context =
        RepoContextDetector::detect_full(&profile, &file_paths, &manifest_deps, is_cargo_workspace);
    let lockfile_state = derived_root.map(CargoLockStateDetector::detect);
    let pid = pack_id_from_dir(&pack_dir);
    // Compute scan summary so the test-inventory finding can fire when the
    // legacy `low-test-ratio` signal would.
    let scan = repolens_app::summary::compute_scan_summary_in_pack(&index, Some(&pack_dir));
    let referenced = repolens_app::references::referenced_files(&index, &pack_dir);
    let evidence_findings = EvidenceFindingsBuilder {
        pack_id: &pid,
        commit: index.manifest.head_commit.as_deref(),
        profile: &profile,
        repo_context: &repo_context,
        graph: Some(&graph),
        graph_health: Some(&graph_health),
        scan: Some(&scan),
        lockfile_state: lockfile_state.as_ref(),
        cargo_has_binary: derived_root
            .and_then(repolens_profile::repo_context::cargo_root_has_binary_target),
        referenced_files: Some(&referenced),
        dependency_risk: Some(&report.dependency_risk),
    }
    .build();
    report.evidence_findings = evidence_findings;

    // Phase 4B Step 4 (2026-05-13): retire the response-level `dependencyRisk`
    // and `graphHealth` legacy keys so the envelope grade can promote to
    // `Derived`. Both blocks remain on the in-memory `HealthReport` because
    // `EvidenceFindingsBuilder` consumes `dependency_risk` (F-LOCKFILE-MISSING-NON-CARGO)
    // and `graph_health` (F-DEPENDENCY-CYCLES, F-ORPHAN-FILES, F-TEST-INVENTORY-RATIO).
    // What the *response* carries is the calibrated view:
    //   - `structure`           — top-level dirs / dominant module / entry points
    //   - `evidenceFindings`    — catalogued per-finding view of cycles, orphans,
    //                             test ratio, lockfile state, etc.
    // Agents that previously read `graphHealth.*` raw counters now read the
    // same numbers off each finding's `evidence` block.
    let response_struct = serde_json::json!({
        "structure": report.structure,
        "evidenceFindings": report.evidence_findings,
    });

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let response = triage::augment_data(
        response_struct,
        &pid,
        &generated_at,
        &repo_context.tags,
        // No legacy fields remain — `dependencyRisk` and `graphHealth.*`
        // were retired in the Phase 4B Step 4 amendment (2026-05-13), the
        // same retirement that unblocks the envelope grade promotion below.
        &[],
    );

    // Envelope grade: the candidate is `Derived` because the substantive
    // payload (`structure` + per-finding `evidenceFindings`) is catalogue-
    // backed via doc 06 §5. But the envelope contract is that mixed
    // responses use the lowest grade present — so if any emitted finding
    // is `Unaudited` (e.g., `F-LOCKFILE-MISSING-NON-CARGO` for non-Cargo
    // build systems, which is heuristic-grade until the cross-language
    // lockfile-state catalogue lands), the envelope falls back. This
    // mirrors the conditional grading in `find_dead_code` at
    // `crates/repolens-mcp/src/tools/graph.rs:799-805`.
    let envelope_grade = crate::epistemic::min_envelope_grade(
        EvidenceGrade::Derived,
        &report.evidence_findings,
        false,
    );

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        evidence_findings = report.evidence_findings.len(),
        envelope_grade = ?envelope_grade,
        "analyze_health: exit"
    );
    ok_epistemic_graded(
        KnowledgeLayerField::Multiple(vec![KnowledgeLayer::Observation, KnowledgeLayer::Heuristic]),
        EpistemicConfidence::Codified,
        envelope_grade,
        ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: pid.clone(),
                source: "multiple".into(),
            },
            Provenance::HeuristicRule {
                rule: "healthAnalysis".into(),
                threshold: "multiple".into(),
                pack_id: Some(pid),
            },
        ]),
        response,
    )
}
