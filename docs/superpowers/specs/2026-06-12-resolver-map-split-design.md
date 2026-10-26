# Resolver map split + scoped-call path disambiguation — design

**Status:** approved (maintainer "go" 2026-06-12); implementation in flight.
**Decision context:** the Rust call-edges v1 acceptance gate
(`2026-06-11-rust-call-edges-design.md` §acceptance, criterion 2) names
unresolved-edge growth as the pre-expansion blocker: +72% self-scan, +76% on
gm-crypto-rs. This spec is that fix.
**Method:** pack-data investigation + Codex design consult (2026-06-12, seven
decisions D1–D7; recommendations adopted in full, including one lean reversed
by evidence — see D4).

## Root cause (measured, not assumed)

Three stacked findings from real pack analysis:

1. **The Rust symbol extractor types every `fn` as `"function"`** — including
   impl-block methods (`rust_symbol.rs` passes `impl_type` to
   `extract_function` but uses it only for the qualified name). The self-pack
   has 1,554 Rust `"function"` symbols and zero `"method"`.
2. **The resolver's single `method_to_files` map** (graph.rs:100-131, built
   from `"method" | "function"`) therefore mixes impl methods into the
   candidate set for bare/scoped free-fn calls. Strategy 3.5 requires a
   unique candidate; `is_external` counts present-but-ambiguous names as
   unresolved. Classic impl names (`new`, `default`, `fmt`, `encode`) are the
   ambiguity mass: 66 ambiguous names self-repo, 33 on gm-crypto-rs.
3. **A resolver-only split would fix zero of them** — nothing is typed
   `"method"`. The extractor reclassification must come first.

Simulation on real packs (impl-ness inferred from the `Type.name`
qualified-name convention):

| Corpus | Ambiguous names today | → newly resolvable | → external | → still ambiguous |
|---|---|---|---|---|
| repolens-rs (self) | 60 | 8 | 17 | 35 |
| gm-crypto-rs (foreign) | 33 | **0** | 10 | 23 |

The foreign-code "still ambiguous" mass is genuinely duplicated free fns
across sibling modules (`encrypt`/`decode`/`decrypt` in sm2/sm4/…; `main`
×27 fuzz targets). The pure split fixes unresolved *accounting* there but
resolves no new edges — which is why D4 ships path disambiguation now.

**Known false-edge class this kills:** `u128::from` →
`sm2/public_key.rs` exists in the gm pack today (primitive-scoped call dodges
the uppercase-Type route; `from` is unique in the combined map because
exactly one file has an `impl From`). Post-split, `from` leaves the free-fn
map → external. Found outside the adjudicated 60-edge sample — recorded here
as a counterexample to over-reading that sample's 100%.

## What ships (three layers, one commit each)

### Layer 1 — extractor: impl-block fns become `"method"`

`rust_symbol.rs` `extract_function`: `symbol_type = "method"` when
`impl_type.is_some()`. Trait default methods stay unextracted (Codex D1 —
separate measurable). Qualified names stay `Type.name` (D6 — no byte churn
beyond the type string). `"method"` is already in the wire vocabulary (Java).

### Layer 1.5 — extractor hygiene: primitive-scoped calls filtered

`extract_call`'s scoped branch: when the second-from-right segment is in
`PRIMITIVE_TYPES` (`u128::from`, `f64::from_bits`, …) emit nothing — same
posture as the `COMMON_RUST_TYPES`/`Self` filter, closing the lowercase gap
the uppercase-initial check leaves open.

### Layer 1.6 — extractor: std-use-alias call filter (added mid-arc, measured)

First post-split self-scan surfaced the next dominant false class: with
impl methods out of the free-fn map, `use std::fs;` + `fs::write(..)`
resolved uniquely to whatever local file defined a free fn named `write` —
37 of 369 calls edges (10%). A names-only pre-pass collects the file's
std/core/alloc use-bindings (module, item, renamed, `{self}`);
`extract_call` skips bare and scoped calls through them. Emission order
unchanged (pre-pass records names, never emits). Control: a local
`mod fs` without a std binding still emits.

### Layer 2 — resolver: split maps with per-language selection

`function_to_files` (`"function"`) and `method_to_files` (`"method"`),
replacing the combined map. Strategy 3.5 and `is_external` select the SAME
map (D2): Rust → function map; Java → method map (keeping its package
disambiguation); any other language → union (status-quo semantics until a
language opts in). Two kind maps, not per-language maps (D3; the unique
foreign-method false-positive case is pre-existing mixed-repo behavior,
unchanged by this spec).

### Layer 3 — Strategy 3.6: Rust scoped-call detail-path disambiguation (D4)

Applies ONLY when: language is Rust, rel_type is `calls`, `detail` contains
`::`, and the detail's final segment equals `target_name` (this excludes
Type-targeted associated-fn calls, where target is the Type). Candidate pool
is `function_to_files[target_name]`. Matching rule (Codex D4, adopted
verbatim):

- Normalize each candidate path to a module path: `…/x.rs` → `…/x`,
  `…/x/mod.rs` → `…/x`.
- Match the call detail's module-prefix segments against it: rooted
  resolution for `crate`/`self`/`super`/workspace-crate prefixes, suffix
  matching for relative prefixes (`mode_xts::encrypt_sectors` matches
  `…/sm4/mode_xts.rs`).
- Return a target only when EXACTLY ONE candidate matches. No ordering
  tie-breaks. Bare calls (no `::`) never reach this strategy.

## Blast radius

- **Pack bytes change** (Symbol `type` values; graph edges/metrics):
  `rust_small_pack_bytes` byte-identity snapshot regen (one fn flips —
  `Helper.assist`); `golden-pack-diff.sh` red-by-design on rust fixtures.
  java-small unaffected; mixed-small has no Rust files — must stay
  byte-identical (regression tripwire for the Java path).
- **Production consumers of the type string:** graph.rs:124 only (verified
  by grep + Codex D5; MCP outline/search/summary surface the raw string,
  profile/wiki do not branch on it).
- **Fan-in is NOT immune** (D7): degree maps count all edges; newly resolved
  or newly external calls shift `F-HIGH-FAN-IN-FILE` inputs. Drift review is
  an acceptance step, not assumed away.
- Cycles/orphans structurally immune as before (`calls` excluded from
  `DEPENDENCY_EDGE_TYPES`).

## Tests (TDD, red watched first)

1. Extractor: impl fn → `"method"`, free fn → `"function"`, nested-in-impl
   classification; `u128::from(x)` emits nothing; `mode::fn()` still emits.
2. Resolver unit: bare Rust call to a name that is method-only → external
   (not unresolved); name unique among free fns but duplicated by methods →
   resolves; Java call still resolves via methods; non-Rust/Java language
   uses union (behavior pinned).
3. Strategy 3.6: two same-named free fns in sibling modules, scoped detail
   resolves to the right one; relative-prefix suffix match; `crate::`-rooted
   match; zero-match and two-match both return nothing; Type-assoc detail
   (final segment ≠ target) bypasses 3.6.
4. mixed-small pack byte-identity unchanged (Java/JS/Go/Py path untouched).
5. Snapshot/golden regen reviewed as type-string + calls-resolution diffs only.

## Acceptance (gates Go/JS/Python call-edge expansion per the v1 spec)

Old-vs-new full scans of repolens-rs and gm-crypto-rs:
unresolved-edge counts drop materially on both (self baseline 319 total /
314 Rust, pack-20260611-194938); ≥30-edge precision sample over NEWLY
resolved calls edges at 85–90%+; cycles/orphans byte-invariant; dead-code
list invariant; fan-in/top-connected drift reviewed and explained in the
eval record.
