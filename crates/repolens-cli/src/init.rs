//! `repolens init` — scaffold a `.repolens/` memory workspace from the
//! workspace templates. Idempotent by default; `--force` overwrites.
//!
//! Templates are embedded at compile time via `include_str!` so the binary
//! is self-contained and does not require the `docs/templates/` tree to be
//! present at runtime.

use anyhow::{anyhow, Result};
use clap::Args;
use repolens_core::atomic_write::write_atomic;
use repolens_core::storage::RepoStoragePaths;
use std::fs;
use std::path::{Path, PathBuf};

use crate::resolve_root_from;

/// Filename + embedded template body. Order is preserved in the output
/// summary so users see a stable list.
const TEMPLATES: &[(&str, &str)] = &[
    (
        "repo-overview.md",
        include_str!("../../../docs/templates/repolens/repo-overview.md"),
    ),
    (
        "known-constraints.md",
        include_str!("../../../docs/templates/repolens/known-constraints.md"),
    ),
    (
        "session-memory.md",
        include_str!("../../../docs/templates/repolens/session-memory.md"),
    ),
    (
        "agent-friction-log.md",
        include_str!("../../../docs/templates/repolens/agent-friction-log.md"),
    ),
    (
        "current-agent-brief.md",
        include_str!("../../../docs/templates/repolens/current-agent-brief.md"),
    ),
    (
        "eval-results.md",
        include_str!("../../../docs/templates/repolens/eval-results.md"),
    ),
];

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Rewrite all workspace files. Files you edited are first copied to
    /// .repolens/backup/<time>/
    #[arg(long, default_value_t = false)]
    pub force: bool,
    /// Keep blank templates; do not auto-draft workspace files from the repo
    #[arg(long, default_value_t = false)]
    pub no_draft: bool,
    /// Also list every workspace file and what happened to it
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct InitOutcome {
    created: Vec<String>,
    overwritten: Vec<String>,
    skipped: Vec<String>,
}

pub fn run_init(args: InitArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;

    let storage_dir = RepoStoragePaths::storage_dir(&root);
    fs::create_dir_all(&storage_dir).map_err(|e| {
        anyhow!(
            "Cannot create .repolens directory at {}: {}",
            storage_dir.display(),
            e
        )
    })?;
    RepoStoragePaths::ensure_storage_ignored(&storage_dir);

    // --force: copy every edited workspace file aside before rewriting.
    let backup = if args.force {
        backup_edited(&storage_dir, &chrono::Local::now())?
    } else {
        None
    };

    let outcome = scaffold_templates(&storage_dir, args.force)?;
    // `.repolens/.gitignore` (written above and by every command that writes
    // to `.repolens/`) keeps git status clean. init never edits the root
    // `.gitignore`: that edit left the tree dirty, so scans stayed full (#39).
    let git_ignored = in_git_worktree(&root) && storage_dir.join(".gitignore").is_file();

    let pack = crate::findings::resolve_findings_pack(&root, None).ok();
    let facts = crate::autodraft::gather(&root, pack.as_deref());
    let report = if args.no_draft {
        None
    } else {
        Some(draft_workspace(&storage_dir, &facts)?)
    };
    let kept = kept_edited(&storage_dir, args.force);

    let screen = Screen {
        backup: backup.as_ref(),
        outcome: &outcome,
        git_ignored,
        facts: &facts,
        report: report.as_ref(),
        kept: &kept,
        verbose: args.verbose,
    };
    for line in render_screen(&screen) {
        println!("{line}");
    }
    Ok(())
}

/// Files copied aside by `--force`.
#[derive(Debug, PartialEq, Eq)]
struct Backup {
    /// Relative to the repo root, with a trailing slash.
    dir: String,
    files: Vec<String>,
}

/// A workspace file counts as edited when it is not empty, not the shipped
/// template and not byte-identical to the last auto-draft.
fn is_edited(
    storage_dir: &Path,
    name: &str,
    ledger: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Ok(current) = fs::read_to_string(storage_dir.join(name)) else {
        return false;
    };
    let prior_draft = ledger
        .get(name)
        .is_some_and(|h| *h == crate::autodraft::sha256_hex(&current));
    !crate::autodraft::is_untouched(&current, template_for(name)) && !prior_draft
}

/// Copy edited workspace files to `.repolens/backup/<YYYYMMDD-HHMMSS>/`
/// (suffix `-2`, `-3`, ... if that folder already exists). Returns `None`
/// when nothing was edited.
fn backup_edited(
    storage_dir: &Path,
    now: &chrono::DateTime<chrono::Local>,
) -> Result<Option<Backup>> {
    let ledger = crate::autodraft::load_ledger(storage_dir);
    let edited: Vec<String> = TEMPLATES
        .iter()
        .map(|(n, _)| (*n).to_string())
        .filter(|n| is_edited(storage_dir, n, &ledger))
        .collect();
    if edited.is_empty() {
        return Ok(None);
    }
    let stamp = now.format("%Y%m%d-%H%M%S").to_string();
    let base = storage_dir.join("backup");
    let mut name = stamp.clone();
    let mut n = 2;
    while base.join(&name).exists() {
        name = format!("{stamp}-{n}");
        n += 1;
    }
    let dir = base.join(&name);
    fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("Cannot create backup folder {}: {}", dir.display(), e))?;
    for f in &edited {
        fs::copy(storage_dir.join(f), dir.join(f))
            .map_err(|e| anyhow!("Cannot back up {f} to {}: {}", dir.display(), e))?;
    }
    Ok(Some(Backup {
        dir: format!(".repolens/backup/{name}/"),
        files: edited,
    }))
}

/// Edited files left as they are (not with --force, which rewrote them).
fn kept_edited(storage_dir: &Path, force: bool) -> Vec<String> {
    if force {
        return vec![];
    }
    let ledger = crate::autodraft::load_ledger(storage_dir);
    TEMPLATES
        .iter()
        .map(|(n, _)| (*n).to_string())
        .filter(|n| is_edited(storage_dir, n, &ledger))
        .collect()
}

struct Screen<'a> {
    backup: Option<&'a Backup>,
    outcome: &'a InitOutcome,
    /// Inside a git worktree, with `.repolens/.gitignore` in place.
    git_ignored: bool,
    facts: &'a crate::autodraft::RepoFacts,
    report: Option<&'a DraftReport>,
    kept: &'a [String],
    verbose: bool,
}

/// The `init` screen: status line, findings, drafts, one Next step.
/// Every line fits in 80 columns.
fn render_screen(sc: &Screen) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(b) = sc.backup {
        let noun = if b.files.len() == 1 { "file" } else { "files" };
        out.push(format!(
            "--force: backed up {} edited {noun} to {}",
            b.files.len(),
            b.dir
        ));
    }

    let total = TEMPLATES.len();
    let new = sc.outcome.created.len();
    let files = if new == total || new == 0 {
        format!("{total} files in .repolens/")
    } else {
        format!("{total} files in .repolens/ ({new} new)")
    };
    out.push(format!("RepoLens is set up: {files}"));
    if sc.git_ignored {
        out.push(GIT_IGNORED_LINE.into());
    }

    if sc.verbose {
        out.push(String::new());
        out.push("Files:".into());
        for (name, _) in TEMPLATES {
            let what = if sc.outcome.created.iter().any(|n| n == name) {
                "created"
            } else if sc.outcome.overwritten.iter().any(|n| n == name) {
                "rewritten (--force)"
            } else {
                "kept"
            };
            out.push(format!("  {name:<24}{what}"));
        }
    }

    // Findings.
    out.push(String::new());
    match &sc.facts.scan {
        None => out.push("No scan yet. Run `repolens scan` first to see findings.".into()),
        Some(scan) => out.extend(findings_block(scan)),
    }

    // Drafts.
    out.push(String::new());
    let kc_drafted = match sc.report {
        None => {
            out.push("Blank templates created (--no-draft).".into());
            false
        }
        Some(r) if r.drafted.is_empty() => {
            out.push("Nothing new to draft.".into());
            false
        }
        Some(r) => {
            out.push("Drafted for you to check (nothing is enforced yet):".into());
            for (name, what) in &r.drafted {
                out.push(format!("  {name:<21} {what}"));
                if name == "known-constraints.md" {
                    if let Some(ci) = ci_commands_line(&r.ci_tools) {
                        out.push(format!("{:CI_LINE_INDENT$}{ci}", ""));
                    }
                }
            }
            out.push(format!("  Read from: {}", sources_line(sc.facts)));
            if r.drafted.iter().any(|(n, _)| n == "repo-overview.md") && !has_commands(sc.facts) {
                // Workflows were read but none runs a command RepoLens
                // knows: say so, never "none found in CI" (#13).
                let workflows = ci_files(sc.facts).len();
                out.push(match workflows {
                    0 => {
                        "  No build/test commands drafted: none found in CI or package.json.".into()
                    }
                    1 => {
                        "  No build/test commands drafted: none recognised in 1 CI workflow.".into()
                    }
                    n => format!(
                        "  No build/test commands drafted: none recognised in {n} CI workflows."
                    ),
                });
                out.push("  Add them in repo-overview.md.".into());
            }
            r.constraints > 0
        }
    };
    for k in sc.kept {
        out.push(format!("Kept your edited file: {k}"));
    }

    out.push(String::new());
    if kc_drafted {
        out.push(
            "Next: open .repolens/known-constraints.md. Rename a draft from D-NNN to C-NNN".into(),
        );
        out.push("once you've checked it; only C-NNN rules are enforced by validate-plan.".into());
    } else if sc.kept.iter().any(|k| k == "known-constraints.md") {
        out.push("Next: review .repolens/known-constraints.md.".into());
    } else {
        out.push("Next: fill in .repolens/known-constraints.md.".into());
    }
    // Guard: no line wider than 80 columns, whatever a title or path holds.
    out.into_iter().map(|l| fit_80(&l)).collect()
}

/// The findings part of the screen: the same headline, rows and coverage
/// note as `repolens findings`, limited to the top rows (#32).
fn findings_block(scan: &crate::autodraft::ScanFacts) -> Vec<String> {
    let p = &scan.plain;
    let mut out = Vec::new();
    let n = p.flagged.len();
    out.push(match n {
        0 => p.headline(&scan.pack_id),
        n if n > MAX_SCREEN_FINDINGS => {
            format!("{}; top {MAX_SCREEN_FINDINGS}:", p.headline(&scan.pack_id))
        }
        _ => format!("{}:", p.headline(&scan.pack_id)),
    });
    // Same blank line under the headline as `repolens findings` (#32).
    if n > 0 {
        out.push(String::new());
    }
    for row in p.flagged.iter().take(MAX_SCREEN_FINDINGS) {
        out.extend(row.iter().cloned());
    }
    out.extend(p.coverage.iter().cloned());
    if n > 0 || !p.info.is_empty() {
        out.push(format!("  {}", crate::autodraft::FINDINGS_HINT));
        out.push(
            "  Run `repolens findings` to see them all; add --verbose for the evidence.".into(),
        );
    }
    out
}

/// Screen width every `init` line must fit in.
pub(crate) const SCREEN_WIDTH: usize = 80;

/// Last-resort guard, measured in terminal columns (wide CJK characters
/// count 2). Rows are already fitted at a folder boundary before this.
fn fit_80(s: &str) -> String {
    if crate::findings::cols(s) <= SCREEN_WIDTH {
        return s.to_string();
    }
    let mut cut = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > SCREEN_WIDTH - 1 {
            break;
        }
        used += w;
        cut.push(c);
    }
    cut.push('…');
    cut
}

const MAX_SCREEN_FINDINGS: usize = 3;

/// Up to 3 named files, then a summary of the rest ("2 CI workflows").
fn sources_line(f: &crate::autodraft::RepoFacts) -> String {
    let mut named = Vec::new();
    if let Some(r) = &f.readme {
        named.push(r.path.clone());
    }
    if f.cargo.is_some() {
        named.push("Cargo.toml".to_string());
    }
    if f.toolchain_channel.is_some() {
        named.push("rust-toolchain.toml".to_string());
    }
    if f.package_json.is_some() {
        named.push("package.json".to_string());
    }
    let workflows = ci_files(f).len();
    let mut parts: Vec<String> = named.iter().take(3).cloned().collect();
    let more = named.len().saturating_sub(3);
    if more > 0 {
        let noun = if more == 1 { "file" } else { "files" };
        parts.push(format!("{more} more {noun}"));
    }
    if workflows > 0 {
        let noun = if workflows == 1 {
            "workflow"
        } else {
            "workflows"
        };
        parts.push(format!("{workflows} CI {noun}"));
    }
    parts.join(", ")
}

/// Every workflow file read (#13), plus any a command was drafted from.
fn ci_files(f: &crate::autodraft::RepoFacts) -> Vec<String> {
    let mut ci: Vec<String> =
        f.ci.iter()
            .flat_map(|c| {
                std::iter::once(c.file.clone()).chain(
                    c.also_in
                        .iter()
                        .map(|l| l.rsplit_once(':').map(|x| x.0).unwrap_or(l).to_string()),
                )
            })
            .chain(f.ci_workflows.iter().cloned())
            .collect();
    ci.sort();
    ci.dedup();
    ci
}

/// Printed in a git worktree: `.repolens/.gitignore` hides the folder, and
/// the user's own files (root `.gitignore` included) are left alone (#39).
const GIT_IGNORED_LINE: &str =
    "Git ignores .repolens/ via its own .gitignore; your files are unchanged.";

/// True when `root` is inside a git worktree.
fn in_git_worktree(root: &Path) -> bool {
    gix::discover(root).is_ok_and(|r| r.workdir().is_some())
}

/// Write each template to `storage_dir`, honoring the `force` flag.
/// Existing files are preserved unless `force` is set.
fn scaffold_templates(storage_dir: &Path, force: bool) -> Result<InitOutcome> {
    let mut outcome = InitOutcome::default();

    for (name, body) in TEMPLATES {
        let target = storage_dir.join(name);
        let pre_existed = target.exists();

        if pre_existed && !force {
            outcome.skipped.push((*name).to_string());
            continue;
        }

        write_atomic(&target, body.as_bytes()).map_err(|e| {
            anyhow!(
                "Failed to write template {} to {}: {}",
                name,
                target.display(),
                e
            )
        })?;

        if pre_existed {
            outcome.overwritten.push((*name).to_string());
        } else {
            outcome.created.push((*name).to_string());
        }
    }

    Ok(outcome)
}

/// Files `init` can auto-draft, in output order.
const DRAFTABLE: &[&str] = &["known-constraints.md", "repo-overview.md"];

#[derive(Debug, Default, PartialEq, Eq)]
struct DraftReport {
    /// (file, one-line description)
    drafted: Vec<(String, String)>,
    /// Draft constraints written to known-constraints.md (D-NNN).
    constraints: usize,
    /// CI drafts per tool, most first, when known-constraints.md was
    /// drafted (#45).
    ci_tools: Vec<(String, usize)>,
}

/// Indent of the CI count line under the known-constraints.md row: a
/// little past the row's own, so four tools and `…and N more` fit.
const CI_LINE_INDENT: usize = 4;

/// `12 CI commands (cargo 7, npm 3, make 2)`: at most four tools, then
/// `…and N more`, fewer if the line would pass 80 columns (#45).
fn ci_commands_line(tools: &[(String, usize)]) -> Option<String> {
    let total: usize = tools.iter().map(|(_, n)| n).sum();
    if total == 0 {
        return None;
    }
    let commands = if total == 1 { "command" } else { "commands" };
    let budget = SCREEN_WIDTH - CI_LINE_INDENT;
    for shown in (1..=tools.len().min(4)).rev() {
        let mut parts: Vec<String> = tools[..shown]
            .iter()
            .map(|(t, n)| format!("{t} {n}"))
            .collect();
        if tools.len() > shown {
            parts.push(format!("\u{2026}and {} more", tools.len() - shown));
        }
        let line = format!("{total} CI {commands} ({})", parts.join(", "));
        if crate::findings::cols(&line) <= budget {
            return Some(line);
        }
    }
    Some(format!("{total} CI {commands} from {} tools", tools.len()))
}

fn template_for(name: &str) -> &'static str {
    TEMPLATES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
        .unwrap_or("")
}

/// Draft each still-untouched workspace file from repo facts. A file counts
/// as untouched if it equals the shipped template, is empty, or still equals
/// the bytes the previous `init` drafted (tracked by sha256 in
/// `.repolens/.autodraft.json`). Anything else was edited and is left alone.
fn draft_workspace(storage_dir: &Path, facts: &crate::autodraft::RepoFacts) -> Result<DraftReport> {
    let mut ledger = crate::autodraft::load_ledger(storage_dir);
    let mut report = DraftReport::default();

    for name in DRAFTABLE {
        if is_edited(storage_dir, name, &ledger) {
            continue;
        }
        let path = storage_dir.join(name);
        let current = fs::read_to_string(&path).unwrap_or_default();
        let rendered = match *name {
            "known-constraints.md" => {
                crate::autodraft::render_known_constraints(facts).map(|(body, n)| {
                    report.constraints = n;
                    report.ci_tools = crate::autodraft::ci_draft_groups(facts)
                        .into_iter()
                        .map(|(tool, ids)| (tool, ids.len()))
                        .collect();
                    let what = match n {
                        0 => "no draft constraints yet".to_string(),
                        1 => "1 draft constraint, D-001".to_string(),
                        n => format!("{n} draft constraints, D-001 to D-{n:03}"),
                    };
                    (body, what)
                })
            }
            "repo-overview.md" => {
                crate::autodraft::render_repo_overview(facts).map(|b| (b, overview_parts(facts)))
            }
            _ => None,
        };
        let Some((body, what)) = rendered else {
            continue;
        };
        if current != body {
            write_atomic(&path, body.as_bytes())
                .map_err(|e| anyhow!("Failed to write {}: {}", path.display(), e))?;
        }
        ledger.insert((*name).to_string(), crate::autodraft::sha256_hex(&body));
        report.drafted.push(((*name).to_string(), what));
    }
    crate::autodraft::save_ledger(storage_dir, &ledger)
        .map_err(|e| anyhow!("Failed to write draft ledger: {e}"))?;
    Ok(report)
}

/// True when repo-overview.md gets build/test commands.
fn has_commands(f: &crate::autodraft::RepoFacts) -> bool {
    !f.ci.is_empty()
        || f.package_json
            .as_ref()
            .is_some_and(|p| !p.scripts.is_empty())
}

/// What the drafted repo-overview.md contains, e.g. "package, languages,
/// build and test commands".
fn overview_parts(f: &crate::autodraft::RepoFacts) -> String {
    let mut v = Vec::new();
    let has_name = f.cargo.as_ref().is_some_and(|c| c.name.is_some())
        || f.package_json.as_ref().is_some_and(|p| p.name.is_some());
    if has_name {
        v.push("package");
    }
    if f.scan.is_some() {
        v.push("languages");
    }
    let has_cmds = !f.ci.is_empty()
        || f.package_json
            .as_ref()
            .is_some_and(|p| !p.scripts.is_empty());
    if has_cmds {
        v.push("build and test commands");
    }
    if v.is_empty() {
        v.push("README description");
    }
    v.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_creates_all_templates_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = scaffold_templates(dir.path(), false).unwrap();
        assert_eq!(outcome.created.len(), TEMPLATES.len());
        assert!(outcome.overwritten.is_empty());
        assert!(outcome.skipped.is_empty());
        for (name, _) in TEMPLATES {
            assert!(dir.path().join(name).exists(), "{name} not written");
        }
    }

    #[test]
    fn scaffold_skips_existing_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let preserved = dir.path().join("repo-overview.md");
        fs::write(&preserved, "user content\n").unwrap();

        let outcome = scaffold_templates(dir.path(), false).unwrap();
        assert!(outcome.skipped.iter().any(|n| n == "repo-overview.md"));
        assert_eq!(fs::read_to_string(&preserved).unwrap(), "user content\n");
    }

    #[test]
    fn scaffold_overwrites_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("repo-overview.md");
        fs::write(&target, "stale\n").unwrap();

        let outcome = scaffold_templates(dir.path(), true).unwrap();
        assert!(outcome.overwritten.iter().any(|n| n == "repo-overview.md"));
        let body = fs::read_to_string(&target).unwrap();
        assert_ne!(body, "stale\n");
    }

    #[test]
    fn template_count_is_six() {
        assert_eq!(TEMPLATES.len(), 6);
    }

    #[test]
    fn template_bodies_are_non_empty_and_start_with_html_comment() {
        for (name, body) in TEMPLATES {
            assert!(!body.is_empty(), "{name} body empty");
            assert!(
                body.starts_with("<!--"),
                "{name} should start with template header"
            );
        }
    }

    fn scan(findings: Vec<(&str, &str, &str)>) -> crate::autodraft::ScanFacts {
        crate::autodraft::ScanFacts {
            pack_id: "pack-1".into(),
            included_files: 21,
            languages: vec![],
            test_file_count: 1,
            source_file_count: 21,
            entry_points: vec![],
            findings: findings
                .into_iter()
                .map(|(a, b, c)| (a.into(), b.into(), c.into()))
                .collect(),
            plain: Default::default(),
        }
    }

    fn screen_for(facts: &crate::autodraft::RepoFacts) -> Vec<String> {
        let outcome = InitOutcome {
            created: TEMPLATES.iter().map(|(n, _)| n.to_string()).collect(),
            ..Default::default()
        };
        let report = DraftReport::default();
        render_screen(&Screen {
            backup: None,
            outcome: &outcome,
            git_ignored: false,
            facts,
            report: Some(&report),
            kept: &[],
            verbose: false,
        })
    }

    /// #39: in a git repo, one line says git ignores `.repolens/` and the
    /// user's files are untouched; outside git, nothing about git.
    #[test]
    fn screen_git_line_only_in_git_worktree() {
        let facts = crate::autodraft::RepoFacts::default();
        let outcome = InitOutcome {
            created: TEMPLATES.iter().map(|(n, _)| n.to_string()).collect(),
            ..Default::default()
        };
        let report = DraftReport::default();
        let render = |git_ignored| {
            render_screen(&Screen {
                backup: None,
                outcome: &outcome,
                git_ignored,
                facts: &facts,
                report: Some(&report),
                kept: &[],
                verbose: false,
            })
        };
        let git = render(true);
        assert_eq!(git[0], "RepoLens is set up: 6 files in .repolens/");
        assert_eq!(
            git[1],
            "Git ignores .repolens/ via its own .gitignore; your files are unchanged."
        );
        let plain = render(false);
        assert_eq!(plain[0], "RepoLens is set up: 6 files in .repolens/");
        assert_eq!(&plain[1..], &git[2..]);
        assert!(
            !plain.iter().any(|l| l.to_lowercase().contains("git")),
            "{plain:?}"
        );
    }

    #[test]
    fn screen_scan_without_findings() {
        let facts = crate::autodraft::RepoFacts {
            scan: Some(scan(vec![])),
            ..Default::default()
        };
        let l = screen_for(&facts);
        assert_eq!(l[2], "The last scan (pack-1) found nothing to flag.");
        assert_eq!(l[3], "", "no hint lines when there is nothing to see");
    }

    fn plain(flagged: &[&str], info: &[&str], coverage: &[&str]) -> crate::findings::PlainFindings {
        let rows = |v: &[&str]| v.iter().map(|r| vec![r.to_string()]).collect();
        crate::findings::PlainFindings {
            flagged: rows(flagged),
            info: rows(info),
            coverage: coverage.iter().map(|c| c.to_string()).collect(),
        }
    }

    /// #32: same headline and rows as `findings`, top 3 only, then the
    /// coverage note once below them; never in a top slot.
    #[test]
    fn screen_shows_top_three_then_coverage_note() {
        let mut s = scan(vec![]);
        s.plain = plain(
            &[
                "  High  2 groups of files import each other in a loop (largest: 5 files)",
                "  Med   Dominant module: 'core' holds 80% of LOC",
                "  Low   0 of 16 code files have tests (file count)",
                "  Low   No lockfile found for Python build system",
            ],
            &["  Info  Cargo.lock is present (whether git tracks it wasn't checked)"],
            &[
                "  Coverage: imports aren't read for 2 languages: Ruby, Shell (900 lines)",
                "            Python: imports read; cross-file calls and types not yet",
            ],
        );
        let facts = crate::autodraft::RepoFacts {
            scan: Some(s),
            ..Default::default()
        };
        let l = screen_for(&facts);
        assert_eq!(
            l[2],
            "The last scan (pack-1) found 4 things worth a look; top 3:"
        );
        assert_eq!(l[3], "", "blank line under the headline, as in findings");
        assert_eq!(
            l[4],
            "  High  2 groups of files import each other in a loop (largest: 5 files)"
        );
        assert_eq!(l[6], "  Low   0 of 16 code files have tests (file count)");
        assert_eq!(
            l[7],
            "  Coverage: imports aren't read for 2 languages: Ruby, Shell (900 lines)"
        );
        assert_eq!(
            l[8],
            "            Python: imports read; cross-file calls and types not yet"
        );
        assert!(!l.iter().any(|x| x.contains("No lockfile")), "max 3");
        assert!(!l.iter().any(|x| x.starts_with("  Info")), "no Info rows");
        for x in &l {
            assert!(crate::findings::cols(x) <= 80, "{x}");
        }
    }

    /// gm-crypto-web: only no-action findings is "nothing to flag" here
    /// and in `findings`, with a pointer to see them.
    #[test]
    fn screen_info_only_is_nothing_to_flag() {
        let mut s = scan(vec![]);
        s.plain = plain(&[], &["  Info  Cargo.lock is present"], &[]);
        let facts = crate::autodraft::RepoFacts {
            scan: Some(s),
            ..Default::default()
        };
        let l = screen_for(&facts);
        assert_eq!(l[2], "The last scan (pack-1) found nothing to flag.");
        assert!(l[4].starts_with("  Run `repolens findings`"), "{l:?}");
    }

    fn screen_with(facts: &crate::autodraft::RepoFacts, report: &DraftReport) -> Vec<String> {
        let outcome = InitOutcome::default();
        render_screen(&Screen {
            backup: None,
            outcome: &outcome,
            git_ignored: true,
            facts,
            report: Some(report),
            kept: &[],
            verbose: false,
        })
    }

    #[test]
    fn screen_has_hint_saying_what_findings_dont_prove() {
        let mut s = scan(vec![]);
        s.plain = plain(
            &["  Low   Some files may be unused (imports only): 2, e.g. a.rs"],
            &[],
            &[],
        );
        let facts = crate::autodraft::RepoFacts {
            scan: Some(s),
            ..Default::default()
        };
        let l = screen_for(&facts);
        assert!(l
            .iter()
            .any(|x| x.contains("Some files may be unused (imports only)")));
        assert!(l
            .iter()
            .any(|x| x.trim() == "Findings are heuristics; each one lists what it doesn't check."));
    }

    #[test]
    fn screen_zero_drafts_says_fill_in_and_names_missing_commands() {
        let facts = crate::autodraft::RepoFacts::default();
        let report = DraftReport {
            drafted: vec![
                (
                    "known-constraints.md".into(),
                    "no draft constraints yet".into(),
                ),
                ("repo-overview.md".into(), "languages".into()),
            ],
            constraints: 0,
            ..Default::default()
        };
        let l = screen_with(&facts, &report);
        assert!(!l.iter().any(|x| x.contains("D-NNN")), "{l:#?}");
        assert!(!l.iter().any(|x| x.contains("scan signals only")));
        assert_eq!(
            l.last().unwrap(),
            "Next: fill in .repolens/known-constraints.md."
        );
        assert!(l
            .iter()
            .any(|x| x == "  No build/test commands drafted: none found in CI or package.json."));
        assert!(l.iter().any(|x| x == "  Add them in repo-overview.md."));
    }

    /// #13: with workflows present, the screen counts every workflow read
    /// and never claims CI has no commands; it says none was recognised.
    #[test]
    fn screen_counts_all_workflows_and_never_says_none_found_in_ci() {
        let facts = crate::autodraft::RepoFacts {
            ci_workflows: vec![
                ".github/workflows/a.yml".into(),
                ".github/workflows/b.yml".into(),
            ],
            ..Default::default()
        };
        let report = DraftReport {
            drafted: vec![("repo-overview.md".into(), "languages".into())],
            constraints: 0,
            ..Default::default()
        };
        let l = screen_with(&facts, &report);
        assert!(
            l.iter().any(|x| x == "  Read from: 2 CI workflows"),
            "{l:#?}"
        );
        assert!(!l.iter().any(|x| x.contains("none found in CI")), "{l:#?}");
        assert!(
            l.iter().any(
                |x| x == "  No build/test commands drafted: none recognised in 2 CI workflows."
            ),
            "{l:#?}"
        );
    }

    #[test]
    fn screen_with_drafts_keeps_rename_hint() {
        let facts = crate::autodraft::RepoFacts::default();
        let report = DraftReport {
            drafted: vec![(
                "known-constraints.md".into(),
                "1 draft constraint, D-001".into(),
            )],
            constraints: 1,
            ..Default::default()
        };
        let l = screen_with(&facts, &report);
        assert!(l.iter().any(|x| x.contains("D-NNN to C-NNN")));
    }

    fn tools(v: &[(&str, usize)]) -> Vec<(String, usize)> {
        v.iter().map(|(t, n)| ((*t).to_string(), *n)).collect()
    }

    /// #45: the drafted-for-you rows count CI commands per tool.
    #[test]
    fn ci_commands_line_counts_each_tool_once() {
        assert_eq!(
            ci_commands_line(&tools(&[("cargo", 7), ("npm", 3), ("make", 2)])).unwrap(),
            "12 CI commands (cargo 7, npm 3, make 2)"
        );
        assert_eq!(
            ci_commands_line(&tools(&[("cargo", 1)])).unwrap(),
            "1 CI command (cargo 1)"
        );
        assert_eq!(ci_commands_line(&[]), None);
        let six = tools(&[
            ("cargo", 6),
            ("npm", 5),
            ("make", 4),
            ("tox", 3),
            ("go", 2),
            ("uv", 1),
        ]);
        assert_eq!(
            ci_commands_line(&six).unwrap(),
            "21 CI commands (cargo 6, npm 5, make 4, tox 3, \u{2026}and 2 more)"
        );
    }

    #[test]
    fn ci_commands_line_drops_tools_to_fit_80_columns() {
        let long = "./scripts/ci/a-very-long-name-for-the-nightly-ci-check-script.sh";
        let l = ci_commands_line(&tools(&[("cargo", 9), (long, 2), ("npm", 1)])).unwrap();
        assert_eq!(l, "12 CI commands (cargo 9, \u{2026}and 2 more)");
        let huge = "x".repeat(90);
        let l = ci_commands_line(&tools(&[(huge.as_str(), 2), ("npm", 1)])).unwrap();
        assert_eq!(l, "3 CI commands from 2 tools");
    }

    #[test]
    fn screen_puts_ci_counts_under_known_constraints() {
        let facts = crate::autodraft::RepoFacts::default();
        let report = DraftReport {
            drafted: vec![(
                "known-constraints.md".into(),
                "13 draft constraints, D-001 to D-013".into(),
            )],
            constraints: 13,
            ci_tools: tools(&[("cargo", 7), ("npm", 3), ("make", 2)]),
        };
        let l = screen_with(&facts, &report);
        let i = l
            .iter()
            .position(|x| x.starts_with("  known-constraints.md"))
            .unwrap();
        assert_eq!(l[i + 1], "    12 CI commands (cargo 7, npm 3, make 2)");
        for x in &l {
            assert!(crate::findings::cols(x) <= SCREEN_WIDTH, "too wide: {x}");
        }
    }

    #[test]
    fn every_screen_line_fits_80_columns() {
        let long = "x".repeat(200);
        let facts = crate::autodraft::RepoFacts {
            scan: Some(scan(vec![
                (
                    "F-SOMETHING-WITH-A-VERY-LONG-IDENTIFIER-NAME",
                    long.as_str(),
                    "High",
                ),
                ("F-ORPHAN-FILES", crate::autodraft::ORPHAN_LINE, "Low"),
                ("F-TEST-INVENTORY-RATIO", "x", "Low"),
            ])),
            ..Default::default()
        };
        let report = DraftReport {
            drafted: vec![(long.clone(), long.clone())],
            constraints: 0,
            ..Default::default()
        };
        let kept = vec![long.clone()];
        let outcome = InitOutcome::default();
        let l = render_screen(&Screen {
            backup: None,
            outcome: &outcome,
            git_ignored: false,
            facts: &facts,
            report: Some(&report),
            kept: &kept,
            verbose: true,
        });
        for x in &l {
            assert!(crate::findings::cols(x) <= SCREEN_WIDTH, "too wide: {x}");
        }
    }

    #[test]
    fn backup_folder_gets_suffix_on_collision() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("session-memory.md"), "mine\n").unwrap();
        let now = chrono::Local::now();
        let a = backup_edited(dir.path(), &now).unwrap().unwrap();
        let b = backup_edited(dir.path(), &now).unwrap().unwrap();
        let c = backup_edited(dir.path(), &now).unwrap().unwrap();
        assert_ne!(a.dir, b.dir);
        assert!(b.dir.ends_with("-2/"), "{}", b.dir);
        assert!(c.dir.ends_with("-3/"), "{}", c.dir);
        assert_eq!(a.files, vec!["session-memory.md".to_string()]);
    }

    #[test]
    fn backup_skips_untouched_files() {
        let dir = tempfile::tempdir().unwrap();
        scaffold_templates(dir.path(), false).unwrap();
        assert_eq!(
            backup_edited(dir.path(), &chrono::Local::now()).unwrap(),
            None
        );
        assert!(!dir.path().join("backup").exists());
    }
}
