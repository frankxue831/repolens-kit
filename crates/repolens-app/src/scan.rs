use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use rayon::prelude::*;
use repolens_core::discovery::FileDiscovery;
use repolens_core::metadata;
use repolens_core::model::file_entry::FileEntry;
use repolens_core::model::session::RepoSession;
use repolens_core::pack_writer::PackWriter;
use repolens_core::session::SessionStore;
use repolens_core::storage::RepoStoragePaths;
use repolens_extract::lang;
use repolens_extract::registry::Extractors;
use repolens_extract::Relationship;
use std::collections::HashMap;

use crate::graph::GraphBuilder;
use crate::scan_inputs::ScanInputs;
use repolens_core::model::graph::FileGraph;
use repolens_core::pack_reader::PackIndex;
use repolens_profile::health::HealthAnalyzer;
use repolens_profile::profile::ProfileAnalyzer;
use time::OffsetDateTime;
use uuid::Uuid;

/// Generate and write `project-profile.json` from in-memory scan data.
///
/// Shared by full and incremental scan paths to avoid re-reading the pack
/// and graph from disk.
pub(crate) fn generate_and_write_profile(
    entries: &[FileEntry],
    graph: &FileGraph,
    pack_dir: &Path,
    writer: &PackWriter,
) -> Result<()> {
    let index = PackIndex::from_entries(entries.to_vec());
    let profile = ProfileAnalyzer::analyze(&index, pack_dir);
    let ghi = crate::graph_health::build_graph_health_input_in_pack(
        &index,
        &profile,
        graph,
        Some(pack_dir),
    );
    let mut health = HealthAnalyzer::analyze(&index, &profile, Some(&ghi));

    // Populate evidence-aware findings on the persisted HealthReport so
    // direct pack consumers see the calibrated view. Mirrors the
    // population done by SummaryService.
    let file_paths: Vec<String> = index
        .entries
        .iter()
        .filter(|e| e.included)
        .map(|e| e.path.clone())
        .collect();
    let derived_root = repolens_core::storage::RepoStoragePaths::repo_root_from_pack_dir(pack_dir);
    let manifest_deps: Vec<String> = derived_root
        .map(|root| {
            repolens_profile::repo_context::extract_cargo_dependency_names_recursive(
                root,
                &file_paths,
            )
        })
        .unwrap_or_default();
    let is_cargo_workspace = derived_root
        .map(repolens_profile::repo_context::cargo_root_is_workspace)
        .unwrap_or(false);
    let repo_context = repolens_profile::repo_context::RepoContextDetector::detect_full(
        &profile,
        &file_paths,
        &manifest_deps,
        is_cargo_workspace,
    );
    let scan_summary = crate::summary::compute_scan_summary_in_pack(&index, Some(pack_dir));
    let lockfile_state =
        derived_root.map(repolens_profile::lockfile::CargoLockStateDetector::detect);
    let pack_id = repolens_core::model::epistemic::pack_id_from_dir(pack_dir);
    let referenced = crate::references::referenced_files(&index, pack_dir);
    let evidence_findings = crate::evidence_findings::EvidenceFindingsBuilder {
        pack_id: &pack_id,
        commit: index.manifest.head_commit.as_deref(),
        profile: &profile,
        repo_context: &repo_context,
        graph: Some(graph),
        graph_health: Some(&ghi),
        scan: Some(&scan_summary),
        lockfile_state: lockfile_state.as_ref(),
        cargo_has_binary: derived_root
            .and_then(repolens_profile::repo_context::cargo_root_has_binary_target),
        referenced_files: Some(&referenced),
        dependency_risk: Some(&health.dependency_risk),
    }
    .build();
    health.evidence_findings = evidence_findings;

    writer.write_profile(&profile, Some(&health))?;
    info!(
        name = %profile.name,
        project_type = ?profile.project_type,
        "Profile written"
    );
    Ok(())
}

pub struct ScanResult {
    pub pack_dir: PathBuf,
    pub scan_mode: String,
    pub total_files: usize,
    pub included_files: usize,
    pub excluded_files: usize,
    pub reprocessed_files: i32,
    pub deleted_files: Option<i32>,
    pub carried_forward_files: Option<i32>,
    pub duration: Duration,
    /// Set when this full scan happened because no earlier pack could be
    /// used as an incremental base; holds the resolver's reason.
    pub fell_back_reason: Option<String>,
}

pub struct ScanService {
    extractors: Extractors,
    graph_builder: GraphBuilder,
}

impl ScanService {
    pub fn new() -> Self {
        Self {
            extractors: Extractors::new(),
            graph_builder: GraphBuilder::new(),
        }
    }

    pub fn run(&self, root: &Path, force_full: bool, keep: usize) -> Result<ScanResult> {
        let start = Instant::now();
        let inputs = ScanInputs::resolve(root)?;

        let result = if force_full || !RepoStoragePaths::is_git_repo(root) {
            let reason = if force_full {
                "--full"
            } else {
                "not a git repo"
            };
            info!(root = %root.display(), reason, "Forcing full scan");
            self.run_full(root, &inputs, start)?
        } else {
            use crate::incremental::{IncrementalExecutor, IncrementalResolver, ScanMode};

            let plan = IncrementalResolver::resolve(root, &inputs.config_hash);
            info!(
                root = %root.display(),
                mode = ?plan.mode,
                reason = %plan.reason,
                "Scan plan resolved"
            );

            match plan.mode {
                ScanMode::Full => {
                    let mut r = self.run_full(root, &inputs, start)?;
                    if plan.fell_back {
                        r.fell_back_reason = Some(plan.reason.clone());
                    }
                    r
                }
                ScanMode::Incremental => {
                    let executor = IncrementalExecutor::new();
                    executor.execute(root, &plan, &inputs)?
                }
            }
        };

        // Best-effort cleanup after successful scan
        match crate::cleanup::cleanup_old_packs(root, keep) {
            Ok(cr) if !cr.deleted.is_empty() => {
                info!(
                    deleted = cr.deleted.len(),
                    failed = cr.failed.len(),
                    kept = cr.kept,
                    "Pack cleanup complete"
                );
            }
            Ok(_) => {} // nothing to clean
            Err(e) => {
                warn!(error = %e, "Pack cleanup failed (scan succeeded)");
            }
        }

        Ok(result)
    }

    fn run_full(&self, root: &Path, inputs: &ScanInputs, start: Instant) -> Result<ScanResult> {
        info!(root = %root.display(), "Starting full scan");

        // 1. Discover files
        let phase = Instant::now();
        let records = FileDiscovery::new(root)
            .follow_symlinks(inputs.follow_symlinks)
            .include_binaries(inputs.include_binaries)
            .reposnapshot_files(inputs.reposnapshot_files.clone())
            .discover()?;
        let records = crate::scan_budget::apply_full_scan_budgets(
            &records,
            inputs.max_file_bytes,
            inputs.max_total_bytes,
        )?;
        info!(
            total = records.len(),
            phase_ms = phase.elapsed().as_millis(),
            "File discovery complete"
        );

        // 2. Build FileEntry values (parallel via rayon). Each included entry is
        //    parsed ONCE, yielding both its symbols (stored on the entry) and its
        //    relationships (carried in memory, not serialized).
        let phase = Instant::now();
        let materialized: Vec<(FileEntry, Option<Vec<Relationship>>)> = records
            .par_iter()
            .map(|record| {
                if record.included {
                    self.build_included_entry(record)
                } else {
                    Ok((self.build_excluded_entry(record), None))
                }
            })
            .collect::<Result<Vec<_>>>()?;

        // Split into entries (written to the pack) and an in-memory relationships
        // map handed to graph build so it need not re-read+re-parse every file.
        // Only fused (tree-sitter) entries carry `Some(rels)`; Python/unknown
        // carry `None` and graph build extracts their relationships from the
        // packed copy itself (the fallback path). `rels_by_path` is never
        // serialized, so pack output is unchanged.
        let mut entries: Vec<FileEntry> = Vec::with_capacity(materialized.len());
        let mut rels_by_path: HashMap<String, Vec<Relationship>> = HashMap::new();
        for (entry, rels) in materialized {
            if let Some(rels) = rels {
                rels_by_path.insert(entry.path.clone(), rels);
            }
            entries.push(entry);
        }

        let included_files = entries.iter().filter(|e| e.included).count();
        let excluded_files = entries.len() - included_files;
        info!(
            included = included_files,
            excluded = excluded_files,
            phase_ms = phase.elapsed().as_millis(),
            "Entry materialization complete"
        );

        // 3. Git provenance
        let head_commit = repolens_core::git::head_commit(root).unwrap_or_default();
        let worktree_dirty = repolens_core::git::is_worktree_dirty(root);
        info!(head = %head_commit, dirty = worktree_dirty, "Git provenance resolved");

        // 4. Write pack
        let phase = Instant::now();
        let now = OffsetDateTime::now_utc();
        let pack_dir = RepoStoragePaths::create_pack_dir(root, now)?;
        let writer = PackWriter::new(&pack_dir, root);
        let write_options = repolens_core::pack_writer::PackWriteOptions {
            scan_mode: "full",
            head_commit: &head_commit,
            worktree_dirty,
            scan_config_hash: &inputs.config_hash,
            base_pack_id: None,
            base_head_commit: None,
            reprocessed_files: entries.len() as i32,
            deleted_files: None,
            carried_forward_files: None,
        };
        writer.write(&entries, &write_options)?;
        info!(
            pack_dir = %pack_dir.display(),
            phase_ms = phase.elapsed().as_millis(),
            "Pack written"
        );

        // 4.5. Build and write graph
        let phase = Instant::now();
        let graph_result = self.graph_builder.build_with_relationships(
            &entries,
            &pack_dir,
            Some(root),
            Some(&rels_by_path),
        );
        writer.write_graph(&graph_result.graph, &graph_result.metrics)?;
        info!(
            edges = graph_result.graph.edges.len(),
            resolved = graph_result.metrics.resolved_edges,
            phase_ms = phase.elapsed().as_millis(),
            "Graph written"
        );

        // 4.6. Generate and write profile
        let phase = Instant::now();
        generate_and_write_profile(&entries, &graph_result.graph, &pack_dir, &writer)?;
        info!(phase_ms = phase.elapsed().as_millis(), "Profile generated");

        // 4.7. Post-scan wiki lint
        let entry_count = entries.len();
        let pack_index = PackIndex::from_entries(entries);
        post_scan_wiki_lint(root, &pack_index);

        // 5. Update session
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
            last_scan_mode: "full".to_string(),
            reprocessed_files: Some(entry_count as i32),
            deleted_files: None,
            carried_forward_files: None,
        };
        SessionStore::save(root, &session)?;
        info!(session_id = %session.session_id, "Session updated");

        let duration = start.elapsed();
        info!(duration_ms = duration.as_millis(), "Full scan complete");

        Ok(ScanResult {
            pack_dir,
            scan_mode: "full".to_string(),
            total_files: entry_count,
            included_files,
            excluded_files,
            reprocessed_files: entry_count as i32,
            deleted_files: None,
            carried_forward_files: None,
            duration,
            fell_back_reason: None,
        })
    }

    fn build_included_entry(
        &self,
        record: &repolens_core::discovery::FileRecord,
    ) -> Result<(FileEntry, Option<Vec<Relationship>>)> {
        let (sha256, loc) = metadata::sha256_and_line_count(&record.absolute_path)?;
        let language = lang::detect(Path::new(&record.relative_path)).to_string();

        // Parse once: symbols are stored on the entry; relationships (for a
        // tree-sitter language) are returned for the in-memory hand-off to graph
        // build. `None` => non-tree-sitter / parse failure, and graph build
        // extracts relationships from the packed copy itself.
        let (symbols, relationships) = self
            .extractors
            .extract_symbols_and_relationships(&record.absolute_path, &language);

        let content_path = Some(format!("files/{}", record.relative_path));

        Ok((
            FileEntry {
                path: record.relative_path.clone(),
                sha256,
                language,
                loc,
                included: true,
                reason: "included".to_string(),
                content_path,
                symbols,
            },
            relationships,
        ))
    }

    fn build_excluded_entry(&self, record: &repolens_core::discovery::FileRecord) -> FileEntry {
        FileEntry {
            path: record.relative_path.clone(),
            sha256: String::new(),
            language: String::new(),
            loc: 0,
            included: false,
            reason: record.reason.clone(),
            content_path: None,
            symbols: vec![],
        }
    }
}

impl Default for ScanService {
    fn default() -> Self {
        Self::new()
    }
}

/// Run wiki lint after a scan if a wiki exists. Non-blocking: logs warnings
/// on failure but never fails the scan.
pub(crate) fn post_scan_wiki_lint(root: &Path, pack: &PackIndex) {
    let wiki_dir = root.join(".repolens").join("wiki");
    if !wiki_dir.exists() {
        return;
    }

    let mut index = match repolens_wiki::store::WikiStore::load_index(root) {
        Ok(idx) => idx,
        Err(e) => {
            warn!(error = %e, "Post-scan wiki lint: failed to load wiki index");
            return;
        }
    };

    if index.articles.is_empty() {
        return;
    }

    let newly_stale = repolens_wiki::staleness::flag_stale_articles(&mut index, Some(pack));

    let coverage = repolens_wiki::coverage::compute_coverage(&index, pack);

    index.updated_at = time::OffsetDateTime::now_utc();
    if let Err(e) = repolens_wiki::store::WikiStore::save_index(root, &index) {
        warn!(error = %e, "Post-scan wiki lint: failed to save wiki index");
        return;
    }

    let log_entry = repolens_core::model::wiki::WikiLogEntry {
        timestamp: time::OffsetDateTime::now_utc(),
        event: repolens_core::model::wiki::WikiEventKind::Lint,
        article_id: None,
        detail: format!(
            "post-scan: stale={}, coverage={:.1}%",
            newly_stale.len(),
            coverage.coverage_percent
        ),
    };
    let _ = repolens_wiki::store::WikiStore::append_log(root, log_entry);

    info!(
        stale = newly_stale.len(),
        coverage_pct = format!("{:.1}", coverage.coverage_percent),
        "Post-scan wiki lint complete"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_produces_pack_and_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/App.java"), "package com.example;\n\npublic class App {\n    public static void main(String[] args) {}\n}\n").unwrap();
        fs::write(root.join("README.md"), "# Hello").unwrap();

        let result = ScanService::new().run(root, false, 100).unwrap();

        assert!(result.pack_dir.exists());
        assert!(result.pack_dir.join("manifest.json").exists());

        let chunks: Vec<_> = fs::read_dir(&result.pack_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("files-"))
            .collect();
        assert!(!chunks.is_empty(), "should have at least one chunk file");

        assert_eq!(result.included_files, 2);
        assert_eq!(result.total_files, 2);
        assert_eq!(result.scan_mode, "full");

        let session_path = root.join(".repolens/session.json");
        assert!(session_path.exists());
        let session: RepoSession =
            serde_json::from_str(&fs::read_to_string(&session_path).unwrap()).unwrap();
        assert_eq!(session.last_scan_mode, "full");
        assert_eq!(session.reprocessed_files, Some(2));

        assert!(result.pack_dir.join("files/src/App.java").exists());
        assert!(result.pack_dir.join("files/README.md").exists());
    }

    #[test]
    fn scan_excluded_entries_have_empty_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        fs::write(root.join("app.log"), "some log data").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();

        let result = ScanService::new().run(root, false, 100).unwrap();

        let chunk_path = result.pack_dir.join("files-0001.json");
        let entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&chunk_path).unwrap()).unwrap();

        let log_entry = entries.iter().find(|e| e.path == "app.log").unwrap();
        assert!(!log_entry.included);
        assert_eq!(log_entry.sha256, "");
        assert_eq!(log_entry.language, "");
        assert_eq!(log_entry.loc, 0);
        assert!(log_entry.symbols.is_empty());
        assert!(log_entry.content_path.is_none());

        let rs_entry = entries.iter().find(|e| e.path == "main.rs").unwrap();
        assert!(rs_entry.included);
        assert!(!rs_entry.sha256.is_empty());
        assert_eq!(rs_entry.language, "Rust");
    }

    #[test]
    fn rescan_does_not_include_repolens_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("hello.txt"), "hello").unwrap();

        let r1 = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(r1.included_files, 1);

        let r2 = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(
            r2.included_files, 1,
            "second scan should not include pack artifacts"
        );

        let chunk_path = r2.pack_dir.join("files-0001.json");
        let entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&chunk_path).unwrap()).unwrap();
        // .repolens/ directory is now pruned at walk level — no entries should appear
        let repolens_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.path.starts_with(".repolens/"))
            .collect();
        assert!(
            repolens_entries.is_empty(),
            "expected .repolens/ to be pruned at walk level, but found {} entries",
            repolens_entries.len()
        );
    }

    #[test]
    fn full_scan_excludes_file_larger_than_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("small.txt"), "ok").unwrap();
        fs::write(root.join("large.txt"), "x".repeat(32)).unwrap();

        let inputs = ScanInputs {
            follow_symlinks: false,
            include_binaries: false,
            max_file_bytes: 8,
            max_total_bytes: 64,
            reposnapshot_files: vec![],
            reposnapshot_contents: vec![],
            config_hash: "test".to_string(),
        };

        let result = ScanService::new()
            .run_full(root, &inputs, Instant::now())
            .unwrap();
        let chunk_path = result.pack_dir.join("files-0001.json");
        let entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&chunk_path).unwrap()).unwrap();

        let large = entries.iter().find(|e| e.path == "large.txt").unwrap();
        assert!(!large.included);
        assert_eq!(large.reason, "excluded:max_file_bytes");

        let small = entries.iter().find(|e| e.path == "small.txt").unwrap();
        assert!(small.included);
    }

    #[test]
    fn full_scan_excludes_files_after_total_budget_is_exhausted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.txt"), "12345").unwrap();
        fs::write(root.join("b.txt"), "67890").unwrap();

        let inputs = ScanInputs {
            follow_symlinks: false,
            include_binaries: false,
            max_file_bytes: 16,
            max_total_bytes: 5,
            reposnapshot_files: vec![],
            reposnapshot_contents: vec![],
            config_hash: "test".to_string(),
        };

        let result = ScanService::new()
            .run_full(root, &inputs, Instant::now())
            .unwrap();
        let chunk_path = result.pack_dir.join("files-0001.json");
        let entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&chunk_path).unwrap()).unwrap();

        let a = entries.iter().find(|e| e.path == "a.txt").unwrap();
        let b = entries.iter().find(|e| e.path == "b.txt").unwrap();

        assert!(a.included, "first file should fit within total budget");
        assert!(
            !b.included,
            "later files should be excluded once total budget is spent"
        );
        assert_eq!(b.reason, "excluded:max_total_bytes");
    }

    #[test]
    fn session_id_preserved_across_rescans() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("hello.txt"), "hello").unwrap();

        ScanService::new().run(root, false, 100).unwrap();
        let s1: RepoSession =
            serde_json::from_str(&fs::read_to_string(root.join(".repolens/session.json")).unwrap())
                .unwrap();

        // Sleep to ensure pack dir timestamp differs (second-precision)
        std::thread::sleep(std::time::Duration::from_secs(1));

        ScanService::new().run(root, false, 100).unwrap();
        let s2: RepoSession =
            serde_json::from_str(&fs::read_to_string(root.join(".repolens/session.json")).unwrap())
                .unwrap();

        assert_eq!(s1.session_id, s2.session_id);
        assert_ne!(s1.active_pack_path, s2.active_pack_path);
    }

    #[test]
    fn scan_java_small_fixture() {
        let fixture_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/java-small");

        if !fixture_root.exists() {
            eprintln!("Skipping: fixture not found at {}", fixture_root.display());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("java-small");
        copy_dir_recursive(&fixture_root, &root);

        let result = ScanService::new().run(&root, false, 100).unwrap();

        let golden_chunk_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scenarios/java-small/pack/files-0001.json");
        let golden_entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&golden_chunk_path).unwrap()).unwrap();

        let our_chunk_path = result.pack_dir.join("files-0001.json");
        let our_entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&our_chunk_path).unwrap()).unwrap();

        let golden_included: Vec<_> = golden_entries.iter().filter(|e| e.included).collect();
        let our_included: Vec<_> = our_entries.iter().filter(|e| e.included).collect();

        assert_eq!(
            golden_included.len(),
            our_included.len(),
            "included file count mismatch"
        );

        for golden in &golden_included {
            let ours = our_included
                .iter()
                .find(|e| e.path == golden.path)
                .unwrap_or_else(|| panic!("missing included entry: {}", golden.path));

            assert_eq!(
                ours.sha256, golden.sha256,
                "sha256 mismatch for {}",
                golden.path
            );
            assert_eq!(
                ours.language, golden.language,
                "language mismatch for {}",
                golden.path
            );
            assert_eq!(ours.loc, golden.loc, "loc mismatch for {}", golden.path);
            assert_eq!(
                ours.symbols.len(),
                golden.symbols.len(),
                "symbol count mismatch for {}",
                golden.path
            );

            for (os, gs) in ours.symbols.iter().zip(golden.symbols.iter()) {
                assert_eq!(
                    os.symbol_type, gs.symbol_type,
                    "symbol type mismatch in {}",
                    golden.path
                );
                assert_eq!(os.name, gs.name, "symbol name mismatch in {}", golden.path);
                assert_eq!(
                    os.qualified_name, gs.qualified_name,
                    "qualified_name mismatch for {} in {}",
                    gs.name, golden.path
                );
                assert_eq!(
                    os.visibility, gs.visibility,
                    "visibility mismatch for {} in {}",
                    gs.name, golden.path
                );
            }
        }

        for e in our_entries.iter().filter(|e| !e.included) {
            assert_eq!(
                e.sha256, "",
                "excluded entry {} should have empty sha256",
                e.path
            );
            assert_eq!(
                e.language, "",
                "excluded entry {} should have empty language",
                e.path
            );
            assert_eq!(e.loc, 0, "excluded entry {} should have loc=0", e.path);
            assert!(
                e.symbols.is_empty(),
                "excluded entry {} should have no symbols",
                e.path
            );
            assert!(
                e.content_path.is_none(),
                "excluded entry {} should have no content_path",
                e.path
            );
        }
    }

    #[test]
    fn scan_produces_graph_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
        fs::write(
            root.join("src/main/java/com/example/App.java"),
            "package com.example;\n\nimport com.example.Helper;\n\npublic class App {\n    public void run() {\n        Helper helper = new Helper();\n        helper.greet();\n    }\n}\n",
        ).unwrap();
        fs::write(
            root.join("src/main/java/com/example/Helper.java"),
            "package com.example;\n\npublic class Helper {\n    public String greet() {\n        return \"hello\";\n    }\n}\n",
        ).unwrap();

        let result = ScanService::new().run(root, false, 100).unwrap();

        let graph_path = result.pack_dir.join("graph.json");
        assert!(graph_path.exists(), "graph.json should exist in pack dir");

        let metrics_path = result.pack_dir.join("graph-metrics.json");
        assert!(
            metrics_path.exists(),
            "graph-metrics.json should exist in pack dir"
        );

        let graph: repolens_core::model::graph::FileGraph =
            serde_json::from_str(&fs::read_to_string(&graph_path).unwrap()).unwrap();
        assert!(
            !graph.edges.is_empty(),
            "graph should have at least 1 edge between App.java and Helper.java"
        );
    }

    #[test]
    fn scan_java_small_graph_matches_golden() {
        let fixture_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inputs/java-small");

        if !fixture_root.exists() {
            eprintln!("Skipping: fixture not found at {}", fixture_root.display());
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("java-small");
        copy_dir_recursive(&fixture_root, &root);

        let result = ScanService::new().run(&root, false, 100).unwrap();

        // Load actual graph output
        let graph_path = result.pack_dir.join("graph.json");
        assert!(graph_path.exists(), "graph.json should exist");
        let our_graph: repolens_core::model::graph::FileGraph =
            serde_json::from_str(&fs::read_to_string(&graph_path).unwrap()).unwrap();

        let metrics_path = result.pack_dir.join("graph-metrics.json");
        assert!(metrics_path.exists(), "graph-metrics.json should exist");
        let our_metrics: repolens_core::model::graph::GraphResolutionMetrics =
            serde_json::from_str(&fs::read_to_string(&metrics_path).unwrap()).unwrap();

        // Load golden fixtures
        let golden_graph_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scenarios/java-small/pack/graph.json");
        let golden_graph: repolens_core::model::graph::FileGraph =
            serde_json::from_str(&fs::read_to_string(&golden_graph_path).unwrap()).unwrap();

        let golden_metrics_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scenarios/java-small/pack/graph-metrics.json");
        let golden_metrics: repolens_core::model::graph::GraphResolutionMetrics =
            serde_json::from_str(&fs::read_to_string(&golden_metrics_path).unwrap()).unwrap();

        // Compare node count
        assert_eq!(
            our_graph.nodes.len(),
            golden_graph.nodes.len(),
            "node count mismatch: got {}, expected {}",
            our_graph.nodes.len(),
            golden_graph.nodes.len()
        );

        // Compare nodes by id (sorted)
        for golden_node in &golden_graph.nodes {
            let our_node = our_graph
                .nodes
                .iter()
                .find(|n| n.id == golden_node.id)
                .unwrap_or_else(|| panic!("missing node: {}", golden_node.id));
            assert_eq!(
                our_node.symbols, golden_node.symbols,
                "symbol mismatch for node {}",
                golden_node.id
            );
        }

        // Compare edges
        assert_eq!(
            our_graph.edges.len(),
            golden_graph.edges.len(),
            "edge count mismatch: got {}, expected {}",
            our_graph.edges.len(),
            golden_graph.edges.len()
        );

        for golden_edge in &golden_graph.edges {
            let our_edge = our_graph
                .edges
                .iter()
                .find(|e| {
                    e.source == golden_edge.source
                        && e.target == golden_edge.target
                        && e.edge_type == golden_edge.edge_type
                })
                .unwrap_or_else(|| {
                    panic!(
                        "missing edge: {} -> {} ({})",
                        golden_edge.source, golden_edge.target, golden_edge.edge_type
                    )
                });
            assert_eq!(
                our_edge.detail, golden_edge.detail,
                "detail mismatch for edge {} -> {} ({})",
                golden_edge.source, golden_edge.target, golden_edge.edge_type
            );
        }

        // Compare metrics
        assert_eq!(
            our_metrics.total_relationships, golden_metrics.total_relationships,
            "totalRelationships mismatch"
        );
        assert_eq!(
            our_metrics.resolved_edges, golden_metrics.resolved_edges,
            "resolvedEdges mismatch"
        );
        assert_eq!(
            our_metrics.unresolved_edges, golden_metrics.unresolved_edges,
            "unresolvedEdges mismatch"
        );
        assert_eq!(
            our_metrics.per_language, golden_metrics.per_language,
            "perLanguage mismatch"
        );
    }

    #[test]
    fn incremental_scan_carries_forward_and_reprocesses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Initialize git repo
        use std::process::Command;
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(root)
            .output()
            .unwrap();

        // Create initial files and commit
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/keep.rs"), "fn keep() {}").unwrap();
        fs::write(root.join("src/modify.rs"), "fn original() {}").unwrap();
        fs::write(root.join("src/delete.rs"), "fn delete_me() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        // Full scan (creates base pack)
        let r1 = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(r1.scan_mode, "full");
        assert_eq!(r1.included_files, 3);

        // Wait for distinct pack timestamp
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Make changes and commit
        fs::write(root.join("src/modify.rs"), "fn modified() {}").unwrap();
        fs::remove_file(root.join("src/delete.rs")).unwrap();
        fs::write(root.join("src/added.rs"), "fn added() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "changes"])
            .current_dir(root)
            .output()
            .unwrap();

        // Incremental scan
        let r2 = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(
            r2.scan_mode, "incremental",
            "should detect compatible base and scan incrementally"
        );

        // Verify counts
        assert_eq!(r2.included_files, 3, "keep + modify + added = 3 included");
        assert!(r2.carried_forward_files.is_some());
        assert!(r2.deleted_files.is_some());
        assert!(
            r2.carried_forward_files.unwrap() >= 1,
            "keep.rs should be carried forward"
        );
        assert_eq!(r2.deleted_files.unwrap(), 1, "delete.rs was deleted");

        // Verify pack contents
        let chunk_path = r2.pack_dir.join("files-0001.json");
        let entries: Vec<FileEntry> =
            serde_json::from_str(&fs::read_to_string(&chunk_path).unwrap()).unwrap();

        // delete.rs should be gone
        assert!(
            !entries.iter().any(|e| e.path == "src/delete.rs"),
            "deleted file should not appear in merged pack"
        );

        // added.rs should be present and included
        let added = entries.iter().find(|e| e.path == "src/added.rs");
        assert!(added.is_some(), "added file should appear in merged pack");
        assert!(added.unwrap().included);

        // modify.rs should be present and included
        let modified = entries.iter().find(|e| e.path == "src/modify.rs").unwrap();
        assert!(modified.included);

        // keep.rs should be present (carried forward)
        let kept = entries.iter().find(|e| e.path == "src/keep.rs").unwrap();
        assert!(kept.included);

        // Verify manifest
        let manifest_str = fs::read_to_string(r2.pack_dir.join("manifest.json")).unwrap();
        let manifest: repolens_core::model::manifest::Manifest =
            serde_json::from_str(&manifest_str).unwrap();
        assert_eq!(manifest.scan_mode.as_deref(), Some("incremental"));
        assert!(manifest.base_pack_id.is_some());
        assert!(manifest.scan_config_hash.is_some());

        // Verify session
        let session: RepoSession =
            serde_json::from_str(&fs::read_to_string(root.join(".repolens/session.json")).unwrap())
                .unwrap();
        assert_eq!(session.last_scan_mode, "incremental");

        // Verify graph was rebuilt
        assert!(r2.pack_dir.join("graph.json").exists());
    }

    #[test]
    fn force_full_bypasses_incremental() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        use std::process::Command;
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(root)
            .output()
            .unwrap();
        fs::write(root.join("a.txt"), "a").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        // First scan (full)
        ScanService::new().run(root, false, 100).unwrap();

        std::thread::sleep(std::time::Duration::from_secs(1));

        // Force full even though base exists
        let r2 = ScanService::new().run(root, true, 100).unwrap();
        assert_eq!(r2.scan_mode, "full", "--full should force full scan");
    }

    #[test]
    fn scan_produces_project_profile_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/App.java"),
            "package com.example;\n\npublic class App {\n    public static void main(String[] args) {}\n}\n",
        )
        .unwrap();
        fs::write(root.join("pom.xml"), "<project></project>").unwrap();

        let result = ScanService::new().run(root, false, 100).unwrap();

        let profile_path = result.pack_dir.join("project-profile.json");
        assert!(
            profile_path.exists(),
            "project-profile.json should exist in pack dir"
        );

        let content = fs::read_to_string(&profile_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should detect Maven build system from pom.xml
        assert_eq!(parsed["buildSystem"]["name"], "Maven");

        // Should detect Java main entry point
        let entry_points = parsed["entryPoints"].as_array().unwrap();
        assert!(
            entry_points.iter().any(|ep| ep["path"] == "src/App.java"),
            "expected entry point for src/App.java, got: {:?}",
            entry_points
        );

        // Health section should be present
        assert!(
            parsed["health"].is_object(),
            "health key should be present in project-profile.json"
        );
    }

    #[test]
    fn incremental_scan_produces_project_profile_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        use std::process::Command;
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(root)
            .output()
            .unwrap();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        // Full scan (creates base pack)
        ScanService::new().run(root, false, 100).unwrap();

        std::thread::sleep(std::time::Duration::from_secs(1));

        // Make a change and commit
        fs::write(root.join("src/lib.rs"), "pub fn hello() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add lib"])
            .current_dir(root)
            .output()
            .unwrap();

        // Incremental scan
        let r2 = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(r2.scan_mode, "incremental");

        let profile_path = r2.pack_dir.join("project-profile.json");
        assert!(
            profile_path.exists(),
            "project-profile.json should exist after incremental scan"
        );

        let content = fs::read_to_string(&profile_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["buildSystem"]["name"], "Cargo");
    }

    #[test]
    fn incremental_manifest_retains_core_metadata_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        use std::process::Command;
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(root)
            .output()
            .unwrap();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let full = ScanService::new().run(root, false, 100).unwrap();

        std::thread::sleep(std::time::Duration::from_secs(1));

        fs::write(root.join("src/lib.rs"), "pub fn hello() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add lib"])
            .current_dir(root)
            .output()
            .unwrap();

        let incremental = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(incremental.scan_mode, "incremental");

        let full_manifest: repolens_core::model::manifest::Manifest =
            serde_json::from_str(&fs::read_to_string(full.pack_dir.join("manifest.json")).unwrap())
                .unwrap();
        let incremental_manifest: repolens_core::model::manifest::Manifest = serde_json::from_str(
            &fs::read_to_string(incremental.pack_dir.join("manifest.json")).unwrap(),
        )
        .unwrap();

        assert!(!full_manifest.schema_version.is_empty());
        assert_eq!(
            incremental_manifest.schema_version,
            full_manifest.schema_version
        );
        assert_eq!(incremental_manifest.schema_url, full_manifest.schema_url);
        assert!(!incremental_manifest.chunks.is_empty());

        assert_eq!(full_manifest.scan_mode.as_deref(), Some("full"));
        assert_eq!(
            incremental_manifest.scan_mode.as_deref(),
            Some("incremental")
        );

        assert_eq!(incremental_manifest.scan_scope, full_manifest.scan_scope);
        assert!(full_manifest.head_commit.is_some());
        assert!(incremental_manifest.head_commit.is_some());
        assert_eq!(
            incremental_manifest.worktree_dirty,
            full_manifest.worktree_dirty
        );
        assert!(full_manifest.scan_config_hash.is_some());
        assert!(incremental_manifest.scan_config_hash.is_some());
        assert!(full_manifest.reprocessed_files.is_some());
        assert!(incremental_manifest.reprocessed_files.is_some());

        assert!(incremental_manifest.base_pack_id.is_some());
        assert!(incremental_manifest.base_head_commit.is_some());
        assert!(incremental_manifest.deleted_files.is_some());
        assert!(incremental_manifest.carried_forward_files.is_some());
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let target = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir_recursive(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    // --- #30: staged changes and edit-then-revert in incremental scans ---

    fn git(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Committed repo with `src/a.rs` and `src/keep.rs`, plus a clean full scan.
    fn committed_repo_with_base_scan() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.com"]);
        git(root, &["config", "user.name", "T"]);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "pub fn alpha() -> u32 { 1 }\n").unwrap();
        fs::write(root.join("src/keep.rs"), "pub fn keep() {}\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);
        let r = ScanService::new().run(root, false, 100).unwrap();
        assert_eq!(r.scan_mode, "full");
        dir
    }

    /// Two scans in the same second used to share one pack directory; the
    /// incremental scan then copied each carried-forward file onto itself,
    /// truncating the stored content to 0 bytes. This end-to-end check only
    /// caught the old bug when both scans fell in one wall-clock second; the
    /// deterministic coverage is in `storage.rs` and `merged_pack_writer.rs`.
    #[test]
    fn back_to_back_scans_write_separate_packs_with_intact_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.com"]);
        git(root, &["config", "user.name", "T"]);
        fs::write(root.join("a.txt"), "hello world\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);

        let first = ScanService::new().run(root, false, 100).unwrap();
        let second = ScanService::new().run(root, false, 100).unwrap();

        assert_eq!(second.scan_mode, "incremental");
        assert_ne!(first.pack_dir, second.pack_dir);
        for pack in [&first.pack_dir, &second.pack_dir] {
            assert_eq!(
                fs::read_to_string(pack.join("files/a.txt")).unwrap(),
                "hello world\n",
                "stored content in {} was damaged",
                pack.display()
            );
        }
    }

    /// Next scan, after waiting for a distinct pack timestamp.
    fn rescan(root: &Path) -> ScanResult {
        std::thread::sleep(std::time::Duration::from_millis(1100));
        ScanService::new().run(root, false, 100).unwrap()
    }

    /// #37: with `--keep 3`, repeated scans of a dirty tree used to prune
    /// the only clean pack, so every later scan fell back to a full scan.
    #[test]
    fn keep_retains_last_clean_pack_so_dirty_rescans_stay_incremental() {
        let dir = committed_repo_with_base_scan();
        let root = dir.path();
        fs::write(root.join("src/a.rs"), "pub fn alpha() -> u32 { 2 }\n").unwrap();

        for i in 1..=5 {
            std::thread::sleep(std::time::Duration::from_millis(1100));
            let r = ScanService::new().run(root, false, 3).unwrap();
            assert_eq!(
                r.scan_mode, "incremental",
                "dirty rescan {i} with --keep 3 fell back to a full scan"
            );
            assert_eq!(pack_manifest(&r.pack_dir).worktree_dirty, Some(true));
        }

        let clean: Vec<_> = fs::read_dir(root.join(".repolens"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("pack-"))
            .filter(|e| pack_manifest(&e.path()).worktree_dirty == Some(false))
            .collect();
        assert_eq!(clean.len(), 1, "the clean base pack must survive pruning");
    }

    /// #37: a full scan that happens because no earlier pack is a usable
    /// base reports why; a first-ever scan does not.
    #[test]
    fn full_scan_without_usable_base_reports_fell_back_reason() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t.com"]);
        git(root, &["config", "user.name", "T"]);
        fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "init"]);
        fs::write(root.join("a.rs"), "pub fn a() { }\n").unwrap();

        let first = ScanService::new().run(root, false, 3).unwrap();
        assert_eq!(first.scan_mode, "full");
        assert_eq!(first.fell_back_reason, None, "first scan is not a fallback");

        let second = rescan(root);
        assert_eq!(second.scan_mode, "full");
        let reason = second.fell_back_reason.expect("fallback reason");
        assert_eq!(reason, crate::incremental::NO_CLEAN_BASE_REASON);
    }

    fn pack_entries(pack_dir: &Path) -> Vec<FileEntry> {
        serde_json::from_str(&fs::read_to_string(pack_dir.join("files-0001.json")).unwrap())
            .unwrap()
    }

    fn pack_paths(pack_dir: &Path) -> Vec<String> {
        pack_entries(pack_dir).into_iter().map(|e| e.path).collect()
    }

    fn pack_manifest(pack_dir: &Path) -> repolens_core::model::manifest::Manifest {
        serde_json::from_str(&fs::read_to_string(pack_dir.join("manifest.json")).unwrap()).unwrap()
    }

    fn counts(r: &ScanResult) -> (i32, Option<i32>) {
        // #40 QA: "reprocessed + carried forward" is the file total, so the
        // scan summary line adds up.
        if r.scan_mode == "incremental" {
            assert_eq!(
                r.reprocessed_files as usize + r.carried_forward_files.unwrap() as usize,
                r.total_files,
                "reprocessed {} + carried forward {:?} != total {}",
                r.reprocessed_files,
                r.carried_forward_files,
                r.total_files
            );
        }
        (r.reprocessed_files, r.deleted_files)
    }

    #[test]
    fn incremental_scan_sees_staged_git_mv_and_move_back() {
        let dir = committed_repo_with_base_scan();
        let root = dir.path();

        git(root, &["mv", "src/a.rs", "src/b.rs"]);
        let moved = rescan(root);
        assert_eq!(moved.scan_mode, "incremental");
        assert_eq!(
            counts(&moved),
            (1, Some(1)),
            "git mv: 1 reprocessed, 1 deleted"
        );
        assert_eq!(pack_paths(&moved.pack_dir), vec!["src/b.rs", "src/keep.rs"]);
        assert_eq!(pack_manifest(&moved.pack_dir).worktree_dirty, Some(true));

        git(root, &["mv", "src/b.rs", "src/a.rs"]);
        let back = rescan(root);
        assert_eq!(back.scan_mode, "incremental");
        assert_eq!(
            counts(&back),
            (1, Some(1)),
            "move back: 1 reprocessed, 1 deleted"
        );
        assert_eq!(pack_paths(&back.pack_dir), vec!["src/a.rs", "src/keep.rs"]);
        assert_eq!(pack_manifest(&back.pack_dir).worktree_dirty, Some(false));
    }

    #[test]
    fn incremental_scan_sees_staged_git_rm() {
        let dir = committed_repo_with_base_scan();
        let root = dir.path();

        git(root, &["rm", "-q", "src/a.rs"]);
        let removed = rescan(root);
        assert_eq!(removed.scan_mode, "incremental");
        assert_eq!(counts(&removed), (0, Some(1)), "git rm: 1 deleted");
        assert_eq!(pack_paths(&removed.pack_dir), vec!["src/keep.rs"]);
        assert_eq!(pack_manifest(&removed.pack_dir).worktree_dirty, Some(true));

        git(root, &["reset", "-q", "--hard"]);
        let restored = rescan(root);
        assert_eq!(counts(&restored), (1, Some(0)), "restore: 1 reprocessed");
        assert_eq!(
            pack_paths(&restored.pack_dir),
            vec!["src/a.rs", "src/keep.rs"]
        );
    }

    #[test]
    fn incremental_scan_reprocesses_file_after_edit_and_revert() {
        let dir = committed_repo_with_base_scan();
        let root = dir.path();
        let original = fs::read_to_string(root.join("src/a.rs")).unwrap();

        fs::write(
            root.join("src/a.rs"),
            format!("{original}pub fn gamma() {{}}\n"),
        )
        .unwrap();
        let edited = rescan(root);
        assert_eq!(counts(&edited), (1, Some(0)));
        let edited_sha = pack_entries(&edited.pack_dir)
            .into_iter()
            .find(|e| e.path == "src/a.rs")
            .unwrap()
            .sha256;

        git(root, &["checkout", "--", "src/a.rs"]);
        let reverted = rescan(root);
        assert_eq!(reverted.scan_mode, "incremental");
        assert_eq!(
            counts(&reverted),
            (1, Some(0)),
            "the reverted file differs from the previous scan and must be reprocessed"
        );

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let full = ScanService::new().run(root, true, 100).unwrap();
        let reverted_entries = pack_entries(&reverted.pack_dir);
        let full_entries = pack_entries(&full.pack_dir);
        let sha = |entries: &[FileEntry], path: &str| {
            entries
                .iter()
                .find(|e| e.path == path)
                .map(|e| e.sha256.clone())
        };
        assert_eq!(
            sha(&reverted_entries, "src/a.rs"),
            sha(&full_entries, "src/a.rs")
        );
        assert_ne!(sha(&reverted_entries, "src/a.rs"), Some(edited_sha));
    }

    #[test]
    fn reprocessed_counts_only_changes_since_the_previous_scan() {
        // #40: edit A, scan; edit B, scan; revert A, scan. Each scan counts
        // only what changed since the scan before it, even though the base
        // pack (the last clean one) is older than the previous scan.
        let dir = committed_repo_with_base_scan();
        let root = dir.path();
        let a = fs::read_to_string(root.join("src/a.rs")).unwrap();
        let keep = fs::read_to_string(root.join("src/keep.rs")).unwrap();

        fs::write(root.join("src/a.rs"), format!("{a}pub fn edit_a() {{}}\n")).unwrap();
        assert_eq!(counts(&rescan(root)), (1, Some(0)), "edit A");

        fs::write(
            root.join("src/keep.rs"),
            format!("{keep}pub fn edit_b() {{}}\n"),
        )
        .unwrap();
        assert_eq!(counts(&rescan(root)), (1, Some(0)), "edit B");

        git(root, &["checkout", "--", "src/a.rs"]);
        let reverted = rescan(root);
        assert_eq!(reverted.scan_mode, "incremental");
        assert_eq!(counts(&reverted), (1, Some(0)), "revert A");

        // Nothing changed since the previous scan.
        let again = rescan(root);
        assert_eq!(counts(&again), (0, Some(0)), "unchanged rescan");
        assert_eq!(pack_manifest(&again.pack_dir).reprocessed_files, Some(0));

        // The pack content still matches a full scan.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let full = ScanService::new().run(root, true, 100).unwrap();
        let key = |entries: Vec<FileEntry>| -> Vec<(String, String, bool)> {
            entries
                .into_iter()
                .map(|e| (e.path, e.sha256, e.included))
                .collect()
        };
        assert_eq!(
            key(pack_entries(&reverted.pack_dir)),
            key(pack_entries(&full.pack_dir))
        );
    }

    #[test]
    fn pack_scanned_with_only_staged_changes_is_dirty_and_not_an_incremental_base() {
        let dir = committed_repo_with_base_scan();
        let root = dir.path();

        fs::write(root.join("src/a.rs"), "pub fn staged() {}\n").unwrap();
        git(root, &["add", "src/a.rs"]);
        let staged = rescan(root);
        assert_eq!(counts(&staged), (1, Some(0)), "staged edit is reprocessed");
        assert_eq!(pack_manifest(&staged.pack_dir).worktree_dirty, Some(true));

        git(root, &["commit", "-q", "-m", "commit staged edit"]);
        let next = rescan(root);
        assert_eq!(next.scan_mode, "incremental");
        let staged_id = staged
            .pack_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_ne!(
            pack_manifest(&next.pack_dir).base_pack_id.as_deref(),
            Some(staged_id.as_str()),
            "a pack taken with staged changes must not become an incremental base"
        );
    }
}
