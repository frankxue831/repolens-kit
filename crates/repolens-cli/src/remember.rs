//! `repolens remember --file <session-summary.md>` - import an agent handoff
//! into `.repolens/session-memory.md` as workspace markdown only.

use anyhow::{anyhow, bail, Result};
use clap::Args;
use repolens_core::atomic_write::write_atomic;
use repolens_core::storage::RepoStoragePaths;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::{brief::is_template_only_workspace_body, resolve_root_from};

const SESSION_MEMORY_FILENAME: &str = "session-memory.md";

#[derive(Args, Debug)]
pub struct RememberArgs {
    /// Session summary markdown file to import
    #[arg(long)]
    pub file: PathBuf,
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

pub fn run_remember(args: RememberArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;

    let storage_dir = RepoStoragePaths::storage_dir(&root);
    if !storage_dir.is_dir() {
        bail!("RepoLens workspace not found. Run 'repolens init' first.");
    }

    let raw_summary = read_summary_file(&args.file)?;
    let summary = raw_summary.trim();
    if summary.is_empty() {
        bail!("Session summary file is empty: {}", args.file.display());
    }

    let source = source_label(&args.file);
    let entry = build_imported_entry(&source, summary);
    let target = storage_dir.join(SESSION_MEMORY_FILENAME);
    let existing = read_existing_session_memory(&target)?;
    let body = merge_session_memory(existing.as_deref(), &entry);

    write_atomic(&target, body.as_bytes()).map_err(|e| {
        anyhow!(
            "Failed to write session memory to {}: {}",
            target.display(),
            e
        )
    })?;

    println!("Session summary recorded in .repolens/session-memory.md");
    println!(
        "This recorded workspace markdown only; .repolens/memory/memory.json was not modified."
    );
    println!("Use MCP memorize_insight for anchored typed long-term memory.");
    println!(
        "Next: run repolens brief --task \"<next task>\" to include this handoff in the next preflight brief."
    );
    Ok(())
}

fn read_summary_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => anyhow!("Session summary file not found: {}", path.display()),
        _ => anyhow!(
            "Failed to read session summary file {}: {}",
            path.display(),
            e
        ),
    })
}

fn read_existing_session_memory(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(body) => Ok(Some(body)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!(
            "Failed to read existing session memory {}: {}",
            path.display(),
            e
        )),
    }
}

fn source_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn build_imported_entry(source: &str, summary: &str) -> String {
    let demoted = demote_markdown_headings(summary.trim());
    format!(
        "## Session Summary: Imported from {source}\n\n\
         **Source:** {source}  \n\
         **Status:** agent-provided, unverified  \n\
         **Use as:** hypothesis until confirmed by files, tests, docs, or human review.\n\n\
         ### Imported Content\n\n\
         {demoted}\n",
    )
}

/// Demote ATX-style markdown headings in `body` by one level so that imported
/// session-summary content nests cleanly under the wrapper headings emitted by
/// [`build_imported_entry`] (`## Session Summary: Imported from …` and
/// `### Imported Content`). Without this pass, a session summary that opens
/// with `# Session Summary — YYYY-MM-DD` would contribute a second `<h1>` to
/// `.repolens/session-memory.md` and to `repolens brief` output, visually
/// outranking the brief's own title.
///
/// Rules:
/// - `# X` → `## X`, `## X` → `### X`, …, `##### X` → `###### X`.
/// - `###### X` (h6) is left unchanged — demoting to `#######` would stop
///   rendering as a heading at all, which is a worse outcome than the
///   collision we are trying to avoid.
/// - Lines inside fenced code blocks (` ``` ` or `~~~`, optionally indented up
///   to 3 spaces, 3+ marker characters) are preserved verbatim — a `# foo`
///   inside a shell-script example must stay a comment.
/// - Non-heading content is preserved byte-for-byte, including the original
///   line endings (`\n` or `\r\n`).
/// - Setext headings (underlined with `===` / `---`) are intentionally not
///   handled; they are rare in agent-authored summaries and supporting them
///   would require lookahead. Out of scope for this hotfix.
fn demote_markdown_headings(body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 16);
    let mut fence: Option<(char, usize)> = None;

    for line in body.split_inclusive('\n') {
        let trimmed_end = line.trim_end_matches(['\n', '\r']);
        let marker = parse_fence_marker(trimmed_end);

        match (fence, marker) {
            (Some((open_char, open_count)), Some((c, n))) if c == open_char && n >= open_count => {
                fence = None;
                out.push_str(line);
            }
            (Some(_), _) => out.push_str(line),
            (None, Some((c, n))) => {
                fence = Some((c, n));
                out.push_str(line);
            }
            (None, None) => match demote_heading_line(line) {
                Some(demoted) => out.push_str(&demoted),
                None => out.push_str(line),
            },
        }
    }

    out
}

/// Returns `(marker_char, marker_length)` if `line` is a CommonMark-style
/// fenced code block opener or closer: 3+ backticks or 3+ tildes, optionally
/// preceded by 0–3 spaces. Returns `None` otherwise.
pub(crate) fn parse_fence_marker(line: &str) -> Option<(char, usize)> {
    let leading = line.len() - line.trim_start_matches(' ').len();
    if leading > 3 {
        return None;
    }
    let rest = &line[leading..];
    let first = rest.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = rest.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    Some((first, count))
}

/// If `line` is an ATX heading (0–3 leading spaces, 1–6 `#`, then a space, tab,
/// or end-of-line), return it with one extra `#` prepended to the hash run.
/// Returns `None` for non-headings and for h6 (which is intentionally clamped
/// to preserve heading semantics).
fn demote_heading_line(line: &str) -> Option<String> {
    let body = line.trim_end_matches(['\n', '\r']);
    let leading = body.len() - body.trim_start_matches(' ').len();
    if leading > 3 {
        return None;
    }
    let after_indent = &body[leading..];
    let hash_count = after_indent.bytes().take_while(|&b| b == b'#').count();
    if hash_count == 0 || hash_count > 6 {
        return None;
    }
    let after_hashes = &after_indent[hash_count..];
    if !after_hashes.is_empty() && !after_hashes.starts_with([' ', '\t']) {
        return None;
    }
    let new_count = hash_count + 1;
    if new_count > 6 {
        return None;
    }
    let mut s = String::with_capacity(line.len() + 1);
    s.push_str(&body[..leading]);
    for _ in 0..new_count {
        s.push('#');
    }
    s.push_str(after_hashes);
    s.push_str(&line[body.len()..]);
    Some(s)
}

fn merge_session_memory(existing: Option<&str>, entry: &str) -> String {
    let entry = entry.trim_end();
    let Some(existing) = existing else {
        return format!("{entry}\n");
    };

    if existing.trim().is_empty() || is_template_only_workspace_body(existing) {
        return format!("{entry}\n");
    }

    format!(
        "{entry}\n\n---\n\n{}",
        ensure_trailing_newline(existing.trim_start_matches(['\n', '\r']))
    )
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_imported_entry_is_deterministic() {
        let first = build_imported_entry("session-summary.md", "Body\n");
        let second = build_imported_entry("session-summary.md", "Body\n");
        assert_eq!(first, second);
    }

    #[test]
    fn build_imported_entry_marks_source_and_status() {
        let entry = build_imported_entry("session-summary.md", "Body");
        assert!(entry.contains("**Source:** session-summary.md"));
        assert!(entry.contains("agent-provided, unverified"));
        assert!(entry.contains("hypothesis until confirmed"));
    }

    #[test]
    fn merge_session_memory_replaces_template_only_body() {
        let template = "<!-- header -->\n# Session Memory\n\n_<a>_\n_<b>_\n_<c>_\n";
        let entry = build_imported_entry("summary.md", "Real handoff");
        let merged = merge_session_memory(Some(template), &entry);
        assert!(merged.contains("Real handoff"));
        assert!(!merged.contains("_<a>_"));
    }

    #[test]
    fn merge_session_memory_prepends_filled_body() {
        let entry = build_imported_entry("summary.md", "New handoff");
        let merged = merge_session_memory(Some("## Existing\n\nOld handoff\n"), &entry);
        assert!(merged.starts_with("## Session Summary: Imported from summary.md"));
        assert!(merged.contains("## Existing"));
        assert!(merged.find("New handoff").unwrap() < merged.find("Old handoff").unwrap());
    }

    // -- demote_markdown_headings --------------------------------------------

    #[test]
    fn demote_headings_h1_becomes_h2() {
        let body = "# Session Summary\n\nbody text\n";
        assert_eq!(
            demote_markdown_headings(body),
            "## Session Summary\n\nbody text\n"
        );
    }

    #[test]
    fn demote_headings_demotes_each_nested_level() {
        let body = "# A\n## B\n### C\n#### D\n##### E\n";
        let expected = "## A\n### B\n#### C\n##### D\n###### E\n";
        assert_eq!(demote_markdown_headings(body), expected);
    }

    #[test]
    fn demote_headings_clamps_h6() {
        // h6 has nowhere to go; demoting to seven `#` would stop rendering as a
        // heading entirely. Leave it alone.
        let body = "###### Bottom\n";
        assert_eq!(demote_markdown_headings(body), "###### Bottom\n");
    }

    #[test]
    fn demote_headings_skips_atx_inside_backtick_fence() {
        let body = "# Real heading\n\
                    ```sh\n\
                    # comment, not a heading\n\
                    echo hi\n\
                    ```\n\
                    ## After\n";
        let expected = "## Real heading\n\
                        ```sh\n\
                        # comment, not a heading\n\
                        echo hi\n\
                        ```\n\
                        ### After\n";
        assert_eq!(demote_markdown_headings(body), expected);
    }

    #[test]
    fn demote_headings_skips_atx_inside_tilde_fence() {
        let body = "~~~\n# still a comment\n~~~\n# real heading\n";
        let expected = "~~~\n# still a comment\n~~~\n## real heading\n";
        assert_eq!(demote_markdown_headings(body), expected);
    }

    #[test]
    fn demote_headings_handles_long_fence_with_inner_short_fence() {
        // CommonMark: closing fence must be at least as long as opener. A 3-tick
        // line inside a 4-tick fence must NOT close it, and `# foo` between the
        // ticks must stay a comment.
        let body = "````\n```\n# inside fence\n```\n````\n# real heading\n";
        let expected = "````\n```\n# inside fence\n```\n````\n## real heading\n";
        assert_eq!(demote_markdown_headings(body), expected);
    }

    #[test]
    fn demote_headings_preserves_non_heading_content_byte_for_byte() {
        let body = "Plain line\n  - bullet\n#hashtag (no space)\n  ## indented heading\n";
        // `#hashtag` has no space → not a heading. Leading-space cases up to 3
        // spaces are still headings; 4+ spaces would be a code block. Two
        // spaces is fine and should demote.
        let expected = "Plain line\n  - bullet\n#hashtag (no space)\n  ### indented heading\n";
        assert_eq!(demote_markdown_headings(body), expected);
    }

    #[test]
    fn demote_headings_preserves_crlf_line_endings() {
        let body = "# A\r\nbody\r\n";
        assert_eq!(demote_markdown_headings(body), "## A\r\nbody\r\n");
    }

    #[test]
    fn demote_headings_handles_lone_hash_with_no_text() {
        // Edge case: a bare `#` is a valid empty h1 in CommonMark.
        let body = "#\n";
        assert_eq!(demote_markdown_headings(body), "##\n");
    }

    #[test]
    fn demote_headings_no_change_for_plain_paragraph() {
        let body = "no headings here\nstill nothing\n";
        assert_eq!(demote_markdown_headings(body), body);
    }

    // -- build_imported_entry × demotion -------------------------------------

    #[test]
    fn build_imported_entry_demotes_summary_h1() {
        let entry = build_imported_entry("session-summary.md", "# Session Summary\n\nbody\n");
        // Wrapper headings are unchanged…
        assert!(entry.contains("## Session Summary: Imported from session-summary.md"));
        assert!(entry.contains("### Imported Content"));
        // …and the imported h1 has been demoted to h2.
        assert!(entry.contains("## Session Summary\n\nbody"));
        assert!(
            !entry.contains("\n# Session Summary\n"),
            "imported h1 leaked into entry: {entry}"
        );
    }
}
