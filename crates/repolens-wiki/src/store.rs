//! Wiki article storage: index load/save, article read/write/delete, and event log.
//!
//! The wiki is stored at `.repolens/wiki/` inside the repo root.
//! Writes are atomic: data is written to a `.tmp` sibling file, then renamed.
//! A `.gitignore` containing `*` is maintained in `.repolens/wiki/` so the
//! wiki directory is never accidentally committed.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use repolens_core::path_safety::{
    resolve_existing_subpath, resolve_subpath_for_create, validate_relative_subpath,
};
use time::OffsetDateTime;
use tracing::{debug, info, warn};

use repolens_core::model::wiki::{ArticleType, WikiArticle, WikiIndex, WikiLog, WikiLogEntry};

// ---------------------------------------------------------------------------
// Storage paths
// ---------------------------------------------------------------------------

fn wiki_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".repolens").join("wiki")
}

fn index_file(repo_root: &Path) -> PathBuf {
    wiki_dir(repo_root).join("wiki-index.json")
}

fn log_file(repo_root: &Path) -> PathBuf {
    wiki_dir(repo_root).join("wiki-log.json")
}

fn gitignore_file(repo_root: &Path) -> PathBuf {
    wiki_dir(repo_root).join(".gitignore")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Stateless handle for reading and writing the wiki store.
pub struct WikiStore;

impl WikiStore {
    /// Load the wiki index from disk.
    ///
    /// Returns an empty index (via [`empty_index`]) if the file does not exist
    /// or cannot be parsed.
    pub fn load_index(repo_root: &Path) -> Result<WikiIndex> {
        let path = index_file(repo_root);
        debug!(path = %path.display(), "Loading wiki index");

        if !path.exists() {
            info!(path = %path.display(), "Wiki index file not found, returning empty index");
            return Ok(empty_index());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read wiki index file: {}", path.display()))?;

        match serde_json::from_str::<WikiIndex>(&raw) {
            Ok(index) => {
                for article in &index.articles {
                    validate_relative_subpath(&article.path).with_context(|| {
                        format!("invalid article path for {}: {}", article.id, article.path)
                    })?;
                }
                info!(
                    path = %path.display(),
                    articles = index.articles.len(),
                    "Loaded wiki index"
                );
                Ok(index)
            }
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Wiki index file corrupt or unparseable, returning empty index"
                );
                Ok(empty_index())
            }
        }
    }

    /// Save the wiki index to disk atomically.
    ///
    /// Writes to a `.tmp` sibling, then renames to the final path.
    pub fn save_index(repo_root: &Path, index: &WikiIndex) -> Result<()> {
        let dir = wiki_dir(repo_root);
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create wiki directory: {}", dir.display()))?;
        repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&dir);

        for article in &index.articles {
            validate_relative_subpath(&article.path).with_context(|| {
                format!("invalid article path for {}: {}", article.id, article.path)
            })?;
        }

        let dst = index_file(repo_root);

        let json = serde_json::to_string_pretty(index).context("Failed to serialize wiki index")?;

        repolens_core::atomic_write::write_atomic(&dst, json.as_bytes())
            .with_context(|| format!("Failed to write wiki index to {}", dst.display()))?;

        info!(
            path = %dst.display(),
            articles = index.articles.len(),
            "Saved wiki index"
        );
        Ok(())
    }

    /// Ensure `.repolens/wiki/.gitignore` exists with `*` content.
    ///
    /// Creates the file if absent with `*\n` to exclude all wiki files from git.
    pub fn ensure_gitignore(repo_root: &Path) -> Result<()> {
        ensure_gitignore(repo_root)
    }

    /// Write a wiki article to disk.
    ///
    /// Creates parent directories if they don't exist. The file format is:
    ///
    /// ```text
    /// ---
    /// {yaml frontmatter}
    /// ---
    ///
    /// {body}
    /// ```
    pub fn write_article(repo_root: &Path, article: &WikiArticle, body: &str) -> Result<()> {
        let base_dir = wiki_dir(repo_root);
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("Failed to create wiki directory: {}", base_dir.display()))?;
        repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&base_dir);
        let file_path =
            resolve_subpath_for_create(&base_dir, &article.path).with_context(|| {
                format!("invalid article path for {}: {}", article.id, article.path)
            })?;

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create article directory: {}", parent.display())
            })?;
        }

        let yaml = serde_yaml::to_string(article)
            .context("Failed to serialize wiki article frontmatter")?;

        let content = format!("---\n{}---\n\n{}", yaml, body);

        fs::write(&file_path, &content)
            .with_context(|| format!("Failed to write wiki article: {}", file_path.display()))?;

        debug!(path = %file_path.display(), id = %article.id, "Wrote wiki article");
        Ok(())
    }

    /// Read a wiki article body from disk, stripping YAML frontmatter.
    ///
    /// Returns only the body content (everything after the closing `---\n\n`).
    pub fn read_article(repo_root: &Path, article: &WikiArticle) -> Result<String> {
        let file_path = resolve_existing_subpath(&wiki_dir(repo_root), &article.path)
            .with_context(|| {
                format!("invalid article path for {}: {}", article.id, article.path)
            })?;
        debug!(path = %file_path.display(), id = %article.id, "Reading wiki article");

        let raw = fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read wiki article: {}", file_path.display()))?;

        Ok(strip_frontmatter(&raw))
    }

    /// Delete a wiki article from disk and remove it from the index.
    ///
    /// Returns an error if the article is not found in the index.
    pub fn delete_article(repo_root: &Path, index: &mut WikiIndex, id: &str) -> Result<()> {
        let pos = index
            .articles
            .iter()
            .position(|a| a.id == id)
            .with_context(|| format!("Article not found in index: {}", id))?;

        let file_path = resolve_existing_subpath(&wiki_dir(repo_root), &index.articles[pos].path)
            .with_context(|| {
            format!(
                "invalid article path for {}: {}",
                index.articles[pos].id, index.articles[pos].path
            )
        })?;

        if file_path.exists() {
            fs::remove_file(&file_path).with_context(|| {
                format!(
                    "Failed to delete wiki article file: {}",
                    file_path.display()
                )
            })?;
            debug!(path = %file_path.display(), id, "Deleted wiki article file");
        } else {
            warn!(
                path = %file_path.display(),
                id,
                "Wiki article file not found on disk during delete"
            );
        }

        index.articles.remove(pos);
        info!(id, "Deleted wiki article from index");
        Ok(())
    }

    /// Append a log entry to the wiki event log atomically.
    ///
    /// Loads the existing log (or starts empty), appends the entry, then writes
    /// atomically via `.tmp` + rename.
    pub fn append_log(repo_root: &Path, entry: WikiLogEntry) -> Result<()> {
        let dir = wiki_dir(repo_root);
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create wiki directory: {}", dir.display()))?;
        repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&dir);

        let mut log = Self::load_log(repo_root)?;
        log.entries.push(entry);

        let dst = log_file(repo_root);

        let json = serde_json::to_string_pretty(&log).context("Failed to serialize wiki log")?;

        repolens_core::atomic_write::write_atomic(&dst, json.as_bytes())
            .with_context(|| format!("Failed to write wiki log to {}", dst.display()))?;

        debug!(path = %dst.display(), entries = log.entries.len(), "Appended to wiki log");
        Ok(())
    }

    /// Load the wiki event log from disk.
    ///
    /// Returns an empty log if the file does not exist or cannot be parsed.
    pub fn load_log(repo_root: &Path) -> Result<WikiLog> {
        let path = log_file(repo_root);
        debug!(path = %path.display(), "Loading wiki log");

        if !path.exists() {
            debug!(path = %path.display(), "Wiki log file not found, returning empty log");
            return Ok(WikiLog { entries: vec![] });
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read wiki log file: {}", path.display()))?;

        match serde_json::from_str::<WikiLog>(&raw) {
            Ok(log) => {
                debug!(
                    path = %path.display(),
                    entries = log.entries.len(),
                    "Loaded wiki log"
                );
                Ok(log)
            }
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Wiki log file corrupt or unparseable, returning empty log"
                );
                Ok(WikiLog { entries: vec![] })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Convert a title string into a URL-safe slug.
///
/// Lowercases the title, replaces all non-alphanumeric characters with `-`,
/// collapses consecutive `-` into one, then trims leading/trailing `-`.
///
/// Examples:
/// - `"Memory Decay Engine"` → `"memory-decay-engine"`
/// - `"How It Works (v2)"` → `"how-it-works-v2"`
pub fn slugify(title: &str) -> String {
    // Replace every non-alphanumeric char with '-'
    let raw: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Lowercase, collapse runs of '-', trim edges
    let lowered = raw.to_lowercase();
    let collapsed = lowered
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    collapsed
}

/// Compute a collision-safe file path for a new wiki article.
///
/// The path has the form `articles/{type_dir}/{slug}.md`.  If that path is
/// already present in `existing_paths`, a numeric suffix is appended:
/// `-2`, `-3`, … until a free slot is found.
pub fn compute_article_path(
    article_type: &ArticleType,
    title: &str,
    existing_paths: &[String],
) -> String {
    let slug = slugify(title);
    let base = format!("articles/{}/{}.md", article_type.dir_name(), slug);

    if !existing_paths.contains(&base) {
        return base;
    }

    let mut counter = 2usize;
    loop {
        let candidate = format!(
            "articles/{}/{}-{}.md",
            article_type.dir_name(),
            slug,
            counter
        );
        if !existing_paths.contains(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Create an empty [`WikiIndex`] with schema version `"1.0"`.
pub fn empty_index() -> WikiIndex {
    WikiIndex {
        schema_version: "1.0".to_string(),
        updated_at: OffsetDateTime::now_utc(),
        articles: Vec::new(),
    }
}

/// Ensure `.repolens/wiki/.gitignore` exists with `*` content.
///
/// Creates the file if absent with `*\n` to exclude all wiki files from git.
pub fn ensure_gitignore(repo_root: &Path) -> Result<()> {
    let dir = wiki_dir(repo_root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create wiki dir: {}", dir.display()))?;
    repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&dir);

    let path = gitignore_file(repo_root);

    if !path.exists() {
        fs::write(&path, "*\n")
            .with_context(|| format!("Failed to create wiki gitignore: {}", path.display()))?;
        debug!(path = %path.display(), "Created wiki gitignore");
    }
    Ok(())
}

/// Strip YAML frontmatter from article content.
///
/// Removes everything from `---\n` through the closing `---\n` and the blank
/// line separator that follows, returning only the body. If no valid frontmatter
/// is found, returns the original content unchanged.
fn strip_frontmatter(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }

    let after_open = &content[4..];
    if let Some(close_pos) = after_open.find("---\n") {
        // Skip past: opening "---\n" (4) + yaml + closing "---\n" (4)
        let body_start = 4 + close_pos + 4;
        let remainder = &content[body_start..];
        // Strip the blank separator line written between frontmatter and body
        if let Some(stripped) = remainder.strip_prefix('\n') {
            stripped.to_string()
        } else {
            remainder.to_string()
        }
    } else {
        content.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::wiki::{ArticleType, Staleness, WikiEventKind};
    use tempfile::TempDir;

    fn make_article(id: &str, title: &str, article_type: ArticleType) -> WikiArticle {
        let now = OffsetDateTime::now_utc();
        WikiArticle {
            id: id.to_string(),
            title: title.to_string(),
            article_type: article_type.clone(),
            path: format!("{}/{}.md", article_type.dir_name(), id),
            anchors: vec![],
            memory_refs: vec![],
            tags: vec![],
            confidence: 0.9,
            staleness: Staleness::Fresh,
            summary: format!("Summary of {}", title),
            created_at: now,
            updated_at: now,
            source: None,
        }
    }

    fn make_log_entry(event: WikiEventKind, detail: &str) -> WikiLogEntry {
        WikiLogEntry {
            timestamp: OffsetDateTime::now_utc(),
            event,
            article_id: None,
            detail: detail.to_string(),
        }
    }

    // --- Task 3: index load/save and gitignore ---

    #[test]
    fn load_returns_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        let index = WikiStore::load_index(dir.path()).unwrap();
        assert_eq!(index.schema_version, "1.0");
        assert!(index.articles.is_empty());
    }

    #[test]
    fn load_returns_empty_on_corrupt_file() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".repolens").join("wiki");
        fs::create_dir_all(&wiki_dir).unwrap();
        fs::write(wiki_dir.join("wiki-index.json"), b"not valid json {{{{").unwrap();

        let index = WikiStore::load_index(dir.path()).unwrap();
        assert_eq!(index.schema_version, "1.0");
        assert!(index.articles.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut index = empty_index();
        index.articles.push(make_article(
            "art-1",
            "Auth Overview",
            ArticleType::Overview,
        ));
        index
            .articles
            .push(make_article("art-2", "UserService", ArticleType::Entity));

        WikiStore::save_index(dir.path(), &index).unwrap();

        let loaded = WikiStore::load_index(dir.path()).unwrap();
        assert_eq!(loaded.schema_version, "1.0");
        assert_eq!(loaded.articles.len(), 2);
        assert_eq!(loaded.articles[0].id, "art-1");
        assert_eq!(loaded.articles[1].title, "UserService");
    }

    #[test]
    fn ensure_gitignore_creates_file() {
        let dir = TempDir::new().unwrap();
        WikiStore::ensure_gitignore(dir.path()).unwrap();

        let gitignore = dir.path().join(".repolens").join("wiki").join(".gitignore");
        assert!(gitignore.exists(), ".gitignore should be created");

        let contents = fs::read_to_string(&gitignore).unwrap();
        assert_eq!(contents, "*\n", "gitignore should contain exactly '*\\n'");
    }

    // --- Task 4: article read/write/delete and log ---

    #[test]
    fn write_and_read_article() {
        let dir = TempDir::new().unwrap();
        let article = make_article("svc-1", "UserService", ArticleType::Entity);
        let body = "## Overview\n\nThe UserService handles user authentication.\n";

        WikiStore::write_article(dir.path(), &article, body).unwrap();
        let read_back = WikiStore::read_article(dir.path(), &article).unwrap();

        assert_eq!(read_back, body);
    }

    #[test]
    fn write_article_creates_frontmatter() {
        let dir = TempDir::new().unwrap();
        let article = make_article("svc-1", "UserService", ArticleType::Entity);
        let body = "Some body content.\n";

        WikiStore::write_article(dir.path(), &article, body).unwrap();

        let file_path = dir
            .path()
            .join(".repolens")
            .join("wiki")
            .join(&article.path);
        let raw = fs::read_to_string(&file_path).unwrap();

        assert!(
            raw.starts_with("---\n"),
            "file should start with frontmatter delimiter"
        );
        assert!(raw.contains("id:"), "frontmatter should contain id field");
        assert!(
            raw.contains("title:"),
            "frontmatter should contain title field"
        );
        assert!(
            raw.contains("UserService"),
            "frontmatter should contain the title value"
        );
        assert!(
            raw.contains("---\n\n"),
            "should have closing delimiter followed by blank line"
        );
        assert!(raw.ends_with(body), "file should end with the body");
    }

    #[test]
    fn delete_article_removes_file_and_index_entry() {
        let dir = TempDir::new().unwrap();
        let article = make_article("svc-1", "UserService", ArticleType::Entity);
        let body = "Body content.\n";

        WikiStore::write_article(dir.path(), &article, body).unwrap();

        let mut index = empty_index();
        index.articles.push(article.clone());

        WikiStore::delete_article(dir.path(), &mut index, "svc-1").unwrap();

        assert!(
            index.articles.is_empty(),
            "article should be removed from index"
        );

        let file_path = dir
            .path()
            .join(".repolens")
            .join("wiki")
            .join(&article.path);
        assert!(
            !file_path.exists(),
            "article file should be deleted from disk"
        );
    }

    #[test]
    fn delete_nonexistent_returns_error() {
        let dir = TempDir::new().unwrap();
        let mut index = empty_index();

        let result = WikiStore::delete_article(dir.path(), &mut index, "nonexistent-id");
        assert!(
            result.is_err(),
            "deleting nonexistent article should return an error"
        );
    }

    #[test]
    fn load_index_rejects_article_path_that_escapes_wiki_dir() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".repolens").join("wiki");
        fs::create_dir_all(&wiki_dir).unwrap();

        let mut index = empty_index();
        let mut article = make_article("svc-1", "UserService", ArticleType::Entity);
        article.path = "../escape.md".to_string();
        index.articles.push(article);

        fs::write(
            wiki_dir.join("wiki-index.json"),
            serde_json::to_string_pretty(&index).unwrap(),
        )
        .unwrap();

        let err = WikiStore::load_index(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("article path"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn append_and_load_log() {
        let dir = TempDir::new().unwrap();

        WikiStore::append_log(
            dir.path(),
            make_log_entry(WikiEventKind::Compile, "Compiled auth article"),
        )
        .unwrap();

        WikiStore::append_log(
            dir.path(),
            make_log_entry(WikiEventKind::Update, "Updated UserService article"),
        )
        .unwrap();

        let log = WikiStore::load_log(dir.path()).unwrap();
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].detail, "Compiled auth article");
        assert_eq!(log.entries[1].detail, "Updated UserService article");
    }

    #[test]
    fn load_log_returns_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        let log = WikiStore::load_log(dir.path()).unwrap();
        assert!(log.entries.is_empty());
    }

    #[test]
    fn load_log_returns_empty_on_corrupt_file() {
        let dir = TempDir::new().unwrap();
        let wiki_dir = dir.path().join(".repolens").join("wiki");
        fs::create_dir_all(&wiki_dir).unwrap();
        fs::write(wiki_dir.join("wiki-log.json"), b"not valid json {{{{").unwrap();

        let log = WikiStore::load_log(dir.path()).unwrap();
        assert!(log.entries.is_empty());
    }

    // --- Task 7: slugify and compute_article_path ---

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Memory Decay Engine"), "memory-decay-engine");
    }

    #[test]
    fn slugify_strips_special_chars() {
        assert_eq!(slugify("How It Works (v2)"), "how-it-works-v2");
    }

    #[test]
    fn compute_path_basic() {
        let path = compute_article_path(&ArticleType::Concept, "Memory Decay Engine", &[]);
        assert_eq!(path, "articles/concept/memory-decay-engine.md");
    }

    #[test]
    fn compute_path_collision() {
        let existing = vec!["articles/entity/user-service.md".to_string()];
        let path = compute_article_path(&ArticleType::Entity, "User Service", &existing);
        assert_eq!(path, "articles/entity/user-service-2.md");
    }

    #[test]
    fn compute_path_double_collision() {
        let existing = vec![
            "articles/overview/auth-flow.md".to_string(),
            "articles/overview/auth-flow-2.md".to_string(),
        ];
        let path = compute_article_path(&ArticleType::Overview, "Auth Flow", &existing);
        assert_eq!(path, "articles/overview/auth-flow-3.md");
    }
}
