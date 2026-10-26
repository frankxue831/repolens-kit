//! `repolens memory save` / `recall` / `list` / `delete`: CLI front end for
//! the typed memory graph (`.repolens/memory/memory.json`). Thin wrappers
//! over `repolens_app::memory_ops`, the same code the MCP tools
//! `memorize_insight`, `recall_longterm` and `forget_insight` use.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};
use repolens_app::discover::PackResolver;
use repolens_app::memory_ops::{self, ForgetOutcome, RecallRequest, SaveOutcome, SaveRequest};
use repolens_core::model::memory::GroundingStatus;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;

use crate::resolve_root_from;

#[derive(Subcommand, Debug)]
pub enum MemoryCommands {
    /// Save one memory (a decision, convention, pattern...) for later recall
    Save(SaveArgs),
    /// Find saved memories by keywords; shows score, decay and grounding
    Recall(RecallArgs),
    /// List every saved memory: type, saved importance, content and id
    #[command(alias = "ls")]
    List(ListArgs),
    /// Delete one memory by id (at least 4 characters); previews unless --yes
    // The name lives in `DELETE_COMMAND`; there are no hidden aliases.
    #[command(name = DELETE_COMMAND)]
    Forget(ForgetArgs),
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TypeArg {
    Pattern,
    Decision,
    Convention,
    TradeOff,
    CodeAnchor,
}

impl TypeArg {
    fn wire(self) -> &'static str {
        match self {
            TypeArg::Pattern => "PATTERN",
            TypeArg::Decision => "DECISION",
            TypeArg::Convention => "CONVENTION",
            TypeArg::TradeOff => "TRADE_OFF",
            TypeArg::CodeAnchor => "CODE_ANCHOR",
        }
    }
}

#[derive(Args, Debug)]
pub struct SaveArgs {
    /// What to remember, in one or two sentences
    pub content: String,
    /// Kind of memory
    #[arg(
        long = "type",
        value_enum,
        ignore_case = true,
        default_value = "PATTERN"
    )]
    pub memory_type: TypeArg,
    /// File (repo-relative) or scanned symbol (e.g. `Type.method`) this
    /// memory is about; repeatable. Recall checks it still exists in the
    /// latest scan.
    #[arg(long = "anchor")]
    pub anchors: Vec<String>,
    /// Starting importance, 0.0-1.0
    #[arg(long, default_value_t = 0.5)]
    pub importance: f32,
    /// Tag; repeatable
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct RecallArgs {
    /// Keywords to search for
    pub query: String,
    /// Only this kind of memory
    #[arg(long = "type", value_enum, ignore_case = true)]
    pub memory_type: Option<TypeArg>,
    /// Maximum results
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Name of the delete subcommand (`repolens memory forget`).
pub const DELETE_COMMAND: &str = "forget";

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ForgetArgs {
    /// Full id, or a unique prefix of 4+ characters (`save` prints 8)
    pub id: String,
    /// Delete it; without this, only show what would be deleted
    #[arg(long)]
    pub yes: bool,
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

pub fn run_memory(cmd: MemoryCommands) -> Result<()> {
    match cmd {
        MemoryCommands::Save(a) => run_save(a),
        MemoryCommands::Recall(a) => run_recall(a),
        MemoryCommands::List(a) => run_list(a),
        MemoryCommands::Forget(a) => run_forget(a),
    }
}

fn root_of(root: Option<PathBuf>) -> Result<PathBuf> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    resolve_root_from(root, &cwd)
}

fn run_save(a: SaveArgs) -> Result<()> {
    let root = root_of(a.root)?;
    let pack = PackResolver::new()
        .resolve(None, Some(&root.to_string_lossy()))
        .ok();
    let req = SaveRequest {
        content: a.content,
        memory_type: Some(a.memory_type.wire().to_string()),
        importance: Some(a.importance),
        anchors: a.anchors.clone(),
        tags: a.tags,
        supersedes: None,
        provenance: "cli:memory save".to_string(),
    };
    match memory_ops::save(&root, req).map_err(|m| anyhow!(m))? {
        SaveOutcome::Created { id } => {
            println!(
                "Saved {} memory {} (importance {:.2}).",
                a.memory_type.wire(),
                short_id(&id),
                a.importance
            );
            let width = output_width();
            for anchor in &a.anchors {
                println!("  Anchor: {anchor}");
                if let Some(w) = anchor_warning(&root, pack.as_ref(), anchor) {
                    for line in fit("  ", &format!("Warning: {w}"), width) {
                        println!("{line}");
                    }
                }
            }
            println!("  Stored in .repolens/memory/memory.json");
        }
        SaveOutcome::Duplicate { id } => {
            println!(
                "Already saved as memory {}; nothing written.",
                short_id(&id)
            );
        }
    }
    Ok(())
}

fn run_recall(a: RecallArgs) -> Result<()> {
    let root = root_of(a.root)?;
    let pack = PackResolver::new()
        .resolve(None, Some(&root.to_string_lossy()))
        .ok();
    let req = RecallRequest {
        query: a.query.clone(),
        memory_type: a.memory_type.map(|t| t.wire().to_string()),
        tags: None,
        min_importance: None,
        limit: Some(a.limit),
    };
    let out = memory_ops::recall(&root, pack, req).map_err(|m| anyhow!(m))?;
    let pack_name = out
        .pack_dir
        .as_deref()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().to_string());

    let width = output_width();
    let say = |indent: &str, text: &str| {
        for line in fit(indent, text, width) {
            println!("{line}");
        }
    };
    if out.hits.is_empty() {
        if memory_ops::search_terms(&a.query).is_empty() && !a.query.trim().is_empty() {
            say(
                "",
                &format!(
                    "No usable search terms in \"{}\": common words like \"the\", \"a\" \
                     and \"of\" are ignored. Add a word from the note.",
                    a.query
                ),
            );
        } else if out.removed > 0 {
            say("", &format!("No memory to show for \"{}\".", a.query));
            say("", &removed_line(out.removed, false));
        } else {
            say("", &format!("No saved memory matches \"{}\".", a.query));
        }
        return Ok(());
    }
    let shown = out.hits.len();
    if shown < out.total {
        say(
            "",
            &format!("{} of {} memories match \"{}\":", shown, out.total, a.query),
        );
    } else {
        let phrase = if shown == 1 {
            "memory matches"
        } else {
            "memories match"
        };
        say("", &format!("{shown} {phrase} \"{}\":", a.query));
    }
    for (i, h) in out.hits.iter().enumerate() {
        println!();
        say(
            "",
            &format!(
                "{}. {}  {}",
                i + 1,
                memory_ops::memory_type_name(&h.memory_type),
                score_terms(h)
            ),
        );
        say("   ", &h.content);
        let days = (out.now - h.last_accessed).as_seconds_f64() / 86_400.0;
        let floor = if h.grounding.grounding_status == GroundingStatus::Confirmed {
            " (a confirmed anchor keeps a minimum)"
        } else {
            ""
        };
        say(
            "   ",
            &format!(
                "Decay: last used {} ago; fades while unused{floor}",
                age(days.max(0.0))
            ),
        );
        say(
            "   ",
            &format!("Grounding: {}", grounding_line(h, pack_name.as_deref())),
        );
        say("   ", &format!("id: {}", h.id));
    }
    if out.removed > 0 {
        println!();
        say("", &removed_line(out.removed, true));
    }
    Ok(())
}

/// Every saved memory, oldest first (#54), with its importance as last
/// saved. Read-only: nothing is locked or written, and no decay is applied.
fn run_list(a: ListArgs) -> Result<()> {
    let root = root_of(a.root)?;
    let nodes = memory_ops::list(&root).map_err(|m| anyhow!(m))?;
    let width = output_width();
    let say = |indent: &str, text: &str| {
        for line in fit(indent, text, width) {
            println!("{line}");
        }
    };
    if nodes.is_empty() {
        say(
            "",
            "No saved memories yet. Save one with `repolens memory save`.",
        );
        return Ok(());
    }
    say(
        "",
        &format!(
            "{} saved {}, oldest first:",
            nodes.len(),
            memories(nodes.len())
        ),
    );
    for (i, n) in nodes.iter().enumerate() {
        println!();
        say(
            "",
            &format!(
                "{}. {}  saved importance {:.2}",
                i + 1,
                memory_ops::memory_type_name(&n.memory_type),
                n.importance
            ),
        );
        say("   ", &n.content);
        say("   ", &format!("id: {}", n.id));
    }
    println!();
    say(
        "",
        "Importance is the value when last saved; recall applies fading.",
    );
    Ok(())
}

/// "memory" or "memories".
fn memories(n: usize) -> &'static str {
    if n == 1 {
        "memory"
    } else {
        "memories"
    }
}

/// `  <full id>  TYPE` and `  <first line of the text>`, each cut to the
/// output width (80 at most).
fn memory_lines(n: &repolens_core::model::memory::MemoryNode, width: usize) -> [String; 2] {
    let width = width.min(80);
    let first = n.content.lines().next().unwrap_or("").trim_end();
    [
        cut(
            &format!(
                "  {}  {}",
                n.id,
                memory_ops::memory_type_name(&n.memory_type)
            ),
            width,
        ),
        cut(&format!("  {}", expand_tabs(first)), width),
    ]
}

/// Display columns of `s`, counting `…` as 2 as some CJK terminals draw it.
fn cols_wide(s: &str) -> usize {
    cols(s) + s.matches('…').count()
}

/// `s` cut to `width` columns (see [`cols_wide`]), ending in `…` when cut.
fn cut(s: &str, width: usize) -> String {
    if cols_wide(s) <= width {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        let mut next = out.clone();
        next.push(c);
        if cols_wide(&next) + 2 > width {
            break;
        }
        out = next;
    }
    out.push('…');
    out
}

/// Forget (delete) one memory by id or unique prefix of at least 4
/// characters (#54, #81), like MCP `forget_insight`: a preview unless
/// `--yes`. An unknown, short or ambiguous id writes nothing.
fn run_forget(a: ForgetArgs) -> Result<()> {
    let root = root_of(a.root)?;
    let id = a.id.trim();
    if id.is_empty() {
        return Err(anyhow!("id must not be empty"));
    }
    let width = output_width();
    match memory_ops::forget(&root, id, a.yes).map_err(|m| anyhow!(m))? {
        ForgetOutcome::NotFound => {
            let msg = format!("No memory with id {id}. Nothing was deleted.");
            Err(anyhow!(fit("", &msg, width).join("\n")))
        }
        ForgetOutcome::Refused { error, matches } => {
            let mut lines = fit("", &error.message(), width);
            for n in &matches {
                let [head, text] = memory_lines(n, width);
                let first = text.trim_start();
                lines.push(cut(&format!("{head}  {first}"), width.min(80)));
            }
            if let memory_ops::IdLookupError::Ambiguous(amb) = &error {
                let (_, more) = amb.listed();
                if more > 0 {
                    lines.push(format!("  …and {more} more"));
                }
            }
            Err(anyhow!(lines.join("\n")))
        }
        ForgetOutcome::Preview(n) => {
            println!("Would delete 1 memory:");
            for line in memory_lines(&n, width) {
                println!("{line}");
            }
            println!("Run again with --yes to delete it.");
            Ok(())
        }
        ForgetOutcome::Deleted(n) => {
            println!("Deleted 1 memory:");
            for line in memory_lines(&n, width) {
                println!("{line}");
            }
            Ok(())
        }
    }
}

/// `score S = importance I [x grounding G] x keywords K%` (#14). The
/// importance is the value before the grounding penalty, so it agrees
/// with the Decay line; the grounding term is the factor the penalty
/// actually left, shown only when it lowers the score.
fn score_terms(h: &memory_ops::RecallHit) -> String {
    let grounding = h
        .grounding_factor
        .map(|g| format!(" x grounding {g:.2}"))
        .unwrap_or_default();
    format!(
        "score {:.2} = importance {:.2}{grounding} x keywords {:.0}%",
        h.score,
        h.importance_before_grounding,
        h.keyword_score * 100.0
    )
}

/// The line for matches this recall's decay run deleted (#14). Never says
/// "hidden": they're gone, so this shows only on the run that removed them.
fn removed_line(n: usize, after_hits: bool) -> String {
    let more = if after_hits { " more" } else { "" };
    if n == 1 {
        format!("1{more} match was removed: it went unused and faded below the recall cutoff.")
    } else {
        format!(
            "{n}{more} matches were removed: they went unused and faded below the recall cutoff."
        )
    }
}

/// Columns recall output may use: the terminal width when stdout is a
/// terminal (`COLUMNS`, else `stty size`), 80 otherwise (#35). Never below
/// 20, so the indent always leaves room for text.
fn output_width() -> usize {
    const FALLBACK: usize = 80;
    if !std::io::stdout().is_terminal() {
        return FALLBACK;
    }
    let from_env = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse::<usize>().ok())
        .filter(|&c| c > 0);
    from_env.or_else(stty_columns).unwrap_or(FALLBACK).max(20)
}

#[cfg(unix)]
fn stty_columns() -> Option<usize> {
    let tty = std::fs::File::open("/dev/tty").ok()?;
    let out = std::process::Command::new("stty")
        .arg("size")
        .stdin(tty)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    text.split_whitespace()
        .nth(1)?
        .parse::<usize>()
        .ok()
        .filter(|&c| c > 0)
}

#[cfg(not(unix))]
fn stty_columns() -> Option<usize> {
    None
}

/// Wrap `text` into lines of at most `width` display columns, each starting
/// with `indent`. Breaks at spaces and keeps the spacing inside a line (the
/// two spaces in "DECISION  score"); a word wider than the room left (a long
/// path, an unbroken CJK run) is split by character. Wide (CJK) characters
/// count 2 columns.
///
/// Tabs are expanded to spaces (stops every 8 columns) and other control
/// characters become spaces before measuring, so a note saved with
/// `printf 'a\tb'` can't run past `width`; a newline in the text starts a
/// new line.
fn fit(indent: &str, text: &str, width: usize) -> Vec<String> {
    text.split('\n')
        .flat_map(|para| fit_line(indent, &expand_tabs(para), width))
        .collect()
}

/// `s` with each tab expanded to the next multiple of 8 columns and other
/// control characters (`\r`, escapes...) replaced by a space, so every
/// character left has the display width `cols` gives it.
fn expand_tabs(s: &str) -> String {
    const TAB: usize = 8;
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for c in s.chars() {
        if c == '\t' {
            let n = TAB - col % TAB;
            out.push_str(&" ".repeat(n));
            col += n;
        } else if c.is_control() {
            out.push(' ');
            col += 1;
        } else {
            out.push(c);
            col += c.width().unwrap_or(0);
        }
    }
    out
}

/// [`fit`] for one line of text without tabs or control characters.
fn fit_line(indent: &str, text: &str, width: usize) -> Vec<String> {
    let room = width.saturating_sub(cols(indent)).max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    // Extra spaces seen since the last word (`split(' ')` yields one empty
    // piece per space beyond the first), kept only if the next word stays
    // on this line.
    let mut gap = 0usize;
    for word in text.split(' ') {
        if word.is_empty() {
            gap += 1;
            continue;
        }
        let sep = if cur.is_empty() { 0 } else { gap + 1 };
        gap = 0;
        if cols(&cur) + sep + cols(word) <= room {
            cur.push_str(&" ".repeat(sep));
            cur.push_str(word);
            continue;
        }
        if !cur.is_empty() && cols(word) <= room {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
            continue;
        }
        // Too wide for any line: fill the current line, then split.
        if !cur.is_empty() {
            if cols(&cur) + sep < room {
                cur.push_str(&" ".repeat(sep));
            } else {
                lines.push(std::mem::take(&mut cur));
            }
        }
        for c in word.chars() {
            let w = c.width().unwrap_or(0);
            if !cur.is_empty() && cols(&cur) + w > room {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines.into_iter().map(|l| format!("{indent}{l}")).collect()
}

/// Display columns of `s`; wide (CJK) characters count 2.
fn cols(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// Warn when an anchor names neither a file on disk nor a symbol in the
/// latest scan, or names a file the latest scan doesn't include. A path-like
/// anchor (one with a `/`) is only ever a file; any other anchor that isn't
/// a file may be a symbol's qualified name (e.g. `Widget.run`) or bare name.
/// The memory is still saved; grounding will report it at recall time.
fn anchor_warning(
    root: &Path,
    pack: Option<&(PathBuf, repolens_core::pack_reader::PackIndex)>,
    anchor: &str,
) -> Option<String> {
    let on_disk = root.join(anchor).is_file();
    if on_disk {
        let in_scan = pack.map(|(_, idx)| idx.entries.iter().any(|e| e.path == anchor));
        return (in_scan == Some(false)).then(|| {
            format!("{anchor} is not in the latest scan; run `repolens scan` to include it")
        });
    }
    if anchor.contains('/') {
        return Some(format!("{anchor} is not a file in this repo"));
    }
    let Some((_, idx)) = pack else {
        return Some(format!(
            "{anchor} is not a file in this repo (no scan to check symbols)"
        ));
    };
    let is_symbol = idx
        .entries
        .iter()
        .flat_map(|e| &e.symbols)
        .any(|s| s.qualified_name.as_deref() == Some(anchor) || s.name == anchor);
    (!is_symbol)
        .then(|| format!("{anchor} is not a file in this repo or a symbol in the latest scan"))
}

fn grounding_line(h: &memory_ops::RecallHit, pack: Option<&str>) -> String {
    if h.anchors.is_empty() {
        return "none (no file anchors)".to_string();
    }
    let status = memory_ops::grounding_name(&h.grounding.grounding_status);
    let Some(pack) = pack else {
        return format!("{status} (no scan yet; run `repolens scan`)");
    };
    let changed = if h.changed_anchors.is_empty() {
        String::new()
    } else {
        format!(
            "; {} changed since this note was saved",
            h.changed_anchors.join(", ")
        )
    };
    match h.grounding.grounding_status {
        GroundingStatus::Confirmed => format!(
            "{status} ({} found in {pack}{changed})",
            h.grounding.found_anchors.join(", ")
        ),
        GroundingStatus::Contradicted => format!(
            "{status} ({} missing from {pack}{changed})",
            h.grounding.missing_anchors.join(", ")
        ),
        _ => format!("{status} (anchors can't be checked against {pack}{changed})"),
    }
}

fn age(days: f64) -> String {
    if days < 1.0 / 24.0 {
        "less than an hour".to_string()
    } else if days < 1.0 {
        format!("{:.0} hours", days * 24.0)
    } else {
        format!("{days:.1} days")
    }
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_every_line_within_the_width() {
        let long = "Release builds must run the full integration suite on both Linux and \
                    macOS runners before tagging, because the macOS file watcher behaves \
                    differently under load.";
        let cjk = "图片检查脚本放在 scripts 目录下，所有项目图片统一存放在 assets/img 目录中，按项目名分子目录。";
        let path = format!("Grounding: CONFIRMED ({} found in pack)", "a/".repeat(60));
        for width in [20, 40, 80, 120] {
            for text in [long, cjk, path.as_str(), "short"] {
                let lines = fit("   ", text, width);
                for l in &lines {
                    assert!(cols(l) <= width, "{width}: {l:?} is {} cols", cols(l));
                    assert!(l.starts_with("   "), "{l:?}");
                }
                // Nothing lost but the spaces it broke at.
                let joined: String = lines.concat().chars().filter(|c| *c != ' ').collect();
                let want: String = text.chars().filter(|c| *c != ' ').collect();
                assert_eq!(joined, want);
            }
        }
        assert_eq!(fit("", "short", 80), vec!["short".to_string()]);
        // Spacing inside a line is kept; a break eats the spaces.
        assert_eq!(
            fit("", "1. DECISION  score 0.50", 80),
            vec!["1. DECISION  score 0.50"]
        );
        assert_eq!(fit("", "aaaa  bbbb", 6), vec!["aaaa", "bbbb"]);
    }

    #[test]
    fn fit_expands_tabs_and_control_characters_before_measuring() {
        // QA's repro: a note saved via `printf 'release\tnotes...'`.
        let tabbed = format!("release\tnotes{}", "\tcolumn\tvalue".repeat(12));
        for width in [20, 40, 80] {
            for line in fit("   ", &tabbed, width) {
                assert!(!line.contains('\t'), "{line:?}");
                assert!(cols(&line) <= width, "{width}: {line:?}");
            }
        }
        assert_eq!(expand_tabs("ab\tc"), "ab      c");
        assert_eq!(expand_tabs("\t"), " ".repeat(8));
        assert_eq!(expand_tabs("a\rb\u{1b}c"), "a b c");
        // A newline in a note starts a new indented line.
        assert_eq!(fit("   ", "one\ntwo", 80), vec!["   one", "   two"]);
        assert_eq!(fit("   ", "", 80), vec!["   ".to_string()]);
    }

    #[test]
    fn width_is_80_when_stdout_is_not_a_terminal() {
        // `cargo test` captures stdout, so this is the not-a-TTY path.
        if !std::io::stdout().is_terminal() {
            assert_eq!(output_width(), 80);
        }
    }

    #[test]
    fn removed_line_picks_singular_or_plural() {
        let cases = [
            (
                1,
                false,
                "1 match was removed: it went unused and faded below the recall cutoff.",
            ),
            (
                3,
                false,
                "3 matches were removed: they went unused and faded below the recall cutoff.",
            ),
            (
                1,
                true,
                "1 more match was removed: it went unused and faded below the recall cutoff.",
            ),
            (
                4,
                true,
                "4 more matches were removed: they went unused and faded below the recall cutoff.",
            ),
        ];
        for (n, after_hits, want) in cases {
            let got = removed_line(n, after_hits);
            assert_eq!(got, want);
            assert!(!got.contains("(es)"), "{got}");
            assert!(!got.contains("hidden"), "{got}");
        }
    }
}
