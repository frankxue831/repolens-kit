# `get_conventions` multi-source — deterministic conventions manifest

**Date:** 2026-05-24
**Status:** design approved (brainstormed + user-reviewed + codex-reviewed 2026-05-24)

## Problem

`get_conventions` reads only `root/CLAUDE.md` (→ `claudeMd`) plus
`root/.claude/rules/*.md` (→ `rules[]`), and returns `{ claudeMd, rules }`.
It silently ignores every other convention file an agent would expect when
asking "what conventions govern this repo?" — `AGENTS.md`, `CONTRIBUTING.md`,
`BACKLOG.md`, `GEMINI.md`, `.cursorrules`, `.github/copilot-instructions.md`,
`CONVENTIONS.md`.

This is a logged, directly-observed friction signal
(`docs/superpowers/loop-journal/friction-candidates.md` →
`get-conventions-narrow-source`). It reproduced live during the 2026-05-24
dogfood loop: `get_conventions` on `gm-crypto-rs` returned only `CLAUDE.md`,
omitting that repo's `CONTRIBUTING.md`; and run against repolens-rs itself it
would omit the repo's own `AGENTS.md` and `BACKLOG.md`. First-contact repo
understanding is degraded because the tool under-reports the convention
surface, and gives no signal that it did so.

## Goal

`get_conventions` returns a **deterministic, multi-source conventions
manifest** over a fixed allowlist of convention files, reporting every
allowlisted source as found-or-missing so the output is auditable. Replace the
`{ claudeMd, rules }` shape with a single uniform `sources[]` abstraction.

This is a single-PR agent-surface improvement. It deliberately precedes the
heavier Phase 4B epistemics work so that later trust/findings work is built on
a more complete conventions source.

## Non-goals

- **No heuristic discovery.** The tool never guesses whether a markdown file
  "looks like" conventions. `README.md` and `CHANGELOG.md` are explicitly
  excluded — they are useful but not primarily convention files, and guessing
  would undermine the trust model.
- **No nested / subdirectory discovery.** Root-level only. Nested
  `CLAUDE.md` / `AGENTS.md` (monorepo pattern) is real but adds traversal,
  output growth, precedence, and `.gitignore` complexity. Deferred to an
  explicit follow-up; the code is structured so it can be added later.
- **No `claudeMd` / `rules` back-compat alias.** Pre-1.0, agent-facing, one
  existing test. Clean break now rather than preserving a stale shape.
- **No envelope-grade change.** This is a raw repository read, not a
  catalogue-backed payload. `evidenceGrade` stays at its default
  (`unaudited`); `knowledgeLayer` stays `Observation`; `confidence` stays
  `Mechanical`.

## Design

### Allowlist + ordering

The allowlist is a fixed, ordered constant. **Output order equals this
order** (agent-instruction primacy first, then human-contributor docs, then
the rules glob):

| # | Repo-relative path | `kind` |
|---|---|---|
| 1 | `CLAUDE.md` | `file` |
| 2 | `AGENTS.md` | `file` |
| 3 | `GEMINI.md` | `file` |
| 4 | `.github/copilot-instructions.md` | `file` |
| 5 | `.cursorrules` | `file` |
| 6 | `CONVENTIONS.md` | `file` |
| 7 | `CONTRIBUTING.md` | `file` |
| 8 | `BACKLOG.md` | `file` |
| 9 | `.claude/rules/*.md` (glob, sorted by filename) | `rule` |

- The **8 fixed files** (rows 1–8) always appear in `sources[]`, whether
  present or missing. This is what makes the output auditable.
- The **rules glob** (row 9) expands to zero-or-more entries; only present
  files appear. "A missing glob entry" is not a meaningful concept, so the
  glob never emits `present: false` placeholders.
- Rule entries always sort **after** all fixed entries, ordered among
  themselves by filename ascending.

### Candidate criteria (what counts as a source)

A path is a valid source candidate only if it is a **regular file** — or a
symlink whose **canonical target is a regular file located under the canonical
repo root**. This keeps the "root-only" constraint honest and mirrors the
existing path-traversal defense in `read_file_content` (canonicalize + in-root
prefix check).

Non-candidates:

- **Directory** at an allowlisted path (e.g. a dir literally named `CLAUDE.md`,
  or `.claude/rules/foo.md/`) → not a candidate.
- **Broken symlink**, or a symlink whose canonical target escapes the canonical
  repo root → not a candidate (do not read; do not follow out of root).

Effect on output: a non-candidate at a **fixed** path yields
`present: false, content: null` (it is not a readable in-root file); a
non-candidate under the **rules** glob is **skipped** (not emitted). Neither
contributes to provenance.

### Output shape

```json
{
  "sources": [
    { "name": "CLAUDE.md", "path": "CLAUDE.md", "kind": "file", "present": true,  "content": "# CLAUDE.md\n..." },
    { "name": "AGENTS.md", "path": "AGENTS.md", "kind": "file", "present": true,  "content": "..." },
    { "name": "GEMINI.md", "path": "GEMINI.md", "kind": "file", "present": false, "content": null },
    { "name": "copilot-instructions.md", "path": ".github/copilot-instructions.md", "kind": "file", "present": false, "content": null },
    { "name": ".cursorrules", "path": ".cursorrules", "kind": "file", "present": false, "content": null },
    { "name": "CONVENTIONS.md", "path": "CONVENTIONS.md", "kind": "file", "present": false, "content": null },
    { "name": "CONTRIBUTING.md", "path": "CONTRIBUTING.md", "kind": "file", "present": false, "content": null },
    { "name": "BACKLOG.md", "path": "BACKLOG.md", "kind": "file", "present": true,  "content": "..." },
    { "name": "style.md", "path": ".claude/rules/style.md", "kind": "rule", "present": true, "content": "..." }
  ],
  "_meta": {
    "fixed": { "considered": 8, "found": 3, "missing": 5 },
    "rules": { "found": 1 }
  }
}
```

`_meta` is **nested by source class** to avoid the ambiguity where a flat
`found` looks like it counts every `sources[]` entry:

- `_meta.fixed.{considered,found,missing}` describe **only** the fixed
  allowlist (rows 1–8). `considered` is always `8`;
  `found + missing == considered`.
- `_meta.rules.found` counts glob-expanded rule entries. Rules are additive
  and are **not** part of the fixed missing/present universe, so they never
  affect `_meta.fixed.*`.
- **Counts key off `present`, not `content`.** Explicitly:
  `fixed.found = count(kind=="file" && present==true)`,
  `fixed.missing = considered - fixed.found`,
  `rules.found = count(kind=="rule" && present==true)`. A file that exists but
  fails to read (`present:true, content:null`) **still counts as found** — it is
  a valid in-root file candidate; only the read failed.

This prevents the "sources has N present entries but `_meta` says fewer"
confusion: `sources[].present==true` count == `_meta.fixed.found +
_meta.rules.found`.

### Field semantics (spec-explicit)

- **`name`** — the final path component (basename) of `path`, uniformly for
  every entry: `CLAUDE.md`, `copilot-instructions.md`, `.cursorrules`,
  `style.md`. `path` disambiguates when the basename loses directory context.
- **`path`** — repo-relative path (e.g. `.github/copilot-instructions.md`,
  `.claude/rules/style.md`).
- **`kind`** — `"file"` for the fixed allowlist, `"rule"` for glob-expanded
  `.claude/rules/*.md`. Lets an agent distinguish first-class convention files
  from rule fragments without parsing paths.
- **`present`** — `true` iff the path resolves to a valid in-root regular-file
  candidate per the Candidate criteria above. It reflects existence as a
  legitimate file candidate, **not** read success.
- **`content`** — populated **only when the file read succeeds**. A file that
  exists but cannot be read (read error, non-UTF8 bytes) is reported as
  `present: true, content: null`, preserving the distinction between "not there
  / not a file" (`present: false`) and "there but unreadable"
  (`present: true, content: null`). An **empty** readable file yields
  `content: ""` (not `null`) and is treated as a successful read.

> **Uniqueness:** `name` is **not** guaranteed unique — a rule file at
> `.claude/rules/CLAUDE.md` would share the basename `CLAUDE.md` with the fixed
> entry. Consumers must key/disambiguate on `path`, never `name`.

### Envelope / provenance

Unchanged axes: `KnowledgeLayer::Observation`, `EpistemicConfidence::Mechanical`,
default `evidenceGrade` (`unaudited`).

Provenance is encoded as **`ProvenanceField::Multiple(vec![Provenance::RepositoryRead
{ source: <path> }, ...])`** — one entry per path whose **content was
successfully read** (`content` is `Some`, including empty-string reads),
ordered the same as `sources[]`. This matches the existing multi-source
pattern (`graph.rs`, `profile.rs`, `summary.rs`, `wiki.rs`). A
`present: true, content: null` entry appears in `sources[]` but is **excluded**
from provenance, because nothing was read from it.

**Empty case:** if zero paths were successfully read, fall back to
`ProvenanceField::Single(Provenance::RepositoryRead { source: "(no readable
convention sources)".into() })` rather than an empty `Multiple(vec![])`.

### Code structure (isolation + testability)

In `crates/repolens-mcp/src/tools/conventions.rs`:

- `const CONVENTION_FILES: &[(&str, &str)]` — ordered `(name, repo-relative
  path)` allowlist for the 8 fixed files. (A single `&[&str]` of paths is
  enough; `name` is derived from the path's final component. Implementation may
  choose either, but ordering is load-bearing and must be a fixed constant.)
- `struct ConventionSource { name, path, kind, present, content }` with a
  `kind` enum/string discriminator (`File` | `Rule`).
- `fn collect_conventions(root: &Path) -> Vec<ConventionSource>` — pure helper:
  canonicalizes `root` once, iterates the fixed allowlist (always emitting an
  entry), then expands `.claude/rules/*.md` (sorted, present-only). Applies the
  Candidate criteria (regular file, or in-root-canonical symlink target) to set
  `present`; reads content only for candidates. No MCP types, no envelope —
  unit-testable in isolation.
- `handle_get_conventions` calls `collect_conventions`, computes `_meta`
  (counting fixed found/missing and rules found), assembles the provenance
  source list from successfully-read paths, and wraps via `ok_epistemic`.

Read-failure handling matches today's CLAUDE.md pattern: log, set
`content: None`, keep `present: true`.

## Testing (TDD)

1. **Rewrite** `get_conventions_reads_claude_md` (integration) to the
   `sources[]` shape: CLAUDE.md present with content; `.claude/rules/style.md`
   present as a `rule` entry.
2. **Multi-source** (integration): fixture with `CLAUDE.md` + `AGENTS.md` +
   `CONTRIBUTING.md` present, `GEMINI.md` missing → assert allowlist order,
   `present` flags, `_meta.fixed = {considered:8, found:3, missing:5}`.
3. **Empty repo** (integration/unit): no convention files → all 8 fixed
   entries `present:false`, `_meta.fixed.found:0`, `_meta.rules.found:0`,
   `sources[]` contains exactly the 8 fixed entries.
4. **Pure helper** (unit): `collect_conventions` over a temp dir — determinism
   + ordering of fixed entries.
5. **Rules ordering** (unit): `.claude/rules/b.md` and `.claude/rules/a.md`
   present →
   - rule entries appear **after** all fixed entries,
   - sorted `a.md` then `b.md`,
   - rules do **not** affect `_meta.fixed` counts,
   - `_meta.rules.found == 2`.
6. **Read failure is deterministic via non-UTF8** (integration): write a fixed
   file (e.g. `CONVENTIONS.md`) containing invalid UTF-8 bytes so
   `read_to_string` fails portably (no permission/platform flakiness) → assert
   `present:true, content:null`; `_meta.fixed.found` **unchanged** (still
   counts it); and its path is **absent** from the envelope provenance list.
   Use a new `call_get_conventions_envelope` helper (the default `call_*`
   helper auto-unwraps `.data` and can't see `provenance` — per the CLAUDE.md
   envelope-helper note).
7. **Empty file** (unit): an empty readable fixed file → `present:true,
   content:""` (not `null`); counts as found; **included** in provenance.
8. **Directory at allowlisted path** (unit): a directory named `CLAUDE.md` (and
   a `.claude/rules/foo.md/` directory) → fixed entry `present:false,
   content:null`; the rules-glob directory is **skipped**; neither in
   provenance.
9. **Symlink handling** (unit): (a) a fixed path that is a symlink to an
   in-root regular file → `present:true`, content read, in provenance; (b) a
   symlink whose canonical target escapes the repo root, and a broken symlink →
   `present:false, content:null`, not read, not in provenance.

## Closeout

- Update the tool-description line in
  `crates/repolens-mcp/tests/wiki_populate_test.rs:140`
  ("read CLAUDE.md and .claude/rules/*.md") to reflect the multi-source
  allowlist.
- Mark `get-conventions-narrow-source` **RESOLVED** in
  `docs/superpowers/loop-journal/friction-candidates.md` (same pattern as the
  `summary-symbol-undercount` resolution entry), citing this spec + the PR.
- Not doc-canon-gated: this spec lives under `docs/superpowers/specs/` and the
  friction journal under `docs/superpowers/loop-journal/`; neither is in the
  doc-registry scope (`docs/philosophy/`, `docs/product-canon.md`,
  `docs/engineering-canon.md`).
- Pre-push gate: `cargo fmt --all && cargo clippy --workspace --all-targets --
  -D warnings && cargo test --workspace`.

## Open questions

None. Both architectural forks (source scope = curated root-level allowlist;
output shape = clean restructure to `sources[]`) and all output refinements
(`_meta` nesting, `present`/`content`/provenance semantics, rules-ordering
test) are resolved.

## Codex review (2026-05-24)

A read-only design review (`codex exec`) raised 7 findings, all incorporated
above:

- **[BLOCKER] Symlink behavior undefined** → added the *Candidate criteria*
  section (regular file or in-root-canonical symlink target; broken/out-of-root
  symlinks are non-candidates).
- **[SHOULD-FIX] Provenance encoding ambiguous** → specified
  `ProvenanceField::Multiple` of per-path `RepositoryRead`, with the empty-case
  `Single` fallback.
- **[SHOULD-FIX] Directory at an allowlisted path** → folded into Candidate
  criteria (directories are non-candidates) + test 8.
- **[SHOULD-FIX] `_meta.found` semantics** → added explicit `count(present)`
  formula; unreadable-but-present still counts as found.
- **[SHOULD-FIX] Flaky read-failure test** → replaced permission-based test
  with a deterministic non-UTF8 test + `call_get_conventions_envelope` helper
  (tests 6).
- **[NIT] Empty files** → specified `content: ""`, counts as found, in
  provenance (test 7).
- **[NIT] `name` not unique** → added the uniqueness note (disambiguate on
  `path`).
