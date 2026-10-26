use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

use rayon::prelude::*;
use repolens_core::git::is_commit_reachable;
use repolens_core::merged_pack_writer::MergedPackWriter;
use repolens_core::model::file_entry::{FileEntry, Symbol};
use repolens_core::model::session::RepoSession;
use repolens_core::pack_reader::{PackIndex, PackReader};
use repolens_core::pack_writer::{PackWriteOptions, PackWriter};
use repolens_core::path_safety::canonicalize_within_base;
use repolens_core::rules::RulesEngine;
use repolens_core::session::SessionStore;
use repolens_core::storage::RepoStoragePaths;
use repolens_core::{git, metadata};
use repolens_extract::lang;
use repolens_extract::registry::SymbolExtractorRegistry;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::graph::GraphBuilder;
use crate::scan::generate_and_write_profile;
use crate::scan::ScanResult;
use crate::scan_inputs::ScanInputs;

/// A candidate pack was made from a dirty worktree, so it is never a base.
pub const DIRTY_PACK_REASON: &str = "worktree was dirty when pack was created";

/// Fallback reason when every earlier pack came from a dirty worktree.
pub const NO_CLEAN_BASE_REASON: &str = "no earlier scan ran on a clean worktree";

#[derive(Debug)]
pub enum ScanMode {
    Full,
    Incremental,
}

#[derive(Debug)]
pub struct ScanPlan {
    pub mode: ScanMode,
    pub base_pack: Option<BasePack>,
    pub reason: String,
    /// Full scan although earlier packs exist: none could be used as a base.
    pub fell_back: bool,
}

#[derive(Debug)]
pub struct BasePack {
    pub path: PathBuf,
    pub index: PackIndex,
    pub head_commit: String,
}

impl ScanPlan {
    pub fn full(reason: &str) -> Self {
        Self {
            mode: ScanMode::Full,
            base_pack: None,
            reason: reason.to_string(),
            fell_back: false,
        }
    }

    /// Full scan because no existing pack could serve as the base.
    pub fn fell_back(reason: &str) -> Self {
        Self {
            fell_back: true,
            ..Self::full(reason)
        }
    }

    pub fn incremental(base: BasePack) -> Self {
        Self {
            mode: ScanMode::Incremental,
            base_pack: Some(base),
            reason: "compatible base pack found".to_string(),
            fell_back: false,
        }
    }
}

pub struct IncrementalResolver;

impl IncrementalResolver {
    /// Resolves whether an incremental scan is possible by finding a compatible base pack.
    ///
    /// Lists `pack-*` directories in `.repolens/`, sorted newest-first by name, and
    /// validates each candidate against the schema version, scan scope, worktree cleanliness,
    /// config hash, commit reachability, and full pack integrity. Returns the first passing
    /// candidate as `ScanPlan::incremental`, or `ScanPlan::full` if none qualifies.
    pub fn resolve(repo_root: &Path, config_hash: &str) -> ScanPlan {
        info!(
            root = %repo_root.display(),
            config_hash,
            "Resolving scan plan"
        );

        let storage_dir = RepoStoragePaths::storage_dir(repo_root);
        if !storage_dir.exists() {
            let reason = "no .repolens storage directory";
            debug!(root = %repo_root.display(), reason, "Falling back to full scan");
            return ScanPlan::full(reason);
        }

        let mut pack_dirs = match Self::list_pack_dirs(&storage_dir) {
            Ok(dirs) => dirs,
            Err(e) => {
                warn!(root = %repo_root.display(), err = %e, "Failed to list pack directories");
                return ScanPlan::fell_back("failed to list pack directories");
            }
        };

        // Sort newest-first by directory name (pack-YYYYMMDD-HHMMSS format sorts lexicographically)
        pack_dirs.sort_by(|a, b| b.cmp(a));

        if pack_dirs.is_empty() {
            let reason = "no pack directories found";
            debug!(root = %repo_root.display(), reason, "Falling back to full scan");
            return ScanPlan::full(reason);
        }

        debug!(
            root = %repo_root.display(),
            candidate_count = pack_dirs.len(),
            "Checking pack candidates for incremental scan"
        );

        let mut last_rejection = String::from("no pack directories found");
        let mut other_rejection: Option<String> = None;

        for pack_dir in &pack_dirs {
            debug!(pack_dir = %pack_dir.display(), "Checking candidate pack");
            match Self::check_candidate(pack_dir, repo_root, config_hash) {
                Ok(base_pack) => {
                    info!(
                        root = %repo_root.display(),
                        pack_dir = %pack_dir.display(),
                        head_commit = %base_pack.head_commit,
                        "Compatible base pack found — using incremental scan"
                    );
                    return ScanPlan::incremental(base_pack);
                }
                Err(reason) => {
                    debug!(
                        pack_dir = %pack_dir.display(),
                        reason = %reason,
                        "Pack candidate rejected"
                    );
                    if reason != DIRTY_PACK_REASON {
                        other_rejection = Some(reason.clone());
                    }
                    last_rejection = reason;
                }
            }
        }
        // Only say "no clean scan" when that's true of every pack; else
        // report a reason that isn't about dirtiness (#59).
        if last_rejection == DIRTY_PACK_REASON {
            last_rejection = match other_rejection {
                Some(other) => other,
                None => NO_CLEAN_BASE_REASON.to_string(),
            };
        }

        info!(
            root = %repo_root.display(),
            last_rejection = %last_rejection,
            "No compatible base pack found — falling back to full scan"
        );
        ScanPlan::fell_back(&last_rejection)
    }

    /// Validates a single pack directory candidate.
    ///
    /// Returns `Ok(BasePack)` if all checks pass, or `Err(reason)` describing the first failure.
    fn check_candidate(
        pack_dir: &Path,
        repo_root: &Path,
        config_hash: &str,
    ) -> std::result::Result<BasePack, String> {
        // a. manifest.json is readable and parseable — done as part of PackReader::read()
        //    but we also need the manifest fields before doing a full read, so we read manifest first.
        let manifest_path = pack_dir.join("manifest.json");
        let manifest_json = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("cannot read manifest.json: {e}"))?;

        let manifest: repolens_core::model::manifest::Manifest =
            serde_json::from_str(&manifest_json)
                .map_err(|e| format!("cannot parse manifest.json: {e}"))?;

        // b. schema_version starts with "1.2." or "1.3."
        let sv = &manifest.schema_version;
        if !sv.starts_with("1.2.") && !sv.starts_with("1.3.") {
            return Err(format!("unsupported schema_version: {sv}"));
        }

        // c. scan_scope is Some("complete")
        match manifest.scan_scope.as_deref() {
            Some("complete") => {}
            other => return Err(format!("scan_scope is not 'complete': {other:?}")),
        }

        // d. worktree_dirty is Some(false)
        match manifest.worktree_dirty {
            Some(false) => {}
            Some(true) => return Err(DIRTY_PACK_REASON.to_string()),
            None => return Err("worktree_dirty field is missing".to_string()),
        }

        // e. scan_config_hash matches config_hash
        match manifest.scan_config_hash.as_deref() {
            Some(h) if h == config_hash => {}
            Some(h) => {
                return Err(format!(
                    "config hash mismatch: pack={h}, current={config_hash}"
                ))
            }
            None => return Err("scan_config_hash field is missing".to_string()),
        }

        // f. head_commit is Some(...) and non-empty
        let head_commit = match manifest.head_commit.as_deref() {
            Some(c) if !c.is_empty() => c.to_string(),
            Some(_) => return Err("head_commit is empty".to_string()),
            None => return Err("head_commit field is missing".to_string()),
        };

        // g. is_commit_reachable(repo_root, &head_commit) returns true
        if !is_commit_reachable(repo_root, &head_commit) {
            return Err(format!(
                "head_commit {head_commit} is not reachable from current HEAD"
            ));
        }

        // h. Full PackReader::read() succeeds (pack integrity)
        let index =
            PackReader::read(pack_dir).map_err(|e| format!("pack integrity check failed: {e}"))?;

        debug!(
            pack_dir = %pack_dir.display(),
            head_commit = %head_commit,
            entry_count = index.entries.len(),
            "Pack candidate passed all checks"
        );

        Ok(BasePack {
            path: pack_dir.to_path_buf(),
            index,
            head_commit,
        })
    }

    /// Lists all `pack-*` subdirectories inside `storage_dir`.
    fn list_pack_dirs(storage_dir: &Path) -> Result<Vec<PathBuf>> {
        let mut dirs = Vec::new();
        for entry in fs::read_dir(storage_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("pack-") && entry.file_type()?.is_dir() {
                dirs.push(entry.path());
            }
        }
        Ok(dirs)
    }
}

/// Executes an incremental scan by detecting changes against a base pack,
/// reprocessing only changed files, and merging with carried-forward entries.
pub struct IncrementalExecutor {
    symbol_registry: SymbolExtractorRegistry,
    graph_builder: GraphBuilder,
}

impl IncrementalExecutor {
    pub fn new() -> Self {
        Self {
            symbol_registry: SymbolExtractorRegistry::new(),
            graph_builder: GraphBuilder::new(),
        }
    }

    /// Execute an incremental scan using the given plan (which must be Incremental mode).
    ///
    /// Steps:
    /// 1. Detect git changes since the base pack's head commit
    /// 2. Build a base map from base pack entries
    /// 3. Remove deletions from the base map
    /// 4. Reprocess changed files (parallel via rayon)
    /// 5. Merge carried-forward + reprocessed entries, sorted by path
    /// 6. Write merged pack with hardlinks for carried-forward content
    /// 7. Build and write the dependency graph
    /// 8. Update session
    /// 9. Return ScanResult
    ///
    /// Known limitation (accepted, self-healing): discovery-set changes that do
    /// NOT alter `scan_config_hash` — e.g. the gitignore-aligned subtree pruning
    /// from commit 1e9b576 — are not detected here. A base pack created before
    /// such a change still validates (the config-hash gate matches), and
    /// deletions are derived from the git changeset, so a file that was wrongly
    /// included under the old discovery rules is carried forward until the next
    /// FULL scan. Hard invalidation via `scan_config_hash` was deliberately
    /// rejected (that value is a Java-parity contract pinned in fixtures + the
    /// pack_contract snapshot); a RepoLens-native discovery-semantics-version
    /// signal is the tracked follow-up.
    pub fn execute(&self, root: &Path, plan: &ScanPlan, inputs: &ScanInputs) -> Result<ScanResult> {
        let start = Instant::now();
        let base = plan
            .base_pack
            .as_ref()
            .expect("incremental plan must have base_pack");

        info!(
            root = %root.display(),
            base_pack = %base.path.display(),
            base_commit = %base.head_commit,
            "Starting incremental scan"
        );

        // 1. Detect changes
        let detector = git::GitChangeDetector::new(root, &base.head_commit);
        let mut changeset = detector.detect()?;

        // Filter out .repolens/ paths — these are internal artifacts, matching
        // the prune-at-walk-level behavior in FileDiscovery.
        changeset.changed.retain(|p| !p.starts_with(".repolens/"));
        changeset.deleted.retain(|p| !p.starts_with(".repolens/"));

        // 1b. The base is the newest *clean* pack, so a dirty pack written by
        //     the previous scan is skipped. Git only reports changes against
        //     the base commit, which misses anything the previous scan saw
        //     that has since been undone (edit then revert, `git mv` then
        //     move back). Re-read every path whose entry differs between the
        //     previous pack and the base so the result and counts reflect
        //     what actually changed since the last scan.
        let previous_state = Self::add_previous_pack_drift(root, base, &mut changeset);

        info!(
            changed = changeset.changed.len(),
            deleted = changeset.deleted.len(),
            "Git change detection complete"
        );

        // 2. Build base map from base pack entries
        let mut base_map: HashMap<String, FileEntry> = HashMap::new();
        for entry in &base.index.entries {
            base_map.insert(entry.path.clone(), entry.clone());
        }
        debug!(base_entries = base_map.len(), "Base map built");

        // 3. Remove deletions — only count files that were actually in the base pack,
        //    since git may report deletions for files that were never included.
        let mut deleted_count = 0i32;
        for deleted_path in &changeset.deleted {
            if base_map.remove(deleted_path).is_some() {
                deleted_count += 1;
                debug!(path = deleted_path, "Removed deleted entry from base map");
            }
        }

        // 4. Reprocess changed files (parallel via rayon).
        //    Budget covers only the new/changed content this scan materializes;
        //    carried-forward entries are hardlinked and do not consume it.
        let rules = RulesEngine::load(root, &inputs.reposnapshot_files)?;
        let mut remaining_total_bytes = inputs.max_total_bytes;
        let mut budget_exclusions: HashMap<String, String> = HashMap::new();
        for rel_path in &changeset.changed {
            if let Some(reason) = Self::precheck_changed_entry(
                root,
                rel_path,
                inputs,
                &rules,
                &mut remaining_total_bytes,
            )? {
                budget_exclusions.insert(rel_path.clone(), reason.to_string());
            }
        }

        let reprocessed: Vec<FileEntry> = changeset
            .changed
            .par_iter()
            .map(|rel_path| {
                if let Some(reason) = budget_exclusions.get(rel_path) {
                    return Ok(Self::build_excluded_entry(rel_path, reason));
                }
                self.reprocess_entry(root, rel_path, inputs, &rules)
            })
            .collect::<Result<Vec<_>>>()?;

        // Count only files whose result differs from what the previous scan
        // recorded (#40). Files re-read only because the base pack is older
        // than the previous scan (they were already in their current state
        // last time) are not "changed since the previous scan".
        let reprocessed_count = {
            let last_seen = |path: &str| -> Option<(bool, &str)> {
                match &previous_state {
                    Some(prev) => prev.get(path).map(|(inc, sha)| (*inc, sha.as_str())),
                    None => base_map.get(path).map(|e| (e.included, e.sha256.as_str())),
                }
            };
            reprocessed
                .iter()
                .filter(|e| last_seen(&e.path) != Some((e.included, e.sha256.as_str())))
                .count() as i32
        };

        // Also remove changed paths from base_map (they will be replaced by reprocessed entries)
        for entry in &reprocessed {
            base_map.remove(&entry.path);
        }

        // 5. Merge entries: carried-forward (remaining base map) + reprocessed, sorted by path
        let carried_paths: HashSet<String> = base_map.keys().cloned().collect();

        let mut merged: Vec<FileEntry> = base_map.into_values().collect();
        merged.extend(reprocessed);
        merged.sort_by(|a, b| a.path.cmp(&b.path));

        // Every file not counted as reprocessed is unchanged since the
        // previous scan, including files re-read only because the base pack
        // is older, so "reprocessed + carried forward" is the total (#40).
        let carried_forward_count = merged.len() as i32 - reprocessed_count;

        // When the previous scan's pack is not the base, count deletions
        // against what the previous scan saw, so the count matches what the
        // user observed since their last scan.
        if let Some(previous_state) = &previous_state {
            let merged_paths: HashSet<&str> = merged.iter().map(|e| e.path.as_str()).collect();
            deleted_count = previous_state
                .keys()
                .filter(|p| !merged_paths.contains(p.as_str()))
                .count() as i32;
        }

        let included_files = merged.iter().filter(|e| e.included).count();
        let excluded_files = merged.len() - included_files;
        info!(
            total = merged.len(),
            included = included_files,
            excluded = excluded_files,
            reprocessed = reprocessed_count,
            deleted = deleted_count,
            carried_forward = carried_forward_count,
            "Entry merge complete"
        );

        // 6. Write merged pack
        let head_commit = repolens_core::git::head_commit(root).unwrap_or_default();
        let worktree_dirty = repolens_core::git::is_worktree_dirty(root);

        let now = OffsetDateTime::now_utc();
        let pack_dir = RepoStoragePaths::create_pack_dir(root, now)?;

        let base_pack_id = base
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let write_options = PackWriteOptions {
            scan_mode: "incremental",
            head_commit: &head_commit,
            worktree_dirty,
            scan_config_hash: &inputs.config_hash,
            base_pack_id: Some(&base_pack_id),
            base_head_commit: Some(&base.head_commit),
            reprocessed_files: reprocessed_count,
            deleted_files: Some(deleted_count),
            carried_forward_files: Some(carried_forward_count),
        };

        let merged_writer = MergedPackWriter::new(&pack_dir);
        merged_writer.write(&merged, &base.path, root, &carried_paths, &write_options)?;
        info!(pack_dir = %pack_dir.display(), "Merged pack written");

        // 7. Build and write graph
        let graph_result = self.graph_builder.build(&merged, &pack_dir, Some(root));
        PackWriter::new(&pack_dir, root).write_graph(&graph_result.graph, &graph_result.metrics)?;
        info!(
            edges = graph_result.graph.edges.len(),
            resolved = graph_result.metrics.resolved_edges,
            "Graph written"
        );

        // 7.5. Generate and write profile
        generate_and_write_profile(
            &merged,
            &graph_result.graph,
            &pack_dir,
            &PackWriter::new(&pack_dir, root),
        )?;

        // 7.6. Post-scan wiki lint
        let merged_count = merged.len();
        let pack_index = PackIndex::from_entries(merged);
        crate::scan::post_scan_wiki_lint(root, &pack_index);

        // 8. Update session
        let existing = SessionStore::load(root)?;
        let session_id = existing
            .map(|s| s.session_id)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let session = RepoSession {
            schema_version: "1.0".to_string(),
            session_id,
            repo_root: root.to_string_lossy().to_string(),
            active_pack_path: pack_dir.to_string_lossy().to_string(),
            active_report_path: None,
            updated_at: now,
            last_accessed: now,
            last_scan_mode: "incremental".to_string(),
            reprocessed_files: Some(reprocessed_count),
            deleted_files: Some(deleted_count),
            carried_forward_files: Some(carried_forward_count),
        };
        SessionStore::save(root, &session)?;
        info!(session_id = %session.session_id, "Session updated");

        // 9. Return ScanResult
        let duration = start.elapsed();
        info!(
            duration_ms = duration.as_millis(),
            "Incremental scan complete"
        );

        Ok(ScanResult {
            pack_dir,
            scan_mode: "incremental".to_string(),
            total_files: merged_count,
            included_files,
            excluded_files,
            reprocessed_files: reprocessed_count,
            deleted_files: Some(deleted_count),
            carried_forward_files: Some(carried_forward_count),
            duration,
            fell_back_reason: None,
        })
    }

    /// Adds paths whose entry in the previous scan's pack (the session's
    /// active pack) differs from the base pack. Paths present on disk are
    /// queued for reprocessing; paths in the base but gone from disk are
    /// queued as deleted. Returns the previous pack's `(included, sha256)`
    /// per path when it was compared, so reprocessed and deleted files can
    /// be counted against the previous scan.
    fn add_previous_pack_drift(
        root: &Path,
        base: &BasePack,
        changeset: &mut git::ChangeSet,
    ) -> Option<HashMap<String, (bool, String)>> {
        let previous_path = match SessionStore::load(root) {
            Ok(Some(session)) => PathBuf::from(session.active_pack_path),
            Ok(None) => return None,
            Err(e) => {
                warn!(err = %e, "Cannot read session, skipping previous-pack comparison");
                return None;
            }
        };
        if previous_path.file_name() == base.path.file_name() || !previous_path.is_dir() {
            return None;
        }
        let previous = match PackReader::read(&previous_path) {
            Ok(index) => index,
            Err(e) => {
                warn!(
                    pack_dir = %previous_path.display(),
                    err = %e,
                    "Cannot read previous pack, skipping previous-pack comparison"
                );
                return None;
            }
        };

        let base_entries: HashMap<&str, &FileEntry> = base
            .index
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e))
            .collect();
        let previous_entries: HashMap<&str, &FileEntry> = previous
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e))
            .collect();
        let same = |a: &FileEntry, b: &FileEntry| a.included == b.included && a.sha256 == b.sha256;

        let mut drift: Vec<&str> = previous_entries
            .iter()
            .filter(|(path, prev)| !base_entries.get(*path).is_some_and(|b| same(b, prev)))
            .map(|(path, _)| *path)
            .chain(
                base_entries
                    .keys()
                    .filter(|path| !previous_entries.contains_key(*path))
                    .copied(),
            )
            .filter(|path| !path.starts_with(".repolens/"))
            .collect();
        drift.sort_unstable();

        let known: HashSet<String> = changeset
            .changed
            .iter()
            .chain(changeset.deleted.iter())
            .cloned()
            .collect();
        for path in drift {
            if known.contains(path) {
                continue;
            }
            if root.join(path).symlink_metadata().is_ok() {
                changeset.changed.push(path.to_string());
            } else if base_entries.contains_key(path) {
                changeset.deleted.push(path.to_string());
            }
        }
        changeset.changed.sort();
        changeset.deleted.sort();
        debug!(
            previous_pack = %previous_path.display(),
            changed = changeset.changed.len(),
            deleted = changeset.deleted.len(),
            "Previous-pack drift applied"
        );
        Some(
            previous_entries
                .iter()
                .map(|(p, e)| (p.to_string(), (e.included, e.sha256.clone())))
                .collect(),
        )
    }

    /// Reprocess a single changed file path, producing a FileEntry.
    fn reprocess_entry(
        &self,
        root: &Path,
        rel_path: &str,
        inputs: &ScanInputs,
        rules: &RulesEngine,
    ) -> Result<FileEntry> {
        let abs_path = root.join(rel_path);

        // Check symlink status first; if the file doesn't exist, symlink_metadata
        // will fail and we treat it as deleted (avoids TOCTOU with a separate exists() check).
        let sym_meta = match abs_path.symlink_metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %rel_path, "Changed file no longer exists on disk, marking excluded");
                return Ok(Self::build_excluded_entry(rel_path, "excluded:deleted"));
            }
            Err(e) => return Err(e.into()),
        };

        // Keep symlink handling consistent with full scans: either symlinks are
        // disallowed entirely, or they must still resolve inside the repo root.
        if sym_meta.file_type().is_symlink()
            && (!inputs.follow_symlinks || canonicalize_within_base(root, &abs_path).is_err())
        {
            debug!(path = %rel_path, "Symlink excluded");
            return Ok(Self::build_excluded_entry(rel_path, "excluded:symlink"));
        }

        // Evaluate via RulesEngine
        let decision = rules.evaluate(&abs_path, false);
        if !decision.included {
            debug!(path = %rel_path, reason = %decision.reason, "File excluded by rules");
            return Ok(Self::build_excluded_entry(rel_path, &decision.reason));
        }

        // Check if binary
        if metadata::is_binary(&abs_path)? && !inputs.include_binaries {
            debug!(path = %rel_path, "Binary file excluded");
            return Ok(Self::build_excluded_entry(rel_path, "excluded:binary"));
        }

        // Size budget is enforced in `precheck_changed_entry` before this
        // function runs; do not re-stat here.

        // Included: compute metadata and extract symbols
        let (sha256, loc) = metadata::sha256_and_line_count(&abs_path)?;
        let language = lang::detect(Path::new(rel_path)).to_string();

        let symbols: Vec<Symbol> = self
            .symbol_registry
            .for_language(&language)
            .map(|ext| ext.extract(&abs_path))
            .unwrap_or_default();

        let content_path = Some(format!("files/{}", rel_path));

        debug!(path = %rel_path, language = %language, loc, symbols = symbols.len(), "Reprocessed included entry");

        Ok(FileEntry {
            path: rel_path.to_string(),
            sha256,
            language,
            loc,
            included: true,
            reason: "included".to_string(),
            content_path,
            symbols,
        })
    }

    fn build_excluded_entry(rel_path: &str, reason: &str) -> FileEntry {
        FileEntry {
            path: rel_path.to_string(),
            sha256: String::new(),
            language: String::new(),
            loc: 0,
            included: false,
            reason: reason.to_string(),
            content_path: None,
            symbols: vec![],
        }
    }

    fn precheck_changed_entry(
        root: &Path,
        rel_path: &str,
        inputs: &ScanInputs,
        rules: &RulesEngine,
        remaining_total_bytes: &mut i64,
    ) -> Result<Option<&'static str>> {
        let abs_path = root.join(rel_path);
        let sym_meta = match abs_path.symlink_metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        if sym_meta.file_type().is_symlink()
            && (!inputs.follow_symlinks || canonicalize_within_base(root, &abs_path).is_err())
        {
            return Ok(None);
        }

        let decision = rules.evaluate(&abs_path, false);
        if !decision.included {
            return Ok(None);
        }

        if metadata::is_binary(&abs_path)? && !inputs.include_binaries {
            return Ok(None);
        }

        crate::scan_budget::changed_file_budget_reason(
            &abs_path,
            remaining_total_bytes,
            inputs.max_file_bytes,
        )
    }
}

impl Default for IncrementalExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::manifest::Manifest;
    use repolens_core::serde_util::java_epoch_seconds::JavaEpochSeconds;
    use rust_decimal::Decimal;
    use std::fs;
    use std::process::Command;
    use std::str::FromStr;

    fn init_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .expect("git config name");
    }

    fn git_add_commit(dir: &Path, message: &str) -> String {
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(dir)
            .output()
            .expect("git commit");
        // Return HEAD SHA
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .expect("git rev-parse HEAD");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn write_valid_pack(pack_dir: &Path, head_commit: &str, config_hash: &str) {
        fs::create_dir_all(pack_dir).unwrap();

        let manifest = Manifest {
            schema_version: "1.3.0".to_string(),
            schema_url: "https://repolens.dev/schema/v1.3.0".to_string(),
            timestamp: JavaEpochSeconds(Decimal::from_str("1774753241.321919000").unwrap()),
            chunks: vec!["files-0001.json".to_string()],
            scan_mode: Some("FULL".to_string()),
            scan_scope: Some("complete".to_string()),
            base_pack_id: None,
            base_head_commit: None,
            head_commit: Some(head_commit.to_string()),
            worktree_dirty: Some(false),
            scan_config_hash: Some(config_hash.to_string()),
            reprocessed_files: None,
            deleted_files: None,
            carried_forward_files: None,
        };

        let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(pack_dir.join("manifest.json"), manifest_json).unwrap();
        fs::write(pack_dir.join("files-0001.json"), "[]").unwrap();
    }

    #[test]
    fn resolve_returns_full_when_no_storage_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        let plan = IncrementalResolver::resolve(root, "abc123");
        assert!(
            matches!(plan.mode, ScanMode::Full),
            "should be full scan when .repolens does not exist"
        );
        assert!(plan.base_pack.is_none());
    }

    #[test]
    fn resolve_returns_full_when_no_packs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".repolens")).unwrap();

        let plan = IncrementalResolver::resolve(root, "abc123");
        assert!(
            matches!(plan.mode, ScanMode::Full),
            "should be full scan when .repolens is empty"
        );
        assert!(plan.base_pack.is_none());
    }

    #[test]
    fn resolve_returns_full_on_config_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::write(root.join("hello.txt"), "hello").unwrap();
        let head = git_add_commit(root, "init");

        let pack_dir = root.join(".repolens").join("pack-20260101-000000");
        write_valid_pack(&pack_dir, &head, "hash-in-pack");

        let plan = IncrementalResolver::resolve(root, "different-hash");
        assert!(
            matches!(plan.mode, ScanMode::Full),
            "should be full scan when config hash does not match"
        );
        assert!(plan.base_pack.is_none());
    }

    #[test]
    fn resolve_returns_full_on_dirty_worktree_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::write(root.join("hello.txt"), "hello").unwrap();
        let head = git_add_commit(root, "init");

        let pack_dir = root.join(".repolens").join("pack-20260101-000000");
        fs::create_dir_all(&pack_dir).unwrap();

        // Write a manifest with worktree_dirty = true
        let manifest = Manifest {
            schema_version: "1.3.0".to_string(),
            schema_url: "https://repolens.dev/schema/v1.3.0".to_string(),
            timestamp: JavaEpochSeconds(Decimal::from_str("1774753241.321919000").unwrap()),
            chunks: vec!["files-0001.json".to_string()],
            scan_mode: Some("FULL".to_string()),
            scan_scope: Some("complete".to_string()),
            base_pack_id: None,
            base_head_commit: None,
            head_commit: Some(head.clone()),
            worktree_dirty: Some(true),
            scan_config_hash: Some("myhash".to_string()),
            reprocessed_files: None,
            deleted_files: None,
            carried_forward_files: None,
        };
        let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(pack_dir.join("manifest.json"), manifest_json).unwrap();
        fs::write(pack_dir.join("files-0001.json"), "[]").unwrap();

        let plan = IncrementalResolver::resolve(root, "myhash");
        assert!(
            matches!(plan.mode, ScanMode::Full),
            "should be full scan when pack was created from dirty worktree"
        );
        assert!(plan.base_pack.is_none());
    }

    #[test]
    fn resolve_returns_incremental_for_compatible_pack() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::write(root.join("hello.txt"), "hello").unwrap();
        let head = git_add_commit(root, "init");

        let pack_dir = root.join(".repolens").join("pack-20260101-000000");
        write_valid_pack(&pack_dir, &head, "myhash");

        let plan = IncrementalResolver::resolve(root, "myhash");
        assert!(
            matches!(plan.mode, ScanMode::Incremental),
            "should be incremental scan for a compatible pack"
        );
        let base = plan.base_pack.expect("should have base pack");
        assert_eq!(base.head_commit, head);
        assert_eq!(base.path, pack_dir);
    }

    #[test]
    fn resolve_picks_newest_pack() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        // First commit — used for the older pack with wrong hash
        fs::write(root.join("a.txt"), "a").unwrap();
        let first_head = git_add_commit(root, "first commit");

        // Second commit — used for the newer pack with correct hash
        fs::write(root.join("b.txt"), "b").unwrap();
        let second_head = git_add_commit(root, "second commit");

        // Older pack: wrong config hash
        let old_pack = root.join(".repolens").join("pack-20260101-000000");
        write_valid_pack(&old_pack, &first_head, "wrong-hash");

        // Newer pack: correct config hash, newer name sorts first
        let new_pack = root.join(".repolens").join("pack-20260102-000000");
        write_valid_pack(&new_pack, &second_head, "correct-hash");

        let plan = IncrementalResolver::resolve(root, "correct-hash");
        assert!(
            matches!(plan.mode, ScanMode::Incremental),
            "should pick the newer compatible pack"
        );
        let base = plan.base_pack.expect("should have base pack");
        assert_eq!(base.path, new_pack, "should pick the newer pack");
        assert_eq!(base.head_commit, second_head);
    }
}
