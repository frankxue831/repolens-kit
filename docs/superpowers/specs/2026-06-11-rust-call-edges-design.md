# Rust call edges — v1 design (conservative, measured)

**Status:** spec for review — implementation gated on maintainer approval.
**Decision context:** resolves audit maintainer-decision #2
(`docs/reviews/2026-06-10-agentic-code-map-audit.md`) affirmatively at
**Rust-only** scope. Evidence basis: across every adoption measurement to date
(ACE-011 ×4, tranche-1 blind instrumentation), agents stay grep-first for
"who calls this?" because `calls` edges exist for Java only — the tools are
capability-short for that question in 4 of 5 languages, and no ergonomics fix
changes that. Rust first because the repo dogfoods in Rust and the tranche-1
line anchors + bounding land their full value only once call edges exist.
**Method:** investigation + Codex design consult (2026-06-11, seven decisions
D1–D7; recommendations adopted in full).

## What ships in v1

`RustRelationshipExtractor` emits `calls` relationships for `call_expression`
nodes in two forms only:

| Callee form | target_name | detail | Resolution path |
|---|---|---|---|
| bare `func()` | `func` | `func` | Strategy 3.5 unique method name (`method_to_files` is already language-agnostic, built from `"method" \| "function"` symbols — graph.rs:124-129) |
| `module::func()` / `crate::a::func()` | `func` (rightmost) | full scoped path | Strategy 3.5; Strategy 2 via qualified name when unique |
| `Type::assoc()` / `crate::a::Type::assoc()` | `Type` (second-from-right, uppercase-initial) | full scoped path | Strategy 2/3 via the type's defining file — mirrors Java's uppercase-receiver route (java_rel.rs:342) |

**Deliberately deferred (v2+, evidence-gated):** receiver method calls
(`obj.method()`). Receiver type is unknown to the extractor; resolution is
name-only, so a unique local `parse`/`get`/`run` would attract unrelated
receiver calls, and duplicated names inflate `unresolvedCount`. This is the
highest-noise form for the least-confident signal — it waits for the v1
measurement (below).

### Filters (all reusing existing machinery)

- **Macros excluded naturally** — `println!` etc. are `macro_invocation`, not
  `call_expression`; only `call_expression` is handled.
- **Stdlib scoped calls** (`std::mem::swap()`, `core::…`, `alloc::…`) filtered
  via the existing `is_stdlib_path` (rust_rel.rs:198-201) before emission.
- **No snake_case deny-list in v1** (Codex D3): with receiver calls deferred
  and associated functions Type-targeted, the noisy names (`new`, `clone`,
  `from`, `unwrap`, …) are either not emitted or absorbed by `is_external`
  (graph.rs: calls targets absent from `method_to_files` → external, NOT
  unresolved). If measurement shows `module::new()`-class noise, the fix is
  separating free-function and impl-method maps in the resolver — not hiding
  names in the extractor.

### One extractor bug-fix required by this change

`add_rel`'s dedup key for non-imports is `rel_type:target_name`
(rust_rel.rs:384-403) — `a::run()` and `b::run()` would collapse before
resolution. **`calls` joins `imports` in keying on `detail`.** No other
rel_type's keying changes.

### Why `Type::assoc()` must target the Type (the `new` trap)

`method_to_files["new"]` has many candidates in any real Rust repo. A `calls`
rel targeting `new` is *present* in the map but not unique → falls through
Strategy 3.5 → `unresolved_count += 1`, polluting graph-metrics and the
profile signals built on them. Targeting `Type` resolves via the type's
defining file (unique by construction in the qualified map) and matches the
Java template's semantics.

## Resolver: zero changes

All resolution uses existing strategies (2, 3, 3.5) and the existing
`is_external` calls handling. Cycle and orphan analysis are structurally
immune: `calls` is excluded from `DEPENDENCY_EDGE_TYPES`
(graph_analysis.rs:20), invariant-tested by `i_graph_cycles_exclude_calls`
(:288) and `i_graph_orphans_ignore_calls`.

## Disclosure contract (same commit as extraction — tripwire-enforced)

The `calls_edge_coverage_test.rs` Rust negative assertion flips by design.
In the same commit (Codex D5):

- `callEdgeCoverage`: `"java-only"` → **`"java-and-rust"`** (stays a string —
  consumers may string-match; do not replace with an object).
- New additive structured companion on the `get_callers` payload:

```json
"callEdgeCoverageDetail": {
  "java": "method-invocations",
  "rust": "bare-and-scoped-function-calls; associated-functions-target-type; receiver-method-calls-excluded"
}
```

- `get_callers` tool description updated to match.
- Tripwire test updated: Rust moves to the positive set with form-specific
  assertions (bare + scoped emit; `obj.method()` does NOT emit; stdlib paths
  do NOT emit); Go/JS/Python negatives unchanged.

## Blast radius (Codex D6 + investigation)

- `graph.json` + `graph-metrics.json` for Rust packs change →
  `rust_small_pack_bytes` insta snapshot regen; `golden-pack-diff.sh` fails
  by design on rust-small vs a pre-change baseline.
- `rust_small_fixture_produces_correct_graph` (graph.rs:1350) currently
  expects 4 resolved Rust edges — count and assertions updated with the
  real new edges enumerated, not just bumped.
- Raw-edge consumers may shift: `get_dependencies`/`get_impact` totals,
  architecture/summary degree counts, `F-HIGH-FAN-IN-FILE`. Cycles/orphans
  provably cannot.
- All wire changes additive; no schema break.

## Tests (TDD, red watched first)

1. Extractor unit tests: bare call emits (`target=func, detail=func`);
   scoped call emits full-path detail; `Type::assoc()` targets Type;
   `obj.method()` emits nothing; `std::mem::swap()` emits nothing; macro
   invocations emit nothing; `a::run()` + `b::run()` both survive dedup.
2. Graph integration: a two-file fixture where `main.rs` calls
   `helper::run()` produces a resolved `calls` edge main→helper; ambiguous
   names fall to external/unresolved per `is_external` rules.
3. Tripwire rewrite as above.
4. MCP integration: `get_callers` on a Rust fixture returns a `calls`-typed
   entry; `edgeTypeBreakdown` includes `calls`; `callEdgeCoverage ==
   "java-and-rust"`; `callEdgeCoverageDetail` present.
5. Snapshot/fixture regen reviewed as calls-edges-only diffs.

## Acceptance gate for language expansion (Codex D7)

Go/JS/Python call edges are **not** approved by this spec. Expansion requires
a measurement pass on v1 showing: ≥30 sampled new resolved call edges with
85–90%+ pointing at the correct file-level target; unresolved-edge growth
small and explainable; zero cycle/orphan signal change (structural guarantee,
verified anyway); high-fan-in findings not dominated by call edges; and a
blind "who calls X?" run on a Rust target answered via `get_callers` without
grep. The same gate then applies per-language.

## Out of scope

Receiver method calls; trait-method dispatch; macro-generated calls
(invisible pre-expansion — already a disclosed extraction limitation via
F-LANGUAGE-EXTRACTION-GAP); call-SITE line numbers on edges (separate
audit item); any resolver strategy changes.
