use crate::model::file_entry::FileEntry;
use crate::model::graph::{FileGraph, GraphResolutionMetrics};
use crate::model::manifest::Manifest;
use crate::path_safety::{resolve_existing_subpath, resolve_prefixed_subpath_for_create};
use crate::serde_util::java_epoch_seconds::JavaEpochSeconds;
use rust_decimal::Decimal;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

const SCHEMA_VERSION: &str = "1.3.0";
const SCHEMA_URL: &str = "https://example.com/schema/1.3.0";
const DEFAULT_CHUNK_SIZE: usize = 500;

/// Options for writing a pack manifest, shared between full and incremental writers.
pub struct PackWriteOptions<'a> {
    pub scan_mode: &'a str,
    pub head_commit: &'a str,
    pub worktree_dirty: bool,
    pub scan_config_hash: &'a str,
    pub base_pack_id: Option<&'a str>,
    pub base_head_commit: Option<&'a str>,
    pub reprocessed_files: i32,
    pub deleted_files: Option<i32>,
    pub carried_forward_files: Option<i32>,
}

pub struct PackWriter {
    pack_dir: PathBuf,
    repo_root: PathBuf,
    chunk_size: usize,
}

impl PackWriter {
    pub fn new(pack_dir: &Path, repo_root: &Path) -> Self {
        Self {
            pack_dir: pack_dir.to_path_buf(),
            repo_root: repo_root.to_path_buf(),
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }

    pub fn chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }

    pub fn write(&self, entries: &[FileEntry], options: &PackWriteOptions<'_>) -> io::Result<()> {
        if self.chunk_size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chunk_size must be greater than 0",
            ));
        }

        info!(pack_dir = %self.pack_dir.display(), entries = entries.len(), "Writing pack");

        fs::create_dir_all(&self.pack_dir)?;
        crate::storage::RepoStoragePaths::ensure_storage_ignored(&self.pack_dir);
        self.write_content_files(entries)?;
        let chunk_names = Self::write_chunks(&self.pack_dir, entries, self.chunk_size)?;
        self.write_manifest(&chunk_names, options)?;

        info!(chunks = chunk_names.len(), "Pack write complete");
        Ok(())
    }

    fn write_content_files(&self, entries: &[FileEntry]) -> io::Result<()> {
        for entry in entries.iter().filter(|e| e.included) {
            let Some(content_path) = &entry.content_path else {
                continue;
            };
            let source = resolve_existing_subpath(&self.repo_root, &entry.path).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("failed to resolve content for {}: {e}", entry.path),
                )
            })?;
            let target =
                resolve_prefixed_subpath_for_create(&self.pack_dir, content_path, "files")?;

            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::copy(&source, &target).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("failed to copy content for {}: {e}", entry.path),
                )
            })?;
            debug!(path = %entry.path, "Copied content file");
        }
        Ok(())
    }

    /// Write chunk files and return their names. Shared by both PackWriter and MergedPackWriter.
    pub fn write_chunks(
        pack_dir: &Path,
        entries: &[FileEntry],
        chunk_size: usize,
    ) -> io::Result<Vec<String>> {
        let chunks: Vec<&[FileEntry]> = entries.chunks(chunk_size).collect();
        let mut chunk_names = Vec::new();

        for (i, chunk) in chunks.iter().enumerate() {
            let name = format!("files-{:04}.json", i + 1);
            let path = pack_dir.join(&name);
            let json = serde_json::to_string_pretty(chunk).map_err(io::Error::other)?;
            fs::write(&path, json)?;
            chunk_names.push(name);
            debug!(chunk = i + 1, entries = chunk.len(), "Wrote chunk file");
        }

        Ok(chunk_names)
    }

    fn write_manifest(&self, chunks: &[String], options: &PackWriteOptions<'_>) -> io::Result<()> {
        let now = time::OffsetDateTime::now_utc();
        // Build Decimal from integer parts to avoid f64 precision loss.
        let secs = Decimal::from(now.unix_timestamp());
        let nanos = Decimal::new(now.nanosecond() as i64, 9);
        let timestamp = JavaEpochSeconds(secs + nanos);

        let manifest = Manifest {
            schema_version: SCHEMA_VERSION.to_string(),
            schema_url: SCHEMA_URL.to_string(),
            timestamp,
            chunks: chunks.to_vec(),
            scan_mode: Some(options.scan_mode.to_string()),
            scan_scope: Some("complete".to_string()),
            base_pack_id: options.base_pack_id.map(|s| s.to_string()),
            base_head_commit: options.base_head_commit.map(|s| s.to_string()),
            head_commit: if options.head_commit.is_empty() {
                None
            } else {
                Some(options.head_commit.to_string())
            },
            worktree_dirty: Some(options.worktree_dirty),
            scan_config_hash: if options.scan_config_hash.is_empty() {
                None
            } else {
                Some(options.scan_config_hash.to_string())
            },
            reprocessed_files: Some(options.reprocessed_files),
            deleted_files: options.deleted_files,
            carried_forward_files: options.carried_forward_files,
        };

        let json = serde_json::to_string_pretty(&manifest).map_err(io::Error::other)?;
        let final_path = self.pack_dir.join("manifest.json");
        crate::atomic_write::write_atomic(&final_path, json.as_bytes())?;
        debug!("Wrote manifest.json");
        Ok(())
    }

    /// Write only manifest.json with the given chunk names and options.
    /// Used by MergedPackWriter to share manifest serialization logic.
    pub fn write_manifest_with_chunks(
        &self,
        chunks: &[String],
        options: &PackWriteOptions<'_>,
    ) -> io::Result<()> {
        self.write_manifest(chunks, options)
    }

    /// Writes `graph.json` and `graph-metrics.json` into the pack directory.
    pub fn write_graph(
        &self,
        graph: &FileGraph,
        metrics: &GraphResolutionMetrics,
    ) -> io::Result<()> {
        info!(
            pack_dir = %self.pack_dir.display(),
            nodes = graph.nodes.len(),
            edges = graph.edges.len(),
            "Writing graph files"
        );

        fs::create_dir_all(&self.pack_dir)?;
        crate::storage::RepoStoragePaths::ensure_storage_ignored(&self.pack_dir);

        let graph_json = serde_json::to_string_pretty(graph).map_err(io::Error::other)?;
        let graph_path = self.pack_dir.join("graph.json");
        fs::write(&graph_path, &graph_json)?;
        debug!("Wrote graph.json");

        let metrics_json = serde_json::to_string_pretty(metrics).map_err(io::Error::other)?;
        let metrics_path = self.pack_dir.join("graph-metrics.json");
        fs::write(&metrics_path, &metrics_json)?;
        debug!("Wrote graph-metrics.json");

        info!(
            resolved = metrics.resolved_edges,
            unresolved = metrics.unresolved_edges,
            "Graph write complete"
        );
        Ok(())
    }

    /// Writes `project-profile.json` into the pack directory.
    ///
    /// The output combines the `ProjectProfile` with an optional `HealthReport`
    /// into a single JSON object, matching the Java contract fixture format.
    pub fn write_profile(
        &self,
        profile: &crate::model::profile::ProjectProfile,
        health: Option<&crate::model::health::HealthReport>,
    ) -> io::Result<()> {
        fs::create_dir_all(&self.pack_dir)?;
        crate::storage::RepoStoragePaths::ensure_storage_ignored(&self.pack_dir);

        let mut value = serde_json::to_value(profile).map_err(io::Error::other)?;
        if let Some(h) = health {
            let health_value = serde_json::to_value(h).map_err(io::Error::other)?;
            value
                .as_object_mut()
                .ok_or_else(|| io::Error::other("profile did not serialize to a JSON object"))?
                .insert("health".to_string(), health_value);
        }

        let json = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
        let path = self.pack_dir.join("project-profile.json");
        fs::write(&path, json)?;
        debug!("Wrote project-profile.json");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::graph::{GraphEdge, GraphNode};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn write_profile_produces_expected_file() {
        use crate::model::health::{DependencyRisk, HealthReport, Structure};
        use crate::model::profile::{ProjectProfile, ProjectType};

        let tmp = TempDir::new().unwrap();
        let pack_dir = tmp.path().join("pack-test");
        let repo_root = tmp.path().join("repo");

        let writer = PackWriter::new(&pack_dir, &repo_root);

        let profile = ProjectProfile {
            name: "test-project".to_string(),
            build_system: None,
            frameworks: vec![],
            project_type: ProjectType::Library,
            entry_points: vec![],
            description: None,
            language_distribution: BTreeMap::new(),
            modules: vec![],
        };

        let health = HealthReport {
            structure: Structure {
                module_count: 1,
                dominant_module: "src".to_string(),
                dominant_module_share: 1.0,
                entry_point_count: 0,
            },
            dependency_risk: DependencyRisk {
                build_system: None,
                framework_count: 0,
                missing_version_count: 0,
                lockfile_present: false,
                risk_level: "low".to_string(),
            },
            graph_health: None,
            evidence_findings: vec![],
        };

        writer.write_profile(&profile, Some(&health)).unwrap();

        let path = pack_dir.join("project-profile.json");
        assert!(path.exists(), "project-profile.json should exist");

        let content = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["name"], "test-project");
        assert_eq!(parsed["projectType"], "LIBRARY");
        assert!(parsed["health"].is_object(), "health key should be present");
        assert_eq!(parsed["health"]["structure"]["moduleCount"], 1);
    }

    #[test]
    fn write_graph_produces_expected_files() {
        let tmp = TempDir::new().unwrap();
        let pack_dir = tmp.path().join("pack-test");
        let repo_root = tmp.path().join("repo");

        let writer = PackWriter::new(&pack_dir, &repo_root);

        let graph = FileGraph {
            nodes: vec![
                GraphNode {
                    id: "src/main.rs".to_string(),
                    symbols: vec!["main".to_string()],
                },
                GraphNode {
                    id: "src/lib.rs".to_string(),
                    symbols: vec!["Config".to_string(), "run".to_string()],
                },
            ],
            edges: vec![GraphEdge {
                source: "src/main.rs".to_string(),
                target: "src/lib.rs".to_string(),
                edge_type: "imports".to_string(),
                detail: Some("Config".to_string()),
            }],
        };

        let mut per_lang = BTreeMap::new();
        per_lang.insert("Rust".to_string(), vec![1, 0]);

        let metrics = GraphResolutionMetrics {
            total_relationships: 1,
            resolved_edges: 1,
            unresolved_edges: 0,
            per_language: Some(per_lang),
            unresolved_imports: None,
        };

        writer.write_graph(&graph, &metrics).unwrap();

        // Verify graph.json
        let graph_path = pack_dir.join("graph.json");
        assert!(graph_path.exists(), "graph.json should exist");
        let graph_content = fs::read_to_string(&graph_path).unwrap();
        let parsed_graph: FileGraph = serde_json::from_str(&graph_content).unwrap();
        assert_eq!(parsed_graph.nodes.len(), 2);
        assert_eq!(parsed_graph.edges.len(), 1);
        assert_eq!(parsed_graph.edges[0].edge_type, "imports");
        // Verify serde rename: "type" in JSON, not "edge_type"
        assert!(graph_content.contains("\"type\""));
        assert!(!graph_content.contains("\"edge_type\""));

        // Verify graph-metrics.json
        let metrics_path = pack_dir.join("graph-metrics.json");
        assert!(metrics_path.exists(), "graph-metrics.json should exist");
        let metrics_content = fs::read_to_string(&metrics_path).unwrap();
        let parsed_metrics: GraphResolutionMetrics =
            serde_json::from_str(&metrics_content).unwrap();
        assert_eq!(parsed_metrics.total_relationships, 1);
        assert_eq!(parsed_metrics.resolved_edges, 1);
        assert_eq!(parsed_metrics.unresolved_edges, 0);
        // Verify camelCase serialization
        assert!(metrics_content.contains("\"totalRelationships\""));
        assert!(metrics_content.contains("\"resolvedEdges\""));
        assert!(metrics_content.contains("\"perLanguage\""));
    }
}
