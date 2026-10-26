//! Populate the repolens-rs wiki with real articles and leave them on disk.
//! Run with: cargo test -p repolens-mcp --test wiki_populate_test -- --nocapture

use repolens_app::discover::PackResolver;
use repolens_mcp::RepoLensMcpServer;

fn repo_root() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

#[test]
#[ignore] // writes real wiki articles to disk; run manually
fn populate_repolens_wiki() {
    let root = repo_root();
    let server = RepoLensMcpServer::with_resolver(PackResolver::new());

    // Clear previous wiki state
    let wiki_dir = std::path::Path::new(&root).join(".repolens").join("wiki");
    if wiki_dir.exists() {
        std::fs::remove_dir_all(&wiki_dir).ok();
    }

    println!("\nPopulating repolens-rs wiki...\n");

    // -- Overview --
    server.call_wiki_compile_full(
        &root,
        "Architecture Overview",
        "OVERVIEW",
        r#"# Architecture Overview

RepoLens is a Rust port of the Java RepoLens tool. It scans repositories to produce structured "packs" containing file metadata, symbols, dependency graphs, and health analysis — exposed via an MCP server for LLM consumption.

## Crate Dependency Graph

```
repolens-cli (entry point)
  ├── repolens-app (orchestration: scan, graph, summary, incremental)
  │   ├── repolens-core (models, pack I/O, discovery, rules engine)
  │   ├── repolens-extract (symbol/relationship extraction per language)
  │   ├── repolens-profile (build system detection, health analysis)
  │   └── rayon (parallel file processing)
  └── repolens-mcp (MCP server: 24 tools over STDIO via rmcp)
      ├── repolens-app
      ├── repolens-core
      ├── repolens-memory (memory graph with decay-based persistence)
      ├── repolens-wiki (compiled knowledge articles)
      └── repolens-profile
```

## Core Data Flow

1. **Discovery** — walks repo, evaluates .gitignore + rules
2. **Extraction** — trait-based symbol/relationship extraction per language
3. **Graph building** — multi-strategy import resolution
4. **Pack writing** — chunked JSON output to timestamped directories
5. **MCP serving** — 24 tools exposed over STDIO for agent consumption

## Three Truth Layers

- **Packs** (structural truth) — immutable snapshots of what the code IS
- **Memory** (interpretive truth) — mutable agent insights that decay over time
- **Wiki** (compiled truth) — synthesized articles grounded to code"#,
        "High-level architecture of the repolens-rs workspace: crate graph, data flow, and three truth layers",
        Some(vec![
            "crates/repolens-cli/src/main.rs",
            "crates/repolens-app/src/scan.rs",
            "crates/repolens-mcp/src/lib.rs",
        ]),
        None,
        Some(vec!["architecture", "overview"]),
        Some(0.95),
        None,
    );
    println!("  [OVERVIEW] Architecture Overview");

    // -- Entity: Memory Decay Engine --
    server.call_wiki_compile_full(
        &root,
        "Memory Decay Engine",
        "ENTITY",
        r#"# Memory Decay Engine

The `MemoryDecayEngine` manages importance decay for persistent memory nodes stored in `.repolens/memory/memory.json`.

## Algorithm

Each memory node has an `importance` field (0.0–1.0). On recall, the engine applies:

1. **Half-life decay**: importance * 0.5^(elapsed_days / 30)
2. **Access floor**: `min(0.3, access_count * 0.02)` — frequently accessed nodes retain importance
3. **Supersession multiplier**: 0.5x for nodes marked as superseded by newer insights
4. **Pruning**: nodes below 0.05 importance are removed entirely

## Lazy Evaluation

Decay runs lazily — only when `recall_longterm` is called and `last_decay_run` is either None or more than 1 hour old. This avoids unnecessary I/O on every memory operation.

## Key Types

- `MemoryDecayEngine::decay(graph, now) -> MemoryGraph` — pure function, returns new graph
- `MemoryNode.importance` — the field being decayed
- `MemoryNode.access_count` — drives the access floor"#,
        "Exponential decay engine for memory node importance with 30-day half-life, access floor, and lazy evaluation",
        Some(vec![
            "crates/repolens-memory/src/decay.rs",
            "crates/repolens-memory/src/store.rs",
        ]),
        None,
        Some(vec!["memory", "decay", "persistence"]),
        None,
        None,
    );
    println!("  [ENTITY]   Memory Decay Engine");

    // -- Entity: MCP Server --
    server.call_wiki_compile_full(
        &root,
        "MCP Server",
        "ENTITY",
        r#"# MCP Server

The `RepoLensMcpServer` exposes 24 tools over STDIO using the rmcp framework. It is the primary interface for LLM agents to interact with RepoLens.

## Tool Categories

### Code Exploration (6 tools)
- `discover_packs` — find available scan packs
- `read_file_content` — read source with detail levels (full/outline/signatures)
- `search_code` — path/content/symbol search with regex support
- `get_profile` — project profile (build system, frameworks, languages)
- `analyze_health` — structural health metrics
- `get_conventions` — read the curated convention allowlist (CLAUDE.md, AGENTS.md, GEMINI.md, .github/copilot-instructions.md, .cursorrules, CONVENTIONS.md, CONTRIBUTING.md, BACKLOG.md) + .claude/rules/*.md as a sources[] manifest

### Graph Navigation (6 tools)
- `get_graph` — file dependency graph with neighbor extraction
- `get_callers` / `get_dependencies` — directed traversal
- `get_impact` — blast radius analysis
- `find_cycles` — dependency cycle detection
- `find_dead_code` — unreferenced file detection

### Summary & Architecture (3 tools)
- `get_summary` — tiered summaries (compact ~2KB, standard ~8KB, full ~25KB)
- `compare_summaries` — diff between two summary snapshots
- `get_architecture` — scoped or full architectural view

### Memory (3 tools)
- `memorize_insight` — store persistent knowledge with anchors and tags
- `recall_longterm` — keyword search with decay and grounding assessment
- `forget_insight` — delete memories (dry-run by default)

### Wiki (6 tools)
- `wiki_compile` — create/update knowledge articles
- `wiki_read` / `wiki_search` / `wiki_index` — read and discover articles
- `wiki_lint` — detect stale anchors and broken memory refs
- `wiki_delete` — remove articles (dry-run by default)

## Architecture Pattern

Uses `#[tool_router]` macro from rmcp. Each tool takes a `Parameters<T>` struct with JsonSchema for type-safe arguments. Shared state (PackResolver, SearchService) is wrapped in `Arc` for thread-safe access."#,
        "MCP server exposing 24 tools over STDIO for LLM agent interaction with RepoLens",
        Some(vec![
            "crates/repolens-mcp/src/lib.rs",
            "crates/repolens-mcp/src/tools/wiki.rs",
            "crates/repolens-mcp/src/tools/memory.rs",
        ]),
        None,
        Some(vec!["mcp", "tools", "api"]),
        Some(0.9),
        None,
    );
    println!("  [ENTITY]   MCP Server");

    // -- Entity: Grounding System --
    server.call_wiki_compile_full(
        &root,
        "Grounding System",
        "ENTITY",
        r#"# Grounding System

Grounding validates whether memory insights and wiki articles are still accurate by checking their anchors against the latest scan pack.

## Two Consumers

### Memory Grounding (`repolens-app::grounding`)
- Called during `recall_longterm`
- Returns `GroundingStatus`: Confirmed, Contradicted, Unverifiable, Unchecked
- Checks file paths and qualified symbol names against pack entries

### Wiki Staleness (`repolens-wiki::staleness`)
- Called during `wiki_compile` and `wiki_lint`
- Returns `Staleness`: Fresh, Stale, Unverifiable
- Same anchor-matching logic, different output type

## Anchor Matching

An anchor is "groundable" if it contains `/` or `.` (file paths and qualified symbols). Bare names like `AuthService` are not checked — too ambiguous.

For each groundable anchor:
1. Check `FileEntry.path` for exact match
2. Check `FileEntry.symbols[].qualified_name` for exact match
3. All found → Fresh/Confirmed, any missing → Stale/Contradicted"#,
        "Anchor-based validation of memory and wiki against scan pack data",
        Some(vec![
            "crates/repolens-app/src/grounding.rs",
            "crates/repolens-wiki/src/staleness.rs",
        ]),
        None,
        Some(vec!["grounding", "staleness", "validation"]),
        None,
        None,
    );
    println!("  [ENTITY]   Grounding System");

    // -- Concept: Extract Pipeline --
    server.call_wiki_compile_full(
        &root,
        "Extract Pipeline",
        "CONCEPT",
        r#"# Extract Pipeline

The extraction pipeline turns source code into structured symbols and relationships using a pluggable, trait-based registry pattern.

## Two Traits

- `SymbolExtractor` — extracts symbols (classes, functions, methods, types) from source
- `RelationshipExtractor` — extracts relationships (imports, extends, implements, calls) between files

Each trait has a registry (`SymbolExtractorRegistry`, `RelationshipExtractorRegistry`) that maps languages to their extractor implementation.

## Language Support

| Language | Symbol Extractor | Relationship Extractor |
|----------|-----------------|----------------------|
| Java | tree-sitter (full AST) | tree-sitter |
| Rust | tree-sitter | tree-sitter |
| JavaScript/TypeScript | tree-sitter | regex |
| Go | regex | regex |
| Python | regex | regex |

## Resolution

After extraction, the `GraphBuilder` resolves relationships to file paths using multiple strategies: qualified name lookup, simple name matching, same-package Java resolution, Go module paths, and JS relative import expansion."#,
        "Pluggable symbol and relationship extraction pipeline with tree-sitter and regex backends",
        Some(vec![
            "crates/repolens-extract/src/registry.rs",
            "crates/repolens-app/src/graph.rs",
        ]),
        None,
        Some(vec!["extraction", "symbols", "relationships", "tree-sitter"]),
        None,
        None,
    );
    println!("  [CONCEPT]  Extract Pipeline");

    // -- Concept: Three Truth Layers --
    server.call_wiki_compile_full(
        &root,
        "Three Truth Layers",
        "CONCEPT",
        r#"# Three Truth Layers

RepoLens organizes knowledge into three distinct layers, each with its own storage format, lifecycle, and source of truth.

## Layer 1: Packs (Structural Truth)

What the code IS at a point in time. Produced by scans. Immutable once written.

- Storage: chunked JSON in `.repolens/pack-YYYYMMDD-HHMMSS/`
- Contains: file metadata, symbols, dependency graph, profile, health
- Lifecycle: created on scan, old packs cleaned up by retention policy

## Layer 2: Memory (Interpretive Truth)

What agents and humans BELIEVE about the code. Mutable, subject to decay.

- Storage: single `memory.json` in `.repolens/memory/`
- Contains: typed insights (Pattern, Decision, Convention, TradeOff, CodeAnchor) with edges
- Lifecycle: created by agents via `memorize_insight`, decays over 30-day half-life, pruned below 0.05

## Layer 3: Wiki (Compiled Truth)

What has been UNDERSTOOD by synthesizing packs and memory. Mutable, grounded.

- Storage: markdown articles + `wiki-index.json` in `.repolens/wiki/`
- Contains: entity pages, concept pages, overviews, comparisons
- Lifecycle: created by agents via `wiki_compile`, staleness checked against packs

## Design Principle

Keeping these layers separate is a core architectural decision. Each models a different kind of truth with different confidence levels and update patterns."#,
        "RepoLens three-layer truth model separating structural, interpretive, and compiled knowledge",
        Some(vec![
            "crates/repolens-core/src/model/wiki.rs",
            "crates/repolens-core/src/model/memory.rs",
            "crates/repolens-core/src/pack_writer.rs",
        ]),
        None,
        Some(vec!["architecture", "philosophy", "truth-model"]),
        Some(0.95),
        None,
    );
    println!("  [CONCEPT]  Three Truth Layers");

    // -- Comparison: Tree-sitter vs Regex Extraction --
    server.call_wiki_compile_full(
        &root,
        "Tree-sitter vs Regex Extraction",
        "COMPARISON",
        r#"# Tree-sitter vs Regex Extraction

RepoLens uses two approaches for extracting symbols and relationships from source code.

## Tree-sitter (Java, Rust)

**Pros:**
- Full AST parsing — captures nested structures, visibility, qualified names
- Handles complex syntax reliably (generics, annotations, macros)
- Extracts relationships from actual import/use statements

**Cons:**
- Requires a grammar crate per language (`tree-sitter-java`, `tree-sitter-rust`)
- More code to write and maintain (tree-sitter query patterns)
- Slower than regex for simple patterns

## Regex (Go, Python, JS/TS relationships)

**Pros:**
- Simple to implement and maintain
- Fast for straightforward patterns (function declarations, import statements)
- No grammar dependency

**Cons:**
- Fragile with complex syntax (nested functions, multi-line declarations)
- Cannot determine visibility or qualified names reliably
- Misses context (e.g., methods inside anonymous classes)

## Migration Path

The project is migrating from regex to tree-sitter for all languages. Java and Rust are complete. JavaScript/TypeScript symbols use tree-sitter but relationships still use regex."#,
        "Trade-offs between tree-sitter AST parsing and regex-based extraction in RepoLens",
        Some(vec![
            "crates/repolens-extract/src/java_symbol.rs",
            "crates/repolens-extract/src/rust_symbol.rs",
            "crates/repolens-extract/src/go_symbol.rs",
        ]),
        None,
        Some(vec!["extraction", "tree-sitter", "regex", "trade-offs"]),
        None,
        None,
    );
    println!("  [COMPARISON] Tree-sitter vs Regex");

    // -- Print final state --
    println!("\n--- Wiki Index ---");
    let result = server.call_wiki_index(&root, None);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("{}", serde_json::to_string_pretty(&v).unwrap());

    println!("\n--- Lint Results ---");
    let result = server.call_wiki_lint(&root);
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    println!("{}", serde_json::to_string_pretty(&v).unwrap());

    // DO NOT clean up — leave files on disk for inspection
    println!("\nWiki files written to: {}/.repolens/wiki/", root);
    println!("Done. Articles left on disk for inspection.");
}
