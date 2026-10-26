//! `repolens brief [--task "<task>"]` — generate a deterministic preflight
//! brief from `.repolens/` workspace files (no LLM, no MCP, no scan).
//! `--task` is optional; without it the brief is composed for an honest
//! default ("No task was provided…") that never trips the Memory Safety
//! Notice keyword check.
//!
//! The brief is a static composition: it reads four required workspace
//! files and one optional file, classifies each as Filled / Partial /
//! TemplateOnly / Empty / Missing, and assembles a markdown document that
//! a coding agent can read before editing code. The composer
//! (`compose_brief_with_freshness`) is byte-deterministic given the same
//! workspace contents, task string, and resolved `FreshnessSnapshot`.
//!
//! The `## Runtime Freshness` section additionally reflects live runtime
//! state: `run_brief` reads `.repolens/session.json` (the authoritative
//! active pack) and the git HEAD, so the rendered brief surfaces the real
//! active pack and warns when the human-edited `repo-overview.md` cites a
//! stale pack id. These reads happen in `run_brief`, not in the pure composer.
//!
//! ## Round 3.5 hardening
//!
//! - Workspace Status table summarizing every input file's classification.
//! - Memory Safety Notice triggered deterministically by task keywords.
//! - Repo freshness checklist embedded in Suggested First Checks.
//! - Tightened template-marker detector: only complete italic placeholders
//!   like `_<repo-name>_` count, plus literal `_..._`, `YYYY-MM-DD`, and
//!   `<tag>`. Code-shaped strings such as `run_<name>`, `Vec<T>`, and
//!   `<session-summary.md>` no longer false-positive as template markers.

use anyhow::{anyhow, bail, Result};
use clap::Args;
use once_cell::sync::Lazy;
use regex::Regex;
use repolens_app::summary::{SummaryMemoryTier, SummaryService};
use repolens_core::atomic_write::write_atomic;
use repolens_core::model::evidence::EvidenceFinding;
use repolens_core::session::SessionStore;
use repolens_core::storage::RepoStoragePaths;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::resolve_root_from;
use crate::task_match::{self, TaskMatch};

/// Filename inside `.repolens/` that the brief is written to.
const BRIEF_FILENAME: &str = "current-agent-brief.md";

const REPO_OVERVIEW: &str = "repo-overview.md";
const KNOWN_CONSTRAINTS: &str = "known-constraints.md";
const SESSION_MEMORY: &str = "session-memory.md";
const AGENT_FRICTION: &str = "agent-friction-log.md";
const EVAL_RESULTS: &str = "eval-results.md";

/// Order of files in the Workspace Status table. Stable for determinism.
const WORKSPACE_FILES: &[&str] = &[
    REPO_OVERVIEW,
    KNOWN_CONSTRAINTS,
    SESSION_MEMORY,
    AGENT_FRICTION,
    EVAL_RESULTS,
];

/// If `count_template_markers(body)` returns at least this value, the file
/// is classified as `TemplateOnly` (raw scaffold). Counts in the range
/// `1..TEMPLATE_ONLY_THRESHOLD` are classified as `Partial` — some content
/// has been written, but stray placeholders remain.
const TEMPLATE_ONLY_THRESHOLD: usize = 3;

/// Warning text emitted into the brief when a section is empty or template-only.
const SECTION_TEMPLATED_WARNING: &str =
    "_This section has little or no filled workspace content yet._";

/// Shown above the Known Constraints body when it holds no `C-NNN` rule, so
/// the brief says what `validate-plan` will report (#34).
const NO_ENFORCED_CONSTRAINTS_NOTE: &str =
    "_No checked rules (C-NNN) yet, so `repolens validate-plan` enforces\n\
none. Treat D-NNN drafts as unverified._";

/// Lowercase substrings in the `--task` string that trigger the Memory
/// Safety Notice section.
const MEMORY_KEYWORDS: &[&str] = &[
    "remember",
    "memory",
    "session-memory",
    "memory.json",
    "memorystore",
    "memorize",
    "recall",
    "ingest",
    "session summary",
];

const MEMORY_SAFETY_NOTICE: &str = "**Memory Safety Notice:**\n\
This task appears to involve memory capture or memory ingestion. Treat \
agent-written summaries as unverified hypotheses until confirmed by files, \
tests, docs, or human review. Do not promote session notes into verified \
facts automatically. Prefer workspace markdown handoff before typed memory \
graph writes unless the task explicitly requires schema-level memory \
ingestion.";

/// Matches genuine template placeholders. Two alternatives:
///
/// 1. `_<...>_` — an italicized angle-bracketed placeholder (e.g. `_<repo-name>_`).
///    The content class `[^_>\n]+` excludes `_`, `>`, and newlines so that
///    code-shaped strings like `run_<name>(args)` (which lack a closing `>_`
///    pair) do NOT match.
/// 2. `_..._` — an italic ellipsis used in templates as "fill in here".
pub(crate) static TEMPLATE_PLACEHOLDER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"_<[^_>\n]+>_|_\.\.\._").unwrap());

/// Matches a pack-directory id, e.g. `pack-20260516-015206`. Used to detect
/// stale pack citations a human typed into `repo-overview.md`. The strict
/// `\d{8}-\d{6}` shape means bare `pack-` prose does not false-positive.
static PACK_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"pack-\d{8}-\d{6}").unwrap());

/// Workspace inputs whose mtime predates the active pack by more than this
/// many seconds are flagged as possibly stale. A 24h grace absorbs clock
/// skew and routine same-day edits.
const STALE_WORKSPACE_GRACE_SECS: i64 = 86_400;

/// Task string used when `--task` is not given. Honest about the absence —
/// the brief's `## Task` section must not pretend a task was provided.
/// Deliberately avoids every `MEMORY_KEYWORDS` substring so a task-less
/// brief never triggers the Memory Safety Notice.
const DEFAULT_TASK: &str =
    "No task was provided; this brief is for general repository orientation only.";

#[derive(Args, Debug)]
pub struct BriefArgs {
    /// Task description for which to generate the preflight brief
    /// (optional; defaults to a general repository-orientation brief)
    #[arg(long)]
    pub task: Option<String>,
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Print the brief to stdout instead of writing
    /// .repolens/current-agent-brief.md (no filesystem side effects;
    /// for agents operating under a read-only contract)
    #[arg(long)]
    pub stdout: bool,
}

/// Resolve the task string the brief is composed for: the explicit `--task`
/// value when given, otherwise `DEFAULT_TASK`.
fn effective_task(task: Option<&str>) -> &str {
    task.unwrap_or(DEFAULT_TASK)
}

/// Classification of a single workspace file, including the embedded body
/// where applicable. Drives both the Workspace Status table and the
/// per-section rendering.
#[derive(Debug)]
enum SectionContent {
    Filled(String),
    Partial(String),
    TemplateOnly(String),
    Empty,
    Missing,
}

impl SectionContent {
    fn status_label(&self) -> &'static str {
        match self {
            SectionContent::Filled(_) => "Filled",
            SectionContent::Partial(_) => "Partial",
            SectionContent::TemplateOnly(_) => "TemplateOnly",
            SectionContent::Empty => "Empty",
            SectionContent::Missing => "Missing",
        }
    }

    fn status_note(&self) -> &'static str {
        match self {
            SectionContent::Filled(_) => "Looks ready.",
            SectionContent::Partial(_) => "Some workspace placeholders remain.",
            SectionContent::TemplateOnly(_) => "Pristine template; fill before relying.",
            SectionContent::Empty => "File present but no content.",
            SectionContent::Missing => "File not present. Run `repolens init` to scaffold.",
        }
    }
}

/// Resolved runtime-freshness inputs threaded into the brief composer.
///
/// Built by `build_freshness_snapshot` from real readers in `run_brief`;
/// tests pass synthetic values so `compose_brief_with_freshness` stays a
/// pure, deterministic function of (workspace files, task, freshness).
#[derive(Debug, Default)]
struct FreshnessSnapshot {
    /// Session facts from `.repolens/session.json`, `None` if absent/corrupt.
    session: Option<SessionFreshness>,
    /// Git HEAD facts, `None` outside a git checkout.
    git: Option<GitHeadSnapshot>,
    /// Workspace filename -> file mtime in Unix seconds (`None` if unreadable).
    workspace_mtimes: BTreeMap<String, Option<i64>>,
}

/// Session facts surfaced in the `## Runtime Freshness` table.
#[derive(Debug, Clone)]
struct SessionFreshness {
    /// Basename of `activePackPath`, e.g. `pack-20260516-015206`.
    active_pack: Option<String>,
    /// `updatedAt` rendered in local time with an explicit offset, plus the
    /// same instant in UTC (see [`format_session_time`]).
    updated_at: Option<String>,
    /// `updatedAt` as Unix seconds, for workspace-age comparison.
    updated_at_unix: Option<i64>,
    /// `lastScanMode`, e.g. `incremental` or `full`.
    last_scan_mode: Option<String>,
}

/// Git HEAD facts surfaced in the `## Runtime Freshness` table.
#[derive(Debug, Clone)]
struct GitHeadSnapshot {
    /// Short branch name, `None` when HEAD is detached.
    branch: Option<String>,
    /// Abbreviated commit hash.
    short: Option<String>,
    /// Working-tree state: `Some(true)` dirty (uncommitted or untracked
    /// changes), `Some(false)` clean, `None` when git status errored.
    dirty: Option<bool>,
}

pub fn run_brief(args: BriefArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;

    let storage_dir = RepoStoragePaths::storage_dir(&root);
    if !storage_dir.is_dir() {
        bail!(
            "RepoLens workspace not found at {}. Run 'repolens init' first.",
            storage_dir.display()
        );
    }

    let freshness = build_freshness_snapshot(&root, &storage_dir);
    let findings = load_brief_findings(&root);
    let task = effective_task(args.task.as_deref());
    let task_match = load_task_match(&root, args.task.as_deref());
    let body = compose_brief_full(&storage_dir, task, &freshness, &findings, &task_match);

    if args.stdout {
        // Read-only mode: full brief to stdout, no artifact write.
        println!("{body}");
        return Ok(());
    }

    let target = storage_dir.join(BRIEF_FILENAME);
    write_atomic(&target, body.as_bytes())
        .map_err(|e| anyhow!("Failed to write brief to {}: {}", target.display(), e))?;

    println!("Agent preflight brief written to {}", target.display());
    println!("Next: give this brief to your coding agent before it edits code.");
    if let Some(line) = task_match::terminal_summary(&task_match) {
        println!("{line}");
    }
    Ok(())
}

/// Match the task against the active pack's file list and saved memory
/// (#34). Read-only: memory is loaded without the lock and nothing is
/// written. A missing pack or unreadable memory leaves that side empty.
fn load_task_match(repo_root: &Path, task: Option<&str>) -> TaskMatch {
    if task.is_none() {
        return TaskMatch::NoTask;
    }
    let entries = crate::findings::resolve_findings_pack(repo_root, None)
        .ok()
        .and_then(|dir| repolens_core::pack_reader::PackReader::read(&dir).ok())
        .map(|idx| idx.entries)
        .unwrap_or_default();
    let notes = repolens_memory::store::MemoryStore::load(repo_root)
        .map(|g| g.nodes)
        .unwrap_or_default();
    task_match::match_task(task, &entries, &notes)
}

/// Local UTC offset (seconds) in effect at `at`, via the system time zone.
/// Falls back to 0 (UTC) when the local zone cannot be determined.
pub(crate) fn local_utc_offset_secs(at: time::OffsetDateTime) -> i32 {
    use chrono::{Local, Offset, TimeZone};
    Local
        .timestamp_opt(at.unix_timestamp(), 0)
        .single()
        .map(|dt| dt.offset().fix().local_minus_utc())
        .unwrap_or(0)
}

/// Render a session timestamp as local time with an explicit offset, followed
/// by the same instant in UTC, e.g.
/// `2026-10-13 05:40:53 +08:00 (2026-10-12 21:40:53 UTC)`.
/// When the offset is zero only the UTC form is printed. Returns `None` only if
/// the offset is out of range or formatting fails.
pub(crate) fn format_session_time(at: time::OffsetDateTime, offset_secs: i32) -> Option<String> {
    let fmt = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let utc = at.to_offset(time::UtcOffset::UTC);
    let utc_str = utc.format(&fmt).ok()?;
    if offset_secs == 0 {
        return Some(format!("{utc_str} UTC"));
    }
    let off = time::UtcOffset::from_whole_seconds(offset_secs).ok()?;
    let local = at.to_offset(off);
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let abs = offset_secs.unsigned_abs();
    Some(format!(
        "{} {}{:02}:{:02} ({} UTC)",
        local.format(&fmt).ok()?,
        sign,
        abs / 3600,
        (abs % 3600) / 60,
        utc_str
    ))
}

/// Best-effort load of the active pack's calibrated findings for inlining into
/// the brief. Mirrors `repolens findings`: resolves the active pack (or latest)
/// and summarizes it read-only (`SummaryMemoryTier::Skip`, no memory mutation).
/// Any failure (no pack, summarize error) degrades to an empty list so the brief
/// still renders — the same degrade-to-empty discipline as the freshness readers.
fn load_brief_findings(repo_root: &Path) -> Vec<EvidenceFinding> {
    let Ok(pack_dir) = crate::findings::resolve_findings_pack(repo_root, None) else {
        return Vec::new();
    };
    match SummaryService::summarize(&pack_dir, Some(repo_root), SummaryMemoryTier::Skip) {
        Ok(summary) => summary.evidence_findings,
        Err(_) => Vec::new(),
    }
}

/// Build the runtime-freshness snapshot from live readers: `session.json`,
/// the git HEAD, and workspace file mtimes. Called once per `repolens brief`.
/// Every reader degrades to `None`/`None`-mtime rather than failing the brief.
fn build_freshness_snapshot(repo_root: &Path, storage_dir: &Path) -> FreshnessSnapshot {
    let session = SessionStore::load(repo_root)
        .ok()
        .flatten()
        .map(|s| SessionFreshness {
            active_pack: Path::new(&s.active_pack_path)
                .file_name()
                .and_then(|f| f.to_str())
                .map(String::from),
            updated_at: format_session_time(s.updated_at, local_utc_offset_secs(s.updated_at)),
            updated_at_unix: Some(s.updated_at.unix_timestamp()),
            last_scan_mode: Some(s.last_scan_mode),
        });

    let git = real_git_head_snapshot(repo_root);

    let mut workspace_mtimes = BTreeMap::new();
    for name in WORKSPACE_FILES {
        let mtime = fs::metadata(storage_dir.join(name))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_secs()).ok());
        workspace_mtimes.insert((*name).to_string(), mtime);
    }

    FreshnessSnapshot {
        session,
        git,
        workspace_mtimes,
    }
}

/// Read the git HEAD via the shared `repolens_core::git` helpers. Returns
/// `None` outside a git checkout or before the first commit (unresolvable
/// HEAD). `is_worktree_dirty` counts untracked files as dirty, matching the
/// `## Runtime Freshness` "uncommitted or untracked changes" wording.
fn real_git_head_snapshot(repo_root: &Path) -> Option<GitHeadSnapshot> {
    let sha = repolens_core::git::head_commit(repo_root)?;
    let short = sha.chars().take(7).collect::<String>();
    // Distinguish clean / dirty / unknown so a status error is reported
    // honestly instead of masquerading as "clean".
    let dirty = match repolens_core::git::list_worktree_changes(repo_root) {
        Ok(changes) => Some(!changes.changed.is_empty() || !changes.deleted.is_empty()),
        Err(_) => None,
    };
    Some(GitHeadSnapshot {
        branch: repolens_core::git::head_branch(repo_root),
        short: Some(short),
        dirty,
    })
}

/// Compose the brief with empty runtime freshness (no session.json, no git).
/// Thin wrapper over `compose_brief_with_freshness` used by unit tests where
/// freshness is not relevant.
#[cfg(test)]
fn compose_brief(storage_dir: &Path, task: &str) -> String {
    compose_brief_with_freshness(storage_dir, task, &FreshnessSnapshot::default())
}

/// 3-arg shim: compose with no inlined findings. Kept so the existing
/// freshness-focused unit tests (which don't exercise the findings section)
/// stay unchanged. The live `run_brief` path calls `compose_brief_full`.
#[cfg(test)]
fn compose_brief_with_freshness(
    storage_dir: &Path,
    task: &str,
    freshness: &FreshnessSnapshot,
) -> String {
    compose_brief_full(storage_dir, task, freshness, &[], &TaskMatch::NoTask)
}

/// Compose the full brief markdown body. Pure function of the workspace state,
/// the task string, the resolved `freshness` snapshot, and the active pack's
/// calibrated `findings` — no clocks, no IDs, no I/O side-effects.
fn compose_brief_full(
    storage_dir: &Path,
    task: &str,
    freshness: &FreshnessSnapshot,
    findings: &[EvidenceFinding],
    task_match: &TaskMatch,
) -> String {
    // Read each required input once so the same classification powers both
    // the Workspace Status table and the per-section rendering.
    let states: Vec<(&str, SectionContent)> = WORKSPACE_FILES
        .iter()
        .map(|name| (*name, read_section(storage_dir, name)))
        .collect();

    let lookup = |target: &str| -> &SectionContent {
        states
            .iter()
            .find_map(|(n, c)| if *n == target { Some(c) } else { None })
            .expect("WORKSPACE_FILES is exhaustive")
    };

    // Raw repo-overview body for stale-pack detection (the human-edited file
    // is where stale pack ids creep in). Empty for Empty/Missing.
    let repo_overview_body = match lookup(REPO_OVERVIEW) {
        SectionContent::Filled(b)
        | SectionContent::Partial(b)
        | SectionContent::TemplateOnly(b) => b.as_str(),
        SectionContent::Empty | SectionContent::Missing => "",
    };

    // Count C-NNN rules with validate-plan's own parser, so the brief and
    // validate-plan agree on what known-constraints.md holds (#34).
    let enforced = match lookup(KNOWN_CONSTRAINTS) {
        SectionContent::Filled(b) | SectionContent::Partial(b) => {
            Some(crate::validate_plan::enforced_constraint_count(b))
        }
        _ => None,
    };
    let repo_overview = render_section(REPO_OVERVIEW, lookup(REPO_OVERVIEW));
    let mut known_constraints = render_section(KNOWN_CONSTRAINTS, lookup(KNOWN_CONSTRAINTS));
    if enforced == Some(0) {
        known_constraints = format!("{NO_ENFORCED_CONSTRAINTS_NOTE}\n\n{known_constraints}");
    }
    let session_memory = render_session_memory_section(lookup(SESSION_MEMORY));
    let agent_friction = render_section(AGENT_FRICTION, lookup(AGENT_FRICTION));

    // eval-results.md is optional in the body — only surfaced when filled.
    let eval_results_filled = match lookup(EVAL_RESULTS) {
        SectionContent::Filled(body) => Some(body.clone()),
        _ => None,
    };

    let mut out = String::new();

    out.push_str(
        "<!-- This file is generated by `repolens brief` from .repolens/ workspace files.\n     \
         Do not edit by hand — your changes will be lost the next time\n     \
         `repolens brief` runs. -->\n\n",
    );

    out.push_str("# Current Agent Brief\n\n");
    out.push_str(
        "> Generated deterministically by `repolens brief` from `.repolens/` workspace files.\n\
         > File pointers are matched by name; this brief does **not** analyze code,\n\
         > run tests, or call MCP tools.\n\
         > Trust its claims only as far as the underlying workspace files are accurate.\n\n",
    );

    out.push_str("## Task\n\n");
    out.push_str(task);
    out.push_str("\n\n");
    if let Some(line) = task_match::task_note_line(task_match) {
        out.push_str(line);
        out.push_str("\n\n");
    }
    if let Some(section) = task_match::render_section(task_match) {
        out.push_str(&section);
    }

    if task_is_memory_related(task) {
        out.push_str("## Memory Safety Notice\n\n");
        out.push_str(MEMORY_SAFETY_NOTICE);
        out.push_str("\n\n");
    }

    out.push_str("## Workspace Status\n\n");
    out.push_str(&render_workspace_status_table(&states, enforced));
    out.push('\n');

    out.push_str("## Runtime Freshness\n\n");
    out.push_str(&render_runtime_freshness(freshness, repo_overview_body));
    out.push('\n');

    out.push_str("## Calibrated Findings\n\n");
    out.push_str(&render_calibrated_findings(findings));
    out.push('\n');

    out.push_str("## Repo Context\n\n");
    out.push_str(&repo_overview);
    out.push_str("\n\n");

    out.push_str("## Known Constraints\n\n");
    out.push_str(&known_constraints);
    out.push_str("\n\n");

    out.push_str("## Prior Session Memory\n\n");
    out.push_str(&session_memory);
    out.push_str("\n\n");

    out.push_str("## Agent Friction Warnings\n\n");
    out.push_str(&agent_friction);
    out.push_str("\n\n");

    out.push_str("## Suggested First Checks\n\n");
    out.push_str(
        "- Re-read **Known Constraints** before drafting your plan.\n\
         - Search **Prior Session Memory** for keywords that overlap with the task description.\n\
         - Read **Agent Friction Warnings** for known sharp edges.\n\
         - Trust the active pack id in the **Runtime Freshness** table over any pack id cited elsewhere in this brief, and heed its staleness warnings.\n\
         - The **Calibrated Findings** section above lists the active pack's evidence findings inline; read them before assuming anything about repo structure or health, then run `repolens findings --format json` for full per-finding evidence, limitations, and scope.\n\
         - Confirm any file paths you intend to edit by reading them first; do not assume they exist.\n",
    );
    if let Some(eval_body) = eval_results_filled {
        out.push_str(
            "- Recent eval results may indicate areas with thin coverage; relevant excerpt:\n\n",
        );
        out.push_str(&indent(&eval_body, "  "));
        out.push('\n');
    }
    out.push_str("\n### Repo freshness checklist (do not run automatically; checklist only)\n\n");
    out.push_str(
        "- Confirm the repo path you are operating in (`pwd`).\n\
         - Confirm the current branch (`git branch --show-current`).\n\
         - Confirm the HEAD commit (`git log -1 --oneline`).\n\
         - Run `git status --short` to see uncommitted and untracked changes.\n\
         - If you are checking what a CLI can do, run its `--help` before claiming\n  a command is missing.\n\
         - If remote freshness matters, run `git fetch` and compare local against `origin/main` before concluding a command is missing; a stale checkout gives false results.\n\n",
    );

    out.push_str("## Risks\n\n");
    out.push_str(
        "- This brief is a deterministic composition of `.repolens/` workspace files.\n\
         - Any section above carrying the warning \"_This section has little or no filled workspace content yet._\" indicates the workspace is not yet filled in. Fill those files before relying on the brief.\n\
         - Memory in `session-memory.md` may be stale. Treat older entries as hypotheses unless they cite evidence.\n\n",
    );

    out.push_str("## Acceptance Criteria\n\n");
    out.push_str(
        "- [ ] The change addresses the task described above.\n\
         - [ ] No items in **Known Constraints** are violated.\n\
         - [ ] Tests relevant to the affected code pass.\n\
         - [ ] A session summary is recorded with `repolens remember --file <session-summary.md>` before the session ends.\n\n",
    );

    out.push_str("## Open Questions\n\n");
    out.push_str(
        "_Use this section during the session to record questions for the human. \
         Leave empty if none._\n\n",
    );

    out.push_str("## Next Steps for Coding Agent\n\n");
    out.push_str(
        "1. Read every section above.\n\
         2. Cite a file path before claiming anything about the repo.\n\
         3. Produce a short plan that respects **Known Constraints** and the **Agent Friction Warnings**.\n\
         4. Run the **Suggested First Checks** (including the Repo freshness checklist).\n\
         5. Edit code; run tests.\n\
         6. Write a session summary file and run `repolens remember --file <session-summary.md>`.\n\
         7. Optionally run `repolens validate-plan --file <plan.md>` to surface possible plan/constraint risks. Warnings only — this does not prove the plan is valid; review against source files and tests.\n",
    );

    out
}

/// Render the `## Calibrated Findings` section: the active pack's evidence
/// findings inlined as compact bullets (title, classification/severity, action
/// one-liner), so an agent reading the brief consumes them without a separate
/// `repolens findings` call. Full per-finding evidence stays in that command.
/// Pure function of the findings slice.
fn render_calibrated_findings(findings: &[EvidenceFinding]) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    if findings.is_empty() {
        out.push_str(
            "_No calibrated findings for the active pack (or none could be loaded). \
             Run `repolens scan` if you have not scanned this repo yet._\n",
        );
        return out;
    }

    let _ = writeln!(
        out,
        "_{} calibrated finding(s) from the active pack — observations to verify, not \
         verdicts. Run `repolens findings --format json` for full evidence, limitations, \
         and scope._",
        findings.len()
    );
    let _ = writeln!(out);
    for f in findings {
        let bullet = format!(
            "- **{}** — `{:?}`/sev={:?}. {}",
            f.title,
            f.classification,
            f.severity,
            crate::findings::render_action(&f.action)
        );
        // Wrap at 78 so the two-space continuation indent, which keeps
        // Markdown in the same list item, stays within 80 columns (#20).
        for (i, line) in crate::findings::wrap_words(&bullet, 78).iter().enumerate() {
            let indent = if i == 0 { "" } else { "  " };
            let _ = writeln!(out, "{indent}{line}");
        }
    }
    out
}

/// Render the Workspace Status table for the brief.
fn render_workspace_status_table(
    states: &[(&str, SectionContent)],
    enforced_constraints: Option<usize>,
) -> String {
    let mut out = String::new();
    out.push_str("| File | Status | Note |\n");
    out.push_str("|---|---|---|\n");
    for (name, content) in states {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            name,
            content.status_label(),
            match (*name, enforced_constraints) {
                // Short enough for the row to fit in 80 columns.
                (KNOWN_CONSTRAINTS, Some(0)) => "No C-NNN rules yet; none enforced.".to_string(),
                (KNOWN_CONSTRAINTS, Some(1)) => "validate-plan enforces 1 C-NNN rule.".to_string(),
                (KNOWN_CONSTRAINTS, Some(n)) => {
                    format!("validate-plan enforces {n} C-NNN rules.")
                }
                _ => content.status_note().to_string(),
            },
        ));
    }
    out
}

/// Render the `## Runtime Freshness` table from the resolved snapshot, plus
/// any staleness warnings.
///
/// Rows are sourced from `.repolens/session.json` and the git HEAD; when
/// either is absent a single explanatory fallback row is rendered instead.
/// Warnings (warnings-only posture, never errors) flag a `repo-overview.md`
/// that cites a non-active pack id and workspace inputs that predate the
/// active pack.
fn render_runtime_freshness(freshness: &FreshnessSnapshot, repo_overview_body: &str) -> String {
    let mut out = String::new();
    out.push_str("| Source | Value | Note |\n");
    out.push_str("|---|---|---|\n");

    match &freshness.session {
        Some(s) => {
            out.push_str(&format!(
                "| Active pack | `{}` | Authoritative pack from `.repolens/session.json`. |\n",
                s.active_pack.as_deref().unwrap_or("(unknown)"),
            ));
            out.push_str(&format!(
                "| Session updated | {} | When the active pack was last written. |\n",
                s.updated_at.as_deref().unwrap_or("(unknown)"),
            ));
            out.push_str(&format!(
                "| Last scan mode | {} | |\n",
                s.last_scan_mode.as_deref().unwrap_or("(unknown)"),
            ));
        }
        None => {
            out.push_str(
                "| Session | No `.repolens/session.json` found | Run `repolens scan` first. |\n",
            );
        }
    }

    match &freshness.git {
        Some(g) => {
            out.push_str(&format!(
                "| Git branch | {} | |\n",
                g.branch.as_deref().unwrap_or("(detached HEAD)"),
            ));
            out.push_str(&format!(
                "| Git HEAD | {} | |\n",
                g.short.as_deref().unwrap_or("(unknown)"),
            ));
            out.push_str(&format!(
                "| Working tree | {} | |\n",
                match g.dirty {
                    Some(true) => "dirty (uncommitted or untracked changes)",
                    Some(false) => "clean",
                    None => "unknown (git status unavailable)",
                },
            ));
        }
        None => {
            out.push_str("| Git | Not a git checkout or git unavailable | |\n");
        }
    }

    let mut warnings = Vec::new();
    if let Some(active) = freshness
        .session
        .as_ref()
        .and_then(|s| s.active_pack.as_deref())
    {
        warnings.extend(detect_stale_pack_citations(repo_overview_body, active));
    }
    if let Some(session_unix) = freshness.session.as_ref().and_then(|s| s.updated_at_unix) {
        warnings.extend(detect_stale_workspace_inputs(
            &freshness.workspace_mtimes,
            session_unix,
        ));
    }
    if !warnings.is_empty() {
        out.push('\n');
        for w in warnings {
            out.push_str("- ");
            out.push_str(&w);
            out.push('\n');
        }
    }

    out
}

/// Detect pack ids cited in `repo-overview.md` that differ from the active
/// pack. Deduped and sorted for deterministic output.
fn detect_stale_pack_citations(repo_overview_body: &str, active_pack: &str) -> Vec<String> {
    let bytes = repo_overview_body.as_bytes();
    let mut stale: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for m in PACK_ID_RE.find_iter(repo_overview_body) {
        // Reject matches embedded in a longer token: an alphanumeric char
        // immediately before `pack-` or immediately after the 6-digit time
        // (e.g. `pack-20260101-0000000` or `pack-20260101-000000abc`) means
        // this is not a standalone pack id.
        let before_ok = m.start() == 0 || !bytes[m.start() - 1].is_ascii_alphanumeric();
        let after_ok = m.end() == bytes.len() || !bytes[m.end()].is_ascii_alphanumeric();
        if before_ok && after_ok && m.as_str() != active_pack {
            stale.insert(m.as_str());
        }
    }
    stale
        .into_iter()
        .map(|cited| {
            format!(
                "Repo overview cites stale pack `{cited}` (active: `{active_pack}`). \
                 Update `.repolens/repo-overview.md`."
            )
        })
        .collect()
}

/// Flag workspace inputs whose mtime predates the active pack by more than
/// the grace window. `BTreeMap` iteration keeps the order deterministic.
fn detect_stale_workspace_inputs(
    mtimes: &BTreeMap<String, Option<i64>>,
    session_unix: i64,
) -> Vec<String> {
    let mut out = Vec::new();
    for (name, mtime) in mtimes {
        if let Some(m) = mtime {
            let age = session_unix - m;
            if age > STALE_WORKSPACE_GRACE_SECS {
                let days = age / 86_400;
                out.push(format!(
                    "Workspace input `{name}` predates the active pack by {days} day(s); \
                     contents may be stale."
                ));
            }
        }
    }
    out
}

/// Read a workspace file and classify it.
fn read_section(storage_dir: &Path, name: &str) -> SectionContent {
    let path = storage_dir.join(name);
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return SectionContent::Missing,
    };
    let body = strip_leading_html_comment(&raw).trim().to_string();
    if body.is_empty() {
        return SectionContent::Empty;
    }
    let marker_count = count_template_markers(&body);
    if marker_count >= TEMPLATE_ONLY_THRESHOLD {
        SectionContent::TemplateOnly(body)
    } else if marker_count > 0 {
        SectionContent::Partial(body)
    } else {
        SectionContent::Filled(body)
    }
}

pub(crate) fn is_template_only_workspace_body(raw: &str) -> bool {
    let body = strip_leading_html_comment(raw).trim();
    !body.is_empty() && count_template_markers(body) >= TEMPLATE_ONLY_THRESHOLD
}

/// Count template-placeholder markers in a body. The detector is tightened
/// to avoid false-positives on real code-like text:
///
/// - `_<repo-name>_` (italic angle placeholder) → 1
/// - `_..._` (italic ellipsis) → 1
/// - `YYYY-MM-DD` (literal date placeholder) → 1 per occurrence
/// - `<tag>` (literal placeholder string) → 1 per occurrence
/// - `run_<name>` / `Vec<T>` / `<session-summary.md>` → 0 (these are real
///   technical strings, not template scaffold)
fn count_template_markers(body: &str) -> usize {
    let italic = TEMPLATE_PLACEHOLDER_RE.find_iter(body).count();
    let date = body.matches("YYYY-MM-DD").count();
    let tag = body.matches("<tag>").count();
    italic + date + tag
}

/// Render a section's body for the brief, embedding a warning where appropriate.
///
/// Round 5.2 hardening: `TemplateOnly` sections are rendered as a notice
/// only — the raw template body is **not** embedded in the brief, so
/// agents cannot mistake placeholders like `C-001 _<short-name>_` for
/// real constraints. `Partial` sections still show their body verbatim,
/// but the warning is now placed **above** the body so the warning is
/// the first thing the agent reads.
fn render_section(name: &str, content: &SectionContent) -> String {
    match content {
        SectionContent::Filled(body) => body.clone(),
        SectionContent::Partial(body) => format!(
            "_Some workspace placeholders remain in `.repolens/{name}` — \
             treat italic-bracketed text and `YYYY-MM-DD` as unfilled, not as real entries. \
             The verbatim content below is not yet authoritative._\n\n\
             {body}"
        ),
        SectionContent::TemplateOnly(_body) => format!(
            "{warning}\n\n\
             _The workspace file `.repolens/{name}` is still a raw template — its body is \
             only italicized angle-bracket placeholders and date-shaped scaffolding. The brief \
             deliberately does not inline that raw template here, so its placeholders cannot \
             be mistaken for real entries. Read the file directly if you need to see the \
             template structure._",
            warning = SECTION_TEMPLATED_WARNING,
        ),
        SectionContent::Empty => format!(
            "{}\n\n_File `.repolens/{}` is empty._",
            SECTION_TEMPLATED_WARNING, name
        ),
        SectionContent::Missing => format!(
            "_(missing: `.repolens/{}` does not exist; run `repolens init` to create it.)_",
            name
        ),
    }
}

/// Render the Prior Session Memory section, lifting the latest "Pick up
/// here" block (if any) to the top so the most load-bearing handoff cue
/// is the first thing the next agent reads. The original prior session
/// memory body is still rendered in full below the lift, so context is
/// not lost. The lift preserves remember-import safety framing
/// (agent-provided / unverified / hypothesis) so the next agent does not
/// upgrade a lifted cue to confirmed fact.
fn render_session_memory_section(content: &SectionContent) -> String {
    let raw = match content {
        SectionContent::Filled(b)
        | SectionContent::Partial(b)
        | SectionContent::TemplateOnly(b) => Some(b.as_str()),
        SectionContent::Empty | SectionContent::Missing => None,
    };
    let lift = raw.and_then(lift_pickup_here_block);
    let rendered = render_section(SESSION_MEMORY, content);
    match lift {
        Some(block) => format!(
            "### Latest \"Pick up here\" (lifted from prior session memory)\n\n\
             > **Source:** prior session summary, surfaced verbatim from \
             `.repolens/session-memory.md`. \
             **Status:** agent-provided, unverified. \
             **Use as:** hypothesis until confirmed by files, tests, docs, or human review.\n\n\
             {block}\n\n---\n\n{rendered}"
        ),
        None => rendered,
    }
}

/// Find the latest `Pick up here` (or `Pick up here.`) ATX heading in
/// `body` and return its block — the heading line plus all content up to
/// the next heading at the same or higher level (smaller-or-equal `#`
/// count). Returns `None` if no such heading exists. Case-insensitive on
/// the title; tolerant of trailing periods and trailing whitespace.
///
/// "Latest" means the first occurrence in `body` because
/// `repolens remember --file` prepends new entries to the top of
/// `.repolens/session-memory.md`.
fn lift_pickup_here_block(body: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let mut start_line: Option<usize> = None;
    let mut start_level: usize = 0;
    for (i, line) in lines.iter().enumerate() {
        if let Some((level, title)) = parse_atx_heading(line) {
            let title_norm = title
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            if title_norm == "pick up here" {
                start_line = Some(i);
                start_level = level;
                break;
            }
        }
    }
    let start = start_line?;
    let mut end = lines.len();
    for (j, line) in lines.iter().enumerate().skip(start + 1) {
        if let Some((level, _)) = parse_atx_heading(line) {
            if level <= start_level {
                end = j;
                break;
            }
        }
    }
    let block = lines[start..end].join("\n");
    Some(block.trim_end().to_string())
}

/// Parse an ATX-style heading (0–3 leading spaces, 1–6 `#`, then a space
/// or end-of-line). Returns `(level, title)` on match. Setext headings
/// are intentionally not handled; agent-written summaries use ATX.
fn parse_atx_heading(line: &str) -> Option<(usize, &str)> {
    let leading = line.len() - line.trim_start_matches(' ').len();
    if leading > 3 {
        return None;
    }
    let after_indent = &line[leading..];
    let level = after_indent.bytes().take_while(|&b| b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let after_hashes = &after_indent[level..];
    if after_hashes.is_empty() {
        return Some((level, ""));
    }
    if !after_hashes.starts_with([' ', '\t']) {
        return None;
    }
    Some((level, after_hashes.trim_start_matches([' ', '\t'])))
}

/// Returns `true` if `task` contains any of the memory-related keywords
/// (case-insensitive substring match).
fn task_is_memory_related(task: &str) -> bool {
    let lower = task.to_lowercase();
    MEMORY_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Indent every line of `s` with `prefix`.
fn indent(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip a leading `<!-- ... -->` comment block (and its trailing newlines)
/// from a markdown file's text. Inner HTML comments are preserved.
fn strip_leading_html_comment(s: &str) -> &str {
    let trimmed = s.trim_start();
    if let Some(rest) = trimmed.strip_prefix("<!--") {
        if let Some(end_idx) = rest.find("-->") {
            return rest[end_idx + 3..].trim_start_matches(['\n', '\r']);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(c: &SectionContent) -> &'static str {
        c.status_label()
    }

    #[test]
    fn strip_leading_html_comment_removes_first_block() {
        let s = "<!-- header\n  comment -->\n\n# Body\n";
        assert_eq!(strip_leading_html_comment(s), "# Body\n");
    }

    #[test]
    fn strip_leading_html_comment_preserves_inner_comments() {
        let s = "<!-- a -->\n# Body\n<!-- inline -->\nmore\n";
        let stripped = strip_leading_html_comment(s);
        assert!(stripped.contains("<!-- inline -->"));
    }

    #[test]
    fn strip_leading_html_comment_no_op_when_no_leading_comment() {
        let s = "# Already body\nmore\n";
        assert_eq!(strip_leading_html_comment(s), s);
    }

    // -- Template marker detector ---------------------------------------------

    #[test]
    fn count_template_markers_counts_italic_angle_placeholder() {
        assert_eq!(count_template_markers("**Repo:** _<repo-name>_"), 1);
    }

    #[test]
    fn count_template_markers_counts_italic_ellipsis() {
        assert_eq!(count_template_markers("body: _..._\n"), 1);
    }

    #[test]
    fn count_template_markers_counts_date_placeholder() {
        assert_eq!(count_template_markers("Created: YYYY-MM-DD"), 1);
    }

    #[test]
    fn count_template_markers_counts_literal_tag_placeholder() {
        assert_eq!(count_template_markers("- **Tag:** <tag>"), 1);
    }

    #[test]
    fn count_template_markers_zero_for_run_name_pattern() {
        // Round 3.5 false-positive fix: code identifier with `_<` substring
        // but no closing `>_` should not count as a template marker.
        let body = "Commands::<Name>(args) => <name>::run_<name>(args)";
        assert_eq!(
            count_template_markers(body),
            0,
            "run_<name> should not be treated as a template placeholder"
        );
    }

    #[test]
    fn count_template_markers_zero_for_vec_t() {
        assert_eq!(count_template_markers("type Result = Vec<T>;"), 0);
    }

    #[test]
    fn count_template_markers_zero_for_command_doc_path() {
        // Literal command documentation like `<session-summary.md>` should
        // not be confused with a template placeholder.
        assert_eq!(
            count_template_markers("repolens remember --file <session-summary.md>"),
            0,
        );
    }

    #[test]
    fn count_template_markers_handles_mixed_kinds() {
        let body = "**Tag:** <tag>\n**Created:** YYYY-MM-DD\n_..._";
        assert_eq!(count_template_markers(body), 3);
    }

    // -- read_section classification -----------------------------------------

    #[test]
    fn read_section_returns_missing_for_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = read_section(dir.path(), "nonexistent.md");
        assert_eq!(kind(&c), "Missing");
    }

    #[test]
    fn read_section_classifies_raw_template_as_template_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("template.md");
        let body =
            "<!-- header -->\n\n# H\n\n- _<a>_\n- _<b>_\n- _<c>_\n- _<d>_\n- _<e>_\n- _<f>_\n";
        fs::write(&p, body).unwrap();
        let c = read_section(dir.path(), "template.md");
        assert_eq!(kind(&c), "TemplateOnly");
    }

    #[test]
    fn read_section_classifies_single_placeholder_as_partial() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("template.md");
        fs::write(
            &p,
            "<!-- header -->\n# Overview\n\n**Repo name:** _<repo-name>_\n\nFilled-looking explanatory scaffold text with enough body to clear the empty threshold.\n",
        )
        .unwrap();
        let c = read_section(dir.path(), "template.md");
        assert_eq!(kind(&c), "Partial");
    }

    #[test]
    fn read_section_classifies_three_scaffold_markers_as_template_only() {
        // <tag> + _..._ + YYYY-MM-DD = 3 markers = at-or-above threshold.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agent-friction-log.md");
        fs::write(
            &p,
            "<!-- header -->\n# Agent Friction Log\n\n### F-YYYY-MM-DD-NNN — `<tag>`\n\n- **What happened:** _..._\n",
        )
        .unwrap();
        let c = read_section(dir.path(), "agent-friction-log.md");
        assert_eq!(kind(&c), "TemplateOnly");
    }

    #[test]
    fn read_section_classifies_blank_body_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("empty.md");
        fs::write(&p, "<!-- header -->\n   \n\t\n").unwrap();
        let c = read_section(dir.path(), "empty.md");
        assert_eq!(kind(&c), "Empty");
    }

    #[test]
    fn read_section_classifies_short_real_constraint_as_filled() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("known-constraints.md");
        fs::write(&p, "Do not write memory.json.").unwrap();
        let c = read_section(dir.path(), "known-constraints.md");
        assert_eq!(kind(&c), "Filled");
    }

    #[test]
    fn read_section_classifies_filled_as_filled() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("filled.md");
        fs::write(
            &p,
            "<!-- header -->\n# Constraints\n\n- All JSON output uses camelCase.\n\
             - Atomic writes via temp+rename.\n- No env-var-driven flags (failed_attempt F-2025-XX).\n",
        )
        .unwrap();
        let c = read_section(dir.path(), "filled.md");
        match c {
            SectionContent::Filled(body) => assert!(body.contains("camelCase")),
            other => panic!("expected Filled, got {}", kind(&other)),
        }
    }

    #[test]
    fn read_section_classifies_filled_with_run_name_pattern_as_filled() {
        // Round 3.5 regression: real Rust code like `run_<name>` in prose
        // must not trip the templated detector.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("known-constraints.md");
        fs::write(
            &p,
            "# Known\n\n- **Statement:** Subcommands wire via `Commands::<Name>(args) => <name>::run_<name>(args)`.\n\
             - Generic helpers may use `Vec<T>` and `Option<T>` freely.\n\
             - `repolens remember --file <session-summary.md>` is the planned spelling.\n",
        )
        .unwrap();
        let c = read_section(dir.path(), "known-constraints.md");
        assert_eq!(
            kind(&c),
            "Filled",
            "code-shaped strings must not trip the templated detector"
        );
    }

    // -- effective_task / DEFAULT_TASK ----------------------------------------

    #[test]
    fn effective_task_defaults_when_task_absent() {
        assert_eq!(effective_task(None), DEFAULT_TASK);
    }

    #[test]
    fn effective_task_passes_explicit_task_through() {
        assert_eq!(effective_task(Some("add a flag")), "add a flag");
    }

    #[test]
    fn default_task_is_honest_and_not_memory_related() {
        // The default must say no task was given (not pretend one was) and
        // must not trip the Memory Safety Notice keyword check.
        assert!(DEFAULT_TASK.contains("No task was provided"));
        assert!(!task_is_memory_related(DEFAULT_TASK));
    }

    #[test]
    fn compose_brief_with_default_task_renders_it_under_task_heading() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(dir.path(), DEFAULT_TASK);
        assert!(body.contains("## Task"));
        assert!(body.contains("No task was provided"));
        assert!(!body.contains("## Memory Safety Notice"));
    }

    // -- task_is_memory_related ----------------------------------------------

    #[test]
    fn task_is_memory_related_matches_remember_keyword() {
        assert!(task_is_memory_related(
            "Implement minimal repolens remember --file <session-summary.md>"
        ));
    }

    #[test]
    fn task_is_memory_related_matches_case_insensitively() {
        assert!(task_is_memory_related("Add MemoryStore quarantine sidecar"));
        assert!(task_is_memory_related("Tune Recall Performance"));
    }

    #[test]
    fn task_is_memory_related_does_not_false_positive_on_unrelated_tasks() {
        for t in [
            "Add a --quiet flag to scan",
            "Refactor compare-summaries renderer",
            "Tighten clippy lints",
            "Add wiki staleness threshold",
        ] {
            assert!(
                !task_is_memory_related(t),
                "task '{t}' should NOT be memory-related"
            );
        }
    }

    // -- compose_brief -------------------------------------------------------

    #[test]
    fn compose_brief_includes_task_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(dir.path(), "weird $TASK string");
        assert!(body.contains("weird $TASK string"));
        assert!(body.contains("# Current Agent Brief"));
    }

    #[test]
    fn compose_brief_includes_all_required_sections() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(dir.path(), "any");
        for h in [
            "## Task",
            "## Workspace Status",
            "## Runtime Freshness",
            "## Repo Context",
            "## Known Constraints",
            "## Prior Session Memory",
            "## Agent Friction Warnings",
            "## Suggested First Checks",
            "### Repo freshness checklist",
            "## Risks",
            "## Acceptance Criteria",
            "## Open Questions",
            "## Next Steps for Coding Agent",
        ] {
            assert!(body.contains(h), "missing required section: {h}");
        }
    }

    #[test]
    fn compose_brief_points_to_findings_command() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(dir.path(), "task");
        assert!(
            body.contains("repolens findings"),
            "brief must point agents at the findings command; body=\n{body}"
        );
    }

    #[test]
    fn compose_brief_omits_memory_safety_notice_for_unrelated_task() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(dir.path(), "Add a --quiet flag to scan");
        assert!(!body.contains("Memory Safety Notice"));
    }

    #[test]
    fn compose_brief_includes_memory_safety_notice_for_memory_task() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(
            dir.path(),
            "Implement minimal repolens remember --file <session-summary.md>",
        );
        assert!(body.contains("## Memory Safety Notice"));
        assert!(body.contains("unverified hypotheses"));
    }

    #[test]
    fn compose_brief_workspace_status_table_lists_every_input() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief(dir.path(), "task");
        for name in WORKSPACE_FILES {
            assert!(
                body.contains(&format!("`{name}`")),
                "Workspace Status row for {name} missing"
            );
        }
    }

    #[test]
    fn compose_brief_is_pure_function_of_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let a = compose_brief(dir.path(), "task");
        let b = compose_brief(dir.path(), "task");
        assert_eq!(a, b, "compose_brief must be deterministic");
    }

    // -- Runtime Freshness ---------------------------------------------------

    #[test]
    fn compose_brief_renders_runtime_freshness_section_with_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief_with_freshness(dir.path(), "task", &FreshnessSnapshot::default());
        assert!(
            body.contains("## Runtime Freshness"),
            "missing Runtime Freshness section; body=\n{body}"
        );
        assert!(
            body.contains("No `.repolens/session.json` found"),
            "empty snapshot must render the no-session fallback row; body=\n{body}"
        );
        assert!(
            body.contains("Not a git checkout or git unavailable"),
            "empty snapshot must render the no-git fallback row; body=\n{body}"
        );
        let ws = body
            .find("## Workspace Status")
            .expect("workspace status missing");
        let rf = body
            .find("## Runtime Freshness")
            .expect("runtime freshness missing");
        let rc = body.find("## Repo Context").expect("repo context missing");
        assert!(
            ws < rf && rf < rc,
            "Runtime Freshness must sit between Workspace Status and Repo Context"
        );
    }

    fn session_with_active(active: &str, updated_unix: Option<i64>) -> SessionFreshness {
        SessionFreshness {
            active_pack: Some(active.to_string()),
            updated_at: None,
            updated_at_unix: updated_unix,
            last_scan_mode: None,
        }
    }

    #[test]
    fn compose_brief_warns_when_repo_overview_cites_stale_pack() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace_file(
            dir.path(),
            "repo-overview.md",
            "# Repo\n\n**Last scan:** pack-20260101-000000 (`.repolens/pack-20260101-000000/`)\n",
        );
        let snapshot = FreshnessSnapshot {
            session: Some(session_with_active("pack-20260516-015206", None)),
            git: None,
            workspace_mtimes: BTreeMap::new(),
        };
        let body = compose_brief_with_freshness(dir.path(), "task", &snapshot);
        assert!(
            body.contains("cites stale pack `pack-20260101-000000`"),
            "expected stale-pack warning; body=\n{body}"
        );
        assert!(
            body.contains("active: `pack-20260516-015206`"),
            "stale-pack warning must name the active pack; body=\n{body}"
        );
    }

    #[test]
    fn compose_brief_no_stale_pack_warning_when_citation_matches_active() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace_file(
            dir.path(),
            "repo-overview.md",
            "# Repo\n\n**Last scan:** pack-20260516-015206\n",
        );
        let snapshot = FreshnessSnapshot {
            session: Some(session_with_active("pack-20260516-015206", None)),
            git: None,
            workspace_mtimes: BTreeMap::new(),
        };
        let body = compose_brief_with_freshness(dir.path(), "task", &snapshot);
        assert!(
            !body.contains("cites stale pack"),
            "no warning expected when citation matches active pack; body=\n{body}"
        );
    }

    #[test]
    fn compose_brief_warns_when_workspace_input_predates_active_pack() {
        let dir = tempfile::tempdir().unwrap();
        let session_unix = 1_747_360_000_i64;
        let mut mtimes = BTreeMap::new();
        mtimes.insert(
            "repo-overview.md".to_string(),
            Some(session_unix - 2 * 86_400),
        );
        let snapshot = FreshnessSnapshot {
            session: Some(session_with_active(
                "pack-20260516-015206",
                Some(session_unix),
            )),
            git: None,
            workspace_mtimes: mtimes,
        };
        let body = compose_brief_with_freshness(dir.path(), "task", &snapshot);
        assert!(
            body.contains(
                "Workspace input `repo-overview.md` predates the active pack by 2 day(s)"
            ),
            "expected workspace-age warning; body=\n{body}"
        );
    }

    #[test]
    fn compose_brief_no_age_warning_within_grace_window() {
        let dir = tempfile::tempdir().unwrap();
        let session_unix = 1_747_360_000_i64;
        let mut mtimes = BTreeMap::new();
        // One hour older — inside the 24h grace window.
        mtimes.insert("repo-overview.md".to_string(), Some(session_unix - 3_600));
        let snapshot = FreshnessSnapshot {
            session: Some(session_with_active(
                "pack-20260516-015206",
                Some(session_unix),
            )),
            git: None,
            workspace_mtimes: mtimes,
        };
        let body = compose_brief_with_freshness(dir.path(), "task", &snapshot);
        assert!(
            !body.contains("predates the active pack"),
            "no age warning expected within the grace window; body=\n{body}"
        );
    }

    #[test]
    fn detect_stale_pack_citations_flags_non_active_id() {
        let warnings = detect_stale_pack_citations(
            "Last scan: pack-20260101-000000 see pack-20260101-000000",
            "pack-20260516-015206",
        );
        assert_eq!(warnings.len(), 1, "duplicate citations should dedupe");
        assert!(warnings[0].contains("pack-20260101-000000"));
        assert!(warnings[0].contains("pack-20260516-015206"));
    }

    #[test]
    fn detect_stale_pack_citations_ignores_bare_pack_prose() {
        // Non date-shaped `pack-` strings must not be treated as pack ids.
        let warnings = detect_stale_pack_citations(
            "The pack-building step writes a pack-file; see `pack-foo`.",
            "pack-20260516-015206",
        );
        assert!(
            warnings.is_empty(),
            "bare pack- prose must not false-positive: {warnings:?}"
        );
    }

    #[test]
    fn detect_stale_pack_citations_empty_when_only_active_cited() {
        let warnings =
            detect_stale_pack_citations("Active: pack-20260516-015206", "pack-20260516-015206");
        assert!(warnings.is_empty());
    }

    #[test]
    fn detect_stale_pack_citations_rejects_embedded_tokens() {
        // Leading alphanumeric (`xpack-...`) and a trailing extra digit
        // (`...-0000000`) mean the match is part of a longer token, not a
        // standalone pack id.
        let warnings = detect_stale_pack_citations(
            "xpack-20260101-000000 and pack-20260101-0000000 and pack-20260101-000000abc",
            "pack-20260516-015206",
        );
        assert!(
            warnings.is_empty(),
            "embedded/malformed ids must not match: {warnings:?}"
        );
    }

    #[test]
    fn compose_brief_runtime_freshness_renders_git_rows() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = FreshnessSnapshot {
            session: None,
            git: Some(GitHeadSnapshot {
                branch: Some("main".to_string()),
                short: Some("abc1234".to_string()),
                dirty: Some(true),
            }),
            workspace_mtimes: BTreeMap::new(),
        };
        let body = compose_brief_with_freshness(dir.path(), "task", &snapshot);
        assert!(body.contains("| Git branch | main |"), "body=\n{body}");
        assert!(body.contains("| Git HEAD | abc1234 |"), "body=\n{body}");
        assert!(body.contains("dirty (uncommitted"), "body=\n{body}");
    }

    #[test]
    fn compose_brief_runtime_freshness_renders_unknown_dirty_state() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = FreshnessSnapshot {
            session: None,
            git: Some(GitHeadSnapshot {
                branch: None,
                short: Some("abc1234".to_string()),
                dirty: None,
            }),
            workspace_mtimes: BTreeMap::new(),
        };
        let body = compose_brief_with_freshness(dir.path(), "task", &snapshot);
        assert!(
            body.contains("| Git branch | (detached HEAD) |"),
            "body=\n{body}"
        );
        assert!(
            body.contains("unknown (git status unavailable)"),
            "body=\n{body}"
        );
    }

    #[test]
    fn compose_brief_with_populated_freshness_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace_file(
            dir.path(),
            "repo-overview.md",
            "# Repo\n\n**Last scan:** pack-20260101-000000\n",
        );
        let session_unix = 1_747_360_000_i64;
        let mut mtimes = BTreeMap::new();
        mtimes.insert(
            "repo-overview.md".to_string(),
            Some(session_unix - 5 * 86_400),
        );
        mtimes.insert("known-constraints.md".to_string(), Some(session_unix));
        let make = || FreshnessSnapshot {
            session: Some(SessionFreshness {
                active_pack: Some("pack-20260516-015206".to_string()),
                updated_at: Some("2026-05-16T01:52:06Z".to_string()),
                updated_at_unix: Some(session_unix),
                last_scan_mode: Some("incremental".to_string()),
            }),
            git: Some(GitHeadSnapshot {
                branch: Some("main".to_string()),
                short: Some("abc1234".to_string()),
                dirty: Some(false),
            }),
            workspace_mtimes: mtimes.clone(),
        };
        let a = compose_brief_with_freshness(dir.path(), "task", &make());
        let b = compose_brief_with_freshness(dir.path(), "task", &make());
        assert_eq!(a, b, "populated freshness must render deterministically");
    }

    // -- Round 5.2 — TemplateOnly hardening ---------------------------------

    #[test]
    fn render_section_template_only_does_not_embed_raw_body() {
        let raw = "# Known Constraints\n\n### C-001 _<short-name>_\n";
        let out = render_section(KNOWN_CONSTRAINTS, &SectionContent::TemplateOnly(raw.into()));
        assert!(
            !out.contains("C-001 _<short-name>_"),
            "TemplateOnly must not embed raw placeholder body; out=\n{out}"
        );
        assert!(
            out.contains("little or no filled workspace content"),
            "TemplateOnly must surface the canonical warning; out=\n{out}"
        );
        assert!(
            out.contains("known-constraints.md"),
            "TemplateOnly notice should name the underlying file; out=\n{out}"
        );
        assert!(
            !out.contains("<details>"),
            "Round 5.2: TemplateOnly no longer wraps raw body in <details>; out=\n{out}"
        );
    }

    #[test]
    fn render_section_partial_places_warning_above_body() {
        let body = "# Known Constraints\n\n- C-001: Real constraint.\n- C-002: TODO _..._";
        let out = render_section(KNOWN_CONSTRAINTS, &SectionContent::Partial(body.into()));
        let warn = out
            .find("Some workspace placeholders remain")
            .expect("partial warning missing");
        let body_pos = out.find("# Known Constraints").expect("body missing");
        assert!(
            warn < body_pos,
            "warning must appear above the body so the agent reads it first; out=\n{out}"
        );
        assert!(
            out.contains("- C-001: Real constraint."),
            "real partial content must still surface verbatim"
        );
    }

    // -- Round 5.2 — Pick up here lift --------------------------------------

    #[test]
    fn parse_atx_heading_basic_levels() {
        assert_eq!(parse_atx_heading("# A"), Some((1, "A")));
        assert_eq!(
            parse_atx_heading("### Pick up here"),
            Some((3, "Pick up here"))
        );
        assert_eq!(parse_atx_heading("###### deep"), Some((6, "deep")));
    }

    #[test]
    fn parse_atx_heading_rejects_non_headings() {
        assert_eq!(parse_atx_heading("not a heading"), None);
        assert_eq!(parse_atx_heading("#hashtag"), None);
        assert_eq!(parse_atx_heading("    # too indented"), None);
        assert_eq!(parse_atx_heading("####### h7"), None);
    }

    #[test]
    fn lift_pickup_here_block_extracts_simple_block() {
        let body = "## Session A\n\n### Pick up here\n\nDo X next.\n\n### Other notes\nblah\n";
        let lifted = lift_pickup_here_block(body).expect("expected lift");
        assert!(lifted.starts_with("### Pick up here"));
        assert!(lifted.contains("Do X next."));
        assert!(
            !lifted.contains("Other notes"),
            "lift must stop at the next same-or-higher-level heading; lifted=\n{lifted}"
        );
    }

    #[test]
    fn lift_pickup_here_block_handles_trailing_period_and_case() {
        let body = "#### pick up here.\n\nNext step is Y.\n";
        let lifted = lift_pickup_here_block(body).expect("expected lift");
        assert!(lifted.contains("Next step is Y."));
    }

    #[test]
    fn lift_pickup_here_block_returns_none_when_absent() {
        let body = "## Session A\n\n### Outcome\n\nShipped X.\n";
        assert!(lift_pickup_here_block(body).is_none());
    }

    #[test]
    fn lift_pickup_here_block_picks_first_occurrence_as_latest() {
        let body = "## Session B (newest, prepended)\n\n### Pick up here\n\nNew next step.\n\n\
                    ## Session A (older)\n\n### Pick up here\n\nOld next step.\n";
        let lifted = lift_pickup_here_block(body).expect("expected lift");
        assert!(lifted.contains("New next step."));
        assert!(
            !lifted.contains("Old next step."),
            "lift must take the first (newest) occurrence; lifted=\n{lifted}"
        );
    }

    #[test]
    fn lift_pickup_here_block_stops_at_top_level_heading_when_lifted_is_deep() {
        // A deeply nested Pick up here should still terminate at any
        // higher-level heading (smaller number of `#`s).
        let body = "## Session\n\n##### Pick up here\n\nbody\n\n## Next session\n\nelse\n";
        let lifted = lift_pickup_here_block(body).expect("expected lift");
        assert!(lifted.contains("body"));
        assert!(!lifted.contains("Next session"));
    }

    fn write_workspace_file(dir: &std::path::Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn compose_brief_lifts_latest_pick_up_here_into_session_memory_section() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace_file(
            dir.path(),
            "session-memory.md",
            "# Session Memory\n\n## Session 2026-04-26 — feature X\n\n\
             ### What I did\n\nshipped a thing\n\n\
             ### Pick up here\n\nFinish wiring foo to bar in `crates/baz/src/lib.rs`.\n",
        );
        let body = compose_brief(dir.path(), "continue work");

        let prior_pos = body
            .find("## Prior Session Memory")
            .expect("Prior Session Memory section missing");
        let lift_pos = body
            .find("Latest \"Pick up here\"")
            .expect("lift heading missing");
        let next_section_pos = body[prior_pos..]
            .find("\n## ")
            .map(|i| prior_pos + i)
            .unwrap_or(body.len());
        // The lift heading should be inside the Prior Session Memory section.
        assert!(
            prior_pos < lift_pos && lift_pos < next_section_pos,
            "lift heading must sit inside ## Prior Session Memory; lift at {lift_pos}, prior at {prior_pos}, next at {next_section_pos}"
        );
        assert!(
            body.contains("Finish wiring foo to bar"),
            "lift content missing"
        );
        // Safety framing preserved.
        for phrase in ["agent-provided", "unverified", "hypothesis"] {
            assert!(
                body.contains(phrase),
                "lift must preserve safety framing phrase '{phrase}'"
            );
        }
        // Full prior session memory body still appears below the lift.
        let after_divider = body[lift_pos..]
            .find("\n---\n")
            .expect("divider missing between lift and full body");
        let tail = &body[lift_pos + after_divider..];
        assert!(
            tail.contains("Finish wiring foo to bar"),
            "full prior session memory should still appear below the lift; tail=\n{tail}"
        );
    }

    #[test]
    fn compose_brief_omits_lift_when_no_pick_up_here_block() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace_file(
            dir.path(),
            "session-memory.md",
            "# Session Memory\n\n## Session 2026-04-26\n\nShipped X. No explicit handoff.\n",
        );
        let body = compose_brief(dir.path(), "continue");
        assert!(
            !body.contains("Latest \"Pick up here\""),
            "no lift heading when no Pick up here block exists"
        );
        assert!(
            body.contains("Shipped X. No explicit handoff."),
            "session memory body still surfaced"
        );
    }

    #[test]
    fn compose_brief_lift_picks_newest_when_multiple_remembered_sessions() {
        let dir = tempfile::tempdir().unwrap();
        // remember prepends new entries to the top, so the *first* Pick up
        // here in the file is the newest.
        write_workspace_file(
            dir.path(),
            "session-memory.md",
            "# Session Memory\n\n## Session B (newest)\n\n### Pick up here\n\n\
             NEW: investigate failing test in foo.rs.\n\n\
             ## Session A (older)\n\n### Pick up here\n\nOLD: refactor bar.rs.\n",
        );
        let body = compose_brief(dir.path(), "continue");
        let lift_pos = body
            .find("Latest \"Pick up here\"")
            .expect("lift heading missing");
        let after_lift = &body[lift_pos..];
        let divider = after_lift.find("\n---\n").expect("divider missing");
        let lifted = &after_lift[..divider];
        assert!(
            lifted.contains("NEW: investigate failing test"),
            "lift should carry the newest cue; lifted=\n{lifted}"
        );
        assert!(
            !lifted.contains("OLD: refactor bar.rs."),
            "lift must not carry the older cue; lifted=\n{lifted}"
        );
    }

    // -- Calibrated Findings section ----------------------------------------

    fn sample_finding(title: &str, action_steps: Vec<&str>) -> EvidenceFinding {
        use repolens_core::model::epistemic::{EpistemicConfidence, EvidenceGrade, KnowledgeLayer};
        use repolens_core::model::evidence::{
            metric_names, EvidenceItem, EvidenceScope, EvidenceSource, FindingClassification,
            FindingId, FindingSeverity, InferenceStrength, RecommendedAction, ResponsibleInference,
            ScopeTime,
        };
        EvidenceFinding {
            id: FindingId::new("F-LANGUAGE-EXTRACTION-GAP"),
            title: title.to_string(),
            classification: FindingClassification::Informational,
            severity: FindingSeverity::Low,
            knowledge_layer: KnowledgeLayer::Observation,
            evidence_grade: EvidenceGrade::Derived,
            confidence: EpistemicConfidence::Codified,
            evidence: vec![EvidenceItem::Observation {
                metric: metric_names::EXTRACTION_LANGUAGES_BY_FIDELITY.into(),
                observed_value: serde_json::json!({"unsupported": {"loc": 1902, "languages": ["Shell"]}}),
                source_paths: vec![],
                source_type: EvidenceSource::ProjectProfile,
                invariant_id: None,
            }],
            inference: ResponsibleInference {
                statement: "RepoLens has no symbol extractor for 1 language(s) (1902 LOC: Shell)."
                    .into(),
                strength: InferenceStrength::Strong,
                depends_on: vec![],
                does_not_imply: vec![],
            },
            limitations: vec![],
            action: RecommendedAction::ConcreteAction {
                steps: action_steps.into_iter().map(String::from).collect(),
            },
            scope: EvidenceScope {
                paths: vec![],
                time: ScopeTime {
                    pack_id: "pack-test".into(),
                    commit: None,
                },
                languages: vec!["Shell".into()],
            },
            not_checked: vec![],
            repo_context: vec![],
            false_positive_notes: vec![],
            provenance: None,
        }
    }

    #[test]
    fn render_calibrated_findings_lists_title_class_sev_and_action() {
        let f = sample_finding(
            "Source files in languages without full symbol extraction (1 language(s), 1902 LOC)",
            vec![
                "Treat the graph as incomplete for Shell",
                "Read source instead",
            ],
        );
        let md = render_calibrated_findings(&[f]);
        // Bullets wrap at 80 columns (#20); compare with lines joined.
        let md = md.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            md.contains("1 calibrated finding(s) from the active pack"),
            "count line missing; md=\n{md}"
        );
        assert!(
            md.contains("Source files in languages without full symbol extraction"),
            "title missing; md=\n{md}"
        );
        assert!(
            md.contains("Informational"),
            "classification missing; md=\n{md}"
        );
        assert!(md.contains("sev=Low"), "severity missing; md=\n{md}");
        assert!(
            md.contains("Treat the graph as incomplete for Shell"),
            "action one-liner missing; md=\n{md}"
        );
    }

    /// #20: a bullet longer than 80 columns wraps, with continuation lines
    /// indented so Markdown keeps them in the same list item.
    #[test]
    fn render_calibrated_findings_wraps_bullets_within_80_columns() {
        use repolens_core::model::evidence::RecommendedAction;
        let mut f = sample_finding("No unused files found (imports only)", vec![]);
        f.severity = repolens_core::model::evidence::FindingSeverity::None;
        f.action = RecommendedAction::NoAction;
        let md = render_calibrated_findings(&[f]);
        let bullet: Vec<&str> = md
            .lines()
            .skip_while(|l| !l.starts_with("- **"))
            .take_while(|l| !l.is_empty())
            .collect();
        assert!(bullet.len() >= 2, "expected a wrapped bullet; md=\n{md}");
        assert!(
            bullet[0].starts_with("- **No unused files found (imports only)**"),
            "{md}"
        );
        for l in &bullet[1..] {
            assert!(l.starts_with("  ") && !l.starts_with("   "), "{md}");
        }
        for l in md.lines().skip(2) {
            assert!(crate::findings::cols(l) <= 80, "{} cols: {l}", l.len());
        }
    }

    #[test]
    fn render_calibrated_findings_empty_states_none() {
        let md = render_calibrated_findings(&[]);
        assert!(
            md.contains("No calibrated findings for the active pack"),
            "empty-state message missing; md=\n{md}"
        );
    }

    #[test]
    fn compose_brief_full_inlines_findings_section() {
        let dir = tempfile::tempdir().unwrap();
        let f = sample_finding("My Finding Title", vec!["do x"]);
        let body = compose_brief_full(
            dir.path(),
            "task",
            &FreshnessSnapshot::default(),
            &[f],
            &TaskMatch::NoTask,
        );
        assert!(
            body.contains("## Calibrated Findings"),
            "section heading missing; body=\n{body}"
        );
        assert!(
            body.contains("My Finding Title"),
            "finding title not inlined; body=\n{body}"
        );
        // Suggested First Checks now points at the inline section, not a bare command.
        assert!(
            body.contains(
                "**Calibrated Findings** section above lists the active pack's evidence \
                 findings inline"
            ),
            "suggested-first-checks pointer not updated; body=\n{body}"
        );
    }

    #[test]
    fn compose_brief_full_without_findings_keeps_section_with_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let body = compose_brief_full(
            dir.path(),
            "task",
            &FreshnessSnapshot::default(),
            &[],
            &TaskMatch::NoTask,
        );
        assert!(
            body.contains("## Calibrated Findings"),
            "section must always be present; body=\n{body}"
        );
        assert!(
            body.contains("No calibrated findings for the active pack"),
            "empty-state missing; body=\n{body}"
        );
    }

    #[test]
    fn session_time_shows_local_offset_and_utc() {
        // 2026-10-12T21:40:53Z rendered for UTC+8 (Asia/Shanghai).
        let at = time::OffsetDateTime::from_unix_timestamp(1_791_841_253).unwrap();
        assert_eq!(
            format_session_time(at, 8 * 3600).as_deref(),
            Some("2026-10-13 05:40:53 +08:00 (2026-10-12 21:40:53 UTC)")
        );
    }

    #[test]
    fn session_time_negative_and_zero_offsets() {
        let at = time::OffsetDateTime::from_unix_timestamp(1_791_841_253).unwrap();
        assert_eq!(
            format_session_time(at, -(7 * 3600)).as_deref(),
            Some("2026-10-12 14:40:53 -07:00 (2026-10-12 21:40:53 UTC)")
        );
        assert_eq!(
            format_session_time(at, 0).as_deref(),
            Some("2026-10-12 21:40:53 UTC")
        );
    }

    #[test]
    fn session_time_is_same_instant_regardless_of_input_offset() {
        // A session stored with a non-UTC offset must still render the true instant.
        let at = time::OffsetDateTime::from_unix_timestamp(1_791_841_253)
            .unwrap()
            .to_offset(time::UtcOffset::from_hms(5, 30, 0).unwrap());
        assert_eq!(
            format_session_time(at, 8 * 3600).as_deref(),
            Some("2026-10-13 05:40:53 +08:00 (2026-10-12 21:40:53 UTC)")
        );
    }
}
