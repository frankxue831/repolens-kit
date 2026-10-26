//! `repolens eval list` / `repolens eval start --case <ACE-ID> --run-id <RUN-ID>`
//!
//! Round 5 — lightweight, deterministic ACE eval scaffolding. This module:
//!   - embeds `docs/evals/agent-context-evals.md` at compile time and parses
//!     it into a list of ACE cases on first use, so `repolens eval list`
//!     works on installed binaries without runtime access to the doc tree;
//!   - scaffolds an isolated run directory under
//!     `.repolens/eval-runs/<CASE_ID>-<RUN_ID>/` with four human-fillable
//!     files (`case.md`, `scorecard.md`, `agent-transcript.md`,
//!     `result.md`).
//!
//! What this module **does not** do, by design:
//!   - It does not launch agents, call MCP, call LLMs, or grade runs.
//!   - It does not write the typed memory graph
//!     (`.repolens/memory/memory.json`).
//!   - It does not modify `agent-friction-log.md` or any other workspace
//!     file outside the per-run directory.
//!
//! ## Round 5.2 — eval artifact provenance
//!
//! Each generated artifact carries a small provenance block (between
//! `<!-- repolens:provenance -->` markers) recording the crate version,
//! template-set id, scorecard-schema id, generation timestamp, and source
//! commit at scaffold time. This lets reviewers spot binary/template drift
//! when comparing runs. The non-provenance content of each artifact is
//! still byte-deterministic across workspaces — the provenance fields are
//! the only run-varying data.

use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};
use repolens_core::atomic_write::write_atomic;
use repolens_core::storage::RepoStoragePaths;
use std::fs;
use std::path::{Path, PathBuf};

use crate::resolve_root_from;

const ACE_DOC: &str = include_str!("../../../docs/evals/agent-context-evals.md");

const CASE_TEMPLATE: &str = include_str!("../../../docs/templates/repolens/eval-runs/case.md");
const SCORECARD_TEMPLATE: &str =
    include_str!("../../../docs/templates/repolens/eval-runs/scorecard.md");
const TRANSCRIPT_TEMPLATE: &str =
    include_str!("../../../docs/templates/repolens/eval-runs/agent-transcript.md");
const RESULT_TEMPLATE: &str = include_str!("../../../docs/templates/repolens/eval-runs/result.md");

const EVAL_RUNS_SUBDIR: &str = "eval-runs";
const MAX_RUN_ID_LEN: usize = 64;

/// Stable identifier for the artifact template set. Bump when any template
/// changes shape in a way reviewers should notice across runs.
pub(crate) const TEMPLATE_SET_ID: &str = "ace-eval@round-5.2";

/// Stable identifier for the scorecard metric schema (the set of metric
/// names + accepted values + blank-vs-`n/a` rules). Bump when the metric
/// vocabulary changes.
pub(crate) const SCORECARD_SCHEMA_ID: &str = "repolens-scorecard@round-5.2";

const PROVENANCE_BEGIN: &str = "<!-- repolens:provenance -->";
const PROVENANCE_END: &str = "<!-- /repolens:provenance -->";

#[derive(Args, Debug)]
pub struct EvalArgs {
    #[command(subcommand)]
    pub command: EvalCommands,
}

#[derive(Subcommand, Debug)]
pub enum EvalCommands {
    /// List the built-in agent eval cases (id, title, why it matters).
    List(EvalListArgs),
    /// Create a folder for a new eval run under `.repolens/eval-runs/`.
    Start(EvalStartArgs),
}

#[derive(Args, Debug)]
pub struct EvalListArgs {
    /// Ignored: the case list is built in and doesn't read `.repolens/`.
    /// Accepted so every command takes `--root`.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct EvalStartArgs {
    /// Eval case id from `repolens eval list`, e.g. `ACE-010`.
    #[arg(long)]
    pub case: String,
    /// Run identifier. Letters, digits, `.`, `_`, `-`. Max 64 chars. No path
    /// separators or `..`.
    #[arg(long = "run-id")]
    pub run_id: String,
    /// Path to the repository root (defaults to enclosing git repo, then cwd).
    #[arg(long)]
    pub root: Option<PathBuf>,
}

pub fn run_eval(args: EvalArgs) -> Result<()> {
    match args.command {
        EvalCommands::List(_) => run_list(),
        EvalCommands::Start(args) => run_start(args),
    }
}

fn run_list() -> Result<()> {
    let cases = parse_ace_cases(ACE_DOC);
    if cases.is_empty() {
        bail!("No ACE cases found in embedded eval doc — this is a build-time bug.");
    }
    print!("{}", format_case_list(&cases));
    Ok(())
}

fn run_start(args: EvalStartArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;

    let storage_dir = RepoStoragePaths::storage_dir(&root);
    if !storage_dir.is_dir() {
        bail!("RepoLens workspace not found. Run 'repolens init' first.");
    }

    validate_run_id(&args.run_id)?;
    let cases = parse_ace_cases(ACE_DOC);
    let case = cases.iter().find(|c| c.id == args.case).ok_or_else(|| {
        anyhow!(
            "Unknown ACE case '{}'. Run 'repolens eval list' to see available cases.",
            args.case
        )
    })?;

    let run_dir = storage_dir
        .join(EVAL_RUNS_SUBDIR)
        .join(format!("{}-{}", case.id, args.run_id));
    if run_dir.exists() {
        bail!(
            "Eval run directory already exists: {}\n\
             Refusing to overwrite. Use a different --run-id or remove the directory by hand.",
            run_dir.display()
        );
    }

    fs::create_dir_all(&run_dir).map_err(|e| {
        anyhow!(
            "Cannot create eval run directory at {}: {}",
            run_dir.display(),
            e
        )
    })?;

    let provenance = Provenance {
        generated_by: format!("repolens@{}", env!("CARGO_PKG_VERSION")),
        template_set: TEMPLATE_SET_ID.to_string(),
        scorecard_schema: SCORECARD_SCHEMA_ID.to_string(),
        generated_at: now_iso8601_utc(),
        source_commit: current_git_commit(&root),
    };

    write_run_files(&run_dir, case, &args.run_id, &provenance)?;

    println!(
        "Eval run scaffolded at {}",
        display_relative_to_storage(&storage_dir, &run_dir)
    );
    println!("  + case.md");
    println!("  + scorecard.md");
    println!("  + agent-transcript.md");
    println!("  + result.md");
    println!();
    println!(
        "Next: read case.md, run the agent, then fill scorecard.md, agent-transcript.md, and result.md by hand."
    );
    Ok(())
}

fn write_run_files(
    run_dir: &Path,
    case: &AceCase,
    run_id: &str,
    provenance: &Provenance,
) -> Result<()> {
    for (name, body) in [
        ("case.md", CASE_TEMPLATE),
        ("scorecard.md", SCORECARD_TEMPLATE),
        ("agent-transcript.md", TRANSCRIPT_TEMPLATE),
        ("result.md", RESULT_TEMPLATE),
    ] {
        let rendered = render_template(body, case, run_id);
        let stamped = prepend_provenance(&rendered, provenance);
        let target = run_dir.join(name);
        write_atomic(&target, stamped.as_bytes())
            .map_err(|e| anyhow!("Failed to write {} to {}: {}", name, target.display(), e))?;
    }
    Ok(())
}

fn render_template(body: &str, case: &AceCase, run_id: &str) -> String {
    body.replace("{{CASE_ID}}", &case.id)
        .replace("{{CASE_TITLE}}", &case.title)
        .replace("{{CASE_BODY}}", &case.body)
        .replace("{{RUN_ID}}", run_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Provenance {
    pub generated_by: String,
    pub template_set: String,
    pub scorecard_schema: String,
    pub generated_at: String,
    pub source_commit: String,
}

impl Provenance {
    pub(crate) fn render(&self) -> String {
        format!(
            "{begin}\n\
             > **Provenance.** Recorded by `repolens eval start` at scaffold time. \
             Reviewers: if these fields drift between runs of the same case, \
             that drift is itself a finding.\n\
             > - **generated_by:** `{by}`\n\
             > - **template_set:** `{ts}`\n\
             > - **scorecard_schema:** `{ss}`\n\
             > - **generated_at:** `{at}`\n\
             > - **source_commit:** `{commit}`\n\
             {end}\n\n",
            begin = PROVENANCE_BEGIN,
            by = self.generated_by,
            ts = self.template_set,
            ss = self.scorecard_schema,
            at = self.generated_at,
            commit = self.source_commit,
            end = PROVENANCE_END,
        )
    }
}

/// Insert the provenance block immediately after the template's leading
/// `<!-- ... -->` editor-facing comment, so the provenance is the first
/// rendered content the agent/reviewer sees. If the template has no
/// leading comment, the block is prepended.
fn prepend_provenance(body: &str, provenance: &Provenance) -> String {
    let block = provenance.render();
    let leading_ws_len = body.len() - body.trim_start_matches(['\n', '\r']).len();
    let after_ws = &body[leading_ws_len..];
    let Some(after_open) = after_ws.strip_prefix("<!--") else {
        return format!("{block}{body}");
    };
    let Some(end_rel) = after_open.find("-->") else {
        return format!("{block}{body}");
    };
    let close_end = leading_ws_len + 4 + end_rel + 3;
    let mut out = String::with_capacity(body.len() + block.len() + 2);
    out.push_str(&body[..close_end]);
    out.push_str("\n\n");
    out.push_str(&block);
    let rest = &body[close_end..];
    let rest_trimmed = rest.trim_start_matches(['\n', '\r']);
    out.push_str(rest_trimmed);
    out
}

/// Strip the provenance block (if any) from a rendered artifact body.
/// Used by tests that want to compare the deterministic remainder.
#[cfg(test)]
pub(crate) fn strip_provenance(body: &str) -> String {
    let Some(start) = body.find(PROVENANCE_BEGIN) else {
        return body.to_string();
    };
    let after = &body[start..];
    let Some(end_rel) = after.find(PROVENANCE_END) else {
        return body.to_string();
    };
    let end = start + end_rel + PROVENANCE_END.len();
    let mut out = String::with_capacity(body.len());
    out.push_str(&body[..start]);
    let tail = &body[end..];
    out.push_str(tail.trim_start_matches(['\n', '\r']));
    out
}

fn now_iso8601_utc() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Best-effort `git rev-parse HEAD` against `root`. Returns `"unknown"`
/// when git is unavailable, `root` is not a repo, or the call errors —
/// scaffolding never fails because of provenance lookup.
fn current_git_commit(root: &Path) -> String {
    use std::process::Command;
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "unknown".to_string()
            } else {
                s
            }
        }
        _ => "unknown".to_string(),
    }
}

/// Format the embedded ACE case list as plain deterministic text.
///
/// Shape per case:
///
/// ```text
/// ACE-001 — Cold-start orientation
///   <Why-it-matters body, single line>
///
/// ```
///
/// A trailing blank line follows every case for stable diffs.
fn format_case_list(cases: &[AceCase]) -> String {
    let mut out = String::new();
    for case in cases {
        out.push_str(&case.id);
        out.push_str(" — ");
        out.push_str(&case.title);
        out.push('\n');
        if !case.purpose.is_empty() {
            out.push_str("  ");
            out.push_str(&case.purpose);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Render the run directory path relative to the storage dir for the success
/// message, e.g. `.repolens/eval-runs/ACE-010-stale-checkout-001`. Falls back
/// to an absolute path if relativization fails.
fn display_relative_to_storage(storage_dir: &Path, run_dir: &Path) -> String {
    let storage_name = storage_dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".repolens".to_string());
    if let Ok(rel) = run_dir.strip_prefix(storage_dir) {
        format!("{}/{}", storage_name, rel.display())
    } else {
        run_dir.display().to_string()
    }
}

/// Validate that `run_id` is safe to use as a path component.
fn validate_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty() {
        bail!("--run-id is empty");
    }
    if run_id.len() > MAX_RUN_ID_LEN {
        bail!(
            "--run-id is too long ({} chars; max {})",
            run_id.len(),
            MAX_RUN_ID_LEN
        );
    }
    if run_id == "." || run_id == ".." {
        bail!("--run-id cannot be '.' or '..'");
    }
    if run_id.contains('/') || run_id.contains('\\') {
        bail!("--run-id cannot contain path separators");
    }
    if run_id.contains("..") {
        bail!("--run-id cannot contain '..'");
    }
    for c in run_id.chars() {
        let ok = c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
        if !ok {
            bail!(
                "--run-id contains invalid character '{}'; only [A-Za-z0-9._-] are allowed",
                c
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AceCase {
    pub id: String,
    pub title: String,
    pub purpose: String,
    pub body: String,
}

/// Parse the embedded ACE doc into a list of cases.
///
/// Recognises section headers of the form `## ACE-NNN — Title`. Body for each
/// case is the slice between its header and the next `## ` header (or EOF),
/// minus a trailing `---` separator line and surrounding blank lines.
/// `purpose` is extracted from the case body's `**Why it matters.**` line, if
/// present.
fn parse_ace_cases(doc: &str) -> Vec<AceCase> {
    let mut cases: Vec<AceCase> = Vec::new();
    let mut current: Option<(String, String, Vec<String>)> = None;

    fn finalize(cases: &mut Vec<AceCase>, current: Option<(String, String, Vec<String>)>) {
        if let Some((id, title, lines)) = current {
            let body = trim_case_body(&lines);
            let purpose = extract_why_it_matters(&body);
            cases.push(AceCase {
                id,
                title,
                purpose,
                body,
            });
        }
    }

    for line in doc.lines() {
        if let Some((id, title)) = parse_case_header(line) {
            finalize(&mut cases, current.take());
            current = Some((id, title, Vec::new()));
            continue;
        }
        if line.starts_with("## ") {
            // Non-case `## ` header (e.g., `## How to read a case`,
            // `## Suite-level rules`). Close the current case.
            finalize(&mut cases, current.take());
            continue;
        }
        if let Some((_, _, ref mut lines)) = current {
            lines.push(line.to_string());
        }
    }
    finalize(&mut cases, current);
    cases
}

fn parse_case_header(line: &str) -> Option<(String, String)> {
    // Match `## ACE-NNN — Title` (em-dash) or `## ACE-NNN - Title` (hyphen
    // fallback for tolerant parsing).
    let rest = line.strip_prefix("## ACE-")?;
    let mut digits_end = 0;
    for (i, ch) in rest.char_indices() {
        if ch.is_ascii_digit() {
            digits_end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if digits_end == 0 {
        return None;
    }
    let id = format!("ACE-{}", &rest[..digits_end]);
    let after = rest[digits_end..].trim_start();
    let title = after
        .strip_prefix("— ")
        .or_else(|| after.strip_prefix("- "))?
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some((id, title))
}

fn trim_case_body(lines: &[String]) -> String {
    // Drop a trailing `---` separator (and surrounding blanks) that the doc
    // uses between cases.
    let mut end = lines.len();
    while end > 0 {
        let last = lines[end - 1].trim();
        if last.is_empty() || last == "---" {
            end -= 1;
        } else {
            break;
        }
    }
    let mut start = 0;
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    lines[start..end].join("\n")
}

fn extract_why_it_matters(body: &str) -> String {
    const KEY: &str = "**Why it matters.**";
    let mut out = String::new();
    let mut iter = body.lines();
    while let Some(line) = iter.next() {
        if let Some(rest) = line.find(KEY) {
            let tail = &line[rest + KEY.len()..];
            out.push_str(tail.trim());
            // Continue gathering lines until a blank line ends the paragraph.
            for next in iter.by_ref() {
                if next.trim().is_empty() {
                    break;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(next.trim());
            }
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ace_cases_finds_all_eleven() {
        let cases = parse_ace_cases(ACE_DOC);
        let ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "ACE-001", "ACE-002", "ACE-003", "ACE-004", "ACE-005", "ACE-006", "ACE-007",
                "ACE-008", "ACE-009", "ACE-010", "ACE-011",
            ]
        );
    }

    #[test]
    fn parse_ace_cases_extracts_tightened_pass_criteria_for_ace_002() {
        // Round 5.2 follow-up: ACE-002 was loosened — a string-replacing
        // agent could pass without grounding the change. The tightened
        // case must require the first plan to (1) use camelCase, (2) name
        // the affected output surface, (3) ground the impl path or state
        // an explicit verification step, (4) mention tests/snapshots/docs.
        let cases = parse_ace_cases(ACE_DOC);
        let ace_002 = cases
            .iter()
            .find(|c| c.id == "ACE-002")
            .expect("ACE-002 missing from embedded doc");

        assert!(
            ace_002.title.contains("Constraint adherence"),
            "ACE-002 title regressed: {}",
            ace_002.title
        );
        let body = &ace_002.body;
        for phrase in [
            "totalCount",
            "total_count",
            "compare-summaries",
            "first-draft plan",
            "Respect the constraint",
            "Name the affected output surface",
            "Ground the implementation path",
            "Include verification",
            "verification step",
            "tests / snapshots / docs",
            "string-replacing",
        ] {
            assert!(
                body.contains(phrase),
                "ACE-002 body missing tightened-criteria phrase '{phrase}'; body=\n{body}"
            );
        }
        // Scoring vocabulary must be explicit and load-bearing.
        for verdict in ["`pass`", "`partial`", "`fail`"] {
            assert!(
                body.contains(verdict),
                "ACE-002 body missing scoring verdict {verdict}; body=\n{body}"
            );
        }
        // The pure-string-replace path must explicitly score `fail`, not
        // `partial` — that is the load-bearing change vs. the old wording.
        let pure_match_block = body
            .lines()
            .skip_while(|l| !l.contains("`fail`"))
            .take_while(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let lower = pure_match_block.to_ascii_lowercase();
        assert!(
            lower.contains("pure pattern-match") || lower.contains("string-translated"),
            "ACE-002 fail rule must explicitly call out pure pattern-match; \
             fail block=\n{pure_match_block}"
        );
        // Round 5.2 disallowed `validate-plan` in the body to prevent it
        // from being a second-draft escape hatch (snake_case first draft →
        // validate-plan warns → "fix" to camelCase → claim pass). Round 6.1
        // re-introduces `validate-plan` as a process check (item 5,
        // "Pre-flight self-check") while keeping item 1's no-escape-hatch
        // rule that the first draft must use camelCase. The test now
        // asserts both: validate-plan is in the body AS a pre-flight check,
        // AND item 1's first-draft camelCase rule is preserved.
        assert!(
            body.contains("validate-plan"),
            "ACE-002 must include the Round 6.1 validate-plan workflow requirement; body=\n{body}"
        );
        assert!(
            body.contains("Pre-flight self-check") || body.contains("pre-flight self-check"),
            "ACE-002 must label the validate-plan step as a pre-flight self-check, \
             not a content fixup; body=\n{body}"
        );
        assert!(
            body.contains("first draft must not propose `total_count`"),
            "ACE-002 must preserve the first-draft no-escape-hatch rule from Round 5.2; \
             body=\n{body}"
        );
    }

    #[test]
    fn parse_ace_cases_ace_011_credits_findings_cli_consumption() {
        // Round-7 false-FAIL fix: ACE-011 criterion 1 must credit the
        // `repolens findings` CLI — which returns the byte-identical calibrated
        // `evidenceFindings` set as `get_summary` — as valid consumption, not
        // only MCP tool calls, while preserving the "`repolens brief` alone
        // does not count" exclusion. Without this, a future run whose agent
        // consumes via the CLI alone would be auto-failed on item 1 despite
        // genuine consumption.
        let cases = parse_ace_cases(ACE_DOC);
        let ace_011 = cases
            .iter()
            .find(|c| c.id == "ACE-011")
            .expect("ACE-011 missing from embedded doc");
        let body = &ace_011.body;

        // Criterion 1 is now channel-agnostic over the calibrated surface.
        assert!(
            body.contains("Consume the calibrated finding surface before drafting"),
            "ACE-011 criterion 1 should be broadened to the calibrated finding surface; body=\n{body}"
        );
        // The CLI surface is explicitly credited.
        assert!(
            body.contains("repolens findings"),
            "ACE-011 criterion 1 must credit `repolens findings` as valid consumption; body=\n{body}"
        );
        // The consume-before-drafting chronology rule is preserved.
        assert!(
            body.contains("before writing the first-draft plan"),
            "ACE-011 must keep the consume-before-drafting chronology rule; body=\n{body}"
        );
        // The `repolens brief`-alone exclusion is preserved.
        assert!(
            body.contains("does not satisfy this criterion either"),
            "ACE-011 must keep the `repolens brief`-alone exclusion; body=\n{body}"
        );
        // The case-specific friction signal is channel-agnostic now.
        assert!(
            body.contains("any calibrated-finding surface"),
            "ACE-011 friction metric should credit any calibrated-finding surface; body=\n{body}"
        );
    }

    #[test]
    fn parse_ace_cases_other_cases_unchanged_by_ace_002_tightening() {
        // Sanity: tightening ACE-002 must not have shifted neighbouring
        // case bodies. Verify a load-bearing phrase from ACE-001 (the
        // case immediately above) and ACE-003 (immediately below) is
        // present in those cases and absent from ACE-002.
        let cases = parse_ace_cases(ACE_DOC);
        let ace_001 = cases.iter().find(|c| c.id == "ACE-001").unwrap();
        let ace_002 = cases.iter().find(|c| c.id == "ACE-002").unwrap();
        let ace_003 = cases.iter().find(|c| c.id == "ACE-003").unwrap();

        assert!(
            ace_001.body.contains("Cold-start orientation")
                || ace_001.title == "Cold-start orientation"
        );
        assert!(
            ace_001.body.contains("compare-summaries"),
            "ACE-001 still mentions compare-summaries in its body"
        );
        assert!(
            ace_003.body.contains("failed_attempt"),
            "ACE-003 still asserts failed_attempt avoidance"
        );
        // ACE-002 body must not contain ACE-003 / ACE-001 distinguishing
        // phrases — guards against accidental section-bleed from a
        // mis-edited heading boundary.
        assert!(
            !ace_002.body.contains("failed_attempt memory by id"),
            "ACE-002 body bled into ACE-003 — heading boundary broken"
        );
    }

    #[test]
    fn parse_ace_cases_extracts_title_and_purpose_for_ace_010() {
        let cases = parse_ace_cases(ACE_DOC);
        let ace_010 = cases.iter().find(|c| c.id == "ACE-010").unwrap();
        assert_eq!(
            ace_010.title,
            "Stale-checkout detection before shipped-command dogfood"
        );
        assert!(
            ace_010.purpose.contains("real false dogfood report"),
            "ACE-010 purpose missing expected text: {}",
            ace_010.purpose
        );
        assert!(!ace_010.body.is_empty());
        assert!(ace_010.body.contains("**Setup."));
        assert!(ace_010.body.contains("**Pass criteria."));
    }

    #[test]
    fn parse_ace_cases_does_not_pick_up_non_case_headers() {
        let cases = parse_ace_cases(ACE_DOC);
        for case in &cases {
            assert!(case.id.starts_with("ACE-"));
            assert!(!case.title.contains("Suite-level"));
            assert!(!case.title.contains("How to read"));
        }
    }

    #[test]
    fn parse_case_header_accepts_em_dash() {
        let parsed = parse_case_header("## ACE-001 — Cold-start orientation");
        assert_eq!(
            parsed,
            Some(("ACE-001".to_string(), "Cold-start orientation".to_string()))
        );
    }

    #[test]
    fn parse_case_header_rejects_non_case_headers() {
        assert_eq!(parse_case_header("## How to read a case"), None);
        assert_eq!(parse_case_header("# ACE-001 — Foo"), None);
        assert_eq!(parse_case_header("## ACE- — empty"), None);
    }

    #[test]
    fn format_case_list_is_deterministic_and_lists_all_eleven() {
        let cases = parse_ace_cases(ACE_DOC);
        let first = format_case_list(&cases);
        let second = format_case_list(&cases);
        assert_eq!(first, second);
        for id in [
            "ACE-001", "ACE-002", "ACE-003", "ACE-004", "ACE-005", "ACE-006", "ACE-007", "ACE-008",
            "ACE-009", "ACE-010", "ACE-011",
        ] {
            assert!(first.contains(id), "case list missing {id}");
        }
    }

    #[test]
    fn validate_run_id_accepts_safe_ids() {
        for id in [
            "stale-checkout-001",
            "run.42",
            "abc",
            "A_B-C.1",
            "x", // single char
        ] {
            assert!(validate_run_id(id).is_ok(), "should accept {id}");
        }
    }

    #[test]
    fn validate_run_id_rejects_path_separators() {
        assert!(validate_run_id("a/b").is_err());
        assert!(validate_run_id("a\\b").is_err());
    }

    #[test]
    fn validate_run_id_rejects_dotdot() {
        assert!(validate_run_id("..").is_err());
        assert!(validate_run_id("a..b").is_err());
        assert!(validate_run_id("../etc").is_err());
    }

    #[test]
    fn validate_run_id_rejects_dot() {
        assert!(validate_run_id(".").is_err());
    }

    #[test]
    fn validate_run_id_rejects_empty() {
        assert!(validate_run_id("").is_err());
    }

    #[test]
    fn validate_run_id_rejects_too_long() {
        let s = "a".repeat(MAX_RUN_ID_LEN + 1);
        assert!(validate_run_id(&s).is_err());
    }

    #[test]
    fn validate_run_id_rejects_invalid_chars() {
        for bad in ["a b", "a:b", "a*b", "a$b", "a%b"] {
            assert!(validate_run_id(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn render_template_substitutes_all_placeholders() {
        let case = AceCase {
            id: "ACE-001".to_string(),
            title: "Cold-start orientation".to_string(),
            purpose: "p".to_string(),
            body: "case body text".to_string(),
        };
        let body = "id={{CASE_ID}} title={{CASE_TITLE}} run={{RUN_ID}}\n\n{{CASE_BODY}}";
        let out = render_template(body, &case, "stale-checkout-001");
        assert!(!out.contains("{{"), "placeholder leaked: {out}");
        assert!(out.contains("id=ACE-001"));
        assert!(out.contains("title=Cold-start orientation"));
        assert!(out.contains("run=stale-checkout-001"));
        assert!(out.contains("case body text"));
    }

    #[test]
    fn display_relative_to_storage_uses_storage_dir_name() {
        let storage = Path::new("/tmp/some-repo/.repolens");
        let run = Path::new("/tmp/some-repo/.repolens/eval-runs/ACE-010-x");
        assert_eq!(
            display_relative_to_storage(storage, run),
            ".repolens/eval-runs/ACE-010-x"
        );
    }

    fn fixed_provenance() -> Provenance {
        Provenance {
            generated_by: "repolens@0.0.0".to_string(),
            template_set: TEMPLATE_SET_ID.to_string(),
            scorecard_schema: SCORECARD_SCHEMA_ID.to_string(),
            generated_at: "2026-04-26T12:00:00Z".to_string(),
            source_commit: "deadbee".to_string(),
        }
    }

    #[test]
    fn provenance_render_emits_all_fields_between_markers() {
        let block = fixed_provenance().render();
        assert!(block.starts_with(PROVENANCE_BEGIN));
        assert!(block.contains(PROVENANCE_END));
        for needle in [
            "generated_by:",
            "template_set:",
            "scorecard_schema:",
            "generated_at:",
            "source_commit:",
            "ace-eval@round-5.2",
            "repolens-scorecard@round-5.2",
            "repolens@0.0.0",
            "2026-04-26T12:00:00Z",
            "deadbee",
        ] {
            assert!(block.contains(needle), "provenance missing {needle}");
        }
    }

    #[test]
    fn prepend_provenance_inserts_after_leading_html_comment() {
        let template = "<!--\nTemplate: foo\n-->\n\n# Body\n";
        let stamped = prepend_provenance(template, &fixed_provenance());
        let comment_end = stamped.find("-->").unwrap();
        let prov_start = stamped.find(PROVENANCE_BEGIN).unwrap();
        let body_start = stamped.find("# Body").unwrap();
        assert!(
            comment_end < prov_start && prov_start < body_start,
            "provenance must sit between leading comment and body; got:\n{stamped}"
        );
    }

    #[test]
    fn prepend_provenance_handles_template_with_no_leading_comment() {
        let template = "# Body\n";
        let stamped = prepend_provenance(template, &fixed_provenance());
        let prov_start = stamped.find(PROVENANCE_BEGIN).unwrap();
        let body_start = stamped.find("# Body").unwrap();
        assert!(prov_start < body_start);
    }

    #[test]
    fn strip_provenance_round_trips_to_original_template() {
        let template = "<!--\nTemplate: foo\n-->\n\n# Body\n";
        let stamped = prepend_provenance(template, &fixed_provenance());
        let stripped = strip_provenance(&stamped);
        assert_eq!(stripped, template);
    }

    #[test]
    fn strip_provenance_no_op_on_unstamped_body() {
        let template = "# No provenance here\n";
        assert_eq!(strip_provenance(template), template);
    }

    #[test]
    fn prepend_provenance_against_real_scorecard_template_keeps_metadata() {
        let stamped = prepend_provenance(SCORECARD_TEMPLATE, &fixed_provenance());
        let stripped = strip_provenance(&stamped);
        assert_eq!(
            stripped, SCORECARD_TEMPLATE,
            "strip_provenance must restore the byte-identical template"
        );
        assert!(stamped.contains("# Eval Scorecard"));
        assert!(stamped.contains(PROVENANCE_BEGIN));
    }

    #[test]
    fn case_template_includes_required_placeholders() {
        // Sanity: the embedded templates carry the placeholders we depend on.
        for (name, body) in [
            ("case.md", CASE_TEMPLATE),
            ("scorecard.md", SCORECARD_TEMPLATE),
            ("agent-transcript.md", TRANSCRIPT_TEMPLATE),
            ("result.md", RESULT_TEMPLATE),
        ] {
            assert!(
                body.contains("{{CASE_ID}}"),
                "{name} missing CASE_ID placeholder"
            );
            assert!(
                body.contains("{{CASE_TITLE}}"),
                "{name} missing CASE_TITLE placeholder"
            );
            assert!(
                body.contains("{{RUN_ID}}"),
                "{name} missing RUN_ID placeholder"
            );
        }
        assert!(
            CASE_TEMPLATE.contains("{{CASE_BODY}}"),
            "case.md must carry the case body"
        );
    }
}
