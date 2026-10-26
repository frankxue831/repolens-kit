use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;
use tracing::{info, warn};

use repolens_core::model::graph::FileGraph;
use repolens_core::model::memory::{MemoryGraph, MemoryNode};
use repolens_core::model::summary::*;
use repolens_core::pack_reader::{PackIndex, PackReader};
use repolens_memory::decay::MemoryDecayEngine;
use repolens_memory::store::MemoryStore;
use repolens_profile::health::HealthAnalyzer;
use repolens_profile::profile::ProfileAnalyzer;

use crate::grounding::assess_grounding;

/// Which tier of memory insights the summary should surface.
///
/// Callers pass the tier they intend to return to the agent; the service
/// selects memories directly from the full graph under that tier's rule so
/// compact / standard / full each rank against the same source, not a
/// pre-truncated intermediate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryMemoryTier {
    /// Do not load or decay memory. The summary's `memory_insights` will
    /// be empty even when `repo_root` is supplied. Use this for paths
    /// that need manifest/lockfile context but must not mutate memory
    /// state (e.g. compare_summaries, which is read-only by contract).
    Skip,
    Compact,
    Standard,
    Full,
}

impl SummaryMemoryTier {
    fn cap(self) -> usize {
        match self {
            SummaryMemoryTier::Skip => 0,
            SummaryMemoryTier::Compact => 3,
            SummaryMemoryTier::Standard => 5,
            SummaryMemoryTier::Full => 20,
        }
    }
}

/// Orchestrates the full repo summary pipeline.
pub struct SummaryService;

impl SummaryService {
    /// Build a `RepoSummary` from the pack at `pack_dir`.
    ///
    /// When `repo_root` is `Some`, the service loads the repo's memory graph,
    /// runs lazy decay mirroring `recall_longterm`, and populates
    /// `memory_insights` with the tier-selected subset. `None` preserves the
    /// prior no-memory behavior (used by `compare_summaries` and unit tests
    /// that do not exercise memory).
    pub fn summarize(
        pack_dir: &Path,
        repo_root: Option<&Path>,
        memory_tier: SummaryMemoryTier,
    ) -> Result<RepoSummary> {
        info!(pack_dir = %pack_dir.display(), "SummaryService::summarize starting");

        let index = PackReader::read(pack_dir)
            .with_context(|| format!("Failed to read pack at {}", pack_dir.display()))?;

        let profile = ProfileAnalyzer::analyze(&index, pack_dir);

        let graph = read_graph(pack_dir);

        let graph_health_input = graph.as_ref().map(|g| {
            crate::graph_health::build_graph_health_input_in_pack(
                &index,
                &profile,
                g,
                Some(pack_dir),
            )
        });

        let mut health = graph
            .as_ref()
            .map(|_g| HealthAnalyzer::analyze(&index, &profile, graph_health_input.as_ref()));

        let scan = compute_scan_summary_in_pack(&index, Some(pack_dir));
        let degree_map = graph.as_ref().map(compute_degree_map).unwrap_or_default();
        let graph_summary = graph
            .as_ref()
            .map(|g| compute_graph_summary(g, &degree_map));

        let top_symbols = compute_top_symbols(&index, &degree_map, 20);
        let top_files = compute_top_files(&index, &degree_map, 20);
        let total_symbols = compute_total_symbols(&index);

        // Memory load/decay is gated on BOTH having a repo root AND a
        // tier that surfaces memory. SummaryMemoryTier::Skip leaves
        // .repolens/memory/memory.json untouched even when repo_root is
        // supplied — used by read-only paths like compare_summaries that
        // need manifest context but must not mutate memory state.
        let memory_insights = match (repo_root, memory_tier) {
            (Some(root), SummaryMemoryTier::Compact)
            | (Some(root), SummaryMemoryTier::Standard)
            | (Some(root), SummaryMemoryTier::Full) => {
                load_and_select_memory(root, &index, memory_tier)
            }
            _ => Vec::new(),
        };

        // Evidence-aware findings — the calibrated replacement for the
        // retired `recommendations` / `signals` fields.
        // Repo-context detection uses included file paths from the pack index
        // plus manifest dependency names from Cargo.toml (when a repo root
        // is available) so that crate-name indicators (`ring`, `sqlx`,
        // `heapless`, …) actually fire. Excluded entries (vendored sources,
        // ignored fixtures) are filtered so they cannot drive indicator
        // tags or false-positive notes for code RepoLens did not include.
        let file_paths: Vec<String> = index
            .entries
            .iter()
            .filter(|e| e.included)
            .map(|e| e.path.clone())
            .collect();
        let manifest_deps: Vec<String> = repo_root
            .map(|root| {
                repolens_profile::repo_context::extract_cargo_dependency_names_recursive(
                    root,
                    &file_paths,
                )
            })
            .unwrap_or_default();
        let is_cargo_workspace = repo_root
            .map(repolens_profile::repo_context::cargo_root_is_workspace)
            .unwrap_or(false);
        let repo_context = repolens_profile::repo_context::RepoContextDetector::detect_full(
            &profile,
            &file_paths,
            &manifest_deps,
            is_cargo_workspace,
        );
        let lockfile_state =
            repo_root.map(repolens_profile::lockfile::CargoLockStateDetector::detect);
        let pack_id = repolens_core::model::epistemic::pack_id_from_dir(pack_dir);
        let referenced = crate::references::referenced_files(&index, pack_dir);
        let evidence_findings = crate::evidence_findings::EvidenceFindingsBuilder {
            pack_id: &pack_id,
            commit: index.manifest.head_commit.as_deref(),
            profile: &profile,
            repo_context: &repo_context,
            graph: graph.as_ref(),
            graph_health: graph_health_input.as_ref(),
            scan: Some(&scan),
            lockfile_state: lockfile_state.as_ref(),
            cargo_has_binary: repo_root
                .and_then(repolens_profile::repo_context::cargo_root_has_binary_target),
            referenced_files: Some(&referenced),
            dependency_risk: health.as_ref().map(|h| &h.dependency_risk),
        }
        .build();

        // Mirror the findings inside the embedded HealthReport so consumers
        // that read `summary.health` get the same view as `summary`.
        if let Some(h) = health.as_mut() {
            h.evidence_findings = evidence_findings.clone();
        }

        info!(
            file_count = scan.file_count,
            included_count = scan.included_count,
            evidence_findings = evidence_findings.len(),
            memory_insights = memory_insights.len(),
            "SummaryService::summarize complete"
        );

        Ok(RepoSummary {
            schema_version: "1.0".to_string(),
            session: None,
            scan,
            profile,
            health,
            graph_summary,
            top_symbols,
            top_files,
            total_symbols,
            recent_commits: vec![],
            memory_insights,
            evidence_findings,
        })
    }
}

/// Load memory for `repo_root`, apply lazy decay consistent with
/// `recall_longterm`, and return the tier-selected insights. On any I/O
/// failure, logs a warning and returns an empty vec — memory is best-effort
/// context, not a correctness input.
fn load_and_select_memory(
    repo_root: &Path,
    pack: &PackIndex,
    tier: SummaryMemoryTier,
) -> Vec<MemoryInsight> {
    let mut guard = match MemoryStore::lock(repo_root) {
        Ok(g) => g,
        Err(e) => {
            warn!(error = %e, "SummaryService: failed to load memory graph, returning empty");
            return Vec::new();
        }
    };

    let now = OffsetDateTime::now_utc();
    if MemoryDecayEngine::should_run(guard.graph(), now) {
        let decayed = MemoryDecayEngine::decay(guard.graph(), now);
        *guard.graph_mut() = decayed;
        if let Err(e) = guard.save() {
            warn!(error = %e, "SummaryService: failed to save decayed memory graph");
        }
    }

    let insights = select_memory_insights(guard.graph(), pack, tier);

    // Write-back: persist the grounding snapshot for the surfaced insights so
    // the next decay run reflects it (selected-only; scan-time handles the
    // unbiased full refresh). One save, only when there are insights.
    if !insights.is_empty() {
        for ins in &insights {
            crate::grounding::record_grounding(
                guard.graph_mut(),
                &ins.id,
                ins.grounding_status.clone(),
                now,
            );
        }
        guard.graph_mut().updated_at = now;
        if let Err(e) = guard.save() {
            warn!(error = %e, "SummaryService: failed to save grounding snapshot");
        }
    }

    insights
}

/// Tier-aware memory selection from the full graph.
///
/// Compact and Standard tiers filter to memories with at least one anchor
/// that resolves against `pack` and rank by `importance` descending. Full
/// tier keeps all memories and ranks by `importance * bonus`, where bonus is
/// `1.5` when at least one anchor resolves and `1.0` otherwise.
pub(crate) fn select_memory_insights(
    graph: &MemoryGraph,
    pack: &PackIndex,
    tier: SummaryMemoryTier,
) -> Vec<MemoryInsight> {
    struct Scored {
        node_idx: usize,
        rank: f32,
        status: repolens_core::model::memory::GroundingStatus,
        resolved: bool,
    }

    let mut scored: Vec<Scored> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| {
            let assessment = assess_grounding(node, Some(pack));
            let resolved = !assessment.found_anchors.is_empty();
            Scored {
                node_idx: idx,
                rank: node.importance,
                status: assessment.grounding_status,
                resolved,
            }
        })
        .collect();

    match tier {
        SummaryMemoryTier::Skip => return Vec::new(),
        SummaryMemoryTier::Compact | SummaryMemoryTier::Standard => {
            scored.retain(|s| s.resolved);
        }
        SummaryMemoryTier::Full => {
            for s in scored.iter_mut() {
                if s.resolved {
                    s.rank *= 1.5;
                }
            }
        }
    }

    scored.sort_by(|a, b| {
        b.rank
            .partial_cmp(&a.rank)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(tier.cap());

    scored
        .into_iter()
        .map(|s| node_to_insight(&graph.nodes[s.node_idx], s.status))
        .collect()
}

fn node_to_insight(
    node: &MemoryNode,
    grounding_status: repolens_core::model::memory::GroundingStatus,
) -> MemoryInsight {
    let memory_type = serde_json::to_value(&node.memory_type)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let last_accessed = node
        .last_accessed
        .format(&Iso8601::DEFAULT)
        .unwrap_or_default();

    MemoryInsight {
        id: node.id.clone(),
        content: node.content.clone(),
        memory_type,
        importance: node.importance,
        last_accessed,
        access_count: node.access_count,
        anchors: node.anchors.clone(),
        tags: node.tags.clone(),
        grounding_status,
    }
}

pub fn read_graph(pack_dir: &Path) -> Option<FileGraph> {
    let path = pack_dir.join("graph.json");
    let json = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&json).ok()
}

pub fn compute_scan_summary(index: &PackIndex) -> ScanSummary {
    compute_scan_summary_in_pack(index, None)
}

/// Like [`compute_scan_summary`], but when `pack_dir` is given also counts
/// source files carrying inline tests (`#[cfg(test)]` / `#[test]`, e.g. a
/// Rust `src/lib.rs` with a `mod tests`) as test-bearing. Such files stay in
/// the source count too: they are both source and test.
pub fn compute_scan_summary_in_pack(index: &PackIndex, pack_dir: Option<&Path>) -> ScanSummary {
    let mode = index
        .manifest
        .scan_mode
        .clone()
        .unwrap_or_else(|| "full".to_string());

    let file_count = index.entries.len() as i32;
    let included_count = index.entries.iter().filter(|e| e.included).count() as i32;
    let total_loc: i64 = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.loc as i64)
        .sum();

    let counts = test_source_counts(index, pack_dir);

    ScanSummary {
        mode,
        file_count,
        included_count,
        total_loc,
        test_file_count: counts.test_bearing as i32,
        source_file_count: counts.source as i32,
        test_to_source_ratio: counts.ratio(),
    }
}

/// File counts behind the test-to-source ratio.
struct TestSourceCounts {
    /// Dedicated test files (by path) plus inline-test-bearing source files.
    test_bearing: usize,
    /// Included files that are not dedicated test files.
    source: usize,
}

impl TestSourceCounts {
    fn ratio(&self) -> f64 {
        if self.source == 0 {
            0.0
        } else {
            self.test_bearing as f64 / self.source as f64
        }
    }
}

fn test_source_counts(index: &PackIndex, pack_dir: Option<&Path>) -> TestSourceCounts {
    let mut path_tests = 0usize;
    let mut inline_tests = 0usize;
    let mut included = 0usize;
    // #11: only program source counts. Licenses, lockfiles, images, docs,
    // HTML pages and config files are neither source nor tests.
    for e in index
        .entries
        .iter()
        .filter(|e| e.included && is_source_code_path(&e.path))
    {
        included += 1;
        if is_test_file(&e.path) {
            path_tests += 1;
        } else if let (Some(dir), Some(cp)) = (pack_dir, e.content_path.as_deref()) {
            if e.language.eq_ignore_ascii_case("rust") || e.path.ends_with(".rs") {
                if let Ok(text) = std::fs::read_to_string(dir.join(cp)) {
                    if has_inline_rust_tests(&text) {
                        inline_tests += 1;
                    }
                }
            }
        }
    }
    TestSourceCounts {
        test_bearing: path_tests + inline_tests,
        source: included - path_tests,
    }
}

/// Extensions counted as program source for the test-to-source ratio.
const SOURCE_CODE_EXTENSIONS: &[&str] = &[
    "rs", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "go", "java", "kt", "kts", "scala", "groovy",
    "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "py", "rb", "php", "cs", "fs", "swift",
    "m", "mm", "dart", "lua", "pl", "pm", "ex", "exs", "erl", "hs", "ml", "clj", "zig", "nim",
    "jl", "r", "vue", "svelte", "sh", "bash", "zsh", "ps1",
];

/// Return true if the path looks like program source (by extension), as
/// opposed to licenses, lockfiles, images, Markdown/HTML pages or config.
pub fn is_source_code_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            SOURCE_CODE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        }
        _ => false,
    }
}

/// Return true if Rust source text contains inline tests
/// (`#[cfg(test)]` or a `#[test]` attribute).
pub fn has_inline_rust_tests(text: &str) -> bool {
    text.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("#[cfg(test)]") || t.starts_with("#[test]")
    })
}

/// Return true if the file path looks like a test file.
pub fn is_test_file(path: &str) -> bool {
    // Check path segments (including leading segment like "test/Foo.java"
    // and top-level `__tests__/` which is the canonical JS/TS layout for
    // repo-rooted test trees).
    if path.starts_with("test/")
        || path.starts_with("tests/")
        || path.starts_with("spec/")
        || path.starts_with("__tests__/")
        || path.contains("/test/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
        || path.contains("/spec/")
    {
        return true;
    }

    // Extract the file stem (filename without extension)
    let filename = path.split('/').next_back().unwrap_or(path);
    let stem = if let Some(dot_pos) = filename.rfind('.') {
        &filename[..dot_pos]
    } else {
        filename
    };
    let stem_lower = stem.to_lowercase();

    // Exact match or separator-bounded patterns (avoids false positives
    // like "contest", "manifest", "quickest")
    if stem_lower.ends_with("_test")
        || stem_lower.ends_with(".test")
        || stem_lower == "test"
        || stem_lower.starts_with("test_")
        || stem_lower.starts_with("test.")
        || stem_lower.ends_with("_spec")
        || stem_lower.ends_with(".spec")
        || stem_lower == "spec"
        || stem_lower.starts_with("spec_")
        || stem_lower.starts_with("spec.")
    {
        return true;
    }

    // CamelCase convention: FooTest, FooSpec (Java/JVM style)
    stem.ends_with("Test") || stem.ends_with("Spec")
}

/// Compute the test-to-source file ratio from a pack index.
///
/// Shared between `SummaryService` and the MCP `analyze_health` tool
/// to keep the metric single-sourced.
pub fn compute_test_to_source_ratio(index: &PackIndex) -> f64 {
    compute_test_to_source_ratio_in_pack(index, None)
}

/// Pack-aware variant of [`compute_test_to_source_ratio`] that also counts
/// inline-test-bearing Rust source files (see [`has_inline_rust_tests`]).
pub fn compute_test_to_source_ratio_in_pack(index: &PackIndex, pack_dir: Option<&Path>) -> f64 {
    test_source_counts(index, pack_dir).ratio()
}

fn compute_graph_summary(
    graph: &FileGraph,
    degree_map: &HashMap<String, (i32, i32)>,
) -> GraphSummary {
    let node_count = graph.nodes.len() as i32;
    let edge_count = graph.edges.len() as i32;

    let mut connected: Vec<ConnectedFile> = graph
        .nodes
        .iter()
        .map(|n| {
            let (in_deg, out_deg) = degree_map.get(&n.id).copied().unwrap_or((0, 0));
            ConnectedFile {
                path: n.id.clone(),
                in_degree: in_deg,
                out_degree: out_deg,
            }
        })
        .collect();

    connected.sort_by(|a, b| {
        let a_total = a.in_degree + a.out_degree;
        let b_total = b.in_degree + b.out_degree;
        b_total.cmp(&a_total)
    });
    connected.truncate(20);

    GraphSummary {
        node_count,
        edge_count,
        top_connected_files: connected,
    }
}

/// Build a map of path → (in_degree, out_degree) from graph edges.
fn compute_degree_map(graph: &FileGraph) -> HashMap<String, (i32, i32)> {
    let mut map: HashMap<String, (i32, i32)> = HashMap::new();

    for node in &graph.nodes {
        map.entry(node.id.clone()).or_insert((0, 0));
    }

    for edge in &graph.edges {
        map.entry(edge.target.clone()).or_insert((0, 0)).0 += 1;
        map.entry(edge.source.clone()).or_insert((0, 0)).1 += 1;
    }

    map
}

/// Count every symbol extracted across the pack's included files.
///
/// This is the repo-wide symbol inventory — distinct from the top-N ranked
/// sample produced by [`compute_top_symbols`], which caps its output at
/// `limit`. Only `included` entries contribute, matching the population
/// `compute_top_symbols` ranks over (excluded entries carry no extracted
/// symbols). Reported as `_meta.totalSymbols` by `get_summary`; the capped
/// sample size is surfaced separately as `_meta.topSymbolCount`.
fn compute_total_symbols(index: &PackIndex) -> i32 {
    index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.symbols.len())
        .sum::<usize>() as i32
}

/// Compute top symbols ranked by file connections.
fn compute_top_symbols(
    index: &PackIndex,
    degree_map: &HashMap<String, (i32, i32)>,
    limit: usize,
) -> Vec<SymbolSummary> {
    let mut symbols: Vec<SymbolSummary> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .flat_map(|entry| {
            let (in_deg, out_deg) = degree_map.get(&entry.path).copied().unwrap_or((0, 0));
            let file_connections = in_deg + out_deg;
            entry.symbols.iter().map(move |sym| SymbolSummary {
                name: sym.name.clone(),
                symbol_type: sym.symbol_type.clone(),
                file: entry.path.clone(),
                file_connections,
            })
        })
        .collect();

    symbols.sort_by_key(|s| std::cmp::Reverse(s.file_connections));
    symbols.truncate(limit);
    symbols
}

/// Compute top files ranked by LOC + connections * 10.
fn compute_top_files(
    index: &PackIndex,
    degree_map: &HashMap<String, (i32, i32)>,
    limit: usize,
) -> Vec<FileSummary> {
    let mut files: Vec<FileSummary> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|entry| {
            let (in_deg, out_deg) = degree_map.get(&entry.path).copied().unwrap_or((0, 0));
            let connections = in_deg + out_deg;
            FileSummary {
                path: entry.path.clone(),
                language: entry.language.clone(),
                loc: entry.loc,
                connections,
            }
        })
        .collect();

    files.sort_by(|a, b| {
        let a_score = a.loc as i64 + a.connections as i64 * 10;
        let b_score = b.loc as i64 + b.connections as i64 * 10;
        b_score.cmp(&a_score)
    });
    files.truncate(limit);
    files
}

// ---------------------------------------------------------------------------
// ComparisonService
// ---------------------------------------------------------------------------

/// Compares two repo summaries to show change over time.
pub struct ComparisonService;

impl ComparisonService {
    /// Backwards-compat shim that uses placeholder pack IDs ("before"/"after")
    /// in emitted `evidence_findings.scope.time.packId`. New callers should
    /// prefer [`ComparisonService::compare_with_pack_ids`] so consumers can
    /// tie delta findings back to real pack directories.
    pub fn compare(before: &RepoSummary, after: &RepoSummary) -> SummaryComparison {
        Self::compare_with_pack_ids(before, after, "before", "after")
    }

    /// Compare two summaries and emit calibrated delta findings whose
    /// `scope.time.packId` reflects the actual pack identifiers passed in.
    pub fn compare_with_pack_ids(
        before: &RepoSummary,
        after: &RepoSummary,
        before_pack_id: &str,
        after_pack_id: &str,
    ) -> SummaryComparison {
        let metrics = vec![
            make_delta(
                "fileCount",
                before.scan.file_count as i64,
                after.scan.file_count as i64,
            ),
            make_delta(
                "includedCount",
                before.scan.included_count as i64,
                after.scan.included_count as i64,
            ),
            make_delta("totalLoc", before.scan.total_loc, after.scan.total_loc),
            make_delta(
                "graphNodes",
                before
                    .graph_summary
                    .as_ref()
                    .map(|g| g.node_count as i64)
                    .unwrap_or(0),
                after
                    .graph_summary
                    .as_ref()
                    .map(|g| g.node_count as i64)
                    .unwrap_or(0),
            ),
            make_delta(
                "graphEdges",
                before
                    .graph_summary
                    .as_ref()
                    .map(|g| g.edge_count as i64)
                    .unwrap_or(0),
                after
                    .graph_summary
                    .as_ref()
                    .map(|g| g.edge_count as i64)
                    .unwrap_or(0),
            ),
        ];

        // Evidence-aware delta findings — the calibrated replacement for the
        // retired `recommendations` delta field. Reuse the repo_context that
        // SummaryService already computed for the `after` summary's
        // findings — it was built from the full pack-included file paths
        // and any manifest dependency names, so it picks up nested
        // crypto/auth/database paths that `profile.modules` (top-level
        // dir names only) would lose. Fall back to a profile-only detect
        // when the after summary has no findings.
        let after_ctx = if let Some(first) = after.evidence_findings.first() {
            repolens_profile::repo_context::RepoContext {
                tags: first.repo_context.clone(),
                notes: first
                    .false_positive_notes
                    .iter()
                    .map(|note| repolens_profile::repo_context::IndicatorMatch {
                        tag: repolens_core::model::evidence::RepoContextTag::Custom {
                            name: "fromAfterSummary".into(),
                        },
                        note: note.clone(),
                    })
                    .collect(),
            }
        } else {
            let after_paths: Vec<String> = after
                .profile
                .modules
                .iter()
                .map(|m| m.path.clone())
                .collect();
            repolens_profile::repo_context::RepoContextDetector::detect(
                &after.profile,
                &after_paths,
            )
        };
        let evidence_findings = crate::compare_findings::DeltaFindingsBuilder {
            before_pack_id,
            after_pack_id,
            before,
            after,
            repo_context: &after_ctx,
        }
        .build();

        SummaryComparison {
            metrics,
            evidence_findings,
        }
    }
}

fn make_delta(name: &str, before: i64, after: i64) -> MetricDelta {
    // `trend` was retired in 2026-05-13 Phase 4B Step 4 — its labels were
    // direction-blind (LOC reduction labelled `degraded` etc.). Calibrated
    // delta findings (`F-DELTA-*`) carry directionality with thresholds.
    MetricDelta {
        name: name.to_string(),
        before: before.to_string(),
        after: after.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_rust_test_markers_detected() {
        assert!(has_inline_rust_tests(
            "fn a() {}\n#[cfg(test)]\nmod tests {}\n"
        ));
        assert!(has_inline_rust_tests("    #[test]\n    fn t() {}\n"));
        assert!(!has_inline_rust_tests(
            "fn a() {}\n// #[cfg(test)] in a comment\n"
        ));
        assert!(!has_inline_rust_tests(
            "#[cfg(feature = \"x\")]\nfn a() {}\n"
        ));
    }

    #[test]
    fn non_source_files_are_not_counted_as_source() {
        for p in [
            "LICENSE-MIT",
            "LICENSE-APACHE",
            ".gitignore",
            "Cargo.lock",
            "Cargo.toml",
            "www/og.png.b64",
            "docs/logo.png",
            "README.md",
            "_layouts/default.html",
            ".github/workflows/ci.yml",
            "package.json",
        ] {
            assert!(!is_source_code_path(p), "{p} must not count as source");
        }
        for p in [
            "src/lib.rs",
            "www/app.js",
            "pw/run.cjs",
            "x/ref.py",
            "a/b.ts",
        ] {
            assert!(is_source_code_path(p), "{p} must count as source");
        }
        let index = make_index(vec![
            make_entry("src/lib.rs", true, 10),
            make_entry("LICENSE-MIT", true, 10),
            make_entry("Cargo.lock", true, 10),
            make_entry("index.html", true, 10),
            make_entry("README.md", true, 10),
            make_entry("tests/a.rs", true, 10),
        ]);
        let s = compute_scan_summary(&index);
        assert_eq!(s.source_file_count, 1);
        assert_eq!(s.test_file_count, 1);
    }

    #[test]
    fn scan_summary_in_pack_counts_inline_test_files() {
        use repolens_core::model::file_entry::FileEntry;
        let pack = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(pack.path().join("files/src")).unwrap();
        std::fs::write(
            pack.path().join("files/src/lib.rs"),
            "pub fn f() {}\n#[cfg(test)]\nmod tests {}\n",
        )
        .unwrap();
        std::fs::write(pack.path().join("files/src/a.rs"), "pub fn a() {}\n").unwrap();
        let entry = |p: &str| FileEntry {
            path: p.to_string(),
            sha256: String::new(),
            language: "rust".into(),
            loc: 1,
            included: true,
            reason: String::new(),
            content_path: Some(format!("files/{p}")),
            symbols: vec![],
        };
        let index = PackIndex::from_entries(vec![entry("src/lib.rs"), entry("src/a.rs")]);
        // Without pack access (legacy path-only view) nothing is a test.
        assert_eq!(compute_scan_summary(&index).test_file_count, 0);
        let s = compute_scan_summary_in_pack(&index, Some(pack.path()));
        assert_eq!(s.test_file_count, 1);
        assert_eq!(s.source_file_count, 2);
        assert!((s.test_to_source_ratio - 0.5).abs() < 1e-9);
        assert!(
            (compute_test_to_source_ratio_in_pack(&index, Some(pack.path())) - 0.5).abs() < 1e-9
        );
    }
    use repolens_core::model::file_entry::FileEntry;
    use repolens_core::model::manifest::Manifest;
    use repolens_core::model::profile::{ProjectProfile, ProjectType};
    use repolens_core::pack_reader::PackIndex;

    fn make_entry(path: &str, included: bool, loc: i32) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            sha256: "abc".to_string(),
            language: "Java".to_string(),
            loc,
            included,
            reason: "included".to_string(),
            content_path: None,
            symbols: vec![],
        }
    }

    fn make_manifest() -> Manifest {
        let json = r#"{
            "schemaVersion": "1.0",
            "schemaUrl": "",
            "timestamp": 0,
            "chunks": [],
            "scanMode": "full"
        }"#;
        serde_json::from_str(json).expect("test manifest parse")
    }

    fn make_index(entries: Vec<FileEntry>) -> PackIndex {
        PackIndex {
            manifest: make_manifest(),
            entries,
        }
    }

    fn empty_profile() -> ProjectProfile {
        ProjectProfile {
            name: "test".to_string(),
            build_system: None,
            frameworks: vec![],
            project_type: ProjectType::Unknown,
            entry_points: vec![],
            description: None,
            language_distribution: Default::default(),
            modules: vec![],
        }
    }

    fn minimal_summary(file_count: i32, _legacy_fitness_unused: f64) -> RepoSummary {
        RepoSummary {
            schema_version: "1.0".to_string(),
            session: None,
            scan: ScanSummary {
                mode: "full".to_string(),
                file_count,
                included_count: file_count,
                total_loc: 1000,
                test_file_count: 0,
                source_file_count: file_count,
                test_to_source_ratio: 0.0,
            },
            profile: empty_profile(),
            health: None,
            graph_summary: None,
            top_symbols: vec![],
            top_files: vec![],
            total_symbols: 0,
            recent_commits: vec![],
            memory_insights: vec![],
            evidence_findings: vec![],
        }
    }

    #[test]
    fn total_symbols_counts_all_included_symbols_not_capped() {
        use repolens_core::model::file_entry::Symbol;

        let sym = |name: &str| Symbol {
            symbol_type: "function".to_string(),
            name: name.to_string(),
            qualified_name: None,
            visibility: None,
            line: None,
        };

        // 25 symbols across two included files — above the top-N cap of 20 —
        // plus an excluded file whose symbols must NOT be counted. This pins
        // both invariants: the count is uncapped, and it draws from the same
        // `included` population that `compute_top_symbols` ranks over.
        let mut a = make_entry("src/a.rs", true, 10);
        a.symbols = (0..15).map(|i| sym(&format!("a{i}"))).collect();
        let mut b = make_entry("src/b.rs", true, 10);
        b.symbols = (0..10).map(|i| sym(&format!("b{i}"))).collect();
        let mut excluded = make_entry("vendor/c.rs", false, 10);
        excluded.symbols = (0..50).map(|i| sym(&format!("c{i}"))).collect();

        let index = make_index(vec![a, b, excluded]);

        assert_eq!(
            compute_total_symbols(&index),
            25,
            "total symbols must sum every included symbol (uncapped), excluding not-included entries"
        );
    }

    #[test]
    fn scan_summary_counts_test_files() {
        let entries = vec![
            make_entry("src/Main.java", true, 100),
            make_entry("test/MainTest.java", true, 50),
            make_entry("src/util_test.go", true, 30),
        ];
        let index = make_index(entries);
        let scan = compute_scan_summary(&index);

        assert_eq!(scan.test_file_count, 2, "expected 2 test files");
        assert_eq!(scan.source_file_count, 1, "expected 1 source file");
    }

    // `FitnessScorer` was retired in the 2026-05-11 Phase 4B Step 1
    // amendment along with `FitnessScore`, `RepoSummary.fitness_score`,
    // and the `delta_fitness`/`before_fitness`/`after_fitness` fields on
    // `SummaryComparison`. The corresponding scorer unit tests
    // (`fitness_scorer_basic`, `fitness_scorer_cycle_penalty`,
    // `fitness_scorer_cycle_penalty_capped`) were removed in the same
    // amendment because they pinned a behaviour the product no longer
    // emits.

    // `RecommendationBuilder` and the `Recommendation` / `RecommendationDelta`
    // types were retired in the 2026-05-13 Phase 4B Step 3 amendment. Their
    // per-rule unit tests (`recommendation_high_coupling`,
    // `recommendation_low_test_ratio`, `recommendation_dependency_cycles`,
    // `recommendation_no_cycles_when_zero`) were removed in the same
    // amendment — each had a calibrated evidence-finding equivalent
    // (F-HIGH-FAN-IN-FILE, F-TEST-INVENTORY-RATIO, F-DEPENDENCY-CYCLES)
    // already pinned in `evidence_findings.rs`.

    #[test]
    fn test_compute_test_to_source_ratio_public() {
        let entries = vec![
            FileEntry {
                path: "src/main.rs".to_string(),
                sha256: "abc".to_string(),
                language: "Rust".to_string(),
                loc: 100,
                included: true,
                reason: "included".to_string(),
                symbols: vec![],
                content_path: None,
            },
            FileEntry {
                path: "tests/test_main.rs".to_string(),
                sha256: "def".to_string(),
                language: "Rust".to_string(),
                loc: 50,
                included: true,
                reason: "included".to_string(),
                symbols: vec![],
                content_path: None,
            },
            FileEntry {
                path: "src/lib.rs".to_string(),
                sha256: "ghi".to_string(),
                language: "Rust".to_string(),
                loc: 200,
                included: true,
                reason: "included".to_string(),
                symbols: vec![],
                content_path: None,
            },
        ];

        let index = make_index(entries);

        let ratio = compute_test_to_source_ratio(&index);
        // 1 test file, 2 source files => 0.5
        assert!((ratio - 0.5).abs() < f64::EPSILON);
    }

    // ------------------------------------------------------------------
    // Test-file detection invariants (catalogue: 2026-05-11-test-file-detection-invariant-catalogue.md)
    // ------------------------------------------------------------------

    /// I-TEST-DETECT-RUST: Rust per-file conventions are recognised.
    /// `*_test.rs` and `tests/**/*.rs` are classified as test.
    #[test]
    fn i_test_detect_rust() {
        assert!(is_test_file("src/foo_test.rs"));
        assert!(is_test_file("crates/foo/src/parser_test.rs"));
        assert!(is_test_file("tests/integration.rs"));
        assert!(is_test_file("crates/foo/tests/api.rs"));
    }

    /// I-TEST-DETECT-JAVA: JVM-style `*Test.java` and `*Spec.java` classnames
    /// and the conventional `/test/`, `/spec/` directories are recognised.
    #[test]
    fn i_test_detect_java() {
        assert!(is_test_file("src/main/java/com/example/FooTest.java"));
        assert!(is_test_file("src/main/java/com/example/FooSpec.java"));
        assert!(is_test_file("src/test/java/com/example/Foo.java"));
        assert!(is_test_file("test/com/example/Foo.java"));
    }

    /// I-TEST-DETECT-PYTHON: `test_*.py` and `*_test.py` are classified as test.
    #[test]
    fn i_test_detect_python() {
        assert!(is_test_file("test_models.py"));
        assert!(is_test_file("services/test_auth.py"));
        assert!(is_test_file("services/auth_test.py"));
        assert!(is_test_file("tests/test_models.py"));
    }

    /// I-TEST-DETECT-GO: `*_test.go` is the canonical Go test pattern.
    #[test]
    fn i_test_detect_go() {
        assert!(is_test_file("foo_test.go"));
        assert!(is_test_file("pkg/auth/user_test.go"));
    }

    /// I-TEST-DETECT-JSTS: `*.test.{js,ts,jsx,tsx}` and `*.spec.*` are
    /// classified as test, as are paths under `__tests__/` (both nested
    /// and at the repo root).
    #[test]
    fn i_test_detect_jsts() {
        assert!(is_test_file("src/foo.test.ts"));
        assert!(is_test_file("src/foo.test.tsx"));
        assert!(is_test_file("src/foo.spec.js"));
        assert!(is_test_file("src/foo.spec.jsx"));
        assert!(is_test_file("src/__tests__/foo.ts"));
        // Root-level `__tests__/` is the canonical JS/TS layout for
        // repo-rooted test trees. Regression for the 2026-05-11 second
        // Codex pass on PR #51 which caught that the prior classifier
        // only matched nested `/__tests__/`.
        assert!(is_test_file("__tests__/foo.ts"));
        assert!(is_test_file("__tests__/nested/bar.test.ts"));
    }

    /// I-TEST-DETECT-PATH-PREFIX: leading `test/`, `tests/`, `spec/` segments.
    #[test]
    fn i_test_detect_path_prefix() {
        assert!(is_test_file("test/Foo.java"));
        assert!(is_test_file("tests/integration.rs"));
        assert!(is_test_file("spec/foo_spec.rb"));
    }

    /// I-TEST-DETECT-NO-FALSE-POSITIVE-CONTEST: words containing "test" as a
    /// non-boundary substring must NOT be classified as test. Regression for
    /// the original `contest.rs` / `manifest.json` false-positive that
    /// motivated the separator-bounded matching in `is_test_file`.
    #[test]
    fn i_test_detect_no_false_positive_contest() {
        assert!(!is_test_file("src/contest.rs"));
        assert!(!is_test_file("src/manifest.rs"));
        assert!(!is_test_file("Cargo.toml"));
        assert!(!is_test_file("src/parser.rs"));
        // "testing" as a project name should NOT trigger — `testing` is not
        // a test-file marker by itself.
        assert!(!is_test_file("crates/testing-utils/src/lib.rs"));
    }

    /// I-TEST-DETECT-FIXTURE-PARITY: detection is stable across fixture
    /// repos. Covered indirectly by the existing
    /// `test_compute_test_to_source_ratio_public` test above plus the
    /// integration tests that scan known fixtures and assert ratios in the
    /// `repolens-app` tests directory.
    #[test]
    fn i_test_detect_fixture_parity() {
        // Spot-check known fixture-style filenames so that fixture-driven
        // ratios remain stable.
        assert!(is_test_file(
            "fixtures/inputs/java-small/src/test/HelperTest.java"
        ));
        assert!(is_test_file(
            "fixtures/inputs/rust-small/tests/integration.rs"
        ));
        assert!(!is_test_file("fixtures/inputs/rust-small/src/lib.rs"));
        assert!(!is_test_file(
            "fixtures/inputs/java-small/src/main/Helper.java"
        ));
    }

    #[test]
    fn comparison_detects_changes() {
        let before = minimal_summary(100, 0.7);
        let mut after = minimal_summary(120, 0.83);
        after.scan.file_count = 120;
        after.scan.included_count = 120;

        let comparison = ComparisonService::compare(&before, &after);

        // `trend` was retired in 2026-05-13 Phase 4B Step 4 — its labels
        // were direction-blind. The test now asserts on the raw delta in
        // before/after, which is the verified observation the surviving
        // shape keeps.
        let file_count_delta = comparison
            .metrics
            .iter()
            .find(|m| m.name == "fileCount")
            .expect("fileCount metric should be present");

        assert_eq!(file_count_delta.before, "100");
        assert_eq!(file_count_delta.after, "120");
    }

    #[test]
    fn summarize_populates_memory_insights_for_compact_tier() {
        use repolens_core::model::file_entry::FileEntry as CoreFileEntry;
        use repolens_core::model::memory::{
            GroundingStatus, MemoryGraph, MemoryNode, MemorySource, MemoryType,
        };
        use repolens_core::pack_writer::{PackWriteOptions, PackWriter};
        use repolens_memory::store::MemoryStore;
        use std::fs;
        use time::OffsetDateTime;

        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let pack_dir = tmp.path().join("pack");
        fs::create_dir_all(repo_root.join("src")).unwrap();
        fs::write(repo_root.join("src/a.rs"), "// a").unwrap();
        fs::write(repo_root.join("src/b.rs"), "// b").unwrap();

        let entries = vec![
            CoreFileEntry {
                path: "src/a.rs".to_string(),
                sha256: "aa".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/a.rs".to_string()),
                symbols: vec![],
            },
            CoreFileEntry {
                path: "src/b.rs".to_string(),
                sha256: "bb".to_string(),
                language: "Rust".to_string(),
                loc: 1,
                included: true,
                reason: "included".to_string(),
                content_path: Some("files/src/b.rs".to_string()),
                symbols: vec![],
            },
        ];
        let opts = PackWriteOptions {
            scan_mode: "full",
            head_commit: "",
            worktree_dirty: false,
            scan_config_hash: "",
            base_pack_id: None,
            base_head_commit: None,
            reprocessed_files: entries.len() as i32,
            deleted_files: None,
            carried_forward_files: None,
        };
        PackWriter::new(&pack_dir, &repo_root)
            .write(&entries, &opts)
            .unwrap();

        let now = OffsetDateTime::now_utc();
        let mem_node = |id: &str, importance: f32, anchors: Vec<String>| MemoryNode {
            id: id.to_string(),
            content: format!("note {id}"),
            memory_type: MemoryType::Convention,
            importance,
            created: now,
            last_accessed: now,
            access_count: 1,
            anchors,
            tags: vec![],
            source: MemorySource::Explicit,
            provenance: "test".to_string(),
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256: Default::default(),
        };
        let graph = MemoryGraph {
            schema_version: "1.0".to_string(),
            repo_root: repo_root.to_string_lossy().into_owned(),
            nodes: vec![
                mem_node("mem-a", 0.9, vec!["src/a.rs".to_string()]),
                mem_node("mem-b", 0.6, vec!["src/b.rs".to_string()]),
                mem_node("mem-missing", 0.8, vec!["src/ghost.rs".to_string()]),
                mem_node("mem-bare", 0.5, vec![]),
            ],
            edges: vec![],
            last_decay_run: Some(now),
            updated_at: now,
        };
        MemoryStore::save(&repo_root, &graph).unwrap();

        let summary =
            SummaryService::summarize(&pack_dir, Some(&repo_root), SummaryMemoryTier::Compact)
                .expect("summarize should succeed");

        let ids: Vec<&str> = summary
            .memory_insights
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["mem-a", "mem-b"],
            "compact tier must surface anchor-resolving memories ranked by importance desc"
        );
        assert_eq!(
            summary.memory_insights[0].grounding_status,
            GroundingStatus::Confirmed
        );
        assert_eq!(
            summary.memory_insights[1].grounding_status,
            GroundingStatus::Confirmed
        );
    }

    // ---------------------------------------------------------------------
    // select_memory_insights unit tests (Task 2.2)
    //
    // These call the selection function directly with in-memory fixtures, so
    // they cover ranking/cap semantics without the disk-pack roundtrip used
    // in the integration-style test above.
    // ---------------------------------------------------------------------

    fn mem_fixture_node(id: &str, importance: f32, anchors: Vec<&str>) -> MemoryNode {
        use repolens_core::model::memory::{MemorySource, MemoryType};
        let now = OffsetDateTime::now_utc();
        MemoryNode {
            id: id.to_string(),
            content: format!("note {id}"),
            memory_type: MemoryType::Convention,
            importance,
            created: now,
            last_accessed: now,
            access_count: 1,
            anchors: anchors.into_iter().map(String::from).collect(),
            tags: vec![],
            source: MemorySource::Explicit,
            provenance: "test".to_string(),
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256: Default::default(),
        }
    }

    fn pack_with_files(paths: &[&str]) -> PackIndex {
        let entries = paths.iter().map(|p| make_entry(p, true, 1)).collect();
        make_index(entries)
    }

    fn mem_graph(nodes: Vec<MemoryNode>) -> MemoryGraph {
        let now = OffsetDateTime::now_utc();
        MemoryGraph {
            schema_version: "1.0".to_string(),
            repo_root: "/tmp/test".to_string(),
            nodes,
            edges: vec![],
            last_decay_run: Some(now),
            updated_at: now,
        }
    }

    #[test]
    fn select_memory_insights_standard_tier_caps_at_five_ranked_by_importance() {
        let pack = pack_with_files(&["src/a.rs"]);
        let nodes = (0..7)
            .map(|i| {
                mem_fixture_node(
                    &format!("mem-{i}"),
                    0.1 + (i as f32) * 0.1,
                    vec!["src/a.rs"],
                )
            })
            .collect();
        let graph = mem_graph(nodes);

        let selected = select_memory_insights(&graph, &pack, SummaryMemoryTier::Standard);

        assert_eq!(selected.len(), 5, "standard tier should cap at 5");
        let ids: Vec<&str> = selected.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["mem-6", "mem-5", "mem-4", "mem-3", "mem-2"],
            "should be the top 5 by importance desc"
        );
    }

    #[test]
    fn select_memory_insights_full_tier_bonus_reranks_without_filtering() {
        use repolens_core::model::memory::GroundingStatus;

        // Two nodes: unresolved-but-high-importance, resolved-but-lower.
        // Without bonus, unresolved would rank first. With the 1.5x bonus,
        // resolved should jump ahead: 0.7 * 1.5 = 1.05 > 0.9 * 1.0.
        let pack = pack_with_files(&["src/a.rs"]);
        let graph = mem_graph(vec![
            mem_fixture_node("mem-unresolved", 0.9, vec!["src/ghost.rs"]),
            mem_fixture_node("mem-resolved", 0.7, vec!["src/a.rs"]),
        ]);

        let selected = select_memory_insights(&graph, &pack, SummaryMemoryTier::Full);

        assert_eq!(
            selected.len(),
            2,
            "full tier must not filter unresolved entries"
        );
        assert_eq!(
            selected[0].id, "mem-resolved",
            "resolved anchor must outrank via 1.5x bonus"
        );
        assert_eq!(selected[0].grounding_status, GroundingStatus::Confirmed);
        assert_eq!(
            selected[1].grounding_status,
            GroundingStatus::Contradicted,
            "unresolved groundable anchor should yield Contradicted"
        );
    }

    #[test]
    fn select_memory_insights_populates_all_grounding_statuses_in_full_tier() {
        use repolens_core::model::memory::GroundingStatus;

        let pack = pack_with_files(&["src/a.rs"]);
        let graph = mem_graph(vec![
            // Confirmed: resolves
            mem_fixture_node("mem-confirmed", 0.9, vec!["src/a.rs"]),
            // Contradicted: groundable anchor missing
            mem_fixture_node("mem-contradicted", 0.8, vec!["src/missing.rs"]),
            // Unchecked: no groundable anchors at all
            mem_fixture_node("mem-unchecked", 0.7, vec![]),
            // Unchecked also applies to bare names (no `/` or `.`)
            mem_fixture_node("mem-bare", 0.6, vec!["AuthService"]),
        ]);

        let selected = select_memory_insights(&graph, &pack, SummaryMemoryTier::Full);

        let by_id: HashMap<&str, &MemoryInsight> =
            selected.iter().map(|m| (m.id.as_str(), m)).collect();
        assert_eq!(
            by_id["mem-confirmed"].grounding_status,
            GroundingStatus::Confirmed
        );
        assert_eq!(
            by_id["mem-contradicted"].grounding_status,
            GroundingStatus::Contradicted
        );
        assert_eq!(
            by_id["mem-unchecked"].grounding_status,
            GroundingStatus::Unchecked
        );
        assert_eq!(
            by_id["mem-bare"].grounding_status,
            GroundingStatus::Unchecked
        );
    }
}
