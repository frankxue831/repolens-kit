//! Persistent storage for the per-repo memory graph.
//!
//! The graph is stored at `.repolens/memory/memory.json` inside the repo root.
//! Writes are atomic: data is written to a `.tmp` sibling file, then renamed.
//! A `.gitignore` is maintained in `.repolens/memory/` so the file is never
//! accidentally committed.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs4::FileExt;
use tracing::{debug, info, warn};
use uuid::Uuid;

use repolens_core::model::memory::{MemoryEdge, MemoryGraph, MemoryNode};

// ---------------------------------------------------------------------------
// Storage paths
// ---------------------------------------------------------------------------

fn memory_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".repolens").join("memory")
}

fn memory_file(repo_root: &Path) -> PathBuf {
    memory_dir(repo_root).join("memory.json")
}

fn memory_tmp_file(repo_root: &Path) -> PathBuf {
    memory_dir(repo_root).join(format!("memory.json.{}.tmp", Uuid::new_v4()))
}

fn memory_lock_file(repo_root: &Path) -> PathBuf {
    memory_dir(repo_root).join("memory.lock")
}

fn gitignore_file(repo_root: &Path) -> PathBuf {
    memory_dir(repo_root).join(".gitignore")
}

const GITIGNORE_ENTRIES: [&str; 3] = ["memory.json", "memory.lock", "memory.json.*.tmp"];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Stateless handle for reading and writing the memory graph.
pub struct MemoryStore;

impl MemoryStore {
    /// Load the memory graph from disk.
    ///
    /// Returns an empty graph (via [`empty_graph`]) if the file does not exist,
    /// and an error if it exists but cannot be parsed.
    pub fn load(repo_root: &Path) -> Result<MemoryGraph> {
        read_graph_unlocked(repo_root)
    }

    /// Acquire an exclusive repo-local memory lock and load the graph under it.
    ///
    /// Use this for any load-modify-save operation. Holding the guard across
    /// the full transaction prevents both temp-file collisions and lost updates
    /// from stale read snapshots.
    pub fn lock(repo_root: &Path) -> Result<MemoryStoreGuard> {
        let dir = memory_dir(repo_root);
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create memory directory: {}", dir.display()))?;
        repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&dir);

        let lock_path = memory_lock_file(repo_root);
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("Failed to open memory lock: {}", lock_path.display()))?;
        lock_file
            .lock_exclusive()
            .with_context(|| format!("Failed to lock memory file: {}", lock_path.display()))?;

        ensure_gitignore(repo_root)?;
        let graph = read_graph_unlocked(repo_root)?;

        Ok(MemoryStoreGuard {
            repo_root: repo_root.to_path_buf(),
            graph,
            _lock_file: lock_file,
        })
    }

    /// Save the memory graph to disk atomically.
    ///
    /// Compatibility API for callers that already have a full graph. New
    /// load-modify-save callers should prefer [`MemoryStore::lock`] so the
    /// read, mutation, and write happen under one exclusive lock.
    ///
    /// Like [`MemoryStore::lock`], it fails (and writes nothing) when the
    /// existing memory.json cannot be parsed.
    ///
    /// # Warning
    /// This acquires the memory lock internally. Never call it while already
    /// holding a [`MemoryStoreGuard`]: the second `flock` on a fresh fd in the
    /// same process blocks forever (self-deadlock). Use `guard.save()` instead.
    pub fn save(repo_root: &Path, graph: &MemoryGraph) -> Result<()> {
        let mut guard = Self::lock(repo_root)?;
        guard.graph = graph.clone();
        guard.save()
    }
}

/// Exclusive transaction guard for `.repolens/memory/memory.json`.
pub struct MemoryStoreGuard {
    repo_root: PathBuf,
    graph: MemoryGraph,
    _lock_file: File,
}

impl MemoryStoreGuard {
    pub fn graph(&self) -> &MemoryGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut MemoryGraph {
        &mut self.graph
    }

    pub fn save(&mut self) -> Result<()> {
        write_graph_unlocked(&self.repo_root, &self.graph)
    }
}

fn read_graph_unlocked(repo_root: &Path) -> Result<MemoryGraph> {
    let path = memory_file(repo_root);
    debug!(path = %path.display(), "Loading memory graph");

    if !path.exists() {
        info!(path = %path.display(), "Memory file not found, returning empty graph");
        return Ok(empty_graph(repo_root));
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read memory file: {}", path.display()))?;

    // A blank file holds no memories, so treating it as empty loses nothing.
    if raw.trim().is_empty() {
        info!(path = %path.display(), "Memory file is blank, returning empty graph");
        return Ok(empty_graph(repo_root));
    }

    match serde_json::from_str::<MemoryGraph>(&raw) {
        Ok(graph) => {
            info!(
                path = %path.display(),
                nodes = graph.nodes.len(),
                edges = graph.edges.len(),
                "Loaded memory graph"
            );
            Ok(graph)
        }
        // Never fall back to an empty graph: the next save would replace the
        // file and delete every memory in it (e.g. a file from a newer version
        // with a node type this one does not know).
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Memory file unparseable");
            Err(anyhow::anyhow!(
                "Cannot parse memory file {}: {e}. It was left unchanged. It may have been written by a newer repolens: upgrade, or fix or move the file aside to continue.",
                path.display()
            ))
        }
    }
}

fn write_graph_unlocked(repo_root: &Path, graph: &MemoryGraph) -> Result<()> {
    let dir = memory_dir(repo_root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create memory directory: {}", dir.display()))?;
    repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&dir);

    let tmp = memory_tmp_file(repo_root);
    let dst = memory_file(repo_root);

    let json = serde_json::to_string_pretty(graph).context("Failed to serialize memory graph")?;

    if let Err(e) = fs::write(&tmp, &json) {
        let _ = fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("Failed to write memory tmp file: {}", tmp.display()));
    }

    if let Err(e) = fs::rename(&tmp, &dst) {
        let _ = fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("Failed to rename {} -> {}", tmp.display(), dst.display()));
    }

    info!(
        path = %dst.display(),
        nodes = graph.nodes.len(),
        edges = graph.edges.len(),
        "Saved memory graph"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Create an empty [`MemoryGraph`] with schema version `"1.0"`.
pub fn empty_graph(repo_root: &Path) -> MemoryGraph {
    MemoryGraph {
        schema_version: "1.0".to_string(),
        repo_root: repo_root.to_string_lossy().into_owned(),
        nodes: Vec::new(),
        edges: Vec::new(),
        last_decay_run: None,
        updated_at: crate::clock::now(),
    }
}

/// Append a [`MemoryNode`] and update `updated_at`.
///
/// Every `updated_at` stamp here uses [`crate::clock::now`], so a save under
/// `REPOLENS_CLOCK_OVERRIDE` dates the file like the node it adds (#8).
pub fn add_node(graph: &mut MemoryGraph, node: MemoryNode) {
    debug!(id = %node.id, "Adding memory node");
    graph.nodes.push(node);
    graph.updated_at = crate::clock::now();
}

/// Remove a node by `id` and all edges that reference it.
pub fn remove_node(graph: &mut MemoryGraph, id: &str) {
    let before = graph.nodes.len();
    graph.nodes.retain(|n| n.id != id);
    graph
        .edges
        .retain(|e| e.source_id != id && e.target_id != id);
    let removed = before - graph.nodes.len();
    if removed > 0 {
        graph.updated_at = crate::clock::now();
        debug!(id, removed, "Removed memory node and its edges");
    } else {
        debug!(id, "remove_node: node not found");
    }
}

/// Increment `access_count` and update `last_accessed` for a node.
pub fn touch_node(graph: &mut MemoryGraph, id: &str) {
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == id) {
        node.access_count += 1;
        node.last_accessed = crate::clock::now();
        debug!(id, access_count = node.access_count, "Touched memory node");
    } else {
        debug!(id, "touch_node: node not found");
    }
}

/// Append an edge if an identical edge (same source, target, relation) does not
/// already exist.
pub fn add_edge(graph: &mut MemoryGraph, edge: MemoryEdge) {
    let duplicate = graph.edges.iter().any(|e| {
        e.source_id == edge.source_id
            && e.target_id == edge.target_id
            && e.relation == edge.relation
    });
    if duplicate {
        debug!(
            source = %edge.source_id,
            target = %edge.target_id,
            "Skipping duplicate memory edge"
        );
        return;
    }
    debug!(
        source = %edge.source_id,
        target = %edge.target_id,
        "Adding memory edge"
    );
    graph.edges.push(edge);
    graph.updated_at = crate::clock::now();
}

/// Ensure `.repolens/memory/.gitignore` contains all generated memory files.
///
/// Creates the file if absent; appends the entry if the file exists but does
/// not already contain the line.
pub fn ensure_gitignore(repo_root: &Path) -> Result<()> {
    let dir = memory_dir(repo_root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create memory dir: {}", dir.display()))?;
    repolens_core::storage::RepoStoragePaths::ensure_storage_ignored(&dir);

    let path = gitignore_file(repo_root);

    if path.exists() {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read gitignore: {}", path.display()))?;
        let existing: std::collections::BTreeSet<String> =
            contents.lines().map(str::to_string).collect();
        let mut updated = contents;
        let mut changed = false;
        for entry in GITIGNORE_ENTRIES {
            if !existing.contains(entry) {
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push_str(entry);
                updated.push('\n');
                changed = true;
            }
        }
        if changed {
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            fs::write(&path, updated)
                .with_context(|| format!("Failed to update gitignore: {}", path.display()))?;
            debug!(path = %path.display(), "Updated memory gitignore");
        }
    } else {
        let entries = format!("{}\n", GITIGNORE_ENTRIES.join("\n"));
        fs::write(&path, entries)
            .with_context(|| format!("Failed to create gitignore: {}", path.display()))?;
        debug!(path = %path.display(), "Created memory gitignore");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::memory::{MemoryRelation, MemorySource, MemoryType};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;
    use time::OffsetDateTime;

    fn make_node(id: &str, content: &str) -> MemoryNode {
        let now = OffsetDateTime::now_utc();
        MemoryNode {
            id: id.to_string(),
            content: content.to_string(),
            memory_type: MemoryType::Pattern,
            importance: 0.5,
            created: now,
            last_accessed: now,
            access_count: 0,
            anchors: vec![],
            tags: vec![],
            source: MemorySource::Explicit,
            provenance: "test".to_string(),
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256: Default::default(),
        }
    }

    #[test]
    fn load_returns_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        let graph = MemoryStore::load(dir.path()).unwrap();
        assert_eq!(graph.schema_version, "1.0");
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn load_errors_on_unparseable_file() {
        let dir = TempDir::new().unwrap();
        let mem_dir = dir.path().join(".repolens").join("memory");
        fs::create_dir_all(&mem_dir).unwrap();
        fs::write(mem_dir.join("memory.json"), b"not valid json {{{{").unwrap();

        assert!(MemoryStore::load(dir.path()).is_err());
    }

    /// An empty file holds no memories, so nothing is lost by treating it as
    /// an empty graph (and refusing it would block every memory command).
    #[test]
    fn load_treats_a_blank_file_as_no_memories() {
        let dir = TempDir::new().unwrap();
        let mem_dir = dir.path().join(".repolens").join("memory");
        fs::create_dir_all(&mem_dir).unwrap();
        fs::write(mem_dir.join("memory.json"), b" \n").unwrap();

        let graph = MemoryStore::load(dir.path()).unwrap();
        assert!(graph.nodes.is_empty());
    }

    /// A file this version cannot parse (here: a node type it does not know)
    /// used to load as an empty graph, and the next save replaced it — every
    /// stored memory was lost.
    #[test]
    fn lock_refuses_unparseable_file_and_leaves_it_untouched() {
        let dir = TempDir::new().unwrap();
        let mem_dir = dir.path().join(".repolens").join("memory");
        fs::create_dir_all(&mem_dir).unwrap();
        let original = r#"{"schemaVersion":"1.0","repoRoot":"x","nodes":[
{"id":"a","content":"keep me","type":"DECISION","importance":0.9,"created":"2026-01-01T00:00:00Z","lastAccessed":"2026-01-01T00:00:00Z","accessCount":0,"source":"EXPLICIT","provenance":"p"},
{"id":"b","content":"from a newer version","type":"GOTCHA","importance":0.5,"created":"2026-01-01T00:00:00Z","lastAccessed":"2026-01-01T00:00:00Z","accessCount":0,"source":"EXPLICIT","provenance":"p"}
],"edges":[],"lastDecayRun":null,"updatedAt":"2026-01-01T00:00:00Z"}"#;
        fs::write(mem_dir.join("memory.json"), original).unwrap();

        assert!(MemoryStore::lock(dir.path()).is_err());
        assert_eq!(
            fs::read_to_string(mem_dir.join("memory.json")).unwrap(),
            original
        );
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut graph = empty_graph(dir.path());
        add_node(&mut graph, make_node("n1", "first insight"));
        add_node(&mut graph, make_node("n2", "second insight"));

        MemoryStore::save(dir.path(), &graph).unwrap();

        let loaded = MemoryStore::load(dir.path()).unwrap();
        assert_eq!(loaded.nodes.len(), 2);
        assert_eq!(loaded.nodes[0].id, "n1");
        assert_eq!(loaded.nodes[1].content, "second insight");
        assert_eq!(loaded.schema_version, "1.0");
    }

    #[test]
    fn save_creates_gitignore() {
        let dir = TempDir::new().unwrap();
        let graph = empty_graph(dir.path());
        MemoryStore::save(dir.path(), &graph).unwrap();

        let gitignore = dir
            .path()
            .join(".repolens")
            .join("memory")
            .join(".gitignore");
        assert!(gitignore.exists(), ".gitignore should be created by save");
        let contents = fs::read_to_string(&gitignore).unwrap();
        assert!(
            contents.contains("memory.json"),
            "gitignore should contain memory.json"
        );
    }

    #[test]
    fn add_node_updates_timestamp() {
        let dir = TempDir::new().unwrap();
        let mut graph = empty_graph(dir.path());
        let before = graph.updated_at;

        // Small sleep to ensure the timestamp advances (OffsetDateTime resolution is
        // sub-millisecond, but the system clock may be coarse).
        std::thread::sleep(std::time::Duration::from_millis(5));
        add_node(&mut graph, make_node("n1", "content"));

        assert!(
            graph.updated_at >= before,
            "updated_at should advance after add_node"
        );
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn remove_node_cleans_up_edges() {
        let dir = TempDir::new().unwrap();
        let mut graph = empty_graph(dir.path());
        add_node(&mut graph, make_node("a", "node a"));
        add_node(&mut graph, make_node("b", "node b"));
        add_node(&mut graph, make_node("c", "node c"));
        add_edge(
            &mut graph,
            MemoryEdge {
                source_id: "a".to_string(),
                target_id: "b".to_string(),
                relation: MemoryRelation::RelatesTo,
            },
        );
        add_edge(
            &mut graph,
            MemoryEdge {
                source_id: "b".to_string(),
                target_id: "c".to_string(),
                relation: MemoryRelation::Supersedes,
            },
        );
        assert_eq!(graph.edges.len(), 2);

        remove_node(&mut graph, "b");

        assert_eq!(graph.nodes.len(), 2, "node b should be removed");
        assert_eq!(
            graph.edges.len(),
            0,
            "both edges referencing b should be removed"
        );
        assert!(!graph.nodes.iter().any(|n| n.id == "b"));
    }

    #[test]
    fn touch_node_increments_access() {
        let dir = TempDir::new().unwrap();
        let mut graph = empty_graph(dir.path());
        add_node(&mut graph, make_node("n1", "content"));
        assert_eq!(graph.nodes[0].access_count, 0);

        touch_node(&mut graph, "n1");
        assert_eq!(graph.nodes[0].access_count, 1);

        touch_node(&mut graph, "n1");
        assert_eq!(graph.nodes[0].access_count, 2);
    }

    #[test]
    fn add_edge_deduplicates() {
        let dir = TempDir::new().unwrap();
        let mut graph = empty_graph(dir.path());
        add_node(&mut graph, make_node("a", "a"));
        add_node(&mut graph, make_node("b", "b"));

        let edge = MemoryEdge {
            source_id: "a".to_string(),
            target_id: "b".to_string(),
            relation: MemoryRelation::RelatesTo,
        };

        add_edge(&mut graph, edge.clone());
        add_edge(&mut graph, edge.clone());
        add_edge(&mut graph, edge);

        assert_eq!(graph.edges.len(), 1, "duplicate edges should not be added");
    }

    #[test]
    fn lock_guard_serializes_concurrent_updates() {
        let dir = TempDir::new().unwrap();
        let root = Arc::new(dir.path().to_path_buf());
        let workers = 16;
        let barrier = Arc::new(Barrier::new(workers));

        let handles: Vec<_> = (0..workers)
            .map(|i| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    let mut guard = MemoryStore::lock(&root).expect("lock memory store");
                    add_node(
                        guard.graph_mut(),
                        make_node(&format!("n{i}"), &format!("content {i}")),
                    );
                    guard.save().expect("save memory graph");
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("worker should not panic");
        }

        let graph = MemoryStore::load(dir.path()).unwrap();
        assert_eq!(graph.nodes.len(), workers);
        for i in 0..workers {
            assert!(
                graph.nodes.iter().any(|n| n.id == format!("n{i}")),
                "missing node n{i}"
            );
        }
    }
}
