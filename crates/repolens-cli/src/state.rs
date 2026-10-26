//! `repolens state` - read-only inventory of local RepoLens persistence surfaces.

use anyhow::{anyhow, Result};
use clap::Args;
use repolens_app::discover::list_pack_dirs;
use repolens_core::model::memory::{MemoryGraph, MemoryType};
use repolens_core::model::session::RepoSession;
use repolens_core::storage::RepoStoragePaths;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{brief::is_template_only_workspace_body, resolve_root_from};

const SESSION_MEMORY_FILENAME: &str = "session-memory.md";

#[derive(Args, Debug)]
pub struct StateArgs {
    /// Path to the repository root (defaults to enclosing git repo, then cwd)
    #[arg(long)]
    pub root: Option<PathBuf>,
}

pub fn run_state(args: StateArgs) -> Result<()> {
    let cwd =
        std::env::current_dir().map_err(|e| anyhow!("Cannot resolve current directory: {}", e))?;
    let root = resolve_root_from(args.root, &cwd)?;
    let storage_dir = RepoStoragePaths::storage_dir(&root);

    println!("RepoLens state");
    println!();
    println!("Root: {}", root.display());
    println!();

    if !storage_dir.is_dir() {
        println!("RepoLens workspace: missing");
        println!("Run `repolens init` or `repolens scan` to create .repolens/.");
        return Ok(());
    }

    print_scan_packs(&root, &storage_dir)?;
    println!();
    print_typed_memory(&storage_dir);
    println!();
    print_session_handoff(&storage_dir);

    Ok(())
}

fn print_scan_packs(root: &Path, storage_dir: &Path) -> Result<()> {
    println!("Scan packs");

    let packs = list_pack_dirs(root)?;
    let session = read_session(&RepoStoragePaths::session_path(root));

    println!(
        "  Status: {}",
        if packs.is_empty() {
            "missing"
        } else {
            "present"
        }
    );
    println!("  Surface: derived repo context, regenerable");
    println!(
        "  Path: {}",
        rel_display(storage_dir, storage_dir.join("pack-*").as_path())
    );
    println!("  Packs on disk: {}", packs.len());

    if let SessionState::Present(session) = session {
        let active_path = Path::new(&session.active_pack_path);
        let active_pack = Path::new(&session.active_pack_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("(unknown)");
        if active_path.join("manifest.json").is_file() {
            println!("  Active pack: {active_pack}");
        } else {
            println!("  Active pack: {active_pack} (missing on disk)");
        }
        println!("  Last scan mode: {}", session.last_scan_mode);
    } else {
        match session {
            SessionState::Missing => {
                println!("  Active pack: (none; no session.json)");
                println!("  Last scan mode: (unknown)");
            }
            SessionState::Unreadable(err) => {
                println!("  Active pack: (unknown; session.json unreadable)");
                println!("  Last scan mode: (unknown)");
                println!("  Session error: {err}");
            }
            SessionState::Present(_) => unreachable!("present session handled above"),
        }
    }

    println!("  Written by: repolens scan");
    println!("  Note: not long-term memory; safe to regenerate.");
    Ok(())
}

fn print_typed_memory(storage_dir: &Path) {
    println!("Typed memory graph");
    println!("  Surface: structured long-term memory");
    println!("  Path: .repolens/memory/memory.json");

    let memory_path = storage_dir.join("memory").join("memory.json");
    match read_memory_graph(&memory_path) {
        MemoryGraphState::Missing => {
            println!("  Status: missing");
            println!("  Nodes: 0");
            println!("  Edges: 0");
        }
        MemoryGraphState::Unreadable(err) => {
            println!("  Status: unreadable");
            println!("  Error: {err}");
        }
        MemoryGraphState::Present(graph) => {
            println!("  Status: present");
            println!("  Nodes: {}", graph.nodes.len());
            println!("  Edges: {}", graph.edges.len());
            if graph.nodes.is_empty() {
                println!("  Types: (none)");
            } else {
                println!("  Types:");
                for (memory_type, count) in memory_type_counts(&graph) {
                    println!("    {memory_type}: {count}");
                }
            }
            if let Some(newest) = graph.nodes.iter().max_by_key(|n| n.created) {
                let mut content: String = newest.content.chars().take(60).collect();
                if newest.content.chars().count() > 60 {
                    content.push('…');
                }
                println!(
                    "  Newest: {} \"{content}\" (importance {:.2})",
                    repolens_app::memory_ops::memory_type_name(&newest.memory_type),
                    newest.importance
                );
            }
            let local = crate::brief::local_utc_offset_secs(graph.updated_at);
            match crate::brief::format_session_time(graph.updated_at, local) {
                Some(t) => println!("  Last updated: {t}"),
                None => println!("  Last updated: {}", graph.updated_at),
            }
        }
    }

    println!(
        "  Written by: repolens memory save/recall, MCP memorize_insight / recall_longterm / forget_insight"
    );
    println!("  Note: repolens remember does not write this file.");
}

fn print_session_handoff(storage_dir: &Path) {
    println!("Session handoff markdown");
    println!("  Surface: unverified session handoff notes");
    println!("  Path: .repolens/{SESSION_MEMORY_FILENAME}");

    let path = storage_dir.join(SESSION_MEMORY_FILENAME);
    match fs::read_to_string(&path) {
        Ok(body) => {
            let status = session_memory_status(&body);
            println!("  Status: {status}");
            println!("  Lines: {}", body.lines().count());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("  Status: missing");
            println!("  Lines: 0");
        }
        Err(e) => {
            println!("  Status: unreadable");
            println!("  Error: {e}");
        }
    }

    println!("  Written by: repolens remember --file");
    println!("  Note: included by repolens brief; not typed memory.");
}

enum MemoryGraphState {
    Missing,
    Unreadable(String),
    Present(MemoryGraph),
}

enum SessionState {
    Missing,
    Unreadable(String),
    Present(RepoSession),
}

fn read_session(path: &Path) -> SessionState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return SessionState::Missing,
        Err(e) => return SessionState::Unreadable(e.to_string()),
    };

    match serde_json::from_str::<RepoSession>(&raw) {
        Ok(session) => SessionState::Present(session),
        Err(e) => SessionState::Unreadable(e.to_string()),
    }
}

fn read_memory_graph(path: &Path) -> MemoryGraphState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return MemoryGraphState::Missing,
        Err(e) => return MemoryGraphState::Unreadable(e.to_string()),
    };

    match serde_json::from_str::<MemoryGraph>(&raw) {
        Ok(graph) => MemoryGraphState::Present(graph),
        Err(e) => MemoryGraphState::Unreadable(e.to_string()),
    }
}

fn memory_type_counts(graph: &MemoryGraph) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for node in &graph.nodes {
        *counts
            .entry(memory_type_label(&node.memory_type))
            .or_insert(0) += 1;
    }
    counts
}

fn memory_type_label(memory_type: &MemoryType) -> &'static str {
    match memory_type {
        MemoryType::CodeAnchor => "CODE_ANCHOR",
        MemoryType::Decision => "DECISION",
        MemoryType::Convention => "CONVENTION",
        MemoryType::TradeOff => "TRADE_OFF",
        MemoryType::Pattern => "PATTERN",
    }
}

fn session_memory_status(body: &str) -> &'static str {
    if body.trim().is_empty() {
        "empty"
    } else if is_template_only_workspace_body(body) {
        "template-only"
    } else {
        "filled"
    }
}

fn rel_display(storage_dir: &Path, path: &Path) -> String {
    path.strip_prefix(storage_dir.parent().unwrap_or(storage_dir))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}
