use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use rmcp::{model::CallToolResult, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::info;

use repolens_app::discover::PackResolver;
use repolens_app::summary::{ComparisonService, SummaryMemoryTier, SummaryService};
use repolens_core::model::summary::RepoSummary;

use crate::epistemic::*;
use crate::err_json;

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetSummaryParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,
    #[serde(rename = "noRescan")]
    #[schemars(
        description = "If true, skip rescan (default true). Returns error if no pack exists."
    )]
    #[allow(dead_code)]
    pub no_rescan: Option<bool>,
    #[schemars(description = "Detail tier: 'compact' (default), 'standard', or 'full'")]
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareSummariesParams {
    #[serde(rename = "beforePack")]
    #[schemars(
        description = "Earlier pack. Accepts an absolute path to the pack directory or a packId (`pack-YYYYMMDD-HHMMSS` / `YYYYMMDD-HHMMSS`); pack-ids require `root`."
    )]
    pub before_pack: String,
    #[serde(rename = "afterPack")]
    #[schemars(
        description = "Later pack. Accepts an absolute path to the pack directory or a packId (`pack-YYYYMMDD-HHMMSS` / `YYYYMMDD-HHMMSS`); pack-ids require `root`."
    )]
    pub after_pack: String,
    #[schemars(
        description = "Absolute path to the repository root. Required when `beforePack`/`afterPack` are passed as packIds rather than absolute paths."
    )]
    pub root: Option<String>,
}

// ---------------------------------------------------------------------------
// Handler: get_summary
// ---------------------------------------------------------------------------

pub fn handle_get_summary(
    resolver: &Arc<PackResolver>,
    params: GetSummaryParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    let detail = params.detail.as_deref().unwrap_or("compact").to_lowercase();
    info!(
        root = %params.root,
        detail = %detail,
        "get_summary: entry"
    );

    // Resolve pack from root
    let (pack_dir, index) = match resolver.resolve(None, Some(&params.root)) {
        Ok(r) => r,
        Err(e) => {
            info!(
                elapsed_ms = %start.elapsed().as_millis(),
                error = %e,
                "get_summary: resolve error"
            );
            return err_json("No pack found for root. Run a scan first.");
        }
    };

    // Build summary
    let memory_tier = match detail.as_str() {
        "standard" => SummaryMemoryTier::Standard,
        "full" => SummaryMemoryTier::Full,
        _ => SummaryMemoryTier::Compact,
    };
    let summary =
        match SummaryService::summarize(&pack_dir, Some(Path::new(&params.root)), memory_tier) {
            Ok(s) => s,
            Err(e) => {
                info!(
                    elapsed_ms = %start.elapsed().as_millis(),
                    error = %e,
                    "get_summary: summarize error"
                );
                return err_json(&format!("Failed to build summary: {}", e));
            }
        };

    let mut response = build_tiered_response(&summary, &detail);

    // Surface evidence-aware findings on the response. Always present,
    // even at compact tier, so consumers don't need to drop to full to see
    // the calibrated view.
    if let serde_json::Value::Object(ref mut map) = response {
        map.insert(
            "evidenceFindings".into(),
            serde_json::to_value(&summary.evidence_findings)
                .unwrap_or_else(|_| serde_json::json!([])),
        );
    }

    let pid = pack_id_from_dir(&pack_dir);
    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    // Use the resolved pack index's included file paths (not the
    // top-level module names from `summary.profile.modules`, which only
    // surface directory names like `src` or `crates`). This keeps the
    // outer triage envelope's repo-context aligned with the per-finding
    // evidence in `summary.evidenceFindings`.
    let file_paths: Vec<String> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.path.clone())
        .collect();
    let manifest_deps = repolens_profile::repo_context::extract_cargo_dependency_names_recursive(
        Path::new(&params.root),
        &file_paths,
    );
    let is_cargo_workspace =
        repolens_profile::repo_context::cargo_root_is_workspace(Path::new(&params.root));
    let repo_context = repolens_profile::repo_context::RepoContextDetector::detect_full(
        &summary.profile,
        &file_paths,
        &manifest_deps,
        is_cargo_workspace,
    );
    let response = crate::triage::augment_data(
        response,
        &pid,
        &generated_at,
        &repo_context.tags,
        // No legacy fields remain — Phase 4B retirement schedule:
        //   Step 1 (2026-05-11): `fitnessScore`
        //   Step 2 (2026-05-12): `health.signals`
        //   Step 3 (2026-05-13): `recommendations`
        //   Step 4 (2026-05-13): `health.dependencyRisk` and
        //   `health.graphHealth.*` — same retirement that unblocks the
        //   envelope grade promotion below. The `stats.testToSourceRatio`
        //   raw key was also dropped from the list since the matching
        //   F-TEST-INVENTORY-RATIO finding now carries the calibrated view
        //   inline; the raw `stats.testToSourceRatio` is a verified
        //   observation, not legacy-tier content.
        &[],
    );

    let has_memory = !summary.memory_insights.is_empty();

    // Interpretation layer whenever the tier surfaces memory insights — the
    // service already tier-selected memory_insights, so has_memory reflects
    // the actual rendered response for every tier including compact.
    let (layers, confidence, provenance) = if has_memory {
        (
            KnowledgeLayerField::Multiple(vec![
                KnowledgeLayer::Observation,
                KnowledgeLayer::Heuristic,
                KnowledgeLayer::Interpretation,
            ]),
            EpistemicConfidence::Variable,
            ProvenanceField::Multiple(vec![
                Provenance::PackExtraction {
                    pack_id: pid.clone(),
                    source: "multiple".into(),
                },
                Provenance::HeuristicRule {
                    rule: "repoSummary".into(),
                    threshold: "multiple".into(),
                    pack_id: Some(pid),
                },
                Provenance::MemoryStore {
                    source: "memory.json".into(),
                    node_ids: vec![],
                },
            ]),
        )
    } else {
        (
            KnowledgeLayerField::Multiple(vec![
                KnowledgeLayer::Observation,
                KnowledgeLayer::Heuristic,
            ]),
            EpistemicConfidence::Codified,
            ProvenanceField::Multiple(vec![
                Provenance::PackExtraction {
                    pack_id: pid.clone(),
                    source: "multiple".into(),
                },
                Provenance::HeuristicRule {
                    rule: "repoSummary".into(),
                    threshold: "multiple".into(),
                    pack_id: Some(pid),
                },
            ]),
        )
    };

    // Envelope grade: the candidate is `Derived` because the substantive
    // payload (project / stats / languages / graphSummary / topFiles /
    // topSymbols / per-finding `evidenceFindings`) is catalogue-backed via
    // doc 06 §5. The envelope falls back when:
    //   - any per-finding `evidence_grade` is `Unaudited`
    //     (e.g. `F-LOCKFILE-MISSING-NON-CARGO` for non-Cargo build systems);
    //   - `memoryInsights` is non-empty (`has_memory` already gates the
    //     `Interpretation` knowledge layer above; memory store content is
    //     not graded by doc 06 §5, so ungraded interpretation in the
    //     response can't be implicitly promoted to `Derived`).
    // Mirrors the conditional grading in `find_dead_code` at
    // `crates/repolens-mcp/src/tools/graph.rs:799-805`.
    let envelope_grade = crate::epistemic::min_envelope_grade(
        EvidenceGrade::Derived,
        &summary.evidence_findings,
        has_memory,
    );

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        detail = %detail,
        evidence_findings = summary.evidence_findings.len(),
        envelope_grade = ?envelope_grade,
        "get_summary: exit"
    );

    ok_epistemic_graded(layers, confidence, envelope_grade, provenance, response)
}

// ---------------------------------------------------------------------------
// Handler: compare_summaries
// ---------------------------------------------------------------------------

pub fn handle_compare_summaries(
    params: CompareSummariesParams,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(
        before_pack = %params.before_pack,
        after_pack = %params.after_pack,
        "compare_summaries: entry"
    );

    // Resolve packId aliases against `root` so agents that received a
    // packId from `discover_packs::availablePacks[].packId` don't need to
    // reconstruct absolute paths. `resolve_pack_ref` passes paths through
    // unchanged; only the packId form requires `root`.
    let resolve_ref = |raw: &str, label: &str| -> Result<std::path::PathBuf, McpError> {
        if repolens_app::discover::looks_like_pack_id(raw) {
            let root = params.root.as_deref().ok_or_else(|| {
                rmcp::ErrorData::invalid_params(
                    format!(
                        "{label} was passed as a packId ({raw}) but `root` is missing. \
                         Either pass an absolute pack directory path or supply `root`."
                    ),
                    None,
                )
            })?;
            repolens_app::discover::resolve_pack_ref(raw, Path::new(root))
                .map_err(|e| rmcp::ErrorData::invalid_params(format!("{label}: {e}"), None))
        } else {
            Ok(std::path::PathBuf::from(raw))
        }
    };
    let before_pack_buf = resolve_ref(&params.before_pack, "beforePack")?;
    let after_pack_buf = resolve_ref(&params.after_pack, "afterPack")?;
    let before_dir = before_pack_buf.as_path();
    let after_dir = after_pack_buf.as_path();
    // Pass params.root (when supplied) so SummaryService can read manifest
    // dependency names from Cargo.toml, populating crate-name indicator
    // tags (Crypto/Database/no_std) on the per-summary evidence findings.
    // For pack-only calls, recover the root from the *after* pack dir so
    // compare output matches get_summary / analyze_health calibration.
    let pack_owned_after_root =
        repolens_core::storage::RepoStoragePaths::repo_root_from_pack_dir(after_dir)
            .map(|p| p.to_path_buf());
    let pack_owned_before_root =
        repolens_core::storage::RepoStoragePaths::repo_root_from_pack_dir(before_dir)
            .map(|p| p.to_path_buf());
    let after_summarize_root = params
        .root
        .as_deref()
        .map(Path::new)
        .or(pack_owned_after_root.as_deref());
    let before_summarize_root = params
        .root
        .as_deref()
        .map(Path::new)
        .or(pack_owned_before_root.as_deref());

    let before_summary =
        match SummaryService::summarize(before_dir, before_summarize_root, SummaryMemoryTier::Skip)
        {
            Ok(s) => s,
            Err(e) => {
                info!(
                    elapsed_ms = %start.elapsed().as_millis(),
                    error = %e,
                    "compare_summaries: before summarize error"
                );
                return err_json(&format!("Failed to build before summary: {}", e));
            }
        };

    let after_summary =
        match SummaryService::summarize(after_dir, after_summarize_root, SummaryMemoryTier::Skip) {
            Ok(s) => s,
            Err(e) => {
                info!(
                    elapsed_ms = %start.elapsed().as_millis(),
                    error = %e,
                    "compare_summaries: after summarize error"
                );
                return err_json(&format!("Failed to build after summary: {}", e));
            }
        };

    let before_id_for_findings = pack_id_from_dir(before_dir);
    let after_id_for_findings = pack_id_from_dir(after_dir);
    // Phase 4B Step 1 (2026-05-11): fitness scores were retired. The
    // comparison is now infallible at this layer — it computes metric
    // deltas and evidence findings directly from the two summaries.
    let comparison = ComparisonService::compare_with_pack_ids(
        &before_summary,
        &after_summary,
        &before_id_for_findings,
        &after_id_for_findings,
    );

    let response = match serde_json::to_value(&comparison) {
        Ok(v) => v,
        Err(e) => {
            info!(
                elapsed_ms = %start.elapsed().as_millis(),
                error = %e,
                "compare_summaries: serialize error"
            );
            return err_json(&format!("Failed to serialize comparison: {}", e));
        }
    };

    let before_id = pack_id_from_dir(before_dir);
    let after_id = pack_id_from_dir(after_dir);

    // Repo context for the triage envelope. Preferred sources, in order:
    //   1. The first emitted delta finding (computed from the after-summary
    //      context with full pack paths + manifest deps).
    //   2. The first per-pack finding on after_summary (also computed from
    //      full pack paths + manifest deps). This catches the case where
    //      no delta crossed a threshold but the after-summary still
    //      detected indicator tags.
    //   3. Profile-only detect as a last resort (top-level dir names only;
    //      may lose nested-path indicators).
    let after_ctx_tags: Vec<repolens_core::model::evidence::RepoContextTag> = comparison
        .evidence_findings
        .first()
        .map(|f| f.repo_context.clone())
        .or_else(|| {
            after_summary
                .evidence_findings
                .first()
                .map(|f| f.repo_context.clone())
        })
        .unwrap_or_else(|| {
            let after_paths: Vec<String> = after_summary
                .profile
                .modules
                .iter()
                .map(|m| m.path.clone())
                .collect();
            repolens_profile::repo_context::RepoContextDetector::detect(
                &after_summary.profile,
                &after_paths,
            )
            .tags
        });
    let generated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let response = crate::triage::augment_data(
        response,
        &after_id,
        &generated_at,
        &after_ctx_tags,
        // No legacy fields remain — Phase 4B retirement schedule:
        //   Step 1 (2026-05-11): `deltaFitness` / `beforeFitness` / `afterFitness`
        //   Step 3 (2026-05-13): `recommendations`
        //   Step 4 (2026-05-13): `metrics.trend` (direction-blind labels);
        //   the surviving `metrics` block now carries pure before/after raw
        //   counts, which are confirmOnly-catalogued observations rather
        //   than legacy interpretive content.
        &[],
    );

    // Envelope grade: candidate `Derived`. `evidenceFindings` (F-DELTA-*)
    // is the calibrated delta path; the surviving `metrics` block carries
    // raw confirmOnly-grade observations after `trend` was retired in the
    // 2026-05-13 Phase 4B Step 4 amendment. The envelope still falls back
    // to `Unaudited` if any per-finding grade is `Unaudited` — none of
    // the current F-DELTA-* findings emit `Unaudited`, but the conditional
    // grading is applied for symmetry with `analyze_health` /
    // `get_summary` and to guard against a future delta finding landing
    // at heuristic-grade.
    let envelope_grade = crate::epistemic::min_envelope_grade(
        EvidenceGrade::Derived,
        &comparison.evidence_findings,
        false,
    );

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        metric_count = %comparison.metrics.len(),
        evidence_finding_count = %comparison.evidence_findings.len(),
        envelope_grade = ?envelope_grade,
        "compare_summaries: exit"
    );

    ok_epistemic_graded(
        KnowledgeLayerField::Multiple(vec![KnowledgeLayer::Observation, KnowledgeLayer::Heuristic]),
        EpistemicConfidence::Codified,
        envelope_grade,
        ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: before_id,
                source: "multiple".into(),
            },
            Provenance::PackExtraction {
                pack_id: after_id,
                source: "multiple".into(),
            },
            Provenance::HeuristicRule {
                rule: "summaryComparison".into(),
                threshold: "multiple".into(),
                pack_id: None,
            },
        ]),
        response,
    )
}

// ---------------------------------------------------------------------------
// Tiered response builder
// ---------------------------------------------------------------------------

fn build_tiered_response(summary: &RepoSummary, detail: &str) -> serde_json::Value {
    // Precompute top-N language distribution entries
    let mut lang_entries: Vec<(&String, &i32)> =
        summary.profile.language_distribution.iter().collect();
    lang_entries.sort_by(|a, b| b.1.cmp(a.1));

    match detail {
        "standard" => build_standard(summary, &lang_entries),
        "full" => build_full(summary, &lang_entries),
        _ => build_compact(summary, &lang_entries), // "compact" or unknown → compact
    }
}

fn build_compact(summary: &RepoSummary, lang_entries: &[(&String, &i32)]) -> serde_json::Value {
    let top3_langs: serde_json::Map<String, serde_json::Value> = lang_entries
        .iter()
        .take(3)
        .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
        .collect();

    // Repo-wide symbol inventory (uncapped) vs. the ranked top-N sample size
    // (capped at 20). `totalSymbols` must report the inventory; the sample
    // size is surfaced honestly as `topSymbolCount`.
    let total_symbols = summary.total_symbols;
    let top_symbol_count = summary.top_symbols.len();
    let total_files = summary.scan.file_count;

    let memory_insights: Vec<serde_json::Value> = summary
        .memory_insights
        .iter()
        .map(|i| serde_json::to_value(i).unwrap_or_default())
        .collect();

    serde_json::json!({
        "project": {
            "name": summary.profile.name,
            "projectType": format!("{:?}", summary.profile.project_type),
            "description": summary.profile.description,
        },
        "languages": top3_langs,
        "stats": {
            "fileCount": summary.scan.file_count,
            "includedFileCount": summary.scan.included_count,
            "totalLoc": summary.scan.total_loc,
            "testToSourceRatio": summary.scan.test_to_source_ratio,
        },
        "memoryInsights": memory_insights,
        "_meta": {
            "detail": "compact",
            "schemaVersion": "1.0",
            "totalFiles": total_files,
            "totalSymbols": total_symbols,
            "topSymbolCount": top_symbol_count,
            "hasHealth": summary.health.is_some(),
            "hasRecentCommits": !summary.recent_commits.is_empty(),
            "hasMemoryInsights": !summary.memory_insights.is_empty(),
        },
        "_hints": {
            "nextCalls": ["get_profile", "get_architecture", "get_summary(detail=standard)"],
        },
    })
}

fn build_standard(summary: &RepoSummary, lang_entries: &[(&String, &i32)]) -> serde_json::Value {
    let all_langs: serde_json::Map<String, serde_json::Value> = lang_entries
        .iter()
        .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
        .collect();

    // Phase 4B Step 4 (2026-05-13): the response-level `dependencyRisk`
    // and `graphHealth` keys were retired so the envelope grade can promote
    // to `Derived`. The same observations live in `summary.evidence_findings`
    // (mirrored under `evidenceFindings` at the top of the response) — that
    // is the catalogued path consumers should prefer.
    let health_partial = summary.health.as_ref().map(|h| {
        serde_json::json!({
            "structure": h.structure,
        })
    });

    let graph_partial = summary.graph_summary.as_ref().map(|g| {
        let top5: Vec<_> = g.top_connected_files.iter().take(5).collect();
        serde_json::json!({
            "nodeCount": g.node_count,
            "edgeCount": g.edge_count,
            "topConnectedFiles": top5,
        })
    });

    let top10_files: Vec<_> = summary.top_files.iter().take(10).collect();
    let top10_symbols: Vec<_> = summary.top_symbols.iter().take(10).collect();

    // SummaryService has already tier-selected memory_insights; serialize as-is.
    let tier_insights: Vec<serde_json::Value> = summary
        .memory_insights
        .iter()
        .map(|i| serde_json::to_value(i).unwrap_or_default())
        .collect();

    let shown_files = top10_files.len();
    let shown_symbols = top10_symbols.len();
    // Sizes of the ranked top-N lists (capped at 20). These describe the
    // truncation of the preview lists below — NOT the repo-wide inventory.
    let top_files_count = summary.top_files.len();
    let top_symbol_count = summary.top_symbols.len();
    // Repo-wide symbol inventory (uncapped). `_meta.totalSymbols` reports
    // this, not the capped `top_symbol_count`.
    let total_symbols = summary.total_symbols;

    serde_json::json!({
        "project": {
            "name": summary.profile.name,
            "projectType": format!("{:?}", summary.profile.project_type),
            "description": summary.profile.description,
        },
        "languages": all_langs,
        "stats": {
            "fileCount": summary.scan.file_count,
            "includedFileCount": summary.scan.included_count,
            "totalLoc": summary.scan.total_loc,
            "testToSourceRatio": summary.scan.test_to_source_ratio,
            "testFileCount": summary.scan.test_file_count,
            "sourceFileCount": summary.scan.source_file_count,
        },
        "health": health_partial,
        "graphSummary": graph_partial,
        "topFiles": top10_files,
        "topSymbols": top10_symbols,
        "memoryInsights": tier_insights,
        "_meta": {
            "detail": "standard",
            "schemaVersion": "1.0",
            "totalFiles": summary.scan.file_count,
            "totalSymbols": total_symbols,
            "topSymbolCount": top_symbol_count,
            "hasHealth": summary.health.is_some(),
            "hasRecentCommits": !summary.recent_commits.is_empty(),
            "hasMemoryInsights": !summary.memory_insights.is_empty(),
            "truncated": {
                "files": { "shown": shown_files, "total": top_files_count },
                "symbols": { "shown": shown_symbols, "total": top_symbol_count },
            },
        },
        "_hints": {
            "nextCalls": ["get_summary(detail=full)"],
        },
    })
}

fn build_full(summary: &RepoSummary, lang_entries: &[(&String, &i32)]) -> serde_json::Value {
    let all_langs: serde_json::Map<String, serde_json::Value> = lang_entries
        .iter()
        .map(|(k, v)| ((*k).clone(), serde_json::json!(**v)))
        .collect();

    // Phase 4B Step 4 (2026-05-13): emit only the catalogued sub-blocks of
    // `HealthReport`. `dependencyRisk` (excluded-tier `riskLevel` heuristic)
    // and `graphHealth.*` (raw counters duplicated by F-DEPENDENCY-CYCLES /
    // F-ORPHAN-FILES / F-TEST-INVENTORY-RATIO) are retired from the response
    // — both stay on the in-memory `HealthReport` so internal builders can
    // still consume them. See `tools/profile.rs::handle_analyze_health` for
    // the same retirement.
    let health_full = summary.health.as_ref().map(|h| {
        serde_json::json!({
            "structure": h.structure,
            "evidenceFindings": h.evidence_findings,
        })
    });

    let graph_partial = summary.graph_summary.as_ref().map(|g| {
        let top10: Vec<_> = g.top_connected_files.iter().take(10).collect();
        serde_json::json!({
            "nodeCount": g.node_count,
            "edgeCount": g.edge_count,
            "topConnectedFiles": top10,
        })
    });

    let top20_files: Vec<_> = summary.top_files.iter().take(20).collect();
    let top20_symbols: Vec<_> = summary.top_symbols.iter().take(20).collect();

    // SummaryService has already tier-selected memory_insights; serialize as-is.
    let tier_insights: Vec<serde_json::Value> = summary
        .memory_insights
        .iter()
        .map(|i| serde_json::to_value(i).unwrap_or_default())
        .collect();

    let top5_commits: Vec<serde_json::Value> = summary
        .recent_commits
        .iter()
        .take(5)
        .map(|c| serde_json::to_value(c).unwrap_or_default())
        .collect();

    let session_val = summary
        .session
        .as_ref()
        .map(|s| serde_json::to_value(s).unwrap_or_default());

    let total_symbols = summary.total_symbols;
    let top_symbol_count = summary.top_symbols.len();

    serde_json::json!({
        "project": {
            "name": summary.profile.name,
            "projectType": format!("{:?}", summary.profile.project_type),
            "description": summary.profile.description,
        },
        "languages": all_langs,
        "stats": {
            "fileCount": summary.scan.file_count,
            "includedFileCount": summary.scan.included_count,
            "totalLoc": summary.scan.total_loc,
            "testToSourceRatio": summary.scan.test_to_source_ratio,
            "testFileCount": summary.scan.test_file_count,
            "sourceFileCount": summary.scan.source_file_count,
        },
        "health": health_full,
        "graphSummary": graph_partial,
        "topFiles": top20_files,
        "topSymbols": top20_symbols,
        "memoryInsights": tier_insights,
        "recentCommits": top5_commits,
        "session": session_val,
        "_meta": {
            "detail": "full",
            "schemaVersion": "1.0",
            "totalFiles": summary.scan.file_count,
            "totalSymbols": total_symbols,
            "topSymbolCount": top_symbol_count,
            "hasHealth": summary.health.is_some(),
            "hasRecentCommits": !summary.recent_commits.is_empty(),
            "hasMemoryInsights": !summary.memory_insights.is_empty(),
        },
    })
}
