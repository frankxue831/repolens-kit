//! Task matching for `repolens brief --task` (#34).
//!
//! The task's words (recall's [`content_terms`]: the same stop words and
//! plural handling) are matched by name against the latest scan (file paths
//! and symbol names) and against saved memory (note text and anchors). Files
//! and notes are ranked by how many task words they match, then by path or
//! id, so the result is deterministic. Nothing here reads file contents.

use repolens_app::memory_ops::{content_terms, memory_type_name, stem, tokenize};
use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::memory::MemoryNode;
use std::collections::BTreeSet;
use std::fmt::Write;

/// Display caps for the brief section. Not tuning values: every match is
/// still counted in the "…and N more" line and the terminal summary.
const MAX_FILES_SHOWN: usize = 8;
const MAX_NOTES_SHOWN: usize = 5;
/// Widest line the new section and terminal lines may use.
const WIDTH: usize = 80;

pub(crate) const NO_MATCH_LINE: &str =
    "_No file or saved note matched the task's words; this is the general brief._";
pub(crate) const STOP_WORDS_ONLY_LINE: &str =
    "_The task has only common words like \"the\"; this is the general brief._";
pub(crate) const NO_MATCH_SUMMARY: &str =
    "No file or saved note matched the task; the brief is the general one.";
pub(crate) const SECTION_HEADING: &str = "## Files and Notes for This Task";
const SECTION_INTRO: &str = "Matched by name against the latest scan and saved memory. These are\n\
pointers to read first, not an analysis of the code.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileMatch {
    pub path: String,
    /// Task words matched, in task order.
    pub words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoteMatch {
    pub type_name: &'static str,
    pub id: String,
    pub content: String,
    pub words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum TaskMatch {
    /// No `--task` given: the brief is unchanged.
    #[default]
    NoTask,
    /// The task has only stop words.
    OnlyStopWords,
    /// Task words exist, but no file or note matched them.
    NoMatch,
    /// At least one file or note matched.
    Matched {
        files: Vec<FileMatch>,
        notes: Vec<NoteMatch>,
    },
}

/// Task words as `(stemmed term, word as the user typed it, lowercased)`,
/// stop words dropped, deduplicated by term, in task order.
fn task_terms(task: &str) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for word in tokenize(task) {
        let Some(term) = content_terms(&word).into_iter().next() else {
            continue;
        };
        if seen.insert(term.clone()) {
            out.push((term, word));
        }
    }
    out
}

/// Split an identifier-ish string into stemmed name tokens: on every
/// non-alphanumeric character and at camelCase boundaries
/// (`showSM4` -> `show`, `sm4`; `HTTPServer` -> `http`, `server`). The
/// unsplit piece is kept too, so `exSM4` also yields `exsm4`.
fn name_tokens(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for piece in s.split(|c: char| !c.is_alphanumeric()) {
        if piece.is_empty() {
            continue;
        }
        out.insert(stem(&piece.to_lowercase()));
        for part in camel_parts(piece) {
            out.insert(stem(&part.to_lowercase()));
        }
    }
    out
}

fn camel_parts(piece: &str) -> Vec<String> {
    let chars: Vec<char> = piece.chars().collect();
    let mut parts = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                parts.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

fn matched_words(terms: &[(String, String)], tokens: &BTreeSet<String>) -> Vec<String> {
    terms
        .iter()
        .filter(|(t, _)| tokens.contains(t))
        .map(|(_, w)| w.clone())
        .collect()
}

/// Match `task` against the scan's included `entries` and the saved memory
/// `notes`. `task == None` gives [`TaskMatch::NoTask`].
pub(crate) fn match_task(
    task: Option<&str>,
    entries: &[FileEntry],
    notes: &[MemoryNode],
) -> TaskMatch {
    let Some(task) = task else {
        return TaskMatch::NoTask;
    };
    let terms = task_terms(task);
    if terms.is_empty() {
        return TaskMatch::OnlyStopWords;
    }

    let mut files: Vec<FileMatch> = entries
        .iter()
        .filter(|e| e.included)
        .filter_map(|e| {
            let mut tokens = name_tokens(&e.path);
            for sym in &e.symbols {
                tokens.extend(name_tokens(&sym.name));
            }
            let words = matched_words(&terms, &tokens);
            (!words.is_empty()).then(|| FileMatch {
                path: e.path.clone(),
                words,
            })
        })
        .collect();
    files.sort_by(|a, b| {
        b.words
            .len()
            .cmp(&a.words.len())
            .then_with(|| a.path.cmp(&b.path))
    });
    files.dedup_by(|a, b| a.path == b.path);

    let mut matched_notes: Vec<NoteMatch> = notes
        .iter()
        .filter_map(|n| {
            let mut tokens: BTreeSet<String> =
                tokenize(&n.content).iter().map(|t| stem(t)).collect();
            for a in &n.anchors {
                tokens.extend(tokenize(a).iter().map(|t| stem(t)));
            }
            let words = matched_words(&terms, &tokens);
            (!words.is_empty()).then(|| NoteMatch {
                type_name: memory_type_name(&n.memory_type),
                id: n.id.clone(),
                content: n.content.clone(),
                words,
            })
        })
        .collect();
    matched_notes.sort_by(|a, b| {
        b.words
            .len()
            .cmp(&a.words.len())
            .then_with(|| a.id.cmp(&b.id))
    });

    if files.is_empty() && matched_notes.is_empty() {
        TaskMatch::NoMatch
    } else {
        TaskMatch::Matched {
            files,
            notes: matched_notes,
        }
    }
}

/// Display columns: wide (CJK) characters and `…` count 2, tabs expand to
/// 8-column stops.
pub(crate) fn cols(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    let mut col = 0usize;
    for c in s.chars() {
        col += match c {
            '\t' => 8 - col % 8,
            '…' => 2,
            c => c.width().unwrap_or(0),
        };
    }
    col
}

/// The line placed under `## Task` when nothing matched, if any.
pub(crate) fn task_note_line(m: &TaskMatch) -> Option<&'static str> {
    match m {
        TaskMatch::OnlyStopWords => Some(STOP_WORDS_ONLY_LINE),
        TaskMatch::NoMatch => Some(NO_MATCH_LINE),
        TaskMatch::NoTask | TaskMatch::Matched { .. } => None,
    }
}

fn more_line(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("- …and 1 more {one} matches")
    } else {
        format!("- …and {n} more {many} match")
    }
}

/// `items` joined with ", ", wrapped into lines of at most [`WIDTH`]
/// columns, each starting with `indent` (the first with `first`).
fn wrap_list(first: &str, indent: &str, items: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = first.to_string();
    let mut empty = first.trim().is_empty();
    for (i, item) in items.iter().enumerate() {
        let piece = if i + 1 < items.len() {
            format!("{item},")
        } else {
            item.clone()
        };
        let sep = if empty { "" } else { " " };
        if !empty && cols(&cur) + sep.len() + cols(&piece) > WIDTH {
            lines.push(std::mem::take(&mut cur));
            cur = format!("{indent}{piece}");
        } else {
            cur.push_str(sep);
            cur.push_str(&piece);
        }
        empty = false;
    }
    lines.push(cur);
    lines
}

fn render_file_line(out: &mut String, f: &FileMatch) {
    let head = format!("- `{}`:", f.path);
    let one = format!("{head} matches {}", f.words.join(", "));
    if cols(&one) <= WIDTH {
        let _ = writeln!(out, "{one}");
        return;
    }
    // Never cut a path: the matched words move to the next line.
    let _ = writeln!(out, "{head}");
    for l in wrap_list("  matches", "  ", &f.words) {
        let _ = writeln!(out, "{l}");
    }
}

fn render_note_line(out: &mut String, n: &NoteMatch) {
    let short_id: String = n.id.chars().take(8).collect();
    let prefix = format!("- {} {}: ", n.type_name, short_id);
    let suffix = format!(" ({})", n.words.join(", "));
    let text = n.content.split_whitespace().collect::<Vec<_>>().join(" ");
    let room = WIDTH.saturating_sub(cols(&prefix) + cols(&suffix));
    let shown = if cols(&text) <= room {
        text
    } else {
        // Cut with `…` (2 columns) so the matched words always fit.
        let budget = room.saturating_sub(cols("…"));
        let mut cut = String::new();
        for c in text.chars() {
            if cols(&cut) + cols(&c.to_string()) > budget {
                break;
            }
            cut.push(c);
        }
        format!("{}…", cut.trim_end())
    };
    let _ = writeln!(out, "{prefix}{shown}{suffix}");
}

/// The `## Files and Notes for This Task` section, or `None` when nothing
/// matched. Ends with a blank line.
pub(crate) fn render_section(m: &TaskMatch) -> Option<String> {
    let TaskMatch::Matched { files, notes } = m else {
        return None;
    };
    let mut out = String::new();
    let _ = writeln!(out, "{SECTION_HEADING}\n\n{SECTION_INTRO}\n");
    if !files.is_empty() {
        let _ = writeln!(out, "Files:");
        for f in files.iter().take(MAX_FILES_SHOWN) {
            render_file_line(&mut out, f);
        }
        if files.len() > MAX_FILES_SHOWN {
            let _ = writeln!(
                out,
                "{}",
                more_line(files.len() - MAX_FILES_SHOWN, "file", "files")
            );
        }
        out.push('\n');
    }
    if !notes.is_empty() {
        let _ = writeln!(out, "Saved notes:");
        for n in notes.iter().take(MAX_NOTES_SHOWN) {
            render_note_line(&mut out, n);
        }
        if notes.len() > MAX_NOTES_SHOWN {
            let _ = writeln!(
                out,
                "{}",
                more_line(notes.len() - MAX_NOTES_SHOWN, "note", "notes")
            );
        }
        out.push('\n');
    }
    Some(out)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The extra terminal line after `repolens brief` writes the brief file,
/// or `None` without `--task`.
pub(crate) fn terminal_summary(m: &TaskMatch) -> Option<String> {
    match m {
        TaskMatch::NoTask => None,
        TaskMatch::OnlyStopWords | TaskMatch::NoMatch => Some(NO_MATCH_SUMMARY.to_string()),
        TaskMatch::Matched { files, notes } => {
            let f = plural(files.len(), "file", "files");
            let n = plural(notes.len(), "saved note", "saved notes");
            Some(match (files.is_empty(), notes.is_empty()) {
                (false, false) => format!("Matched {f} and {n} to the task."),
                (false, true) => format!("Matched {f} to the task."),
                _ => format!("Matched {n} to the task."),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_and_snake_names_split_into_stemmed_tokens() {
        let t = name_tokens("src/showSM4Panel_paddings.rs");
        for want in ["src", "show", "sm4", "panel", "padding", "rs"] {
            assert!(t.contains(&stem(want)), "{want} missing from {t:?}");
        }
        assert!(name_tokens("HTTPServer").contains("server"));
        assert!(name_tokens("HTTPServer").contains("http"));
    }

    #[test]
    fn task_terms_drop_stop_words_and_plurals_dedupe() {
        let t = task_terms("Change the paddings and padding for SM4");
        let words: Vec<&str> = t.iter().map(|(_, w)| w.as_str()).collect();
        assert!(words.contains(&"sm4"));
        assert!(!words.contains(&"the"));
        assert_eq!(words.iter().filter(|w| w.starts_with("padding")).count(), 1);
        assert!(task_terms("the of a").is_empty());
    }

    #[test]
    fn no_task_only_stop_words_and_no_match() {
        assert_eq!(match_task(None, &[], &[]), TaskMatch::NoTask);
        assert_eq!(
            match_task(Some("the of a"), &[], &[]),
            TaskMatch::OnlyStopWords
        );
        assert_eq!(match_task(Some("sm4 cbc"), &[], &[]), TaskMatch::NoMatch);
        assert_eq!(task_note_line(&TaskMatch::NoTask), None);
        assert_eq!(terminal_summary(&TaskMatch::NoTask), None);
        assert_eq!(
            terminal_summary(&TaskMatch::NoMatch).as_deref(),
            Some(NO_MATCH_SUMMARY)
        );
    }

    fn fm(path: &str, n: usize) -> FileMatch {
        FileMatch {
            path: path.into(),
            words: (0..n).map(|i| format!("w{i}")).collect(),
        }
    }

    #[test]
    fn section_caps_files_and_counts_the_rest() {
        let files: Vec<FileMatch> = (0..11).map(|i| fm(&format!("f{i:02}.rs"), 1)).collect();
        let s = render_section(&TaskMatch::Matched {
            files,
            notes: vec![],
        })
        .unwrap();
        assert_eq!(s.lines().filter(|l| l.starts_with("- `")).count(), 8);
        assert!(s.contains("- …and 3 more files match\n"));
        assert!(!s.contains("Saved notes:"), "empty heading omitted");
    }

    #[test]
    fn summary_singular_and_plural() {
        let one = TaskMatch::Matched {
            files: vec![fm("a", 1)],
            notes: vec![],
        };
        assert_eq!(
            terminal_summary(&one).unwrap(),
            "Matched 1 file to the task."
        );
        let note = NoteMatch {
            type_name: "DECISION",
            id: "9c44a430ffff".into(),
            content: "x".into(),
            words: vec!["x".into()],
        };
        let both = TaskMatch::Matched {
            files: vec![fm("a", 1), fm("b", 1), fm("c", 1)],
            notes: vec![note],
        };
        assert_eq!(
            terminal_summary(&both).unwrap(),
            "Matched 3 files and 1 saved note to the task."
        );
        let s = render_section(&both).unwrap();
        assert!(s.contains("- DECISION 9c44a430: x (x)\n"), "{s}");
    }

    #[test]
    fn cols_counts_cjk_ellipsis_and_tabs() {
        assert_eq!(cols("ab"), 2);
        assert_eq!(cols("中文"), 4);
        assert_eq!(cols("…"), 2);
        assert_eq!(cols("a\tb"), 9);
    }

    #[test]
    fn long_notes_are_cut_within_80_columns_keeping_words() {
        let note = NoteMatch {
            type_name: "PATTERN",
            id: "abcdef0123".into(),
            content: "中文".repeat(60),
            words: vec!["sm4".into(), "padding".into()],
        };
        let mut out = String::new();
        render_note_line(&mut out, &note);
        let line = out.trim_end();
        assert!(cols(line) <= 80, "{} cols: {line}", cols(line));
        assert!(line.ends_with("… (sm4, padding)"), "{line}");
    }
}
