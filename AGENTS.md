# AGENTS.md

This file provides guidance to AI coding agents (such as OpenAI Codex) when working with code in this repository.

## Build & Test Commands

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # cargo may not be in PATH by default

cargo build                            # Build all crates
cargo test                             # Run all tests
cargo test -p repolens-core            # Test a single crate
cargo test -p repolens-extract -- java # Run tests matching "java" in one crate
cargo clippy                           # Lint (VS Code runs this via rust-analyzer)
cargo run -- scan --root <path>        # Run a full scan
cargo run -- clean --root <path>           # Remove old packs (--root required; keeps latest 3 by default)
cargo run -- clean --root <path> --keep 1  # Keep the newest pack, plus the newest clean pack if they differ
cargo run -- mcp                       # Start MCP server over STDIO
cargo run -- wiki lint --root <path>   # Lint wiki: stale articles, broken refs, coverage
cargo run -- wiki rebuild-index --root <path>  # Rebuild tantivy search index
cargo fmt --all                        # Format code (CI enforces this)
bash scripts/gate.sh                   # Pre-push gate wrapper (cargo fmt + clippy -D warnings + test)
bash scripts/verify-all.sh             # Non-cargo gates: doc-canon registry + fixture integrity (needs bash 4+; macOS ships 3.2)
bash scripts/golden-pack-diff.sh       # Prove scan pack output is byte-identical vs a baseline ref (default main)
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace  # Pre-push gate (CI enforces all three)
```

Snapshot tests use `insta`. To review/update snapshots: `cargo insta review`.

## Architecture

Rust port of the Java RepoLens tool. Scans repositories to produce structured "packs" containing file metadata, symbols, dependency graphs, and health analysis — exposed via an MCP server for LLM consumption.

### Crate Dependency Graph

```
repolens-cli (entry point: scan + mcp commands; binary name: `repolens`)
  ├── repolens-app (orchestration: scan, graph, summary, incremental)
  │   ├── repolens-core (models, pack I/O, discovery, rules engine)
  │   ├── repolens-extract (symbol/relationship extraction per language)
  │   ├── repolens-profile (build system detection, health analysis)
  │   ├── repolens-wiki (post-scan lint hook, suggestion ranking)
  │   └── rayon (parallel file processing)
  └── repolens-mcp (MCP server: 26 tools over STDIO via rmcp)
      ├── repolens-app
      ├── repolens-core
      ├── repolens-memory (memory graph with decay-based persistence)
      └── repolens-profile
```

### Core Data Flow

1. **Discovery** (`core/discovery.rs`) — Walks repo, evaluates `.gitignore` + `.reposnapshot.rules` via `RulesEngine`
2. **Extraction** (`extract/`) — Trait-based: `SymbolExtractor` and `RelationshipExtractor` per language. Java, Rust, Go, JS/TS use tree-sitter ASTs; only Python uses regex
3. **Graph building** (`app/graph.rs`) — Multi-strategy import resolution (qualified name, simple name, same-package Java, Go module path, JS relative import expansion)
4. **Pack writing** (`core/pack_writer.rs`) — Chunked JSON output to `pack-YYYYMMDD-HHMMSS/` directories with manifest, file chunks, graph, and profile
5. **Incremental scanning** (`app/incremental.rs`) — Validates base pack (schema version, config hash, commit reachability), diffs changed files, merges with carried-forward entries

### MCP Server (`repolens-mcp`)

Uses `#[tool_router]` macro from `rmcp`. Each tool takes a `Parameters<T>` struct with `JsonSchema` for type-safe, self-documenting arguments. Tools return `CallToolResult` via `ok_json()`/`err_json()` helpers.

26 tools: `discover_packs`, `read_file_content`, `search_code`, `get_profile`, `analyze_health`, `get_graph`, `get_callers`, `get_dependencies`, `get_impact`, `find_cycles`, `find_dead_code`, `get_conventions`, `get_architecture`, `get_summary`, `compare_summaries`, `memorize_insight`, `recall_longterm`, `forget_insight`, `wiki_compile`, `wiki_read`, `wiki_search`, `wiki_index`, `wiki_lint`, `wiki_delete`, `wiki_suggest`, `wiki_ingest_external`.

### Extract Pipeline

Pluggable via trait + registry pattern (`SymbolExtractorRegistry` / `RelationshipExtractorRegistry`). Language detection in `lang.rs` maps 60+ file extensions. Six languages have extractors: Java, Rust, Go, JavaScript/TypeScript all use tree-sitter (shared helpers in `ts_util.rs`); Python uses regex.

### Memory System (`repolens-memory`)

`MemoryGraph` stores typed nodes (`CodeAnchor`, `Decision`, `Convention`, `TradeOff`, `Pattern`) with edges (`Supersedes`, `RelatesTo`, `Contradicts`; `GroundedBy`/`ContradictedBy` exist but are unconsumed). Uses exponential decay (`MemoryDecayEngine`) with lazy evaluation on load; decay also consumes a persisted recall-time grounding snapshot (`MemoryNode.last_grounding`): `Confirmed` → importance floor (slower decay), `Contradicted` → multiplier (faster decay). `repolens scan` and `get_summary` also write `last_grounding`. Atomic writes via temp file + rename.

## Key Patterns

- All JSON uses `#[serde(rename_all = "camelCase")]` for Java contract parity
- Builder pattern on `FileDiscovery`, `PackWriter`, `ScanService`
- Atomic file I/O (write to `.tmp`, rename) for durability
- `Arc<Mutex<>>` for shared state in MCP server (pack cache)
- `read_file_content` includes path traversal defense via canonicalization + prefix check
- MCP param structs use per-field `#[serde(rename = "...")]` for camelCase fields, NOT `rename_all` on the struct — always add explicit rename for multi-word fields like `maxDepth`
- MCP integration tests that assert on envelope fields (`evidenceGrade`, `knowledgeLayer`, `confidence`, `provenance`) need `call_*_envelope` helpers — the default `call_*` helpers run through `extract_text` which auto-unwraps `.data`

## Storage Layout

Scan output goes to `.repolens/` inside the target repo:
```
.repolens/
  ├── pack-YYYYMMDD-HHMMSS/   (manifest.json, files-NNNN.json chunks, graph.json, graph-metrics.json, project-profile.json)
  ├── session.json             (active pack, session ID, last scan mode)
  ├── wiki/                    (compiled wiki articles + tantivy search index)
  └── memory/memory.json       (memory graph with decay metadata)
```

## Workflow

For multi-commit arcs (3+ commits), create a feature branch from commit 1 rather than committing directly to `main` — makes PR creation a no-op instead of requiring a late-stage branch-rewind.

### Parallel agent sessions

When multiple agents (Claude Code, Codex, etc.) may touch this repo at once:

- One live session per worktree. Two sessions sharing a worktree race on the working tree and index.
- At most one worktree may have a given branch checked out — git enforces this. If you need to work on a branch another session holds, either pick a different branch or ask the user to close that session.
- Each agent gets its own branch via `git worktree add`, so collisions become impossible at the git layer.
- Run `git worktree list` before touching anything — different tools place their worktrees differently, and you can't assume another session's path.
- Treat worktrees you didn't create as read-only. Never `git worktree remove`, `git branch -D`, or `git reset --hard` a foreign worktree's state without asking the user first.
- Coordinate through committed state and branch refs — both sessions already share the same `.git` dir locally, so every commit is visible via `git log` without a fetch. Use push/pull/fetch only when crossing clones or machines.
- Cargo's `target/` directory is shared by worktrees unless overridden. Parallel `cargo build` runs will lock-contend. Set `CARGO_TARGET_DIR` per-worktree if you need concurrent builds.

## Known Issues

`cargo test -p repolens-core --test discovery_test` — `records_dot_git_entries_as_excluded_git` is marked `#[ignore]` (expects `.git` entries in fixture that aren't present).

## Formatting & Linting

No `rustfmt.toml` or `clippy.toml` — uses Rust defaults. VS Code setting `rust-analyzer.check.command` is set to `clippy`. CI enforces `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

rustfmt collapses short `fn` signatures to one line when they fit the width budget — always run `cargo fmt --all` before committing, even if the code looks tidy.

## Design Documents

`docs/` contains design specs and philosophy docs that drive feature work. Check here for context before implementing new features.

## Test Fixtures

`fixtures/scenarios/` contains captured Java RepoLens output used for contract parity tests. `fixtures/CONTRACT_SOURCES.md` maps each fixture to its Java source. Scripts in `scripts/` manage fixture capture, refresh, sanitization, and verification.

`fixtures/inputs/` contains small repo fixtures (java-small, rust-small, etc.) used by integration tests for scanning. `fixtures/test-data/` contains standalone data fixtures (e.g., cyclic-graph) consumed directly by unit tests.
