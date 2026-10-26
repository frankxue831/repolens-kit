# File Content Extraction Invariant Catalogue

**Date:** 2026-04-13
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-13-grandfathered-families-invariant-catalogue.md](2026-04-13-grandfathered-families-invariant-catalogue.md)

## Goal

Close the third `contradictionEligible` family in doc 06 §5 — *file content extraction* — that was left without a named invariant suite when the grandfathering clause was retired. The original grandfathering clause undercount the affected families: it named *included file presence* and *exact qualified symbols* but missed *file content extraction*, which had the same problem (sitting at `contradictionEligible` with informal coverage only). This spec retires that residual gap by formalizing five invariants over the captured pack-format contract that `read_file_content` depends on.

After this spec lands, every `contradictionEligible` family in doc 06 §5 has a named invariant suite, and rubric §6 requirement 5 is fully satisfied for the highest tier.

This is a docs-and-tests change only. No runtime behaviour changes.

## Scope

In scope:

- A named catalogue of five pack-format invariants over `FileEntry.content_path` and the on-disk `pack/files/...` content store.
- Test implementations under `crates/repolens-core/tests/pack_contract_test.rs`.
- Doc 06 §5 update: refresh the "Current invariant coverage" column for the *file content extraction* row to cite the new IDs, and tighten the grandfathering retirement note to acknowledge it covered three families, not two.

Out of scope:

- Any change to `crates/repolens-mcp/src/lib.rs` or to the `read_file_content` handler.
- Path-traversal *handler* tests. The defense-in-depth canonicalize+contain check at `lib.rs:271–289` is dead code in normal operation — see "Why no path-traversal invariant" below.
- Line-ending normalization, symlink handling, size limits. These are separate handler-behaviour properties, not pack-format properties, and belong in a future MCP-level catalogue.
- Any of the seven `confirmOnly` families still without named invariants. Those are tracked separately and do not block doc 06's self-consistency at the contradiction-eligible tier.

## The Matching Contract

`read_file_content` reads from `pack/<entry.content_path>` after looking up `entry` by `path` in the index. The grounding contract for this family is therefore:

> The bytes returned for a captured entry are the bytes that were captured into the pack at scan time.

For that contract to hold, the pack format itself must guarantee:

1. Every included entry has a content pointer (`content_path: Some(_)`).
2. Excluded entries do not have a content pointer (no leaked storage for files that should not have been captured).
3. The content pointer is a relative path inside the pack's `files/` subdirectory.
4. The pointed-at file exists and is readable.
5. The bytes at that file hash to the SHA-256 the index recorded.

These are the five invariants in the catalogue. Together they make the on-disk pack format strong enough that the handler does not need to defend against bytes drifting out from under it.

## Invariant Catalogue

All five invariants were implemented and verified green on `2026-04-13` against the captured contract scenarios at `fixtures/scenarios/{java-small, mixed-small, excluded-small}`. Each maps to exactly one observation family — *file content extraction* (rubric §6 requirement 2). Each cites its evidence form (requirement 3). Each states the tier it holds (requirement 4). Together they cover the family at `contradictionEligible` (requirement 5). Lockstep maintenance (requirement 6) is specified below the table.

| ID | Property | Evidence form | Tier held |
|---|---|---|---|
| `I-FILE-CONTENT-INDEX-COMPLETE` | Every included `FileEntry` has `Some(content_path)` | Loop over captured scenarios, assert per included entry | `contradictionEligible` |
| `I-FILE-CONTENT-EXCLUDED-NO-CONTENT` | Every excluded `FileEntry` has `content_path: None` | Loop over captured scenarios, assert per excluded entry | `contradictionEligible` |
| `I-FILE-CONTENT-PATH-PREFIX` | Every `Some(content_path)` is relative, starts with `files/`, contains no `..` segment | Per-entry path-shape assertion | `contradictionEligible` |
| `I-FILE-CONTENT-PRESENT-ON-DISK` | For every `Some(content_path)`, the file at `pack/<content_path>` exists and is readable | Per-entry filesystem probe | `contradictionEligible` |
| `I-FILE-CONTENT-SHA256-PARITY` | For every `Some(content_path)`, `sha256(read(pack/<content_path>)) == entry.sha256` | Per-entry hash and compare | `contradictionEligible` |

### Maintenance Rule (Rubric §6 Requirement)

If any invariant in this table is deleted or weakened, the *file content extraction* family MUST drop a tier (`contradictionEligible` → `confirmOnly`) in doc 06 §5, AND `crates/repolens-mcp/src/lib.rs` MUST be updated to no longer trust pack-resident content for grounding (or, more conservatively, to re-verify SHA-256 on every read). These changes ship together. Reviewers of any PR touching `pack_contract_test.rs`, `FileEntry.content_path`, or `read_file_content` must verify this rule.

## Why No Path-Traversal Invariant

CLAUDE.md and doc 06 §5 both call out the path-traversal guard at `crates/repolens-mcp/src/lib.rs:271–289` as part of this family's informal coverage:

```rust
let canonical_pack = fs::canonicalize(&pack_dir)?;
let canonical_content = fs::canonicalize(&content_path)?;
if !canonical_content.starts_with(&canonical_pack) {
    return err_json("Path traversal detected");
}
```

This catalogue does not add a separate `I-FILE-CONTENT-NO-PATH-TRAVERSAL` invariant. The guard is dead code in normal operation, for two reasons that are now both invariant-backed:

1. The handler looks up the entry by `path` in the index *before* resolving content. Any anchor that does not appear in the index is rejected with "File not found" before the canonicalize check runs. Index entries are guaranteed by `I-FILE-PATH-RELATIVE` (in the included-file-presence catalogue) to be relative paths with no `..` segments, so a malicious anchor cannot reach the canonicalize path.
2. `I-FILE-CONTENT-PATH-PREFIX` (above) guarantees that the `content_path` looked up from a valid index entry is itself a `files/`-rooted relative path with no `..` segments. So the resolved `pack/<content_path>` cannot escape `pack_dir` even if the canonicalize check were removed.

The guard is worth keeping as defense in depth — it would catch a class of pack-poisoning attack where the index JSON is hand-edited to point a known entry at `../../etc/passwd`. But that attack is already implausible because it requires writing to the user's `.repolens/` directory, which is the same level of access that would let an attacker write arbitrary files anywhere. The guard is a nice belt-and-suspenders, not the load-bearing protection. The two upstream invariants are.

## Acknowledged Undercount

The previous grandfathering retirement spec stated that *two* families were grandfathered. That count was wrong: doc 06 §5 had *three* families at `contradictionEligible` with informal-only coverage, and the original grandfathering clause inherited the same undercount. This spec corrects the count by retiring the third family on the same model as the other two. Doc 06's grandfathering retirement note will be tightened in this PR to reflect the corrected count.

## File Plan

| File | Change |
|---|---|
| `crates/repolens-core/tests/pack_contract_test.rs` | Extend with 5 new `#[test]` functions named by invariant ID, plus `sha2::{Digest, Sha256}` import. |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Update the §5 table row for *file content extraction* to cite the new invariant IDs; tighten the grandfathering retirement note from "two families" to "three families"; add the new spec to `Depends on`. |
| `docs/superpowers/specs/2026-04-13-file-content-invariant-catalogue.md` | This spec. |

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-core --test pack_contract_test
cargo clippy -p repolens-core --tests -- -D warnings
```

All five new invariant tests must be green alongside the existing 19. `clippy` must remain warning-free.

Manual verification: re-run the rubric audit from the prior session — every `contradictionEligible` family in doc 06 §5 must have at least one named invariant ID in its coverage column. After this spec lands, the audit returns three for three.
