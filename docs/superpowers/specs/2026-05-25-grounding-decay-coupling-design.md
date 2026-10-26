# Grounding→decay coupling — recall-time grounding feeds memory decay

**Date:** 2026-05-25
**Status:** design approved (brainstormed + user-reviewed)

## Problem

`recall_longterm` computes a per-memory `GroundingStatus`
(`Confirmed` / `Contradicted` / `Unverifiable` / `Unchecked`) by cross-referencing
each memory's anchors against the active pack
(`crates/repolens-app/src/grounding.rs`), returns it to the caller, and then
**throws it away**. The memory decay engine
(`crates/repolens-memory/src/decay.rs`) never sees it: decay only reacts to
memory↔memory `Supersedes` and `Contradicts` edges. So:

- A memory whose anchors still resolve against the codebase (`Confirmed`) decays
  at the same rate as one that doesn't.
- A memory whose anchors have vanished (`Contradicted` — e.g. the file/symbol was
  deleted) is not distrusted by decay at all.
- The `MemoryRelation::GroundedBy` / `ContradictedBy` enum variants exist but are
  **unconsumed** dead variants.

This is the gap doc 07 records as **Known Mismatch #3** ("`Confirmed` memories
decay slower" — *still false*) and in row 26 ("`ContradictedBy` edges … are not
consumed yet"). The self-correcting memory loop the product implies — *memory
that quietly distrusts itself when the code it describes drifts away* — does not
exist.

## Goal

Couple recall-time grounding into the **persisted** memory graph so the next
decay run reflects it: `Confirmed` memories decay slower, `Contradicted`
memories decay faster. Scoped to `recall_longterm`, via a reusable helper, and
**idempotent** — repeated recall must not distort importance without bound.

## Non-goals

- **No `get_summary` / on-scan coupling yet.** `get_summary` also computes
  `groundingStatus` per insight, and scan-time re-grounding is attractive, but
  v1 keeps the write surface to `recall_longterm` only. The write-back helper is
  designed so those call sites can reuse it later without change.
- **No edge-based representation.** Grounding is a memory↔**pack** relationship;
  graph edges are memory↔**memory**. Encoding "the pack contradicted this
  memory" as a `ContradictedBy` edge would require a sentinel "pack" target node
  — rejected as awkward. (The unused `GroundedBy`/`ContradictedBy` variants are
  left in place for possible future memory↔memory use; this feature does not
  consume them.)
- **No importance inflation.** A `Confirmed` recall must not push importance up
  on every recall (that compounds and distorts). `Confirmed` only *slows* decay,
  via a floor.
- **No auto-delete of contradicted memories.** `Contradicted` accelerates decay
  but adds no grounding-specific deletion; contradicted-and-ignored memories
  only reach the existing prune over time, and the access-floor still
  protects frequently-accessed ones.
- **No change to the existing edge multipliers** (`Supersedes`,
  `Contradicts`) or to the existing decay cadence / per-run application
  model. The grounding terms slot into the *same* formula and inherit the same
  semantics; the pre-existing per-run compounding behavior of decay is out of
  scope to change here.

## Design

### Mechanism: a persisted grounding snapshot on the node

Add two additive fields to `MemoryNode`
(`crates/repolens-core/src/model/memory.rs`), both `#[serde(default)]` and
camelCase, so existing `.repolens/memory/memory.json` files load unchanged
(absent → `None`):

```rust
/// Grounding status as of the most recent recall that assessed this node.
/// A persisted snapshot of the otherwise compute-on-recall GroundingStatus,
/// read by the decay engine. `None` = never grounded.
#[serde(default)]
pub last_grounding: Option<GroundingStatus>,

/// When `last_grounding` was observed (provenance). Not read by decay today;
/// recorded for staleness reasoning and future on-scan reuse.
#[serde(default, with = "iso8601::optional")]
pub last_grounded_at: Option<OffsetDateTime>,
```

Update the `GroundingStatus` doc comment: it is computed live on recall **and**
a snapshot may be persisted as `MemoryNode.last_grounding` for decay coupling.

### Write-back: a reusable helper in `repolens-app/grounding.rs`

```rust
/// Record a recall-time grounding assessment onto the matching node in the
/// graph, for the decay engine to consume on its next run. Idempotent: it
/// overwrites the snapshot, never accumulates. Unknown ids are a no-op.
pub fn record_grounding(
    graph: &mut MemoryGraph,
    id: &str,
    status: GroundingStatus,
    now: OffsetDateTime,
) {
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == id) {
        node.last_grounding = Some(status);
        node.last_grounded_at = Some(now);
    }
}
```

`recall_longterm` (`crates/repolens-mcp/src/tools/memory.rs`) already computes an
assessment per returned result and already mutates + saves the graph (the
existing "touch" step). The coupling folds in there: after building `results`,
for each result call `record_grounding(&mut graph, &r.id, r.grounding_status,
now)` **before** the existing `MemoryStore::save`. Every `GroundingStatus` value
is recorded (including `Unverifiable`/`Unchecked`) as an honest "last grounding"
snapshot; decay only *acts* on `Confirmed` / `Contradicted`.

### Decay effect (`repolens-memory/decay.rs`)

Two terms slot into the existing per-node formula, keyed on `node.last_grounding`:

```text
grounding_floor      = if Confirmed     { CONFIRMED_FLOOR } else { 0 }
grounding_multiplier = if Contradicted  { CONTRADICTED_FACTOR } else { 1 }

floor          = max(access_floor, grounding_floor)
new_importance = max(floor,
                     importance · access_factor
                                · superseded_multiplier
                                · contradicted_edge_multiplier
                                · grounding_multiplier)
```

- **`Confirmed` → a floor.** Won't decay below a healthy floor ("slow decay /
  refresh"). A floor is idempotent and bounded — it cannot inflate without limit,
  unlike a `> 1.0` multiplier. The floor sits just above the access-floor cap
  because pack-confirmation is a recency-independent signal worth a bit more
  protection than raw access frequency.
- **`Contradicted` → a multiplier below 1.** Decays faster, composing multiplicatively with the
  existing edge multipliers. It still respects the access-floor and adds no
  delete, so a contradicted-and-ignored memory only fades toward the existing
  prune. Deliberately **gentler than the memory↔memory `Contradicts`
  edge**: pack-grounding contradiction is *suggestive* (anchor-format
  drift, partial anchor miss, a stale pack), not an authoritative memory-vs-memory
  assertion.
- **`Unverifiable` / `Unchecked` / `None` → neutral** (no floor, no multiplier).

`CONFIRMED_FLOOR` and `CONTRADICTED_FACTOR` are the two tunables; both are
defaults and easy to adjust. The names are labels for this spec only: `decay.rs`
writes the shipped values inline, so search it for `last_grounding`, not these names.

### Idempotency

The idempotency guarantee is about the **write-back and repeated recall**, not
about chained decay runs:

- `record_grounding` *overwrites* the snapshot — it never appends or increments.
  Re-recalling a node re-stamps the same `last_grounding`, so nothing grows
  (no edge accumulation, no field drift).
- `decay(graph, now)` is a **deterministic pure function** of
  `(importance, last_grounding, time)`; the grounding terms add no hidden state.
- A persistently-`Contradicted` node *does* keep losing importance on each decay
  run — exactly like the existing `Supersedes` / `Contradicts` edge multipliers,
  which also compound per run. That is the intended distrust-over-time behavior,
  not a regression; this feature does not change the existing per-run decay model.

### Backward compatibility

Both new fields are `#[serde(default)]`; existing `memory.json` files load with
`last_grounding: None` (neutral decay). `MemoryStore` save/load roundtrips them
with no schema-version bump required (purely additive optional fields).

## Testing

**Decay unit tests** (`decay.rs`), each path separate:
- `confirmed_grounding_raises_floor` — `last_grounding = Confirmed`, old node that
  would decay below the floor → clamped to the floor.
- `contradicted_grounding_applies_half_multiplier` — fresh node, `Contradicted` →
  importance × the contradicted factor (vs unchanged neutral control).
- `grounding_neutral_for_unverifiable_unchecked_none` — no floor, no multiplier.
- `grounding_multiplier_composes_with_edges` — `Contradicted` snapshot **and** a
  `Contradicts` edge → the two multipliers stack.
- `contradicted_grounding_respects_access_floor` — high access_count + `Contradicted`
  → access_floor wins (no auto-delete).
- `grounding_decay_is_deterministic` — calling `decay(graph, now)` twice on the
  **same input** graph yields identical output (pure function; the snapshot adds
  no hidden state). (Note: chaining output→input still compounds per run, as with
  the existing edge multipliers — that is intended and not asserted here.)

**Grounding unit test** (`grounding.rs`):
- `record_grounding_sets_snapshot` — sets `last_grounding` + `last_grounded_at`;
  unknown id is a no-op; re-call overwrites (idempotent).

**MCP integration tests** (`memory.rs` / integration suite):
- `recall_persists_contradicted_when_anchors_missing` — memorize a node anchored
  to a path absent from the pack, recall, reload graph → `last_grounding ==
  Contradicted`.
- `recall_persists_confirmed_when_anchors_resolve` — anchor resolves in the pack,
  recall, reload → `last_grounding == Confirmed`.
- `re_recall_is_stable` — recall twice → `last_grounding` unchanged, no graph
  distortion.

## Closeout

- Update **doc 07** (`docs/philosophy/07-current-state-vs-direction.md`) — it is
  doc-canon-gated (run `scripts/verify-doc-canon.sh`; no registry field change
  needed for a Living-doc content edit, headers unchanged):
  - Row 26 (memory decay): grounding→decay coupling now ships **on recall**
    (`Confirmed` floor, `Contradicted` multiplier), recorded as
    `MemoryNode.last_grounding`; the "`ContradictedBy` edges not consumed yet"
    note stands (this feature is field-based, not edge-based).
  - Known Mismatch #3 ("`Confirmed` memories decay slower" — *still false*):
    update to **shipped on recall** as of this change, with the on-recall caveat
    (a memory not recalled since the code drifted carries a stale snapshot until
    its next recall).
- Pre-push gate: `cargo fmt --all && cargo clippy --workspace --all-targets --
  -D warnings && cargo test --workspace`.
- Dogfood: memorize an insight anchored to a deleted path, recall, confirm the
  persisted `last_grounding` and that a subsequent decay lowers its importance.

## Open questions

None. Trigger scope (recall only), mechanism (persisted snapshot field, not
edges), and decay effects (Confirmed floor, Contradicted multiplier, neutral
otherwise) are resolved. The two numeric tunables are called out above.
