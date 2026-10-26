use anyhow::{anyhow, bail, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Component, Path, PathBuf};

/// Most outside paths a boundary message lists before "…and N more".
pub const MAX_BOUNDARY_PATHS: usize = 10;

/// Columns the boundary message fits in, apart from a path that is longer
/// than this by itself.
pub const BOUNDARY_WIDTH: usize = 80;

/// The boundary message (#58 items 1 and 5):
///
/// ```text
/// These files changed outside the approved bootstrap boundary:
///   out.txt
///   src/lib.rs
///   …and 3 more
/// Allowed: docs/**, README.md
/// ```
///
/// One path per line, indented 2, at most [`MAX_BOUNDARY_PATHS`] of them,
/// then one "…and N more" line. The `Allowed:` line wraps after a comma
/// to stay within [`BOUNDARY_WIDTH`], continuation lines indented 2.
pub fn boundary_report(paths: &[String], allowed: &[String]) -> String {
    let mut lines =
        vec!["These files changed outside the approved bootstrap boundary:".to_string()];
    lines.extend(
        paths
            .iter()
            .take(MAX_BOUNDARY_PATHS)
            .map(|p| format!("  {p}")),
    );
    if paths.len() > MAX_BOUNDARY_PATHS {
        lines.push(format!("  …and {} more", paths.len() - MAX_BOUNDARY_PATHS));
    }
    lines.extend(wrap_allowed(allowed, BOUNDARY_WIDTH));
    lines.join("\n")
}

/// `Allowed: a, b, c`, wrapped after a comma so each line fits `width`
/// when it can; continuation lines are indented 2. A glob wider than a
/// line by itself still gets a line of its own.
fn wrap_allowed(globs: &[String], width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = "Allowed:".to_string();
    let mut cur_has_glob = false;
    for (i, glob) in globs.iter().enumerate() {
        let token = if i + 1 < globs.len() {
            format!("{glob},")
        } else {
            glob.clone()
        };
        if cur_has_glob && display_cols(&cur) + 1 + display_cols(&token) > width {
            lines.push(std::mem::replace(&mut cur, format!("  {token}")));
        } else {
            cur.push(' ');
            cur.push_str(&token);
        }
        cur_has_glob = true;
    }
    lines.push(cur);
    lines
}

/// Terminal columns of `s`: CJK and other wide characters count 2, and so
/// does `…`, which many CJK terminal fonts draw double width.
pub fn display_cols(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s) + s.matches('…').count()
}

#[derive(Debug)]
pub struct BootstrapPathGuard {
    root: PathBuf,
    matcher: Gitignore,
    approved_patterns: Vec<String>,
}

impl BootstrapPathGuard {
    pub fn compile(root: &Path, patterns: &[String]) -> Result<Self> {
        if patterns.is_empty() {
            bail!("at least one intended path is required");
        }

        let matcher = build_matcher(root, patterns)?;

        Ok(Self {
            root: root.canonicalize()?,
            matcher,
            approved_patterns: patterns.to_vec(),
        })
    }

    pub fn assert_changed_path_allowed(&self, repo_rel: &str) -> Result<()> {
        if self.is_outside_boundary(repo_rel)? {
            bail!(self.boundary_message(repo_rel));
        }
        Ok(())
    }

    /// The boundary message for one changed path outside the boundary.
    pub fn boundary_message(&self, repo_rel: &str) -> String {
        self.boundary_report(&[repo_rel.to_string()])
    }

    /// The boundary message for every changed path outside the boundary
    /// (#58 items 1 and 5); see [`boundary_report`].
    pub fn boundary_report(&self, paths: &[String]) -> String {
        boundary_report(paths, &self.approved_patterns)
    }

    /// `Ok(true)` when the path is outside the approved boundary. Errors
    /// for paths that aren't repo-relative or resolve through a broken or
    /// escaping symlink; those are reported on their own.
    pub fn is_outside_boundary(&self, repo_rel: &str) -> Result<bool> {
        let path = Path::new(repo_rel);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            bail!("changed path must be repo-relative");
        }

        self.assert_path_stays_within_repo(path)?;

        if self
            .matcher
            .matched_path_or_any_parents(path, false)
            .is_ignore()
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn assert_path_stays_within_repo(&self, repo_rel: &Path) -> Result<()> {
        let mut current = self.root.clone();
        let mut walked = PathBuf::new();

        for component in repo_rel.components() {
            let Component::Normal(part) = component else {
                continue;
            };

            current.push(part);
            walked.push(part);
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let resolved = current.canonicalize().map_err(|_| {
                        let rel = walked.to_string_lossy().replace('\\', "/");
                        anyhow!("{rel} is a broken symlink.")
                    })?;
                    if !resolved.starts_with(&self.root) {
                        bail!("changed path escapes repo root via symlink");
                    }
                    current = resolved;
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
                Err(err) => return Err(err.into()),
            }
        }

        if !current.starts_with(&self.root) {
            return Err(anyhow!("changed path escapes repo root"));
        }

        Ok(())
    }
}

pub fn path_matches_any_pattern(patterns: &[String], candidate: &str) -> Result<bool> {
    if patterns.is_empty() {
        return Ok(false);
    }

    let matcher = build_matcher(Path::new("."), patterns)?;
    Ok(matcher
        .matched_path_or_any_parents(Path::new(candidate), false)
        .is_ignore())
}

fn build_matcher(root: &Path, patterns: &[String]) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        add_pattern(&mut builder, pattern)?;
    }

    Ok(builder.build()?)
}

fn add_pattern(builder: &mut GitignoreBuilder, pattern: &str) -> Result<()> {
    let path = Path::new(pattern);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("intended paths must be repo-relative");
    }

    let normalized = normalize_pattern(pattern);
    builder.add_line(None, &normalized)?;
    if let Some(prefix) = pattern.strip_suffix("/**") {
        builder.add_line(None, &format!("{prefix}/"))?;
    }

    Ok(())
}

fn normalize_pattern(pattern: &str) -> String {
    if pattern.contains('/') {
        pattern.to_string()
    } else {
        format!("/{pattern}")
    }
}
