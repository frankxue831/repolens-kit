# Extraction measurement harness (roadmap slice P1)

Purpose: make performance decisions **evidence-based**. Before optimizing the scan
pipeline we measure where per-file extraction time actually goes, and which scan
phase dominates end-to-end. This deliberately answers "does `tree_sitter::Parser`
allocation matter?" with data rather than intuition.

## How to reproduce

```sh
# Per-file component micro-bench (criterion). Must target the bench by name so the
# lib unittest harness doesn't intercept criterion's flags:
cargo bench -p repolens-extract --bench extract

# End-to-end per-phase scan timing (release; logs only, pack output unchanged):
cargo build --release -p repolens-cli
RUST_LOG=info ./target/release/repolens scan --root <some-repo> 2>&1 \
  | grep -E "complete|written|generated"
```

The bench runs four variants per language over `fixtures/inputs/*`:

| Variant | What it measures |
|---|---|
| **A** `extract_full` | `SymbolExtractor::extract` = read + `Parser::new` + `set_language` + parse + AST walk |
| **B** `read_only` | `std::fs::read_to_string` (file I/O) |
| **C** `parse_fresh` | preloaded content, fresh `Parser` each iter (alloc + set_language + parse) |
| **D** `parse_reused` | preloaded content, one reused `Parser` (parse only) |

Derived per file: `I/O ≈ B`, `parser_alloc ≈ C − D`, `parse ≈ D`, `walk ≈ A − B − C`.

## Results (2026-06, fixtures + 171-file `crates/` scan)

> **Pre-P2 snapshot (measured 2026-06-03).** These numbers predate two optimizations that
> have since shipped: graph build was parallelized in `c07e9e4` (rayon `par_iter` — no longer
> serial) and parse-once landed in #87. The "serial" graph-build row and Conclusions 2–3 below
> describe the pre-optimization pipeline and carry per-item DONE notes. Re-measure before
> citing absolute numbers.

Per-file component shares (criterion medians, µs):

| Lang | A | B (I/O) | C | D | **parser_alloc = C−D** | parse ≈ D | walk ≈ A−B−C |
|---|---|---|---|---|---|---|---|
| Java | 48.4 | 9.3 (19%) | 24.1 | 23.9 | **0.25 µs (~0.5%)** | 49% | 31% |
| Go | 41.4 | 8.8 (21%) | 26.3 | 26.0 | **0.31 µs (~0.7%)** | 63% | 15% |
| Rust | 24.8 | 8.9 (36%) | 13.0 | 13.1 | **~0 (noise)** | 53% | 11% |
| JS | 27.7 | 8.4 (30%) | 15.2 | 15.3 | **~0 (noise)** | 55% | 15% |
| TS | 43.5 | 8.6 (20%) | 28.7 | 29.1 | **~0 (noise)** | 67% | 14% |

End-to-end scan phase breakdown (171 source files, release build):

| Phase | ms | Share |
|---|---|---|
| Discovery | 17 | 6.5% |
| Entry materialization (symbol extract, **parallel** via rayon) | 20 | 7.7% |
| Pack write | 25 | 9.6% |
| **Graph build (relationship extract + resolution; serial, pre-`c07e9e4`)** | **194** | **74.6%** |
| Profile | 2 | 0.8% |
| **Total** | **260** | |

## Conclusions (seed for slice P2)

1. **Parser allocation is negligible (~0–0.7% of extract time).** `C ≈ D` across all
   languages. **Thread-local parser reuse is NOT worth pursuing** — rejected on evidence.
2. **`parse` dominates per file (~49–67%)**, then **I/O (~19–36%)**, then `walk`. The
   pipeline parses each file **twice** (symbols in entry materialization, relationships in
   graph build) and reads it **~3×** (sha256+loc, symbol read, relationship read of the pack
   copy). So **parse-once / read-once** is the real per-file win.
   **✅ DONE (#87):** parse-once shipped — relationship extraction now reuses the symbol-phase
   parse instead of re-parsing each file.
3. **Graph build dominates the scan at ~75% and is fully serial**, while the equivalent
   symbol-extraction work is parallelized into ~8%. The highest-leverage optimization is to
   **parallelize graph build and/or extract relationships during the existing parallel phase
   (parse-once)** — not parser tricks. Graph resolution also grows super-linearly, so this
   share increases on larger repos.
   **✅ DONE (`c07e9e4`):** graph relationship extraction + resolution is now parallelized via
   rayon `par_iter` (`crates/repolens-app/src/graph.rs`); the "fully serial" framing above is
   the pre-parallelization baseline.

### Caveats
- Fixtures are small (hundreds of bytes), so fixed costs (read syscall, `Parser::new`) are
  over-represented. On larger files parse+walk dominate even more — which only **strengthens**
  the "parser reuse is pointless" and "parse-once wins" conclusions.
- The phase breakdown is a single release run on 171 files; treat ratios, not absolutes, as the
  signal. Re-measure on the actual target repo before committing to a P2 change, and attach a
  before/after delta to each optimization.
