use std::path::Path;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use rmcp::{model::CallToolResult, ErrorData as McpError};

use crate::epistemic::*;

/// Curated, ordered allowlist of root-level convention files. Output order
/// equals this order. README/CHANGELOG are intentionally excluded.
const FIXED_CONVENTION_FILES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    "GEMINI.md",
    ".github/copilot-instructions.md",
    ".cursorrules",
    "CONVENTIONS.md",
    "CONTRIBUTING.md",
    "BACKLOG.md",
];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetConventionsParams {
    #[serde(default = "repolens_app::discover::default_repo_root")]
    #[schemars(
        description = "Absolute path to the repository root. Optional: defaults to the git repo containing the server's working directory."
    )]
    pub root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConventionKind {
    File,
    Rule,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConventionSource {
    /// Final path component (basename) of `path`.
    pub name: String,
    /// Repo-relative path (forward-slash separated).
    pub path: String,
    pub kind: ConventionKind,
    /// True iff `path` is a valid in-root regular-file candidate.
    pub present: bool,
    /// `Some` only when the file read succeeds (empty file -> `Some("")`).
    pub content: Option<String>,
}

/// A path is a candidate iff it canonicalizes to a regular file located under
/// the canonical repo root. Handles missing paths and broken symlinks
/// (canonicalize errors), directories (`is_file()` false), and out-of-root
/// symlink targets (prefix check fails).
fn is_in_root_regular_file(root_canon: &Path, full: &Path) -> bool {
    match std::fs::canonicalize(full) {
        Ok(canon) => canon.starts_with(root_canon) && canon.is_file(),
        Err(_) => false,
    }
}

fn basename(rel: &str) -> String {
    rel.rsplit('/')
        .next()
        .expect("rsplit always yields at least one element")
        .to_string()
}

/// Pure collector: reads the curated allowlist + `.claude/rules/*.md`,
/// returning one entry per fixed file (present or missing) plus present-only
/// rule entries sorted by filename. No MCP types; unit-testable in isolation.
pub fn collect_conventions(root: &Path) -> Vec<ConventionSource> {
    let root_canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut sources = Vec::new();

    // Fixed allowlist: always emit one ordered entry per path.
    for rel in FIXED_CONVENTION_FILES {
        let full = root.join(rel);
        let present = is_in_root_regular_file(&root_canon, &full);
        let content = if present {
            std::fs::read_to_string(&full).ok()
        } else {
            None
        };
        sources.push(ConventionSource {
            name: basename(rel),
            path: (*rel).to_string(),
            kind: ConventionKind::File,
            present,
            content,
        });
    }

    // Rules glob: `.claude/rules/*.md`, sorted by filename, present-only.
    let rules_dir = root.join(".claude/rules");
    if rules_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&rules_dir) {
            let mut rule_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
                .collect();
            rule_files.sort_by_key(|e| e.file_name());
            for entry in rule_files {
                let full = entry.path();
                if !is_in_root_regular_file(&root_canon, &full) {
                    continue; // skip dirs / broken / out-of-root
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let content = std::fs::read_to_string(&full).ok();
                sources.push(ConventionSource {
                    path: format!(".claude/rules/{name}"),
                    name,
                    kind: ConventionKind::Rule,
                    present: true,
                    content,
                });
            }
        }
    }

    sources
}

pub fn handle_get_conventions(params: GetConventionsParams) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    info!(root = %params.root, "get_conventions: entry");

    let root = Path::new(&params.root);
    let sources = collect_conventions(root);

    let considered = FIXED_CONVENTION_FILES.len();
    let fixed_found = sources
        .iter()
        .filter(|s| s.kind == ConventionKind::File && s.present)
        .count();
    let fixed_missing = considered - fixed_found;
    let rules_found = sources
        .iter()
        .filter(|s| s.kind == ConventionKind::Rule)
        .count();

    let read_paths: Vec<String> = sources
        .iter()
        .filter(|s| s.content.is_some())
        .map(|s| s.path.clone())
        .collect();
    let provenance = if read_paths.is_empty() {
        ProvenanceField::Single(Provenance::RepositoryRead {
            source: "(no readable convention sources)".into(),
        })
    } else {
        ProvenanceField::Multiple(
            read_paths
                .into_iter()
                .map(|p| Provenance::RepositoryRead { source: p })
                .collect(),
        )
    };

    let response = serde_json::json!({
        "sources": sources,
        "_meta": {
            "fixed": {
                "considered": considered,
                "found": fixed_found,
                "missing": fixed_missing,
            },
            "rules": { "found": rules_found },
        },
    });

    info!(
        elapsed_ms = %start.elapsed().as_millis(),
        fixed_found, fixed_missing, rules_found,
        "get_conventions: exit"
    );

    ok_epistemic(
        KnowledgeLayerField::Single(KnowledgeLayer::Observation),
        EpistemicConfidence::Mechanical,
        provenance,
        response,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn find<'a>(sources: &'a [ConventionSource], path: &str) -> &'a ConventionSource {
        sources
            .iter()
            .find(|s| s.path == path)
            .unwrap_or_else(|| panic!("source {path} should be in output"))
    }

    #[test]
    fn collect_reports_fixed_present_missing_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("CLAUDE.md"), "# rules\n").unwrap();
        fs::write(root.join("AGENTS.md"), "agents\n").unwrap();
        // GEMINI.md intentionally absent

        let sources = collect_conventions(root);

        let fixed: Vec<&str> = sources
            .iter()
            .filter(|s| s.kind == ConventionKind::File)
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(
            fixed,
            vec![
                "CLAUDE.md",
                "AGENTS.md",
                "GEMINI.md",
                ".github/copilot-instructions.md",
                ".cursorrules",
                "CONVENTIONS.md",
                "CONTRIBUTING.md",
                "BACKLOG.md",
            ]
        );

        let claude = find(&sources, "CLAUDE.md");
        assert!(claude.present);
        assert_eq!(claude.name, "CLAUDE.md");
        assert_eq!(claude.content.as_deref(), Some("# rules\n"));

        let gemini = find(&sources, "GEMINI.md");
        assert!(!gemini.present);
        assert_eq!(gemini.content, None);
    }

    #[test]
    fn collect_basename_for_nested_fixed_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".github")).unwrap();
        fs::write(root.join(".github/copilot-instructions.md"), "x\n").unwrap();

        let sources = collect_conventions(root);
        let copilot = find(&sources, ".github/copilot-instructions.md");
        assert!(copilot.present);
        assert_eq!(copilot.name, "copilot-instructions.md");
    }

    #[test]
    fn collect_empty_file_yields_empty_string_not_null() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("CONVENTIONS.md"), "").unwrap();

        let sources = collect_conventions(root);
        let c = find(&sources, "CONVENTIONS.md");
        assert!(c.present);
        assert_eq!(c.content.as_deref(), Some(""));
    }

    #[test]
    fn collect_non_utf8_file_is_present_with_null_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("BACKLOG.md"), [0xFF, 0xFE, 0x00]).unwrap();

        let sources = collect_conventions(root);
        let b = find(&sources, "BACKLOG.md");
        assert!(b.present, "exists as a file candidate");
        assert_eq!(b.content, None, "read fails on non-utf8");
    }

    #[test]
    fn collect_directory_named_like_allowlist_is_not_present() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("CLAUDE.md")).unwrap(); // a directory!

        let sources = collect_conventions(root);
        let c = find(&sources, "CLAUDE.md");
        assert!(!c.present);
        assert_eq!(c.content, None);
    }

    #[test]
    fn collect_rules_sorted_after_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rules = root.join(".claude/rules");
        fs::create_dir_all(&rules).unwrap();
        fs::write(rules.join("b.md"), "b\n").unwrap();
        fs::write(rules.join("a.md"), "a\n").unwrap();

        let sources = collect_conventions(root);

        // The 8 fixed entries come first, then rules.
        let first_rule = sources
            .iter()
            .position(|s| s.kind == ConventionKind::Rule)
            .expect("at least one rule");
        assert_eq!(first_rule, 8, "all 8 fixed entries precede rules");
        assert!(sources[..first_rule]
            .iter()
            .all(|s| s.kind == ConventionKind::File));

        let rule_paths: Vec<&str> = sources
            .iter()
            .filter(|s| s.kind == ConventionKind::Rule)
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(rule_paths, vec![".claude/rules/a.md", ".claude/rules/b.md"]);
    }

    #[test]
    fn collect_rules_directory_entry_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rules = root.join(".claude/rules");
        fs::create_dir_all(rules.join("notafile.md")).unwrap(); // a directory ending in .md
        fs::write(rules.join("real.md"), "ok\n").unwrap();

        let sources = collect_conventions(root);
        let rule_paths: Vec<&str> = sources
            .iter()
            .filter(|s| s.kind == ConventionKind::Rule)
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(rule_paths, vec![".claude/rules/real.md"]);
    }

    #[cfg(unix)]
    #[test]
    fn collect_symlink_in_root_read_out_of_root_rejected() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("real-claude.md"), "linked\n").unwrap();
        symlink(root.join("real-claude.md"), root.join("CLAUDE.md")).unwrap();

        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "nope\n").unwrap();
        symlink(outside.path().join("secret"), root.join("AGENTS.md")).unwrap();

        // Broken/dangling symlink: target does not exist -> not a candidate.
        symlink(root.join("does-not-exist"), root.join("GEMINI.md")).unwrap();

        let sources = collect_conventions(root);

        let claude = find(&sources, "CLAUDE.md");
        assert!(claude.present, "in-root symlink target is a candidate");
        assert_eq!(claude.content.as_deref(), Some("linked\n"));

        let agents = find(&sources, "AGENTS.md");
        assert!(!agents.present, "out-of-root symlink target is rejected");
        assert_eq!(agents.content, None);

        let gemini = find(&sources, "GEMINI.md");
        assert!(!gemini.present, "dangling symlink is not a candidate");
        assert_eq!(gemini.content, None);
    }
}
