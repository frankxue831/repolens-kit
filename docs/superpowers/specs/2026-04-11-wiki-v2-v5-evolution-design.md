# Wiki v2--v5 Evolution Design

**Status**: Draft
**Depends on**: [Wiki v1 Compiled Knowledge Layer](2026-04-11-wiki-compiled-knowledge-layer-design.md)

## Overview

Evolves the wiki compiled knowledge layer from v1 (storage, keyword search, on-demand staleness) through four versions that add richer search, auto-suggestion, background lint, and external source ingestion. Designed as a unified spec so interfaces align across versions, but each version ships independently.

### Architectural approach: Thin wiki, smart app

- `repolens-wiki` handles wiki data: storage, search (tantivy), staleness, external-article storage
- `repolens-app` handles cross-cutting intelligence: suggestion logic (needs pack + graph data), post-scan lint hook
- `repolens-cli` adds `wiki` subcommand group
- `repolens-mcp` adds new tools (`wiki_suggest`, `wiki_ingest_external`) and modifies existing ones

## Data Model Changes

### External source attribution (v5)

v5 deliberately expands the v1 source model. In v1, external documents were expected to flow through Memory first; in v5, wiki may also store first-class externally sourced articles directly when the agent supplies explicit `source` metadata. This does **not** require a mirrored memory node. `memoryRefs` remains optional provenance when an external article was also derived from prior memory insights. Packs + memory remain the canonical grounding sources for repo-derived articles.

New struct in `repolens-core::model::wiki`:

```rust
pub struct ExternalSource {
    pub origin: String,          // URL, doc name, or identifier
    pub version: Option<String>, // version/date of the source
    pub fetched_at: String,      // ISO-8601 timestamp when agent fetched it
}
```

`WikiArticle` gains an optional field:

```rust
pub source: Option<ExternalSource>,  // None for code-grounded articles
```

Any MCP response that returns full article metadata also returns `source` when present. This applies to `wiki_read.article`, `wiki_search.results[].article`, and `wiki_index` article metadata. `wiki_ingest_external` also returns the persisted `source` object in its create/update response.

External content uses the existing four article types (ENTITY, CONCEPT, OVERVIEW, COMPARISON). The type describes the article's structure, not its origin -- `source` describes origin.

### Search index (v2)

No model changes. The tantivy index is a derived artifact at `.repolens/wiki/.tantivy/`, rebuilt from source-of-truth markdown files at any time.

## v2: BM25 Search via Tantivy

### Index lifecycle

The tantivy index is a derived cache. Three operations maintain it:

- **Full rebuild**: When index directory is missing or explicitly requested via a `rebuild` flag. Reads all article markdown files and builds the index from scratch.
- **Incremental update**: After `wiki_compile`, `wiki_delete`, or `wiki_ingest_external`, the affected document is added/removed from the index. Avoids full rebuild on every write.
- **Fallback**: If the index doesn't exist or is corrupt, search falls back to the current keyword implementation, logs a warning, and triggers a rebuild. The agent gets results either way.

### Indexed fields

| Field | Tantivy type | Stored | Tokenized | Boost |
|-------|-------------|--------|-----------|-------|
| `id` | STRING | yes | no | -- |
| `title` | TEXT | yes | yes (en_stem) | 3.0 |
| `summary` | TEXT | yes | yes (en_stem) | 2.0 |
| `body` | TEXT | no | yes (en_stem) | 1.0 |
| `tags` | TEXT | yes | yes (raw) | 2.0 |
| `anchors` | TEXT | yes | yes (path tokenizer) | 1.5 |
| `article_type` | STRING | yes | no | -- |

Design choices:

- **Body indexed but not stored** -- search against it but retrieve full body from markdown via `WikiStore::read_article`.
- **English stemmer** on title/summary/body so "configuring" matches "configuration."
- **Path tokenizer** on anchors splits on `/` and `::` so searching "auth" matches `src/auth/middleware.rs` and `auth::middleware::validate`.
- **Boost weights** favor title > summary/tags > anchors > body.

### Query API

The existing `search()` function signature stays the same but internals switch from keyword tokenization to tantivy query parsing. The `wiki_search` MCP tool gains one optional parameter:

- `phraseQuery: bool` (default false) -- when true, query is treated as an exact phrase rather than individual terms. Useful for "dependency injection" or "memory decay."

Final score: `tantivy_bm25_score * article.confidence`.

### New dependency

`repolens-wiki` adds `tantivy` to `Cargo.toml`.

### CLI subcommand

```
repolens wiki rebuild-index --root <path>
```

Manual index rebuild. Optional -- index rebuilds automatically when missing.

## v3: Auto-Suggestion

### Coverage model

A file is "covered" if at least one wiki article has an anchor pointing to it (file path or qualified symbol from that file). Coverage is per-file, not per-symbol.

Coverage is computed over **included** pack entries only. Excluded entries do not count toward `totalFiles`, cannot become coverage gaps, and are ignored during anchor resolution.

Anchor normalization resolves every article anchor to an owning included file path before computing coverage:

- If the anchor exactly matches an included `FileEntry.path`, it covers that file.
- If the anchor exactly matches an included symbol's `qualified_name`, it covers that symbol's owning file.
- If the anchor matches neither, it contributes no coverage (but may still matter for staleness checks).

The covered-file set is the union of all resolved owning file paths. `coverageGaps` is computed as:

`included_pack_file_paths - covered_file_paths`

### `wiki_lint` extension

`wiki_lint` response gains a `coverageGaps` field:

```json
{
  "staleArticles": [...],
  "brokenMemoryRefs": [...],
  "coverageGaps": {
    "uncoveredFiles": 42,
    "totalFiles": 150,
    "coveragePercent": 72.0
  },
  "packAvailable": true,
  "memoryAvailable": true
}
```

The count is cheap to compute once anchors are normalized to included file paths.

### `wiki_suggest` tool

New MCP tool. Handler in `repolens-mcp`, suggestion logic in `repolens-app` (needs pack + graph access).

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `root` | string | yes | -- | Repo root |
| `limit` | number | no | 10 | Max suggestions |
| `directory` | string | no | -- | Filter to subdirectory |
| `minImportance` | number | no | -- | 0.0--1.0 threshold |

**Response:**

```json
{
  "suggestions": [
    {
      "filePath": "src/auth/middleware.rs",
      "importance": 0.85,
      "reason": "High centrality (12 importers), no wiki coverage",
      "suggestedType": "ENTITY",
      "suggestedTitle": "Auth Middleware",
      "symbols": ["auth::middleware::validate", "auth::middleware::AuthConfig"]
    }
  ],
  "coverage": {
    "uncoveredFiles": 42,
    "totalFiles": 150,
    "coveragePercent": 72.0
  }
}
```

### Ranking signals

Four signals, weighted and normalized to 0.0--1.0 within the pack:

| Signal | Weight | Source |
|--------|--------|--------|
| Import centrality | 0.4 | Graph: number of files that import this file |
| Symbol count | 0.2 | Pack: number of exported symbols |
| File size | 0.2 | Pack: lines of code |
| Dependency breadth | 0.2 | Graph: number of distinct files this file depends on via import/call/type relationships |

Weights are hardcoded, not configurable.

### Suggested type and title

Heuristic defaults the agent can override:

- Files in a single module: ENTITY
- Entry points (main.rs, lib.rs, mod.rs, index.ts): OVERVIEW
- Title: file path stem converted to title case

## v4: Background Lint

### Post-scan hook

After `ScanService::scan()` completes in `repolens-app`, it checks whether a wiki exists at `.repolens/wiki/`. If so, it runs lint automatically:

```
ScanService::scan()
  -> write pack
  -> if wiki/ exists:
      -> load wiki index
      -> flag_stale_articles(index, pack)
      -> compute coverage gaps
      -> save updated index
      -> append Lint event to wiki log
      -> log summary to tracing: "Wiki lint: 3 stale, 72% coverage"
```

The hook is **non-blocking** -- if wiki lint fails (corrupt index, I/O error), it logs a warning and the scan still succeeds.

### Dependency impact

`repolens-app` gains a dependency on `repolens-wiki`. One-way (app -> wiki), no circular dependency. Clean addition alongside existing dependencies on `repolens-core`, `repolens-extract`, `repolens-profile`.

### CLI subcommand

```
repolens wiki lint --root <path>
```

Runs the same lint logic as the MCP `wiki_lint` tool. Prints human-readable summary:

```
Wiki lint complete:
  Stale articles:      3  (auth-middleware, old-config, legacy-api)
  Broken memory refs:  1  (decision-node-abc123 in article cache-strategy)
  Coverage:            108/150 files (72.0%)
```

Exit code: 0 if no issues, 1 if any stale articles or broken refs. Composable with CI or external schedulers.

### CLI namespace

`repolens wiki lint` establishes a `wiki` subcommand group that v2's `rebuild-index` also uses:

```
repolens wiki lint --root <path>
repolens wiki rebuild-index --root <path>
```

## v5: External Source Pipeline

### `wiki_ingest_external` tool

New MCP tool. The agent pre-fetches external content and passes it in with source metadata.

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `root` | string | yes | -- | Repo root |
| `title` | string | yes | -- | Article title |
| `articleType` | string | yes | -- | ENTITY, CONCEPT, OVERVIEW, COMPARISON |
| `body` | string | yes | -- | Markdown content |
| `summary` | string | yes | -- | Short summary |
| `sourceOrigin` | string | yes | -- | URL, doc name, or identifier |
| `sourceVersion` | string | no | -- | Version string or date |
| `anchors` | string[] | no | -- | Code anchors to ground to local code |
| `memoryRefs` | string[] | no | -- | Memory node references |
| `tags` | string[] | no | -- | Tags (normalized lowercase) |
| `confidence` | number | no | 0.6 | Default lower than code-grounded (0.8) |
| `id` | string | no | -- | Update existing external article |

**Response:** Same shape as `wiki_compile`:

```json
{
  "status": "created",
  "id": "uuid",
  "path": "external-api-docs",
  "staleness": "FRESH",
  "source": {
    "origin": "https://docs.example.com/api",
    "version": "3.2",
    "fetchedAt": "2026-04-11T10:30:00Z"
  }
}
```

### Why a separate tool from `wiki_compile`

1. **Different defaults** -- external articles default to confidence 0.6 (lower than code-grounded 0.8).
2. **Required source metadata** -- `sourceOrigin` is mandatory. Adding conditionally-required fields to `wiki_compile` would be confusing.
3. **Clear agent intent** -- calling `wiki_ingest_external` signals "this came from outside" vs. `wiki_compile` signals "I synthesized this from the codebase."

### Staleness semantics for external articles

Evaluate external article staleness in this order:

1. If the article has **no groundable code anchors**, it is `FRESH` regardless of pack availability. There is nothing in the local codebase to verify or contradict.
2. If the article has one or more groundable code anchors and **no pack is available**, it is `UNVERIFIABLE`.
3. If the article has groundable code anchors and **any** anchor is missing from the pack, it is `STALE`.
4. If the article has groundable code anchors and **all** anchors are found in the pack, it is `FRESH`.

External articles without code anchors never go stale automatically. They become stale only when the agent re-ingests updated content or explicitly re-compiles them.

### Search behavior

External articles are indexed in tantivy identically to code-grounded articles. They appear in `wiki_search` results with the `source` field available for the agent to distinguish origin. `wiki_read` and `wiki_index` expose the same optional `source` metadata on returned article records. No automatic filtering by origin in v5.

### No CLI subcommand

Ingestion is agent-driven, not human-driven. `repolens wiki lint` includes external articles in its lint pass.

## Version Dependencies and Build Order

```
v2 (search)  <-- v3 (suggestions) depends on tantivy for coverage
     ^                            and search integration
     |
v4 (background lint) --- independent of v2/v3, but ships after
     |                    so it includes coverage gaps from v3
     v
v5 (external pipeline) --- independent, benefits from tantivy indexing
```

Build order: **v2 -> v3 -> v4 -> v5**.

## Final MCP Tool Inventory

| Tool | Version | Status |
|------|---------|--------|
| `wiki_compile` | v1 | existing |
| `wiki_read` | v1 | existing |
| `wiki_search` | v2 | modified (tantivy backend, `phraseQuery` param) |
| `wiki_index` | v1 | existing |
| `wiki_lint` | v3 | modified (adds `coverageGaps` to response) |
| `wiki_delete` | v1 | existing |
| `wiki_suggest` | v3 | new |
| `wiki_ingest_external` | v5 | new |

Total: 8 wiki tools (6 existing, 2 new).

## New Dependencies

| Crate | New dependency | Version added |
|-------|---------------|--------------|
| `repolens-wiki` | `tantivy` | v2 |
| `repolens-app` | `repolens-wiki` | v3 (suggestion logic needs wiki index for coverage analysis; v4 reuses this dependency for post-scan hook) |

## Storage Layout (Final)

```
.repolens/wiki/
  wiki-index.json
  wiki-log.json
  .tantivy/              (v2 -- derived tantivy index)
  articles/
    entity/
    concept/
    overview/
    comparison/
  .gitignore
```

## Testing Strategy

Each version adds tests at two levels:

- **Unit tests** in `repolens-wiki` (tantivy indexing, search scoring, coverage computation) and `repolens-app` (suggestion ranking logic)
- **Integration tests** in `repolens-mcp` (tool handler tests for `wiki_suggest`, `wiki_ingest_external`, modified `wiki_search` and `wiki_lint`)

Tantivy tests use `RAMDirectory` for fast, disk-free execution. Suggestion ranking tests use synthetic pack/graph fixtures.
