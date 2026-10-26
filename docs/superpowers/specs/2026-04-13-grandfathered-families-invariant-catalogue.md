# Grandfathered Families Invariant Catalogue

**Date:** 2026-04-13
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-13-observation-invariant-rollout-design.md](2026-04-13-observation-invariant-rollout-design.md)

## Goal

Retire the grandfathering clause in doc 06 §5 by converting the two families Phase 1 already uses for contradiction detection — *included file presence* and *exact qualified symbols* — into a rubric-conforming invariant catalogue. Doc 06's rubric (§6) requires every `contradictionEligible` family to be backed by a named, maintained invariant suite; those two families have been running on informal fixture coverage since Phase 1 shipped. This spec formalizes that coverage so Path B phase 2+ can widen eligibility for any *other* family without bending the promotion rule.

This is a docs-and-tests change only. No runtime behaviour changes. No new grounding power is granted.

## Scope

In scope:

- A named catalogue of eight invariants (four per family) with evidence form and tier impact.
- Test implementations under `crates/repolens-core/tests/pack_contract_test.rs`.
- Retirement of the grandfathering language in doc 06 §5 and an update to that section's "Current invariant coverage" column to cite the new invariant IDs.

Out of scope:

- Widening eligibility for any family not already at `contradictionEligible`.
- Amending the epistemic envelope to add `evidenceGrade` (still a proposal per doc 06 §7).
- Fixing per-language qualified-name inconsistencies that leave Python/JS/TS/Rust-free-function/Go-free-function symbols unable to pass `is_groundable`. Those become a documented known gap, not a blocker.
- Runtime behaviour in `crates/repolens-app/src/grounding.rs` or any MCP tool.

## The Matching Contract

All invariants in this catalogue protect the exact-equality matching contract implemented in `crates/repolens-app/src/grounding.rs`:

- **File anchors** (`grounding.rs:51`): `entry.path == anchor_str`. Case-sensitive, no trimming, no normalization.
- **Symbol anchors** (`grounding.rs:55–58`): `s.qualified_name.as_deref() == Some(anchor_str)`. Case-sensitive, no fallback to `Symbol.name`.

An anchor is eligible to enter either check only if it passes `is_groundable` (`grounding.rs:14–15`), defined as *contains `/` or `.`*. Symbol entries whose `qualified_name` is a bare name (`main`, `serve`, `Worker`) fail this filter and are silently skipped — they do not enter family (b) at all. This is the precise definition family (b) uses below.

## Invariant Catalogue

All eight invariants were implemented and verified green on `2026-04-13` in commit `<pending>` against the captured contract scenarios at `fixtures/scenarios/{java-small, mixed-small, excluded-small}`. Each maps to exactly one observation family (rubric requirement 2). Each cites its evidence form (requirement 3). Each states the tier it unlocks (requirement 4). Together they cover both families at `contradictionEligible` (requirement 5). Lockstep maintenance (requirement 6) is specified below the tables.

### Family (a) — Included file presence in the active pack

Matches `FileEntry.path` against anchors by exact string equality.

| ID | Property | Evidence form | Tier held |
|---|---|---|---|
| `I-FILE-PATH-RELATIVE` | `FileEntry.path` is non-empty, does not start with `/`, and contains no `..` segment | Loop over captured scenarios, assert per entry | `contradictionEligible` |
| `I-FILE-PATH-FORWARD-SLASH` | `FileEntry.path` contains no `\` | Loop over captured scenarios, assert per entry | `contradictionEligible` |
| `I-FILE-PATH-UNIQUE` | Within a single pack, `FileEntry.path` values are unique across all chunks and entries | `HashSet` count equals total entry count | `contradictionEligible` |
| `I-FILE-PATH-SERDE-ROUNDTRIP` | `FileEntry.path` survives a serde serialize → deserialize byte-for-byte | Per-entry round-trip assertion | `contradictionEligible` |

### Family (b) — Exact qualified symbols (groundable subset)

Matches `FileEntry.symbols[].qualified_name` against anchors by exact string equality, restricted to symbols whose `qualified_name` is `Some(q)` with `is_groundable(q) == true`.

| ID | Property | Evidence form | Tier held |
|---|---|---|---|
| `I-SYMBOL-QNAME-NONEMPTY` | Every `Some(qualified_name)` is non-empty | Loop over captured scenarios, per symbol | `contradictionEligible` |
| `I-SYMBOL-QNAME-UNIQUE-IN-FILE` | Within a single `FileEntry`, groundable `qualified_name` values are unique | Per-file `HashSet` assertion | `contradictionEligible` |
| `I-SYMBOL-QNAME-TRIMMED` | No `qualified_name` has leading or trailing whitespace (internal whitespace such as Java method signatures is permitted) | `q == q.trim()` per symbol | `contradictionEligible` |
| `I-SYMBOL-QNAME-SERDE-ROUNDTRIP` | Every `qualified_name` survives a serde round-trip on its containing `FileEntry` byte-for-byte | Per-entry symbol-list round-trip | `contradictionEligible` |

### Maintenance Rule (Rubric §6 Requirement)

If any invariant in either table is deleted or weakened, the affected family MUST drop a tier (`contradictionEligible` → `confirmOnly`) in doc 06 §5, AND `crates/repolens-app/src/grounding.rs` MUST be updated to no longer match anchors in that family. These two changes ship together or not at all. Reviewers of any PR touching `pack_contract_test.rs`, `FileEntry`, `Symbol`, or `grounding.rs` must verify this rule.

## Known Gaps (Documented, Not Blocking)

1. **Per-language qualified-name asymmetry.** JavaScript, TypeScript, Python, Rust free functions, and Go free functions currently emit `qualified_name` values that do not pass `is_groundable` — examples: `formatMessage`, `process`, `Worker`, `serve`, `main`. Under Phase 1 these are silently `UNCHECKED`, which is safe; the catalogue definition of family (b) explicitly excludes them. However, it means the "exact qualified symbols" family as grounded today effectively covers only Java classes (`com.example.App`), Java fields (where `qualified_name` is the field type), Rust impl methods (`TypeName.method`), and Go methods with receivers (`Server.Start`). Java method signatures like `void main(String[])` also fail `is_groundable` because they contain no `.` — see the captured `java-small` fixture for examples. This is a real coverage limit, not a bug, and fixing it means changing extractors to emit fully qualified names — a separate effort outside this catalogue.

2. **Fixture coverage.** The three captured scenarios (`java-small`, `mixed-small`, `excluded-small`) cover Java heavily and touch other languages only lightly. Any language with no representation in the fixtures is implicitly covered by family-level invariants but has no per-language guarantee. When new extractors land, they should add fixture coverage; absent that, the extractor's sub-family should be treated as `confirmOnly` until a scenario exercises it.

3. **Live extractor stability not tested.** The catalogue asserts serde round-trip, not extractor determinism across repeated runs. A drift where the same source file produces different `qualified_name` values between two extractor runs would not be caught by this suite. A future invariant `I-SYMBOL-QNAME-STABLE` belongs in a `repolens-extract` test that runs the extractor twice against the same fixture and diffs output. Deferred to a later spec.

## File Plan

| File | Change |
|---|---|
| `crates/repolens-core/tests/pack_contract_test.rs` | Extend with 8 new `#[test]` functions named by invariant ID, plus a local `is_groundable` helper and `SCENARIOS` constant. Existing tests in the file are untouched. |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Retire the grandfathering paragraph in §5; update the "Current invariant coverage" column for families (a) and (b) in the §5 table to cite the new invariant IDs; add a pointer to this spec. |
| `docs/superpowers/specs/2026-04-13-grandfathered-families-invariant-catalogue.md` | This spec. |

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-core --test pack_contract_test
cargo test -p repolens-core
cargo clippy -p repolens-core --tests -- -D warnings
```

All eight invariant tests must be green. No existing `repolens-core` tests should regress.

Manual verification:

```sh
grep -n "grandfather" docs/philosophy/06-observation-invariants-and-grounding-eligibility.md
```

After the doc 06 update lands, this should return a single reference — the retirement note — and nothing in the §5 invariant-coverage column.
