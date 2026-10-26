# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # cargo may not be in PATH by default

cargo run -- scan --root <path>        # Run a full scan
cargo run -- clean --root <path>           # Remove old packs (--root required; keeps latest 3 by default)
cargo run -- clean --root <path> --keep 1  # Keep the newest pack, plus the newest clean pack if they differ
cargo run -- brief --root <path>       # Preflight brief: live git state, active pack, inlined findings (--task "<desc>" optional; --stdout prints instead of writing the artifact)
cargo run -- findings --root <path>    # Print the active pack's calibrated F-* evidence findings (markdown or --format json)
cargo run -- state --root <path>       # Read-only inventory: packs, memory graph, session handoff
cargo run -- validate-plan --file <plan.md>  # Warnings-only plan lint (guardrail, not proof)
scripts/dev/mcp-call.sh <tool> <args>  # One-shot MCP call (e.g. discover_packs '{"root":"."}')
bash scripts/dev/test-mcp-call.sh      # Hermetic smoke test for the MCP helper
cargo run -- wiki lint --root <path>   # Lint wiki: stale articles, broken refs, coverage
cargo run -- wiki rebuild-index --root <path>  # Rebuild tantivy search index
bash scripts/verify-doc-canon.sh       # Verify doc-canon registry ↔ tree (CI: Doc Canon job; needs bash 4+ and Ruby)
cargo fmt --all                        # Format code (CI enforces this)
bash scripts/gate.sh                   # Pre-push gate wrapper (cargo fmt + clippy -D warnings + test)
bash scripts/verify-all.sh             # Non-cargo gates: doc-canon registry + fixture integrity (needs bash 4+; macOS ships 3.2)
bash scripts/golden-pack-diff.sh       # Prove scan pack output is byte-identical vs a baseline ref (default main)
```

Snapshot tests use `insta`. To review/update snapshots: `cargo insta review`.

## Architecture

Rust port of the Java RepoLens tool. Scans repositories to produce structured "packs" containing file metadata, symbols, dependency graphs, and health analysis — exposed via an MCP server for LLM consumption.

### Core Data Flow

1. **Discovery** (`core/discovery.rs`) — Walks repo, evaluates `.gitignore` + `.reposnapshot.rules` via `RulesEngine`
2. **Extraction** (`extract/`) — Trait-based: `SymbolExtractor` and `RelationshipExtractor` per language. Java, Rust, Go, JS/TS use tree-sitter ASTs; only Python uses regex
3. **Graph building** (`app/graph.rs`) — Multi-strategy import resolution (qualified name, simple name, same-package Java, Go module path, JS relative import expansion)
4. **Pack writing** (`core/pack_writer.rs`) — Chunked JSON output to `pack-YYYYMMDD-HHMMSS/` directories with manifest, file chunks, graph, and profile
5. **Incremental scanning** (`app/incremental.rs`) — Validates base pack (schema version, config hash, commit reachability), diffs changed files, merges with carried-forward entries

### MCP Server (`repolens-mcp`)

Conventions and gotchas for the MCP crate live in `crates/repolens-mcp/CLAUDE.md`.

### Memory System (`repolens-memory`)

`MemoryGraph` stores typed nodes (`CodeAnchor`, `Decision`, `Convention`, `TradeOff`, `Pattern`) with edges (`Supersedes`, `RelatesTo`, `Contradicts`; `GroundedBy`/`ContradictedBy` exist but are unconsumed). Uses exponential decay (`MemoryDecayEngine`) with lazy evaluation on load; atomic writes via temp file + rename. Decay also consumes a persisted recall-time grounding snapshot (`MemoryNode.last_grounding`): `Confirmed` → importance floor (slower decay), `Contradicted` → multiplier (faster decay) — see doc 07 row 26. `repolens scan` and `get_summary` also write `last_grounding` (scan grounds all memories; get_summary the surfaced insights) — recall is no longer the only writer.

## Key Patterns

- All JSON uses `#[serde(rename_all = "camelCase")]` for Java contract parity
- Builder pattern on `FileDiscovery`, `PackWriter`, `ScanService`
- Atomic file I/O (write to `.tmp`, rename) for durability
- `Arc<Mutex<>>` for shared state in MCP server (pack cache)
- `read_file_content` includes path traversal defense via canonicalization + prefix check
- In `python3 -c 'PYEOF'` heredocs driven from bash, avoid f-strings with `\"`-escaped dict keys (e.g. `f"x={data[\"k\"]}"`) — they raise SyntaxError. Use concatenation: `"x=" + str(data["k"])`
- Additive wire fields are `Option<T>` + explicit `#[serde(skip_serializing_if = "Option::is_none")]` so `None` emits no key — proven byte-invisible by the pack byte-identity snapshots (`Symbol.line` arc). Existing sibling Options serialize `null`; the attribute is per-field, never assumed.
- `scripts/golden-pack-diff.sh` diffs every pack file EXCEPT `manifest.json` (skipped at :110) — additive manifest fields carry no golden risk; fields on `Symbol`/graph land in diffed files and need a snapshot regen when populated.
- In zsh, quote grep include globs (`--include='*.rs'`) — unquoted `--include=*.rs` fails with "no matches found" before grep even runs.

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

CI runs on GitHub-hosted runners only. Pull requests run `Build & Test (ubuntu-latest)` and `Doc Canon`; `Build & Test (macos-latest)` runs only on push to main and on manual `workflow_dispatch`. A green local pre-push gate plus green ubuntu CI is the PR signal; macOS is checked on main.

## Formatting & Linting

No `rustfmt.toml` or `clippy.toml` — uses Rust defaults. VS Code setting `rust-analyzer.check.command` is set to `clippy`. CI enforces `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

rustfmt collapses short `fn` signatures to one line when they fit the width budget — always run `cargo fmt --all` before committing, even if the code looks tidy.

## Design Documents

**`docs/philosophy/07-current-state-vs-direction.md` is the canonical "shipped today" reference** — read it before making claims about what's live; if docs conflict, doc 07 wins. The `docs/` subtree roles, narration discipline, and the CI-gated doc-canon registry rules live in `docs/CLAUDE.md`.
