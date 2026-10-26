//! Helpers for the `compare-summaries` subcommand: pack-pair resolution
//! with neighbor semantics, and markdown rendering. Pack-id validation
//! lives in `repolens_app::discover::validate_pack_id` (lifted from this
//! file in the 2026-05-13 discover_packs expansion so the MCP layer can
//! reuse it).

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use repolens_app::discover::validate_pack_id;
use repolens_core::model::summary::SummaryComparison;

/// Given the sorted-ascending list of packs in `.repolens/` and optional
/// `--before` / `--after` raw flag values, return the resolved
/// `(before_path, after_path)` pair. Applies neighbor semantics for
/// one-sided overrides.
pub fn resolve_pack_pair(
    packs: &[PathBuf],
    before_raw: Option<&str>,
    after_raw: Option<&str>,
    storage_dir: &Path,
) -> Result<(PathBuf, PathBuf)> {
    fn find_pack<'a>(packs: &'a [PathBuf], id: &str) -> Option<&'a PathBuf> {
        packs
            .iter()
            .find(|p| p.file_name().map(|n| n == id).unwrap_or(false))
    }

    match (before_raw, after_raw) {
        (None, None) => {
            if packs.is_empty() {
                bail!(
                    "No pack directory found at {}. Run 'repolens scan' first.",
                    storage_dir.display()
                );
            }
            if packs.len() < 2 {
                bail!(
                    "Only {} pack in .repolens/. Need at least 2 to compare.",
                    packs.len()
                );
            }
            let after = packs.last().unwrap().clone();
            let before = packs[packs.len() - 2].clone();
            Ok((before, after))
        }
        (Some(before_raw), None) => {
            let before_id = validate_pack_id(before_raw)?;
            let before_pack = find_pack(packs, &before_id)
                .ok_or_else(|| anyhow::anyhow!("Pack not found: {before_id}"))?
                .clone();
            // Forward scan: first pack whose name sorts strictly after before_id
            // (packs is sorted ascending, so .find returns the immediate successor).
            let after_pack = packs
                .iter()
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|name| name > before_id.as_str())
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot resolve comparison pair from {before_id}: no newer pack exists."
                    )
                })?
                .clone();
            Ok((before_pack, after_pack))
        }
        (None, Some(after_raw)) => {
            let after_id = validate_pack_id(after_raw)?;
            let after_pack = find_pack(packs, &after_id)
                .ok_or_else(|| anyhow::anyhow!("Pack not found: {after_id}"))?
                .clone();
            // Reverse scan: last pack whose name sorts strictly before after_id
            // (packs is sorted ascending, so .rev().find returns the immediate predecessor).
            let before_pack = packs
                .iter()
                .rev()
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|name| name < after_id.as_str())
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot resolve comparison pair from {after_id}: no older pack exists."
                    )
                })?
                .clone();
            Ok((before_pack, after_pack))
        }
        (Some(before_raw), Some(after_raw)) => {
            let before_id = validate_pack_id(before_raw)?;
            let after_id = validate_pack_id(after_raw)?;
            if before_id == after_id {
                bail!("Before and after resolve to the same pack: {before_id}");
            }
            if before_id >= after_id {
                bail!("Before pack must be older than after pack: {before_id} >= {after_id}");
            }
            let before_pack = find_pack(packs, &before_id)
                .ok_or_else(|| anyhow::anyhow!("Pack not found: {before_id}"))?
                .clone();
            let after_pack = find_pack(packs, &after_id)
                .ok_or_else(|| anyhow::anyhow!("Pack not found: {after_id}"))?
                .clone();
            Ok((before_pack, after_pack))
        }
    }
}

/// Render a `SummaryComparison` as the paste-ready markdown block
/// described in the spec's Output Shapes section.
///
/// The "Recommendations" section was retired in the 2026-05-13 Phase 4B
/// Step 3 amendment along with the `Recommendation` / `RecommendationDelta`
/// types. Calibrated delta findings now live in `cmp.evidence_findings`;
/// callers that want them rendered as markdown should add a new section.
pub fn render_comparison_markdown(
    cmp: &SummaryComparison,
    before_id: &str,
    after_id: &str,
) -> String {
    use std::fmt::Write;

    fn pretty_name(raw: &str) -> String {
        // Metric names arrive as camelCase from ComparisonService.
        // Convert to "File count", "Total LoC", etc. for readability.
        match raw {
            "fileCount" => "File count".into(),
            "includedCount" => "Included count".into(),
            "totalLoc" => "Total LoC".into(),
            "graphNodes" => "Graph nodes".into(),
            "graphEdges" => "Graph edges".into(),
            other => other.to_string(),
        }
    }

    let mut out = String::new();
    let _ = writeln!(out, "## Summary Comparison");
    let _ = writeln!(out);
    let _ = writeln!(out, "Before: `{before_id}` | After: `{after_id}`");
    let _ = writeln!(out);
    // Fitness scores were retired in the 2026-05-11 Phase 4B Step 1
    // amendment — a single composite score collapses too much observable
    // structure into one number. The trend column on `Metrics` was retired
    // in the 2026-05-13 Phase 4B Step 4 amendment — its labels were
    // direction-blind. Calibrated delta findings live in `evidenceFindings`
    // (JSON output) with explicit thresholds and directionality.

    // Metrics
    let _ = writeln!(out, "### Metrics");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Metric         | Before | After |");
    let _ = writeln!(out, "| -------------- | -----: | ----: |");
    for m in &cmp.metrics {
        let _ = writeln!(
            out,
            "| {:<14} | {:>6} | {:>5} |",
            pretty_name(&m.name),
            m.before,
            m.after,
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // `validate_pack_id` unit tests live in `repolens_app::discover::tests`
    // alongside the helper itself; this module only covers pair-resolution
    // and markdown rendering, where the helper's behaviour is consumed.

    fn p(raw: &str) -> PathBuf {
        PathBuf::from(format!("/fake/.repolens/{raw}"))
    }

    #[test]
    fn resolve_pair_default_picks_last_two_ascending() {
        let packs = vec![
            p("pack-20260101-000000"),
            p("pack-20260102-000000"),
            p("pack-20260103-000000"),
        ];
        let (before, after) =
            resolve_pack_pair(&packs, None, None, Path::new("/fake/.repolens")).unwrap();
        assert_eq!(before, p("pack-20260102-000000"));
        assert_eq!(after, p("pack-20260103-000000"));
    }

    #[test]
    fn resolve_pair_default_fails_with_fewer_than_two_packs() {
        let packs = vec![p("pack-20260101-000000")];
        let err = resolve_pack_pair(&packs, None, None, Path::new("/fake/.repolens")).unwrap_err();
        assert!(err.to_string().contains("Only 1 pack"), "got: {err}");
    }

    #[test]
    fn resolve_pair_default_fails_with_zero_packs() {
        let packs: Vec<PathBuf> = vec![];
        let err = resolve_pack_pair(&packs, None, None, Path::new("/fake/.repolens")).unwrap_err();
        assert!(
            err.to_string().contains("No pack directory found"),
            "got: {err}"
        );
    }

    #[test]
    fn resolve_pair_after_only_picks_immediate_predecessor() {
        let packs = vec![
            p("pack-20260101-000000"),
            p("pack-20260102-000000"),
            p("pack-20260103-000000"),
        ];
        let (before, after) = resolve_pack_pair(
            &packs,
            None,
            Some("pack-20260102-000000"),
            Path::new("/fake/.repolens"),
        )
        .unwrap();
        assert_eq!(before, p("pack-20260101-000000"));
        assert_eq!(after, p("pack-20260102-000000"));
    }

    #[test]
    fn resolve_pair_before_only_picks_immediate_successor() {
        let packs = vec![
            p("pack-20260101-000000"),
            p("pack-20260102-000000"),
            p("pack-20260103-000000"),
        ];
        let (before, after) = resolve_pack_pair(
            &packs,
            Some("pack-20260102-000000"),
            None,
            Path::new("/fake/.repolens"),
        )
        .unwrap();
        assert_eq!(before, p("pack-20260102-000000"));
        assert_eq!(after, p("pack-20260103-000000"));
    }

    #[test]
    fn resolve_pair_after_only_fails_when_no_predecessor() {
        let packs = vec![p("pack-20260101-000000")];
        let err = resolve_pack_pair(
            &packs,
            None,
            Some("pack-20260101-000000"),
            Path::new("/fake/.repolens"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no older"), "got: {err}");
    }

    #[test]
    fn resolve_pair_before_only_fails_when_no_successor() {
        let packs = vec![p("pack-20260101-000000")];
        let err = resolve_pack_pair(
            &packs,
            Some("pack-20260101-000000"),
            None,
            Path::new("/fake/.repolens"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no newer"), "got: {err}");
    }

    #[test]
    fn resolve_pair_both_specified_accepts_valid_pair() {
        let packs = vec![
            p("pack-20260101-000000"),
            p("pack-20260102-000000"),
            p("pack-20260103-000000"),
        ];
        let (before, after) = resolve_pack_pair(
            &packs,
            Some("pack-20260101-000000"),
            Some("pack-20260103-000000"),
            Path::new("/fake/.repolens"),
        )
        .unwrap();
        assert_eq!(before, p("pack-20260101-000000"));
        assert_eq!(after, p("pack-20260103-000000"));
    }

    #[test]
    fn resolve_pair_rejects_identical_before_and_after() {
        let packs = vec![p("pack-20260101-000000"), p("pack-20260102-000000")];
        let err = resolve_pack_pair(
            &packs,
            Some("pack-20260101-000000"),
            Some("pack-20260101-000000"),
            Path::new("/fake/.repolens"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("same pack"), "got: {err}");
    }

    #[test]
    fn resolve_pair_rejects_inverted_pair() {
        let packs = vec![p("pack-20260101-000000"), p("pack-20260103-000000")];
        let err = resolve_pack_pair(
            &packs,
            Some("pack-20260103-000000"),
            Some("pack-20260101-000000"),
            Path::new("/fake/.repolens"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("must be older"), "got: {err}");
    }

    #[test]
    fn resolve_pair_rejects_bare_timestamp_not_present_on_disk() {
        let packs = vec![p("pack-20260101-000000")];
        let err = resolve_pack_pair(
            &packs,
            None,
            Some("99999999-999999"),
            Path::new("/fake/.repolens"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("Pack not found"), "got: {err}");
    }

    // `canonicalize_recommendations` and the markdown "Recommendations"
    // section were retired in the 2026-05-13 Phase 4B Step 3 amendment.
    // Their unit tests (`canonicalize_sorts_by_action_then_id_then_file_none_first`,
    // `render_markdown_empty_recommendation_sections_show_none_placeholder`)
    // were removed in the same amendment; the metrics + envelope portion
    // of the markdown render is covered by the test below.

    #[test]
    fn render_markdown_produces_stable_block_for_fixture() {
        use repolens_core::model::summary::MetricDelta;

        let cmp = SummaryComparison {
            metrics: vec![
                MetricDelta {
                    name: "fileCount".into(),
                    before: "306".into(),
                    after: "312".into(),
                },
                MetricDelta {
                    name: "totalLoc".into(),
                    before: "28144".into(),
                    after: "28867".into(),
                },
            ],
            evidence_findings: vec![],
        };

        let md = render_comparison_markdown(&cmp, "pack-20260420-112702", "pack-20260421-091533");

        assert!(md.starts_with("## Summary Comparison\n"), "got:\n{md}");
        assert!(md.contains("`pack-20260420-112702`"));
        assert!(md.contains("`pack-20260421-091533`"));
        // Fitness-score block was retired in 2026-05-11 Phase 4B Step 1.
        assert!(
            !md.contains("Fitness Scores"),
            "fitness block must not appear after Phase 4B Step 1; got:\n{md}"
        );
        assert!(!md.contains("Δ fitness"));
        // Recommendations section was retired in 2026-05-13 Phase 4B Step 3.
        assert!(
            !md.contains("### Recommendations"),
            "recommendations block must not appear after Phase 4B Step 3; got:\n{md}"
        );
        assert!(md.contains("### Metrics"));
        assert!(md.contains("| File count"));
        assert!(md.contains("| Total LoC"));
        // `Trend` column was retired in 2026-05-13 Phase 4B Step 4 — its
        // labels were direction-blind. The render now carries before/after
        // raw counts only; calibrated delta findings live in
        // `evidenceFindings` in the JSON output.
        assert!(
            !md.contains("| Trend") && !md.contains("Trend |"),
            "trend column must not appear after Phase 4B Step 4; got:\n{md}"
        );
        assert!(
            !md.contains("↑") && !md.contains("↓") && !md.contains("→"),
            "trend glyphs must not appear after Phase 4B Step 4; got:\n{md}"
        );
    }
}
