//! `repolens findings` — print a pack's calibrated evidence findings as
//! agent-readable markdown (or JSON).
//!
//! Thin wrapper over `repolens_app::summary::SummaryService`: it resolves a
//! pack (active pack from `session.json` by default), summarizes it read-only
//! (no memory mutation), and renders `evidenceFindings`. This is the first
//! markdown rendering of evidence findings — `compare-summaries` renders only
//! the metrics table, and findings were otherwise reachable only as JSON via
//! the MCP `get_summary --detail full` tool. Surfacing them on a cheap CLI
//! path (and pointing the brief at it) is meant to make Phase 4B calibration
//! visible to agents that never spin up the MCP loop.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use repolens_app::discover::{list_pack_dirs, resolve_pack_ref};
use repolens_app::summary::{SummaryMemoryTier, SummaryService};
use repolens_core::model::evidence::{
    EvidenceFinding, EvidenceItem, FindingSeverity, RecommendedAction,
};
use repolens_core::session::SessionStore;
use repolens_core::storage::RepoStoragePaths;

use crate::resolve_root_from;
use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct FindingsArgs {
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Pack to read (`pack-YYYYMMDD-HHMMSS` or bare `YYYYMMDD-HHMMSS`).
    /// Defaults to the active pack from `session.json`, then the latest pack.
    #[arg(long)]
    pub pack: Option<String>,
    /// Output format for --verbose (markdown) or machine use (json)
    #[arg(long, value_enum, default_value_t = OutputFormat::Markdown)]
    pub format: OutputFormat,
    /// Print findings as JSON (same as --format json)
    #[arg(long, default_value_t = false)]
    pub json: bool,
    /// Show full evidence, limitations and suggested steps for each finding
    #[arg(long, short = 'v', default_value_t = false)]
    pub verbose: bool,
}

pub fn run_findings(args: FindingsArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;

    let pack_dir = resolve_findings_pack(&root, args.pack.as_deref())?;
    let pack_id = pack_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("Pack path has no filename: {}", pack_dir.display()))?
        .to_string();

    let summary =
        SummaryService::summarize(&pack_dir, Some(root.as_path()), SummaryMemoryTier::Skip)
            .map_err(|e| anyhow!("Failed to build summary for {pack_id}: {e}"))?;

    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format
    };
    match format {
        OutputFormat::Markdown if !args.verbose => {
            print!(
                "{}",
                render_findings_plain(&pack_id, &summary.evidence_findings)
            );
        }
        OutputFormat::Markdown => {
            print!(
                "{}",
                render_findings_markdown(&pack_id, &summary.evidence_findings)
            );
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&summary.evidence_findings)?;
            println!("{json}");
        }
    }
    Ok(())
}

/// Resolve which pack `repolens findings` reads. Preference order:
/// 1. `--pack <id>` (validated, must exist on disk),
/// 2. the active pack from `.repolens/session.json` (authoritative pointer),
/// 3. the latest pack on disk.
pub(crate) fn resolve_findings_pack(root: &Path, pack: Option<&str>) -> Result<PathBuf> {
    if let Some(raw) = pack {
        return resolve_pack_ref(raw, root);
    }
    // Genuine IO failures reading session.json surface as errors (consistent
    // with the MCP pack resolver); a missing or corrupt file is `Ok(None)` and
    // falls through to the latest-pack scan.
    if let Some(session) =
        SessionStore::load(root).context("Failed to read .repolens/session.json")?
    {
        let active = PathBuf::from(&session.active_pack_path);
        if active.join("manifest.json").is_file() {
            return Ok(active);
        }
    }
    // Latest usable pack: newest `pack-*` dir that actually has a manifest, so
    // an interrupted scan (dir created before manifest written) does not mask
    // an older usable pack.
    let packs = list_pack_dirs(root)?;
    packs
        .into_iter()
        .rev()
        .find(|p| p.join("manifest.json").is_file())
        .ok_or_else(|| {
            anyhow!(
                "No pack found at {}. Run 'repolens scan' first.",
                RepoStoragePaths::storage_dir(root).display()
            )
        })
}

/// The plain first-screen view of a scan's findings, shared by `findings`
/// and `init` so both count and word them the same way (#32).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlainFindings {
    /// Findings worth a look (severity above none), most severe first.
    /// Each is the full output line plus any lines indented under it.
    pub flagged: Vec<Vec<String>>,
    /// No-action findings (severity none), same shape.
    pub info: Vec<Vec<String>>,
    /// The language-coverage note (F-LANGUAGE-EXTRACTION-GAP): shown once,
    /// below the findings, and never counted as one.
    pub coverage: Vec<String>,
}

pub(crate) const COVERAGE_ID: &str = "F-LANGUAGE-EXTRACTION-GAP";

impl PlainFindings {
    pub fn from_findings(findings: &[EvidenceFinding]) -> Self {
        let rank = |s: &FindingSeverity| match s {
            FindingSeverity::High => 0,
            FindingSeverity::Medium => 1,
            FindingSeverity::Low => 2,
            FindingSeverity::None => 3,
        };
        // A filtered-only orphan finding with nothing left out says nothing;
        // it stays in --verbose and --json (Design call on #44).
        let mut sorted: Vec<&EvidenceFinding> = findings
            .iter()
            .filter(|f| !(is_filtered_only_orphans(f) && loaded_by_name(f).unwrap_or(0) == 0))
            .collect();
        sorted.sort_by(|a, b| {
            rank(&a.severity)
                .cmp(&rank(&b.severity))
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        let mut out = PlainFindings::default();
        for f in sorted {
            if f.id.as_str() == COVERAGE_ID {
                out.coverage = coverage_note(f);
                continue;
            }
            let label = match f.severity {
                FindingSeverity::High => "High",
                FindingSeverity::Medium => "Med ",
                FindingSeverity::Low => "Low ",
                FindingSeverity::None => "Info",
            };
            let mut row = vec![format!(
                "  {label}  {}",
                plain_sentence(f, PLAIN_TEXT_WIDTH)
            )];
            for note in finding_notes(f) {
                row.push(format!("        {}", fit_line(note, PLAIN_TEXT_WIDTH)));
            }
            if matches!(f.severity, FindingSeverity::None) {
                out.info.push(row);
            } else {
                out.flagged.push(row);
            }
        }
        out
    }

    /// "The last scan (pack-…) found 3 things worth a look" or "... found
    /// nothing to flag." — the same sentence on both screens.
    pub fn headline(&self, pack_id: &str) -> String {
        match self.flagged.len() {
            0 => format!("The last scan ({pack_id}) found nothing to flag."),
            n => format!(
                "The last scan ({pack_id}) found {} worth a look",
                plural(n as u64, "thing", "things")
            ),
        }
    }
}

/// The coverage note's lines: "  Coverage: " then the sentences, wrapped at
/// word boundaries within 80 columns.
fn coverage_note(f: &EvidenceFinding) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    if let Some(v) = observed(f, "extraction.languagesByFidelity") {
        if let Some(t) = v.get("unsupported") {
            let langs: Vec<String> = t
                .get("languages")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let loc = t.get("loc").and_then(|x| x.as_u64()).unwrap_or(0);
            if !langs.is_empty() {
                sentences.push(format!(
                    "imports aren't read for {}: {} ({loc} lines)",
                    plural(langs.len() as u64, "language", "languages"),
                    langs.join(", ")
                ));
            }
        }
    }
    sentences.extend(extraction_notes(f));
    if sentences.is_empty() {
        sentences.push(f.title.clone());
    }
    let mut lines = Vec::new();
    for s in sentences {
        for part in wrap_words(&s, COVERAGE_WIDTH) {
            let prefix = if lines.is_empty() {
                COVERAGE_PREFIX.to_string()
            } else {
                " ".repeat(COVERAGE_PREFIX.len())
            };
            lines.push(format!("{prefix}{part}"));
        }
    }
    lines
}

const COVERAGE_PREFIX: &str = "  Coverage: ";
const COVERAGE_WIDTH: usize = 80 - 12;

/// Split `s` into lines of at most `width` columns, breaking only at spaces
/// (a single word longer than `width` stays whole).
pub(crate) fn wrap_words(s: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in s.split_whitespace() {
        if !cur.is_empty() && cols(&cur) + 1 + cols(w) > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// F-ORPHAN-FILES saying no orphan looks unused: every one is loaded by
/// name or isn't code (#20 titles, both differ from `ORPHAN_LINE`).
fn is_filtered_only_orphans(f: &EvidenceFinding) -> bool {
    f.id.as_str() == "F-ORPHAN-FILES" && f.title != crate::autodraft::ORPHAN_LINE
}

/// Orphan files left out because a runner or tool loads them by name (#44).
fn loaded_by_name(f: &EvidenceFinding) -> Option<u64> {
    if f.id.as_str() != "F-ORPHAN-FILES" {
        return None;
    }
    observed(f, "graph.orphanCount")?
        .get("loadedByNameCount")?
        .as_u64()
}

/// Lines indented under a finding's row, e.g. what fan-in left out.
fn finding_notes(f: &EvidenceFinding) -> Vec<String> {
    if let Some(k) = loaded_by_name(f).filter(|k| *k > 0) {
        return vec![format!(
            "Left out: {} loaded by name (tests, build scripts, configs, …)",
            plural(k, "file", "files")
        )];
    }
    if f.id.as_str() != "F-HIGH-FAN-IN-FILE" {
        return vec![];
    }
    let inbound = observed(f, "graph.inboundEdges");
    let count = |key: &str| {
        inbound
            .and_then(|v| v.get(key))
            .and_then(|l| l.get("count"))
            .and_then(|c| c.as_u64())
    };
    let noun = |n: u64| if n == 1 { "file" } else { "files" };
    let listed = inbound
        .and_then(|v| v.get("sample"))
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len());
    if listed == 0 {
        // Nothing listed: every file over the threshold was filtered, so
        // the line counts all of them, module hubs included (#31).
        let n = count("sharedCrateFilesLeftOut").unwrap_or(0)
            + count("moduleHubFilesLeftOut").unwrap_or(0);
        if n == 0 {
            return vec![];
        }
        return vec![format!(
            "Left out: {n} {} (lib.rs/mod.rs/prelude.rs, *-core/*-util crates)",
            noun(n)
        )];
    }
    let Some(n) = count("sharedCrateFilesLeftOut") else {
        return vec![];
    };
    vec![format!(
        "Left out: {n} {} in shared *-core/*-util crates (see --verbose)",
        noun(n)
    )]
}

/// Default view: one plain sentence per finding, most severe first; then
/// no-action notes; then the coverage note.
fn render_findings_plain(pack_id: &str, findings: &[EvidenceFinding]) -> String {
    use std::fmt::Write;
    let p = PlainFindings::from_findings(findings);
    let mut out = String::new();
    if p.flagged.is_empty() && p.info.is_empty() && p.coverage.is_empty() {
        let _ = writeln!(out, "{}", p.headline(pack_id));
        return out;
    }
    if p.flagged.is_empty() {
        let _ = writeln!(out, "{}", p.headline(pack_id));
    } else {
        let _ = writeln!(out, "{}:", p.headline(pack_id));
        let _ = writeln!(out);
        for line in p.flagged.iter().flatten() {
            let _ = writeln!(out, "{line}");
        }
    }
    if !p.info.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Also noted, no action needed:");
        for line in p.info.iter().flatten() {
            let _ = writeln!(out, "{line}");
        }
    }
    if !p.coverage.is_empty() {
        let _ = writeln!(out);
        for line in &p.coverage {
            let _ = writeln!(out, "{line}");
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{}\nRun `repolens findings --verbose` for the evidence behind each one.",
        crate::autodraft::FINDINGS_HINT
    );
    out
}

/// Value of an observation by metric name.
fn observed<'a>(f: &'a EvidenceFinding, name: &str) -> Option<&'a serde_json::Value> {
    f.evidence.iter().find_map(|e| match e {
        EvidenceItem::Observation {
            metric,
            observed_value,
            ..
        } if metric == name => Some(observed_value),
        _ => None,
    })
}

/// F-LANGUAGE-EXTRACTION-GAP notes for regex-extracted languages, such as
/// "Python: imports read; cross-file calls and types not yet". Empty for any
/// other finding.
pub(crate) fn extraction_notes(f: &EvidenceFinding) -> Vec<String> {
    if f.id.as_str() != "F-LANGUAGE-EXTRACTION-GAP" {
        return vec![];
    }
    observed(f, "extraction.languagesByFidelity")
        .and_then(|v| v.get("heuristic"))
        .and_then(|t| t.get("notes"))
        .and_then(|n| n.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Columns left for the sentence after the "  High  " prefix, so every
/// default line fits in 80 columns.
const PLAIN_TEXT_WIDTH: usize = 80 - 8;

/// Fit `path` into `room` columns by dropping leading directories
/// (`…/src/error.rs`). `None` when even the file name doesn't fit: a path
/// is never cut mid-name.
fn fit_path(path: &str, room: usize) -> Option<String> {
    if cols(path) <= room {
        return Some(path.to_string());
    }
    let parts: Vec<&str> = path.split('/').collect();
    (1..parts.len())
        .map(|start| format!("…/{}", parts[start..].join("/")))
        .find(|tail| cols(tail) <= room)
}

/// Cut `s` to `width` columns at a word boundary, ending in "…" when
/// shortened.
fn fit_line(s: String, width: usize) -> String {
    if cols(&s) <= width {
        return s;
    }
    // Keep as many characters as fit in `width - 1` columns (room for
    // the "…"), then back up to the last space.
    let budget = width.saturating_sub(1);
    let chars: Vec<char> = s.chars().collect();
    let mut used = 0;
    let mut keep = 0;
    for c in &chars {
        let w = UnicodeWidthChar::width(*c).unwrap_or(0);
        if used + w > budget {
            break;
        }
        used += w;
        keep += 1;
    }
    let cut = if chars.get(keep) == Some(&' ') {
        keep
    } else {
        chars[..keep]
            .iter()
            .rposition(|c| *c == ' ')
            .filter(|&i| i > 0)
            .unwrap_or(keep)
    };
    let head: String = chars[..cut].iter().collect();
    format!("{}…", head.trim_end())
}

/// Terminal display columns of `s`: CJK and other wide characters count
/// as 2, combining marks as 0 (#41 follow-up; `chars().count()` let a
/// long Chinese path run past 80 columns).
pub(crate) fn cols(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// "{head}{path}{tail}" with the path shortened so the whole fits `width`.
/// When even the file name doesn't fit, the example is dropped: `head`
/// without its trailing ", e.g. " / ": ".
fn with_path(head: &str, path: &str, tail: &str, width: usize) -> String {
    let fixed = cols(head) + cols(tail);
    let room = width.saturating_sub(fixed);
    match fit_path(path, room) {
        Some(p) => fit_line(format!("{head}{p}{tail}"), width),
        None => fit_line(
            head.trim_end_matches(", e.g. ")
                .trim_end_matches(": ")
                .to_string(),
            width,
        ),
    }
}

/// One plain-language sentence for a finding, built from its evidence,
/// at most `width` columns. Falls back to the finding title for kinds
/// without a custom sentence.
fn plain_sentence(f: &EvidenceFinding, width: usize) -> String {
    fit_line(plain_sentence_full(f, width), width)
}

fn plain_sentence_full(f: &EvidenceFinding, width: usize) -> String {
    let num =
        |v: Option<&serde_json::Value>, k: &str| v.and_then(|v| v.get(k)).and_then(|x| x.as_u64());
    match f.id.as_str() {
        "F-DEPENDENCY-CYCLES" => {
            let c = observed(f, "graph.cycleCount");
            if let (Some(n), Some(largest)) = (num(c, "cycleCount"), num(c, "largestCycleSize")) {
                return format!(
                    "{} import each other in a loop (largest: {largest} files)",
                    plural(n, "group of files", "groups of files")
                );
            }
        }
        "F-ORPHAN-FILES" => {
            let c = observed(f, "graph.orphanCount");
            let first = observed(f, "graph.orphanFiles")
                .and_then(|v| v.get("sample"))
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str());
            if let Some(n) = num(c, "sourceOrphanCount") {
                if n > 0 {
                    let head = format!("{}: {n}", crate::autodraft::ORPHAN_LINE);
                    return match first {
                        Some(p) => with_path(&format!("{head}, e.g. "), p, "", width),
                        None => head,
                    };
                }
                // Design's wording for a zero count (#44).
                return "No unused files found (imports only)".to_string();
            }
        }
        "F-HIGH-FAN-IN-FILE" => {
            let top = observed(f, "graph.inboundEdges")
                .and_then(|v| v.get("sample"))
                .and_then(|v| v.as_array())
                .map(|a| a.len() as u64);
            let first = observed(f, "graph.inboundEdges")
                .and_then(|v| v.get("sample"))
                .and_then(|v| v.as_array())
                .and_then(|a| a.first());
            if top == Some(0) {
                return "No widely imported files listed: all are module hubs or shared crates"
                    .to_string();
            }
            if let (Some(n), Some(first)) = (top, first) {
                // #24: Go entries stand for a package; name it.
                let sample = observed(f, "graph.inboundEdges")
                    .and_then(|v| v.get("sample"))
                    .and_then(|v| v.as_array());
                let all_go = sample.is_some_and(|a| a.iter().all(|e| e.get("goPackage").is_some()));
                if let (true, Some(pkg)) = (all_go, first.get("goPackage").and_then(|v| v.as_str()))
                {
                    let k = first.get("importers").and_then(|v| v.as_u64()).unwrap_or(0);
                    let head = plural(n, "widely imported package", "widely imported packages");
                    let sep = if n == 1 { ": " } else { ", e.g. " };
                    let tail = plural(k, "importer", "importers");
                    return with_path(&format!("{head}{sep}"), pkg, &format!(" ({tail})"), width);
                }
                let path = first.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                let k = first
                    .get("inboundEdges")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let head = plural(n, "widely imported file", "widely imported files");
                let sep = if n == 1 { ": " } else { ", e.g. " };
                return with_path(&format!("{head}{sep}"), path, &format!(" ({k})"), width);
            }
        }
        "F-TEST-INVENTORY-RATIO" => {
            let t = observed(f, "scan.testFileCount").and_then(|v| v.as_u64());
            let s = observed(f, "scan.sourceFileCount").and_then(|v| v.as_u64());
            if let (Some(t), Some(s)) = (t, s) {
                return crate::autodraft::test_ratio_line(t, s);
            }
        }
        "F-CARGO-LOCK-STATE" => {
            let state = observed(f, "profile.lockfileState").and_then(|v| v.as_str());
            return match state {
                Some("presentTrackingUnknown") => {
                    "Cargo.lock is present (whether git tracks it wasn't checked)".to_string()
                }
                _ => f.title.clone(),
            };
        }
        "F-NO-ENTRY-POINTS" => return "No entry point (main file) was found".to_string(),
        _ => {}
    }
    f.title.clone()
}

/// Render evidence findings as calibration-first compact markdown. Pure
/// function of the pack id and the findings — deterministic given the same
/// inputs (findings arrive in `SummaryService`'s stable builder order).
fn render_findings_markdown(pack_id: &str, findings: &[EvidenceFinding]) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let _ = writeln!(out, "# Evidence Findings — {pack_id}");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "_Calibrated observations for this pack. Each finding separates what was \
         observed (evidence) from what is inferred, with explicit limitations and a \
         calibration grade. Treat them as premises to verify, not verdicts._"
    );
    let _ = writeln!(out);

    if findings.is_empty() {
        let _ = writeln!(out, "No evidence findings for this pack.");
        return out;
    }

    let _ = writeln!(out, "{} finding(s).", findings.len());
    let _ = writeln!(out);

    for f in findings {
        let _ = writeln!(out, "### {}", f.title);
        let _ = writeln!(
            out,
            "`{:?}` · severity={:?} · grade={:?} · confidence={:?} · layer={:?}",
            f.classification, f.severity, f.evidence_grade, f.confidence, f.knowledge_layer
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "**Inference:** {} _({:?})_",
            f.inference.statement, f.inference.strength
        );
        if !f.inference.depends_on.is_empty() {
            let _ = writeln!(out, "**Depends on:** {}", f.inference.depends_on.join("; "));
        }
        if !f.inference.does_not_imply.is_empty() {
            let _ = writeln!(
                out,
                "**Does not imply:** {}",
                f.inference.does_not_imply.join("; ")
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "**Evidence:**");
        for item in &f.evidence {
            let _ = writeln!(out, "- {}", render_evidence_item(item));
        }
        if !f.limitations.is_empty() {
            let codes = dedup_preserving_order(f.limitations.iter().map(|l| l.code.as_str()));
            let _ = writeln!(out, "**Limitations:** {}", codes.join(", "));
        }
        if !f.not_checked.is_empty() {
            let codes = dedup_preserving_order(f.not_checked.iter().map(|n| n.code.as_str()));
            let _ = writeln!(out, "**Not checked:** {}", codes.join(", "));
        }
        let _ = writeln!(out, "**Action:** {}", render_action(&f.action));
        let _ = writeln!(out);
    }

    out
}

/// Render a single evidence item as one compact line.
fn render_evidence_item(item: &EvidenceItem) -> String {
    match item {
        EvidenceItem::Observation {
            metric,
            observed_value,
            ..
        } => format!("{} = {}", metric, render_value(observed_value)),
        EvidenceItem::HeuristicJudgment {
            rule,
            observed_value,
            threshold,
            ..
        } => format!(
            "rule `{}` (observed {}, threshold {})",
            rule,
            render_value(observed_value),
            render_value(threshold)
        ),
        EvidenceItem::ExternalSignal {
            path, signal_kind, ..
        } => format!("external `{path}` ({signal_kind:?})"),
    }
}

/// Collect codes in first-occurrence order, dropping duplicates. The
/// codes-only views (limitations, not-checked) would otherwise repeat a code
/// when a finding carries several entries that share it.
fn dedup_preserving_order<'a>(items: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = std::collections::HashSet::new();
    items.filter(|code| seen.insert(*code)).collect()
}

/// Render a JSON value compactly: strings unquoted, everything else as JSON.
fn render_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Render the recommended action as a single line.
pub(crate) fn render_action(action: &RecommendedAction) -> String {
    match action {
        RecommendedAction::NoAction => "No action.".to_string(),
        RecommendedAction::ReviewIfMaintainerPain => {
            "Review if this causes maintainer pain.".to_string()
        }
        RecommendedAction::ConcreteAction { steps } if steps.is_empty() => {
            "Action required (no steps provided).".to_string()
        }
        RecommendedAction::ConcreteAction { steps } => format!("Steps: {}", steps.join("; ")),
        RecommendedAction::ExternalVerification { commands_or_checks }
            if commands_or_checks.is_empty() =>
        {
            "External verification required (no checks provided).".to_string()
        }
        RecommendedAction::ExternalVerification { commands_or_checks } => {
            format!("Verify externally: {}", commands_or_checks.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::epistemic::{EpistemicConfidence, EvidenceGrade, KnowledgeLayer};
    use repolens_core::model::evidence::{
        metric_names, EvidenceScope, EvidenceSource, FindingClassification, FindingId,
        FindingSeverity, InferenceStrength, Limitation, ResponsibleInference, ScopeTime,
    };
    use serde_json::json;

    fn sample_finding() -> EvidenceFinding {
        EvidenceFinding {
            id: FindingId::new("F-NO-ENTRY-POINTS"),
            title: "No entry points detected".into(),
            classification: FindingClassification::Expected,
            severity: FindingSeverity::None,
            knowledge_layer: KnowledgeLayer::Observation,
            evidence_grade: EvidenceGrade::Unaudited,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::PROFILE_ENTRY_POINT_COUNT.into(),
                observed_value: json!(0),
                source_paths: vec!["Cargo.toml".into()],
                source_type: EvidenceSource::ProjectProfile,
                invariant_id: None,
            }],
            inference: ResponsibleInference {
                statement: "No CLI binary or main function was detected.".into(),
                strength: InferenceStrength::Suggestive,
                depends_on: vec!["projectType=Library".into()],
                does_not_imply: vec!["the crate is unused".into()],
            },
            limitations: vec![Limitation {
                code: "ENTRY_POINT_DETECTION_LANGUAGE_LIMITED".into(),
                detail: "per-language detection".into(),
            }],
            action: RecommendedAction::NoAction,
            scope: EvidenceScope {
                paths: vec![],
                time: ScopeTime {
                    pack_id: "pack-20260516-015206".into(),
                    commit: None,
                },
                languages: vec!["rust".into()],
            },
            not_checked: vec![],
            repo_context: vec![],
            false_positive_notes: vec![],
            provenance: None,
        }
    }

    fn gap_finding(breakdown: serde_json::Value) -> EvidenceFinding {
        let mut f = sample_finding();
        f.id = FindingId::new("F-LANGUAGE-EXTRACTION-GAP");
        f.severity = FindingSeverity::Low;
        f.evidence = vec![EvidenceItem::Observation {
            metric: metric_names::EXTRACTION_LANGUAGES_BY_FIDELITY.into(),
            observed_value: breakdown,
            source_paths: vec![],
            source_type: EvidenceSource::ProjectProfile,
            invariant_id: None,
        }];
        f
    }

    const PY_NOTE: &str = "Python: imports read; cross-file calls and types not yet";

    /// #32: the coverage note is not a finding row and not counted; it
    /// sits once below the rows.
    #[test]
    fn plain_gap_python_only_is_a_coverage_line_not_a_row() {
        let f = gap_finding(json!({
            "heuristic": {"languages": ["Python"], "loc": 12032, "notes": [PY_NOTE]}
        }));
        let out = render_findings_plain("p", &[f]);
        assert!(
            out.starts_with("The last scan (p) found nothing to flag.\n"),
            "{out}"
        );
        assert!(out.contains(&format!("\n  Coverage: {PY_NOTE}\n")), "{out}");
        assert!(!out.contains("aren't read"), "{out}");
        assert_eq!(out.matches(PY_NOTE).count(), 1, "{out}");
        assert!(out.lines().all(|l| cols(l) <= 80), "{out}");
    }

    #[test]
    fn plain_gap_unsupported_line_then_python_note_below_rows() {
        let gap = gap_finding(json!({
            "heuristic": {"languages": ["Python"], "loc": 11232, "notes": [PY_NOTE]},
            "unsupported": {"languages": ["C"], "loc": 3531}
        }));
        let mut other = sample_finding();
        other.severity = FindingSeverity::Medium;
        let out = render_findings_plain("p", &[gap, other]);
        assert!(
            out.starts_with("The last scan (p) found 1 thing worth a look:\n"),
            "{out}"
        );
        assert!(
            out.contains(&format!(
                "\n  Coverage: imports aren't read for 1 language: C (3531 lines)\n            {PY_NOTE}\n"
            )),
            "{out}"
        );
        let row = out.find("  Med   ").unwrap();
        assert!(out.find("  Coverage: ").unwrap() > row, "{out}");
        assert!(out.lines().all(|l| cols(l) <= 80), "{out}");
    }

    #[test]
    fn coverage_note_wraps_long_language_lists_at_words() {
        let f = gap_finding(json!({
            "unsupported": {"languages": ["C", "C++", "Haskell", "Lua", "Perl", "Ruby", "Shell", "Swift"], "loc": 123456}
        }));
        let lines = PlainFindings::from_findings(&[f]).coverage;
        assert!(lines.len() > 1, "{lines:?}");
        for l in &lines {
            assert!(cols(l) <= 80, "{l}");
            assert!(!l.ends_with('…'), "{l}");
        }
        let joined: Vec<&str> = lines.iter().flat_map(|l| l.split_whitespace()).collect();
        assert!(joined.contains(&"Haskell,"), "{lines:?}");
    }

    fn fan_in_finding(inbound: serde_json::Value) -> EvidenceFinding {
        let mut f = sample_finding();
        f.id = FindingId::new("F-HIGH-FAN-IN-FILE");
        f.evidence = vec![EvidenceItem::Observation {
            metric: metric_names::GRAPH_INBOUND_EDGES.into(),
            observed_value: inbound,
            source_paths: vec![],
            source_type: EvidenceSource::ImportGraph,
            invariant_id: None,
        }];
        f
    }

    /// Design call 3 on #31: when every file over the fan-in threshold was
    /// filtered, the finding shows a "Left out: N files" line that counts
    /// module hubs too, instead of disappearing.
    #[test]
    fn fan_in_with_nothing_listed_shows_left_out_line_counting_hubs() {
        let f = fan_in_finding(json!({
            "sample": [],
            "sampleComplete": true,
            "sharedCrateFilesLeftOut": {"count": 2, "sample": []},
            "moduleHubFilesLeftOut": {"count": 3, "sample": []}
        }));
        let out = render_findings_plain("p", &[f]);
        assert!(
            out.contains(
                "  Info  No widely imported files listed: all are module hubs or shared crates\n"
            ),
            "{out}"
        );
        assert!(
            out.contains(
                "\n        Left out: 5 files (lib.rs/mod.rs/prelude.rs, *-core/*-util crates)\n"
            ),
            "{out}"
        );
        assert!(out.lines().all(|l| cols(l) <= 80), "{out}");
    }

    /// With files listed, the line still counts only shared-crate files
    /// (hub modules stay in --verbose's limitation, as before).
    #[test]
    fn fan_in_with_files_listed_keeps_the_shared_crate_line() {
        let mut f = fan_in_finding(json!({
            "sample": [{"path": "src/a.rs", "inboundEdges": 9}],
            "sampleComplete": true,
            "sharedCrateFilesLeftOut": {"count": 1, "sample": []},
            "moduleHubFilesLeftOut": {"count": 4, "sample": []}
        }));
        f.severity = FindingSeverity::Medium;
        assert_eq!(
            finding_notes(&f),
            vec!["Left out: 1 file in shared *-core/*-util crates (see --verbose)".to_string()]
        );
    }

    /// #24: a Go fan-in entry names the package and its importers, not
    /// the one file that stands for it.
    #[test]
    fn fan_in_names_a_go_package() {
        let mut f = fan_in_finding(json!({
            "sample": [{"path": "cobra.go", "inboundEdges": 11,
                        "goPackage": "cobra",
                        "importPath": "github.com/spf13/cobra",
                        "importers": 11}],
            "sampleComplete": true
        }));
        f.severity = FindingSeverity::Medium;
        let out = render_findings_plain("p", &[f]);
        assert!(
            out.contains("  Med   1 widely imported package: cobra (11 importers)\n"),
            "{out}"
        );
        assert!(!out.contains("cobra.go"), "{out}");
        // Mixed with a non-Go file, the line stays about files.
        let mut f = fan_in_finding(json!({
            "sample": [{"path": "cobra.go", "inboundEdges": 11,
                        "goPackage": "cobra", "importers": 11},
                       {"path": "web/app.js", "inboundEdges": 5}],
            "sampleComplete": true
        }));
        f.severity = FindingSeverity::Medium;
        let out = render_findings_plain("p", &[f]);
        assert!(
            out.contains("2 widely imported files, e.g. cobra.go (11)"),
            "{out}"
        );
        assert!(out.lines().all(|l| cols(l) <= 80), "{out}");
    }

    /// QA bug 2 on #41: widths are terminal columns, so a long Chinese
    /// path is shortened at a folder boundary to fit 80 columns.
    #[test]
    fn long_chinese_path_fits_80_columns_and_is_cut_at_a_folder() {
        let path = "文档/加密算法/国密标准/实现细节/椭圆曲线/签名验证/模块/核心/工具函数.rs";
        let f = fan_in_finding(json!({
            "sample": [{"path": path, "inboundEdges": 12},
                       {"path": "src/b.rs", "inboundEdges": 5}],
            "sampleComplete": true
        }));
        let mut f = f;
        f.severity = FindingSeverity::Medium;
        let out = render_findings_plain("p", &[f]);
        let row = out.lines().find(|l| l.starts_with("  Med   ")).unwrap();
        assert!(cols(row) <= 80, "{} columns: {row}", cols(row));
        assert!(row.contains("…/"), "{row}");
        assert!(row.ends_with("工具函数.rs (12)"), "{row}");
        // The kept tail starts at a folder boundary: "…/" then a whole
        // folder name from the original path.
        let tail = row.split("…/").nth(1).unwrap();
        let tail = tail.trim_end_matches(" (12)");
        assert!(path.ends_with(&format!("/{tail}")), "{tail}");
        assert!(out.lines().all(|l| cols(l) <= 80), "{out}");
    }

    #[test]
    fn fit_line_counts_wide_characters_as_two_columns() {
        let out = fit_line("中文 中文 中文 中文".to_string(), 12);
        assert!(cols(&out) <= 12, "{out}");
        assert_eq!(out, "中文 中文…");
    }

    #[test]
    fn fit_line_cuts_at_a_word_boundary() {
        let out = fit_line("alpha beta gamma delta".to_string(), 13);
        assert_eq!(out, "alpha beta…");
        assert_eq!(fit_line("short".to_string(), 13), "short");
    }

    /// #32: a path is never cut mid-name; an example that doesn't fit is
    /// dropped instead (ManimSimpleRender's "…proxy_pattern_scene.py").
    #[test]
    fn with_path_never_cuts_a_file_name() {
        let out = with_path(
            "Some files may be unused (imports only): 8, e.g. ",
            "explainer/scenes/very_long_proxy_pattern_scene.py",
            "",
            72,
        );
        assert_eq!(out, "Some files may be unused (imports only): 8");
        let out = with_path(
            "8 widely imported files, e.g. ",
            "a/b/c/src/sm4/scalar.rs",
            " (11)",
            50,
        );
        assert_eq!(out, "8 widely imported files, e.g. …/sm4/scalar.rs (11)");
    }

    fn orphan_finding(count: serde_json::Value, sample: &[&str]) -> EvidenceFinding {
        let mut f = sample_finding();
        f.id = FindingId::new("F-ORPHAN-FILES");
        f.title = "No unused files found (imports only)".into();
        f.evidence = vec![EvidenceItem::Observation {
            metric: metric_names::GRAPH_ORPHAN_COUNT.into(),
            observed_value: count,
            source_paths: vec![],
            source_type: EvidenceSource::ImportGraph,
            invariant_id: None,
        }];
        if !sample.is_empty() {
            f.title = crate::autodraft::ORPHAN_LINE.into();
            f.evidence.push(EvidenceItem::Observation {
                metric: metric_names::GRAPH_ORPHAN_FILES.into(),
                observed_value: json!({"sample": sample, "sampleComplete": true}),
                source_paths: vec![],
                source_type: EvidenceSource::ImportGraph,
                invariant_id: None,
            });
        }
        f
    }

    /// Design call on #44: a "filtered non-source artefacts" row says
    /// nothing, so the default view hides it (and doesn't count it).
    #[test]
    fn plain_hides_filtered_only_orphan_row() {
        for count in [
            json!(97),
            json!({"graphOrphanCount": 97, "sourceOrphanCount": 0, "loadedByNameCount": 0}),
        ] {
            let out =
                render_findings_plain("p", &[orphan_finding(count, &[]), gap_finding(json!({}))]);
            assert!(!out.contains("No unused files"), "{out}");
            assert!(!out.contains("may be unused"), "{out}");
            // #41 shows the extraction gap as a coverage note, not a finding,
            // and the hidden row leaves nothing under "Also noted".
            assert!(out.contains("found nothing to flag."), "{out}");
            assert!(out.contains("  Coverage: "), "{out}");
            assert!(!out.contains("Also noted"), "{out}");
        }
        let out = render_findings_plain("p", &[orphan_finding(json!(5), &[])]);
        assert!(out.contains("found nothing to flag"), "{out}");
        // Still in --verbose.
        let md = render_findings_markdown("p", &[orphan_finding(json!(5), &[])]);
        assert!(
            md.contains("### No unused files found (imports only)"),
            "{md}"
        );
    }

    /// Files left out because they're loaded by name are counted, not
    /// dropped silently: under the row, or on a "none" row of their own.
    #[test]
    fn plain_counts_files_left_out_as_loaded_by_name() {
        let f = orphan_finding(
            json!({"graphOrphanCount": 97, "sourceOrphanCount": 0, "loadedByNameCount": 10}),
            &[],
        );
        let out = render_findings_plain("p", &[f]);
        assert!(
            out.contains(
                "  Info  No unused files found (imports only)\n        \
                 Left out: 10 files loaded by name (tests, build scripts, configs, …)\n"
            ),
            "{out}"
        );
        let f = orphan_finding(
            json!({"graphOrphanCount": 191, "sourceOrphanCount": 14, "loadedByNameCount": 1}),
            &["gson/src/main/java/com/google/gson/internal/UnsafeAllocator.java"],
        );
        let out = render_findings_plain("p", &[f]);
        assert!(
            out.contains(
                "\n        Left out: 1 file loaded by name (tests, build scripts, configs, …)\n"
            ),
            "{out}"
        );
        assert!(out.lines().all(|l| cols(l) <= 80), "{out}");
        // zod's count fits whole, measured in display columns.
        let f = orphan_finding(
            json!({"graphOrphanCount": 544, "sourceOrphanCount": 86, "loadedByNameCount": 291}),
            &["mm.mjs"],
        );
        let out = render_findings_plain("p", &[f]);
        let line = "        Left out: 291 files loaded by name (tests, build scripts, configs, …)";
        assert!(out.lines().any(|l| l == line), "{out}");
        assert!(cols(line) <= 80);
        // No count (older packs): no line.
        let f = orphan_finding(
            json!({"graphOrphanCount": 9, "sourceOrphanCount": 2}),
            &["src/a.rs"],
        );
        assert!(!render_findings_plain("p", &[f]).contains("Left out"));
    }

    #[test]
    fn renders_header_with_pack_id_and_count() {
        let md = render_findings_markdown("pack-20260516-015206", &[sample_finding()]);
        assert!(
            md.contains("# Evidence Findings — pack-20260516-015206"),
            "md=\n{md}"
        );
        assert!(md.contains("1 finding"), "md=\n{md}");
        assert!(
            md.contains("premises to verify"),
            "calibration disclaimer missing; md=\n{md}"
        );
    }

    #[test]
    fn renders_title_and_tag_strip() {
        let md = render_findings_markdown("p", &[sample_finding()]);
        assert!(md.contains("### No entry points detected"), "md=\n{md}");
        assert!(md.contains("grade=Unaudited"), "md=\n{md}");
        assert!(md.contains("confidence=Codified"), "md=\n{md}");
        assert!(md.contains("severity=None"), "md=\n{md}");
    }

    #[test]
    fn renders_inference_and_action_and_evidence() {
        let md = render_findings_markdown("p", &[sample_finding()]);
        assert!(
            md.contains("No CLI binary or main function was detected."),
            "md=\n{md}"
        );
        assert!(
            md.contains("profile.entryPointCount = 0"),
            "evidence observation line missing; md=\n{md}"
        );
        assert!(md.contains("No action"), "action missing; md=\n{md}");
    }

    #[test]
    fn renders_limitations_codes() {
        let md = render_findings_markdown("p", &[sample_finding()]);
        assert!(
            md.contains("ENTRY_POINT_DETECTION_LANGUAGE_LIMITED"),
            "limitation code missing; md=\n{md}"
        );
    }

    #[test]
    fn limitation_codes_are_deduped_preserving_order() {
        let mut f = sample_finding();
        f.limitations = vec![
            Limitation {
                code: "DUP".into(),
                detail: "a".into(),
            },
            Limitation {
                code: "DUP".into(),
                detail: "b".into(),
            },
            Limitation {
                code: "OTHER".into(),
                detail: "c".into(),
            },
        ];
        let md = render_findings_markdown("p", &[f]);
        assert!(
            md.contains("**Limitations:** DUP, OTHER"),
            "codes-only view must dedupe repeated codes; md=\n{md}"
        );
    }

    #[test]
    fn empty_findings_renders_explicit_message() {
        let md = render_findings_markdown("pack-20260516-015206", &[]);
        assert!(
            md.contains("# Evidence Findings — pack-20260516-015206"),
            "md=\n{md}"
        );
        assert!(
            md.contains("No evidence findings for this pack."),
            "md=\n{md}"
        );
    }

    #[test]
    fn render_is_deterministic() {
        let findings = [sample_finding()];
        let a = render_findings_markdown("p", &findings);
        let b = render_findings_markdown("p", &findings);
        assert_eq!(a, b);
    }

    #[test]
    fn renders_concrete_action_steps() {
        let mut f = sample_finding();
        f.action = RecommendedAction::ConcreteAction {
            steps: vec!["do x".into(), "do y".into()],
        };
        let md = render_findings_markdown("p", &[f]);
        assert!(md.contains("do x"), "md=\n{md}");
        assert!(md.contains("do y"), "md=\n{md}");
    }

    #[test]
    fn renders_empty_action_vectors_without_dangling_label() {
        let mut f = sample_finding();
        f.action = RecommendedAction::ConcreteAction { steps: vec![] };
        let md = render_findings_markdown("p", &[f]);
        assert!(
            md.contains("Action required (no steps provided)."),
            "md=\n{md}"
        );
        assert!(
            !md.contains("Steps: \n"),
            "must not emit a dangling label; md=\n{md}"
        );

        let mut f2 = sample_finding();
        f2.action = RecommendedAction::ExternalVerification {
            commands_or_checks: vec![],
        };
        let md2 = render_findings_markdown("p", &[f2]);
        assert!(
            md2.contains("External verification required"),
            "md2=\n{md2}"
        );
    }

    // -- Pack resolution -----------------------------------------------------

    fn touch_pack(repolens: &Path, id: &str) -> PathBuf {
        let dir = repolens.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        dir
    }

    fn write_session(repolens: &Path, root: &Path, active: &Path) {
        let session = json!({
            "schemaVersion": "1.0",
            "sessionId": "s",
            "repoRoot": root.to_str().unwrap(),
            "activePackPath": active.to_str().unwrap(),
            "activeReportPath": null,
            "updatedAt": "2026-05-16T01:52:06.000000Z",
            "lastAccessed": "2026-05-16T01:52:06.000000Z",
            "lastScanMode": "full",
            "reprocessedFiles": 0,
            "deletedFiles": 0,
            "carriedForwardFiles": 0
        });
        std::fs::write(
            repolens.join("session.json"),
            serde_json::to_string_pretty(&session).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn resolve_findings_pack_uses_explicit_pack() {
        let dir = tempfile::tempdir().unwrap();
        let repolens = dir.path().join(".repolens");
        let pack = touch_pack(&repolens, "pack-20260101-000000");
        let got = resolve_findings_pack(dir.path(), Some("pack-20260101-000000")).unwrap();
        assert_eq!(got, pack);
    }

    #[test]
    fn resolve_findings_pack_prefers_active_session_pack_over_latest() {
        let dir = tempfile::tempdir().unwrap();
        let repolens = dir.path().join(".repolens");
        let older = touch_pack(&repolens, "pack-20260101-000000");
        let _newer = touch_pack(&repolens, "pack-20260102-000000");
        // Session points at the OLDER pack — resolution must honor it, not
        // silently pick the latest on disk.
        write_session(&repolens, dir.path(), &older);
        let got = resolve_findings_pack(dir.path(), None).unwrap();
        assert_eq!(got, older);
    }

    #[test]
    fn resolve_findings_pack_skips_manifestless_newer_pack() {
        let dir = tempfile::tempdir().unwrap();
        let repolens = dir.path().join(".repolens");
        let older = touch_pack(&repolens, "pack-20260101-000000");
        // Newer dir exists but has no manifest (interrupted scan).
        std::fs::create_dir_all(repolens.join("pack-20260102-000000")).unwrap();
        let got = resolve_findings_pack(dir.path(), None).unwrap();
        assert_eq!(got, older, "must skip the manifest-less newer pack");
    }

    #[test]
    fn resolve_findings_pack_falls_back_to_latest_without_session() {
        let dir = tempfile::tempdir().unwrap();
        let repolens = dir.path().join(".repolens");
        let _older = touch_pack(&repolens, "pack-20260101-000000");
        let newer = touch_pack(&repolens, "pack-20260102-000000");
        let got = resolve_findings_pack(dir.path(), None).unwrap();
        assert_eq!(got, newer);
    }

    #[test]
    fn resolve_findings_pack_errors_when_no_packs() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_findings_pack(dir.path(), None).unwrap_err();
        assert!(
            err.to_string().contains("Run 'repolens scan' first"),
            "got: {err}"
        );
    }
}
