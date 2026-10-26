# Resolver map split — acceptance measurement (spec 2026-06-12)

**Subject:** the four-layer resolver fix on branch `feat/resolver-map-split`
(impl-fns→"method"; primitive-scoped call filter; function/method map split
with per-language selection; Strategy 3.6 detail-path disambiguation; plus
Layer 1.6 std-use-alias filter, added mid-arc on measured evidence).
**Method:** full old-binary vs new-binary scans of two corpora — repolens-rs
(self) and gm-crypto-rs (foreign, unchanged between scans; the call-edges
spec's gate corpus). Old packs: self `pack-20260611-194938`, gm
`pack-20260611-102128`. New packs: both `pack-20260611-204238`. Population
adjudication (not sampling) of every truly-new and truly-dropped file-level
calls pair.

## Headline numbers

| Measure | repolens-rs (self) | gm-crypto-rs (foreign) |
|---|---|---|
| Unresolved edges | 319 → **259** (−19%) | 357 → **285** (−20%) |
| Unresolved growth vs pre-call-edges baseline | +72% → **+39%** | +76% → **+40%** |
| Calls edges (file-level pairs) | 328 → 332 | 198 → 218 |
| Truly-new pairs | 11 | 34 |
| Truly-dropped pairs | 7 | 14 |
| Structural edge set (cycles/orphans/dead-code input) | **byte-identical** | **byte-identical** |

## Precision — full-population adjudication

- **Truly-new: 45/45 mechanically verified** (the target file defines the
  named free fn, or the named Type for associated-fn details). Above the
  spec gate's ≥30-sample bar by covering the population.
- **Truly-dropped: 21/21 confirmed false-edge kills.** Self: `fs::read`×4 +
  `fs::canonicalize` (aliased-std), `gix::open` (external crate call
  matching a local `open`), `has` (bare call matching an impl method). gm:
  all 14 are primitive `bool/u64/u128/usize/u16::from` edges into
  `sm2/public_key.rs` — the `u128::from` class named in the spec.
- Remaining path-mismatch review queue (2 self, 8 gm): all verified correct
  — crate-root fns (`repolens_mcp::run_stdio_server` → lib.rs) and
  re-export facades (`sm2::verify_with_id` → sm2/verify.rs, the defining
  file; `gmcrypto_simd::*` → detect.rs).

## The mid-arc discovery (Layer 1.6)

The first post-split self-scan surfaced a NEW dominant false class: with
impl methods out of the free-fn map, `use std::fs;` + `fs::write(..)`
became uniquely resolvable to whatever local file defined a free fn named
`write` — **37 of 369 calls edges (10%)** were this one pattern (the old
binary had the same class via `fs::read`, just less of it). Fixed in the
extractor: a names-only pre-pass collects std/core/alloc use-bindings
(module, item, renamed, `{self}`) and `extract_call` skips calls through
them. Post-fix mismatch queue: 39 → 2 (both correct).

## Fan-in drift (Codex D7 — reviewed, not assumed away)

- Self: `pack_reader.rs` 40 → 36 inbound (its four `fs::read` false edges
  died); top-3 unchanged.
- gm: **`public_key.rs` 40 → 26 inbound**, falling from #1 to #6. This
  **corrects dogfood round 2's connectivity claim** ("public_key.rs 19→40
  inbound — real connectivity"): 14 of those 40 were primitive-`From`
  false edges manufactured by the combined map. `private_key.rs` and
  `curve.rs` (35 each) are the honest top of the ranking.

## Expansion-gate status (call-edges spec §acceptance)

| Criterion | Status |
|---|---|
| ≥30 new resolved edges at 85–90%+ precision | **Met** — 45/45 (100% mechanical, population) |
| Unresolved growth small and explainable | **Met-with-residual** — growth halved (+76%→+40% foreign); the remainder is honest ambiguity (duplicated free fns: `main` ×27 fuzz targets, `encode`/`decode` across sibling modules where the call site is bare, not scoped) |
| Zero cycle/orphan signal change | **Met** — structural edge sets byte-identical, both corpora |
| Fan-in not dominated by call-edge noise | **Met** — drift is in the false-signal-removal direction; reviewed above |
| Blind who-calls run without grep | **Carried** from dogfood round 2 (get_callers consumed and cited); not re-run this round |

## Caveats

Self-repo corpus drifted slightly between baseline and final scans (this
arc's own commits landed in between); gm-crypto-rs is the clean comparison
— untouched between scans. "Mechanically verified" means symbol-presence
adjudication plus spot reads, not a human read of all 45 call sites.
Scorer built the change. Dogfood residue: one new pack in gm-crypto-rs
(`pack-20260611-204238`), consistent with prior eval-era packs.

> Case-level acceptance evidence for the map-split spec, **not**
> product-level proof and not by itself the Go/JS/Python expansion
> approval — that remains a maintainer decision over this record.
