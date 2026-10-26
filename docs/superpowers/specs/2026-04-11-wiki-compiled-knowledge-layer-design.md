# Wiki: Compiled Knowledge Layer

Authority: Supporting
Lifecycle: Living

Taxonomy note: this spec uses the older `Structural / Interpretive / Compiled` storage-tier framing in a few places because of its original authoring date. The canonical reconciliation for current readers is [../../philosophy/08-taxonomy.md](../../philosophy/08-taxonomy.md): storage tier, claim-production layer, trust state, and retention/ranking state are separate axes.

**Date:** 2026-04-11
**Status:** Draft
**Scope:** New `repolens-wiki` crate — persistent, code-grounded wiki as a third truth layer

## Context

RepoLens has two truth layers today:

- **Packs** (structural truth) — immutable snapshots of what the code *is*, produced by scans
- **Memory** (interpretive truth) — mutable point insights about what agents/humans *believe*, subject to decay

Agents working with RepoLens accumulate understanding across sessions, but that understanding is either ephemeral (lost in chat history) or stored as individual memory insights (unstructured, unlinked). There is no mechanism for synthesizing observations and interpretations into persistent, interlinked, searchable articles that compound over time.

The wiki layer fills this gap. Inspired by Karpathy's "LLM Wiki" pattern (persistent, LLM-maintained knowledge bases), but adapted to RepoLens's architecture: the wiki compiles from packs + memory, stays grounded to code, and detects staleness automatically when the codebase changes.

### What this is NOT

- Not a RAG system — knowledge is compiled once, not re-derived on every query
- Not an auto-generator — the LLM agent writes articles, RepoLens provides infrastructure
- Not a document bucket — external docs flow through the Memory layer first, wiki compiles from the two canonical sources only

## Decision Summary

| Question | Decision | Rationale |
|----------|----------|-----------|
| Architectural role | Third truth layer alongside packs and memory | Clean extension of the existing two-layer model; compiled knowledge is a distinct category |
| Sources | Packs + Memory are the canonical grounding sources; agents may also author articles from direct code reading before a scan exists (staleness is `UNVERIFIABLE` until a pack is available). External docs flow through Memory first via `memorize_insight`. | Grounding requires packs, but article creation does not — agents should not be blocked from documenting what they observe directly |
| Trigger model | On-demand staleness (via `wiki_compile` and `wiki_lint`); scan-triggered staleness deferred to Phase 3 | v1 avoids adding wiki dependency to app/CLI; staleness is assessed when agent interacts with wiki |
| Article taxonomy | Typed articles (entity, concept, overview, comparison) | Flexible hybrid; index organizes by type; extensible |
| Navigation | Index browsing + keyword search | Two access patterns; no new infrastructure for v1; search upgradable later |
| Storage format | Markdown files + JSON index | Human-readable articles, fast metadata search, best of both |
| Crate structure | New `repolens-wiki` crate | Each truth layer owns its own storage format in its own crate; wiki has distinct I/O (markdown + JSON) that doesn't fit in app or core |
| Who writes articles | The LLM agent, not RepoLens | Consistent with "observe, don't judge" philosophy; RepoLens is perception infrastructure |

## Crate Dependency Graph

```
repolens-cli
  ├── repolens-app (orchestration)
  │   ├── repolens-core (models, pack I/O)
  │   ├── repolens-extract (symbol/relationship extraction)
  │   ├── repolens-profile (build detection, health)
  │   └── rayon
  └── repolens-mcp (MCP server, orchestrator)
      ├── repolens-app
      ├── repolens-core
      ├── repolens-memory    ← interpretive truth
      ├── repolens-wiki      ← compiled truth  [NEW]
      └── repolens-profile
```

`repolens-wiki` depends only on `repolens-core` (for `PackIndex`, `MemoryGraph`, `MemoryNode` types). It does not depend on `repolens-memory` or `repolens-app`. MCP passes loaded data in — same boundary pattern as memory.

### Legacy Storage-Tier Framing

This table is preserved as the wiki spec's original storage-tier model. Read it as `pack / memory / wiki`, not as a replacement for the envelope's `knowledgeLayer`.

| Layer | Crate | Storage | Mutability | Source |
|-------|-------|---------|------------|--------|
| Structural truth | `core` (packs) | Chunked JSON in `pack-*/` | Immutable per scan | Code observation |
| Interpretive truth | `memory` | Single JSON `memory.json` | Mutable, decaying | Agent/human input |
| Compiled truth | `wiki` | Markdown files + `wiki-index.json` | Mutable, grounded | Synthesis of packs + memory |

### Data Flow

```
External docs ──> Memory (interpretive truth)
                        \
                         Wiki (compiled knowledge)
                        /
Scans ──> Packs (structural truth)
```

## Storage Layout

```
.repolens/
  wiki/
    wiki-index.json          # Authoritative article manifest
    wiki-log.json            # Append-only activity log
    articles/
      entity/
        repolens-memory.md
        memory-decay-engine.md
        mcp-server.md
      concept/
        how-dependency-resolution-works.md
        the-three-truth-layers.md
      overview/
        architecture-overview.md
      comparison/
        tree-sitter-vs-regex-extraction.md
    .gitignore               # Wiki is agent-generated, not source-controlled
```

### Article Format

Each markdown file has YAML frontmatter:

```markdown
---
id: "a1b2c3d4"
title: "Memory Decay Engine"
articleType: "entity"
anchors:
  - "crates/repolens-memory/src/decay.rs"
  - "repolens_memory::decay::MemoryDecayEngine"
memoryRefs:
  - "uuid-of-decision-insight"
  - "uuid-of-convention-insight"
tags: ["memory", "decay", "architecture"]
confidence: 0.85
staleness: "fresh"
createdAt: "2026-04-11T10:00:00Z"
updatedAt: "2026-04-11T10:00:00Z"
---

# Memory Decay Engine

The decay engine applies a time-based half-life formula...
```

### Wiki Index (`wiki-index.json`)

The authoritative article manifest. All wiki tools read from and write to this file. Markdown files contain the same metadata in YAML frontmatter (for human readability), but the index is the source of truth for tool operations.

```json
{
  "schemaVersion": "1.0",
  "updatedAt": "2026-04-11T10:00:00Z",
  "articles": [
    {
      "id": "a1b2c3d4",
      "title": "Memory Decay Engine",
      "articleType": "ENTITY",
      "path": "articles/entity/memory-decay-engine.md",
      "anchors": ["crates/repolens-memory/src/decay.rs"],
      "memoryRefs": ["uuid-1"],
      "tags": ["memory", "decay"],
      "confidence": 0.85,
      "staleness": "FRESH",
      "summary": "Applies half-life decay to memory nodes...",
      "updatedAt": "2026-04-11T10:00:00Z"
    }
  ]
}
```

Corruption protection: all writes use atomic `.tmp` + rename (same as `memory.json`). On missing or corrupt load, returns an empty index — this is acceptable because the wiki is agent-generated content that can be recompiled. A `wiki_rebuild_index` tool that reconstructs the index from markdown frontmatter could be added in a future version if needed, but atomic writes make corruption unlikely in practice.

### Design Decisions

- **Articles organized by type directory** (`entity/`, `concept/`, etc.) — browsable, avoids flat directory of hundreds of files
- **JSON log, not markdown log** — structured, parseable, consistent with the rest of RepoLens's serialization
- **`.gitignore` by default** — wiki is generated content; same pattern as `memory.json`
- **`memoryRefs`** — explicit provenance linking articles back to the memory insights they compiled from

## Core Data Model

File: `crates/repolens-core/src/model/wiki.rs`

```rust
pub enum ArticleType {
    Entity,
    Concept,
    Overview,
    Comparison,
}

pub enum Staleness {
    Fresh,
    Stale,
    Unverifiable,
}

pub struct WikiArticle {
    pub id: String,
    pub title: String,
    pub article_type: ArticleType,
    pub path: String,
    pub anchors: Vec<String>,
    pub memory_refs: Vec<String>,
    pub tags: Vec<String>,
    pub confidence: f32,
    pub staleness: Staleness,
    pub summary: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

pub struct WikiIndex {
    pub schema_version: String,
    pub updated_at: OffsetDateTime,
    pub articles: Vec<WikiArticle>,
}

pub enum WikiEventKind {
    Compile,
    Update,
    StaleFlag,
    Lint,
    Delete,
}

pub struct WikiLogEntry {
    pub timestamp: OffsetDateTime,
    pub event: WikiEventKind,
    pub article_id: Option<String>,
    pub detail: String,
}

pub struct WikiLog {
    pub entries: Vec<WikiLogEntry>,
}
```

All structs use `#[serde(rename_all = "camelCase")]` for JSON serialization. Enums use `SCREAMING_SNAKE_CASE` (`"ENTITY"`, `"FRESH"`, etc.) matching existing crate conventions.

Article body content is NOT in the model — markdown bodies live on disk. The model tracks metadata only, keeping `WikiIndex` lightweight.

## Wiki Crate (`repolens-wiki`)

### Crate Structure

```
crates/repolens-wiki/
  Cargo.toml
  src/
    lib.rs
    store.rs
    staleness.rs
    search.rs
```

Dependencies: `repolens-core`, `serde`, `uuid`, `time`

### `store.rs` — WikiStore

```rust
pub struct WikiStore;

impl WikiStore {
    pub fn load_index(repo_root: &Path) -> Result<WikiIndex>;
    pub fn save_index(repo_root: &Path, index: &WikiIndex) -> Result<()>;
    pub fn read_article(repo_root: &Path, article: &WikiArticle) -> Result<String>;
    pub fn write_article(repo_root: &Path, article: &WikiArticle, body: &str) -> Result<()>;
    pub fn delete_article(repo_root: &Path, index: &mut WikiIndex, id: &str) -> Result<()>;
    pub fn append_log(repo_root: &Path, entry: WikiLogEntry) -> Result<()>;
    pub fn load_log(repo_root: &Path) -> Result<WikiLog>;
    pub fn ensure_gitignore(repo_root: &Path) -> Result<()>;
}
```

- `load_index` returns empty index if file missing or corrupt (same pattern as `MemoryStore::load`). The index is authoritative — empty means "no articles known". Markdown files may still exist on disk but are not auto-discovered; a future `wiki_rebuild_index` could reconstruct from frontmatter if needed.
- `save_index` uses atomic write (`.tmp` + rename)
- `write_article` creates parent directories as needed, writes frontmatter + body
- `delete_article` removes file and index entry

### `staleness.rs` — Grounding-Based Staleness

```rust
pub fn assess_staleness(
    article: &WikiArticle,
    pack: Option<&PackIndex>,
) -> Staleness;

pub fn flag_stale_articles(
    index: &mut WikiIndex,
    pack: Option<&PackIndex>,
) -> Vec<String>;
```

Anchor-matching logic parallels `app::grounding::assess_grounding`:
- File path anchors: exact match against `FileEntry.path`
- Qualified symbol anchors: exact match against `FileEntry.symbols[].qualified_name`
- All anchors found → `Fresh`
- Any anchor missing → `Stale`
- No pack → `Unverifiable`
- No anchors → `Fresh` (nothing to contradict)

Duplication with memory grounding is intentional — wiki staleness and memory grounding serve different consumers and may diverge.

### `search.rs` — Keyword Search

```rust
pub struct WikiSearchResult {
    pub article: WikiArticle,
    pub score: f64,
}

pub fn search(
    index: &WikiIndex,
    query: &str,
    article_type: Option<ArticleType>,
    tags: Option<Vec<String>>,
    limit: usize,
) -> Vec<WikiSearchResult>;
```

Tokenizes query and matches against title, summary, tags, anchors. Scoring: `token_hits * confidence` (same pattern as `recall_longterm`).

### What this crate does NOT do

- **Does not compile articles** — article content is written by the LLM agent via MCP tools
- **Does not load packs or memory** — MCP passes `&PackIndex` and `&MemoryGraph` in
- **Does not do vector search or BM25** — v1 uses keyword tokenization; upgradable later

## MCP Tools

Six new tools in `crates/repolens-mcp/src/tools/wiki.rs`. All follow existing patterns (`#[tool]` macro, `Parameters<T>` structs, `ok_json()`/`err_json()` helpers, per-field `#[serde(rename = "...")]` for camelCase multi-word params).

### `wiki_compile`

Create or update an article. The agent calls this after synthesizing knowledge.

```
Params:
  root         (required) — repo root path
  title        (required) — article title
  articleType  (required) — ENTITY | CONCEPT | OVERVIEW | COMPARISON
  body         (required) — markdown content (without frontmatter)
  summary      (required) — one-line description for index search
  anchors      (optional) — code grounding points (file paths, qualified symbols)
  memoryRefs   (optional) — memory node IDs this article compiled from
  tags         (optional) — topic tags, normalized to lowercase
  confidence   (optional) — 0.0-1.0, default 0.8 for new articles;
                            on update, inherits previous value unless provided
  id           (optional) — if provided, updates existing article

Flow:
  1. If id provided → look up existing article
     a. Record old path if title or articleType changed (for cleanup in step 6)
     b. Compute new path from updated title/type
     c. Inherit previous confidence unless explicitly overridden
     d. Bump updatedAt
  2. If no id → create
     a. Generate UUID v4
     b. Compute path: articles/{type_dir}/{slug}.md
        - type_dir: lowercase articleType (entity, concept, overview, comparison)
        - slug: lowercase title, spaces to hyphens, strip non-alphanumeric
        - On collision: append -2, -3, etc. until unique
     c. Set confidence to provided value or default 0.8
     d. Set createdAt and updatedAt to now
  3. Resolve latest pack (may be None if no scan has been run)
     a. Pack found → assess_staleness(article, pack)
     b. No pack → set staleness to UNVERIFIABLE
     The tool does NOT error when no pack exists. Agents may write articles
     before scanning (e.g., documenting architecture from reading code directly).
     Ungrounded articles are valid — they just can't be staleness-checked yet.
  4. Write markdown file at NEW path (frontmatter + body) via WikiStore::write_article
  5. Update wiki-index.json atomically (points to new path)
  6. If old path differs from new path → delete old file
     (Safe ordering: new file and index are durable before old file is removed.
     If the process fails between steps 4-5, the old file is still intact.
     If it fails between steps 5-6, the old file is orphaned but the article
     is intact at the new path — a minor leak, not data loss.)
  7. Append Compile/Update event to wiki-log.json

Returns: { status: "created"|"updated", id, path, staleness }
```

The `path` field is always computed by the tool, never accepted as input. This ensures consistent directory structure and prevents path traversal.

### `wiki_read`

Read a specific article's full content plus metadata.

```
Params: root, id

Returns: { article: WikiArticle, body: "markdown content..." }
```

### `wiki_search`

Keyword search across wiki articles.

```
Params: root, query, articleType?, tags?, limit? (default 10)

Returns: { results: [{ article, score }], total }
```

### `wiki_index`

Browse the full article catalog.

```
Params: root, articleType? (filter)

Returns: { articles: { entity: [...], concept: [...], ... }, total }
```

### `wiki_lint`

Health-check the wiki. Returns issues without auto-fixing.

```
Params: root

Flow:
  1. Load wiki index
  2. Attempt to resolve latest pack (may be None if no scan has been run)
     a. Pack found → flag_stale_articles(&mut index, &pack)
        (mutates staleness fields on index entries in memory)
     b. No pack → flag_stale_articles(&mut index, None)
        All articles with anchors are marked UNVERIFIABLE.
        Articles with no anchors remain FRESH.
     The handler does NOT error when no pack exists — same policy as
     wiki_compile. Lint still provides value without a pack (broken
     memory ref detection works regardless).
  3. Load memory graph → check memoryRefs validity
     A ref is broken if and only if the memory node ID does not exist in the
     loaded graph. Lint does NOT run MemoryDecayEngine::decay() — it reads
     the graph as-is. If decay has already pruned a node (importance
     faded below the prune cutoff), the ID will be missing and lint reports it as broken.
     This avoids ordering dependencies between wiki lint and memory decay.
  4. Save updated wiki-index.json (persists staleness flag changes)
  5. Append Lint event to log

Returns: { staleArticles: [...], brokenMemoryRefs: [...], packAvailable: bool }
```

Note: orphan detection (articles with no inbound links) is not in v1. It requires article-to-article edges, which depend on cross-wiki linking — explicitly out of scope. Orphan detection can be added when `[[wikilinks]]` or a `relatedArticles` metadata field are introduced.

### `wiki_delete`

Remove an article.

```
Params: root, id, confirm? (default false = dry run)

Returns: { status: "dry_run"|"deleted", article }
```

### Scan-Triggered Staleness (Phase 3, not v1)

In v1, staleness is assessed on-demand only:
- **`wiki_compile`** assesses staleness for the article being written/updated
- **`wiki_lint`** assesses staleness for all articles when the agent calls it

Automatic post-scan staleness flagging is deferred to Phase 3. The challenge: scans run through the CLI (`repolens-cli`), not through MCP. Wiring a post-scan hook requires either adding a `repolens-wiki` dependency to `repolens-app` (breaking the current boundary) or introducing a callback/event mechanism. Phase 3 will design the right hook layer; v1 avoids the dependency question entirely.

### Tool Count

Current: 18 tools. After wiki: 24 tools.

## Data Flows

### Flow 1: Agent Compiles an Article

```
Agent
  |-- 1. Calls get_summary / get_graph (understand code)
  |-- 2. Calls recall_longterm (find relevant memory insights)
  |-- 3. Synthesizes knowledge (agent reasoning)
  |-- 4. Calls wiki_compile(title, type, body, summary, anchors, memoryRefs)
         |
         MCP handler:
         |-- Resolve latest pack
         |-- assess_staleness(article, pack)
         |-- WikiStore::write_article
         |-- WikiStore::save_index
         |-- WikiStore::append_log
```

### Flow 2: On-Demand Staleness (v1)

Staleness is assessed in two places:

```
wiki_compile (per-article):
  |-- Resolve latest pack
  |-- assess_staleness(article, pack)
  |-- Set article.staleness before writing

wiki_lint (all articles):
  |-- Resolve latest pack
  |-- flag_stale_articles(&mut index, &pack)
  |-- Save updated index
  |-- Return stale articles in response
```

Scan-triggered automatic flagging is a Phase 3 addition (requires resolving the CLI/MCP hook layer question).

### Flow 3: Agent Lints the Wiki

```
Agent calls wiki_lint(root)
  |
  MCP handler:
  |-- Load wiki index
  |-- Resolve latest pack --> flag stale articles
  |-- Load memory graph --> check memoryRefs validity
  |-- Append Lint event to log
  |-- Return { staleArticles, brokenMemoryRefs }
```

### Responsibility Division

| Responsibility | Owner |
|---|---|
| Decide what articles to write | Agent |
| Synthesize content from packs + memory | Agent |
| Choose anchors and memory refs | Agent |
| Decide when to update stale articles | Agent |
| Act on lint results | Agent |
| Store articles and index | `repolens-wiki` |
| Assess staleness against pack data | `repolens-wiki` |
| Flag stale articles (on-demand in v1) | `repolens-wiki` (via `wiki_compile` and `wiki_lint`) |
| Keyword search over index | `repolens-wiki` |
| Serve wiki data via MCP tools | `repolens-mcp` |

## Testing

### Unit Tests — `repolens-wiki`

**`store.rs`:**

| Test | Input | Expected |
|------|-------|----------|
| Load missing index | No `wiki-index.json` | Empty `WikiIndex` |
| Load corrupt index | Invalid JSON | Empty `WikiIndex` |
| Round-trip index | Save then load | Identical `WikiIndex` |
| Write and read article | Article + body | Correct frontmatter and body |
| Delete article | Existing ID | File removed, index entry removed |
| Delete nonexistent | Bad ID | Error |
| Append and load log | Multiple entries | Entries in order |
| Gitignore creation | Fresh repo root | `.gitignore` exists |

All use temp directories. No fixtures needed.

**`staleness.rs`:**

| Test | Input | Expected |
|------|-------|----------|
| All anchors found | File path + symbol in pack | `Fresh` |
| Missing file anchor | Removed file | `Stale` |
| Missing symbol anchor | Renamed symbol | `Stale` |
| Mixed found + missing | One hit, one miss | `Stale` |
| No pack | Anchors present, `pack: None` | `Unverifiable` |
| No anchors | Empty anchors vec | `Fresh` |
| Batch flag | 3 articles, 1 stale | Returns 1 ID |

All use in-memory `PackIndex`. Same approach as grounding tests.

**`search.rs`:**

| Test | Input | Expected |
|------|-------|----------|
| Title match | Query matches title | Ranked first |
| Tag match | Query matches tag | Included |
| Summary match | Query matches summary | Included |
| Type filter | Filter by Entity | Only entity articles |
| Tag filter | Filter by tag | Only matching articles |
| Limit | 20 matches, limit 5 | 5 results |
| Empty index | Any query | Empty results |
| Confidence weighting | Two matches, diff confidence | Higher confidence first |

### Integration Tests — `repolens-mcp`

File: `crates/repolens-mcp/tests/wiki_test.rs`

| Test | Setup | Assert |
|------|-------|--------|
| Compile creates article | `wiki_compile` with title + body | `created`, file exists, index updated |
| Compile updates article | Create then compile with same ID | `updated`, body changed, `updatedAt` bumped |
| Compile with no pack | No scan run, call `wiki_compile` | `created`, `staleness == "UNVERIFIABLE"` |
| Compile with title change | Create, then update with new title | Old file deleted, new file at new path, index has one entry |
| Compile with type change | Create as ENTITY, update as CONCEPT | File moved from `entity/` to `concept/`, old path gone |
| Compile slug collision | Create two articles with same title | Second gets `-2` suffix in path |
| Compile confidence default | Create without confidence param | `confidence == 0.8` |
| Compile confidence inherit | Create with 0.9, update without confidence | Stays 0.9 |
| Compile confidence override | Create with 0.9, update with 0.6 | Changes to 0.6 |
| Read returns content | Create then `wiki_read` | Body matches |
| Search by keyword | Create 3 articles, search | Correct article first |
| Index returns catalog | Create mixed types | Grouped by type |
| Lint detects stale | Article anchored to missing file | In `staleArticles` |
| Lint with no pack | No scan run, articles have anchors | All anchored articles `UNVERIFIABLE`, `packAvailable == false`, no error |
| Lint detects broken ref | Article referencing deleted memory | In `brokenMemoryRefs` |
| Lint with valid ref | Article referencing existing memory node | Not in `brokenMemoryRefs` |
| Delete dry run | `confirm: false` | Article still exists |
| Delete confirmed | `confirm: true` | Article gone |

## Sequencing & Prerequisites

### Prerequisites

1. **Finish tree-sitter Rust extractors** — current in-flight work
2. **Stable scan pipeline** — wiki staleness detection depends on reliable pack data

### Implementation Phases

**Phase 1: Core wiki crate (foundation)**
- `repolens-core::model::wiki` — data models
- `repolens-wiki::store` — storage I/O
- `repolens-wiki::staleness` — staleness assessment
- `repolens-wiki::search` — keyword search
- Unit tests for all modules

**Phase 2: MCP tools**
- `wiki_compile`, `wiki_read`, `wiki_search`, `wiki_index`, `wiki_delete`
- Integration tests
- No scan hook yet

**Phase 3: Lint + scan-triggered staleness**
- `wiki_lint` tool (on-demand staleness + broken memory ref detection)
- Design and implement post-scan staleness hook (requires resolving CLI/MCP hook layer)
- Integration tests for lint and scan-triggered staleness

## Out of Scope for v1

| Feature | Why deferred |
|---------|-------------|
| Background quality agents | No concurrent mutation in v1 |
| BM25 or vector search | Keyword search sufficient at small scale |
| Auto-compilation from scan results | Agent drives compilation; "observe, don't judge" |
| External source ingestion tool | External docs enter via `memorize_insight`; dedicated tooling later |
| Article versioning / history | Git handles this if user opts in |
| Cross-wiki linking syntax (`[[wikilinks]]`) | Not load-bearing for v1 |
| Confidence decay over time | Staleness from grounding is the v1 signal |

## Roadmap Beyond v1

Ideas, not commitments. Each would get its own design spec:

- **v2: Richer search** — BM25 via tantivy or SQLite FTS5, possibly vector search
- **v3: Auto-suggestion** — suggest articles for new/changed modules with no wiki coverage
- **v4: Background lint** — periodic lint on a schedule
- **v5: External source pipeline** — dedicated `wiki_ingest_external` tool
