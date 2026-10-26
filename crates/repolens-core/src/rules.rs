use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Result of evaluating a file path against all loaded rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDecision {
    pub included: bool,
    pub reason: String,
}

impl RuleDecision {
    pub fn include() -> Self {
        Self {
            included: true,
            reason: "included".to_string(),
        }
    }

    pub fn exclude(reason: &str) -> Self {
        Self {
            included: false,
            reason: reason.to_string(),
        }
    }
}

/// Evaluates files against `.gitignore` rules and optional `.reposnapshot.rules`.
///
/// Each `.gitignore` file is compiled with its containing directory as the root,
/// so patterns are correctly scoped to the subdirectory they belong to.
#[derive(Clone)]
pub struct RulesEngine {
    root: PathBuf,
    /// Per-directory gitignore matchers: (directory, compiled gitignore)
    gitignores: Vec<(PathBuf, Gitignore)>,
    reposnapshot: Option<Gitignore>,
    reposnapshot_whitelists: Vec<String>,
}

impl RulesEngine {
    /// Create an engine with no rules (everything is included).
    pub fn new(root: &Path, reposnapshot_files: &[PathBuf]) -> Self {
        let reposnapshot = Self::build_reposnapshot(root, reposnapshot_files);
        let reposnapshot_whitelists = Self::collect_reposnapshot_whitelists(reposnapshot_files);
        Self {
            root: root.to_path_buf(),
            gitignores: vec![],
            reposnapshot,
            reposnapshot_whitelists,
        }
    }

    /// Load rules by scanning for `.gitignore` files under `root`.
    pub fn load(root: &Path, reposnapshot_files: &[PathBuf]) -> std::io::Result<Self> {
        let gitignores = Self::build_gitignores(root)?;
        let reposnapshot = Self::build_reposnapshot(root, reposnapshot_files);
        let reposnapshot_whitelists = Self::collect_reposnapshot_whitelists(reposnapshot_files);
        debug!(root = %root.display(), gitignore_count = gitignores.len(), "RulesEngine loaded");
        Ok(Self {
            root: root.to_path_buf(),
            gitignores,
            reposnapshot,
            reposnapshot_whitelists,
        })
    }

    /// Evaluate whether a file should be included or excluded.
    ///
    /// Git semantics: last matching rule wins.  Gitignore files are evaluated
    /// from shallowest (root) to deepest (closest to file), so a child
    /// `.gitignore` can override a parent's decision.  `.reposnapshot.rules`
    /// are evaluated after all gitignore files and override everything.
    pub fn evaluate(&self, path: &Path, is_dir: bool) -> RuleDecision {
        // Track the last matching gitignore decision (last match wins)
        let mut last_gi_decision: Option<RuleDecision> = None;

        // Evaluate all gitignore files (sorted shallowest-first by build_gitignores)
        for (dir, gi) in &self.gitignores {
            if let Ok(rel) = path.strip_prefix(dir) {
                let m = gi.matched_path_or_any_parents(rel, is_dir);
                if m.is_ignore() {
                    last_gi_decision = Some(RuleDecision::exclude("excluded:gitignore"));
                } else if m.is_whitelist() {
                    last_gi_decision = Some(RuleDecision::include());
                }
            }
        }

        // .reposnapshot.rules override gitignore (evaluated last)
        if let Some(rs) = &self.reposnapshot {
            let rel = match path.strip_prefix(&self.root) {
                Ok(r) => r,
                Err(_) => path,
            };
            let m = rs.matched_path_or_any_parents(rel, is_dir);
            if m.is_ignore() {
                return RuleDecision::exclude("excluded:rules");
            }
            if m.is_whitelist() {
                return RuleDecision::include();
            }
        }

        last_gi_decision.unwrap_or_else(RuleDecision::include)
    }

    pub fn reposnapshot_whitelist_may_touch_subtree(&self, rel_dir: &str) -> bool {
        let prefix = format!("{rel_dir}/");
        self.reposnapshot_whitelists
            .iter()
            .map(|p| p.trim_start_matches('/'))
            .any(|p| {
                // Any glob metacharacter forces us to keep the subtree: an unanchored
                // pattern like `!*.log` could re-include a descendant we've never seen,
                // so we trade pruning coverage for correctness whenever wildcards exist.
                p == rel_dir
                    || p.starts_with(&prefix)
                    || p.chars().any(|ch| matches!(ch, '*' | '?' | '['))
            })
    }

    /// Build per-directory gitignore matchers by walking for `.gitignore` files.
    ///
    /// The walk is **gitignore-aware** (`git_ignore(true)`), so it prunes ignored
    /// subtrees (`target/`, `node_modules/`, `.venv/`, …) instead of traversing
    /// them — on real repos that unpruned traversal dominated scan wall-clock.
    /// This encodes the git-aligned semantic: a `.gitignore` *inside* an excluded
    /// directory does not re-include files (git: "it is not possible to
    /// re-include a file if a parent directory of that file is excluded"). The
    /// supported re-include mechanism is `.reposnapshot.rules` whitelists, which
    /// `discovery::should_descend_dir` honors separately. Only `.gitignore` drives
    /// pruning here (`.ignore`/global/parent sources are disabled) to match
    /// `evaluate()`'s rule set exactly.
    fn build_gitignores(root: &Path) -> std::io::Result<Vec<(PathBuf, Gitignore)>> {
        let mut result: Vec<(PathBuf, Gitignore)> = Vec::new();

        let mut walk = WalkBuilder::new(root);
        walk.hidden(false)
            .parents(false)
            .ignore(false)
            .git_ignore(true)
            .git_exclude(false)
            .git_global(false)
            .require_git(false)
            .filter_entry(|entry| {
                // `.git` / `.repolens` never hold a relevant user `.gitignore`,
                // and the main discovery walk skips them too; pruning here avoids
                // walking large `.git` object stores.
                let name = entry.file_name();
                name != ".git" && name != ".repolens"
            });

        for entry in walk.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_file() && entry.file_name() == ".gitignore" {
                let gitignore_path = entry.path();
                // The .gitignore is scoped to its containing directory
                let dir = match gitignore_path.parent() {
                    Some(d) => d.to_path_buf(),
                    None => continue,
                };
                let mut builder = GitignoreBuilder::new(&dir);
                if let Some(err) = builder.add(gitignore_path) {
                    debug!(path = %gitignore_path.display(), err = %err, "failed to parse .gitignore");
                    continue;
                }
                match builder.build() {
                    Ok(gi) => result.push((dir, gi)),
                    Err(e) => {
                        debug!(path = %gitignore_path.display(), err = %e, "failed to build gitignore");
                    }
                }
            }
        }

        // Sort by directory depth (shallowest first) so child overrides parent
        result.sort_by_key(|(dir, _)| dir.components().count());
        Ok(result)
    }

    fn build_reposnapshot(root: &Path, files: &[PathBuf]) -> Option<Gitignore> {
        if files.is_empty() {
            return None;
        }
        let mut builder = GitignoreBuilder::new(root);
        for file in files {
            if let Some(err) = builder.add(file) {
                debug!(path = %file.display(), err = %err, "failed to parse .reposnapshot.rules");
            }
        }
        Some(builder.build().unwrap_or_else(|_| Gitignore::empty()))
    }

    fn collect_reposnapshot_whitelists(files: &[PathBuf]) -> Vec<String> {
        let mut patterns = Vec::new();
        for file in files {
            let Ok(contents) = std::fs::read_to_string(file) else {
                continue;
            };
            for line in contents.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.starts_with('!') {
                    continue;
                }
                let pattern = trimmed[1..].trim();
                if !pattern.is_empty() {
                    patterns.push(pattern.to_string());
                }
            }
        }
        patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reposnapshot_whitelist_helper_treats_wildcards_as_subtree_touching() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let rules_path = root.join(".reposnapshot.rules");
        std::fs::write(&rules_path, "!b*/keep.txt\n!**/keep.txt\n").unwrap();

        let engine = RulesEngine::load(root, &[rules_path]).unwrap();

        assert!(engine.reposnapshot_whitelist_may_touch_subtree("build"));
        assert!(engine.reposnapshot_whitelist_may_touch_subtree("target/debug"));
        assert!(engine.reposnapshot_whitelist_may_touch_subtree("src"));
    }
}
