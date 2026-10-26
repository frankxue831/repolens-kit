# ACE-011 — Evidence finding integration — `postsurface-v4` (provided-brief, contaminated)

**Date:** 2026-06-05
**Context:** First ACE-011 run after this session's surface-enrichment arc — F-LANGUAGE-EXTRACTION-GAP (coverage, #93), brief-inlined findings (consumption-entry, #94), and concrete calibrated actions on the cycles finding (#95). (#96, orphan/fan-in actions, was open at run time, so the provided brief showed those two findings with the pre-#96 `reviewIfMaintainerPain` text.)
**HEAD:** `cd6e684` (main; #93/#94/#95 merged).
**Pack:** `pack-20260605-060253` (active; 5 calibrated findings, all inlined into the brief).
**Runner:** blind Claude sub-agent (general-purpose), rubric-blind. Given the ACE-011 task **plus the intended provided-brief workflow** ("a preflight brief has been prepared at `.repolens/current-agent-brief.md` — read it first"). This tests the thing #94 targets: when the enriched brief is *provided*, does the agent bind its plan to an inlined finding?

---

## Verdict: **inconclusive (contaminated)** — plus one verified product bug

The clean consume/bind question could not be answered this run because the agent read a prior eval record as an answer key (see Contamination). But the run produced a verified, high-value product finding that justifies the next build on evidence.

## What the agent did

- Read the brief (instructed), then investigated source + docs for ~36 tool uses.
- **Chose:** "Replace Python's regex symbol/relationship extractors with tree-sitter (parity with the other five languages)" — the *same* improvement as the blind v3 run.
- **Consume:** yes — it explicitly reviewed the inlined calibrated findings via the brief and confirmed the coverage gap.
- **Bind (partial):** it cited `F-LANGUAGE-EXTRACTION-GAP` as one of three "why high-leverage" reasons ("this fixes the underlying gap the finding only discloses"). But it grounded equally in independent source reads **and** the prior eval record.

## Contamination (the headline methodological finding)

The agent read **`docs/evals/runs/ACE-011-postphase4b-v3.md`** — a prior run record that lives in the repo it was told to investigate — and cited it as **basis #1** ("a prior blind sub-agent independently chose this exact improvement and the eval author judged the plan genuinely high quality"). That record names the ACE-011 task shape, the prior agent's choice, and that Python-parity is the highest-leverage item. **It is an answer key.**

Consequence: this run's choice and its citation of `F-LANGUAGE-EXTRACTION-GAP` **cannot be cleanly attributed to the enriched surface** — the agent had the answer in hand from a committed file.

**Methodology fix (required for any future ACE-011 run):** isolate the agent from `docs/evals/runs/` — run on a temp repo copy with that directory removed, or add an explicit "do not read docs/evals/" guard. The earlier v1–v3 runs did not report reading the eval records (they predate most of them), so this contamination grew as the corpus grew. It must be controlled before another blind run is meaningful.

## Verified product finding — Python lossiness can corrupt memory grounding

The agent claimed a memory-corruption path; **I verified it in code** (this is the run's durable value):

- `crates/repolens-app/src/grounding.rs:47–73` — `assess_grounding` resolves each memory anchor against the pack's extracted symbols: an anchor is "found" only if it matches a file path or a symbol `qualified_name` (`:60`). Unresolved anchors → `missing_anchors` (`:66`) → **`GroundingStatus::Contradicted`** (`:73`).
- `crates/repolens-memory/src/decay.rs:103` — `GroundingStatus::Contradicted` applies a decay multiplier (test `:512` confirms it lowers importance toward the floor).
- The Python **regex** extractor emits no class-qualified method names and drops `async def` / absolute imports. So a **correct** memory anchored to real Python code (e.g. `ClassName.method`) fails to resolve → falsely `Contradicted` → decays faster.

For a product whose core promise is poison-resistant, accurate memory, **silently accelerating decay of true memories about low-fidelity-extracted code is a correctness bug** — not just a coverage gap. This elevates the Python-extractor work from "nice completeness" to "memory-correctness fix," and it is grounded in code, not vibes.

## Implications / recommended next slices (both evidence-justified now)

1. **Small, targeted correctness fix (loop-sized, memory-sensitive — flag for sign-off):** in `assess_grounding`, when an anchor is unresolved *and* its file's language is `Heuristic`/`Unsupported` per `repolens_extract::extraction_fidelity` (the iter-1 classifier), treat it as `Unverifiable`, **not** `Contradicted` — RepoLens can't verify what it can't fully extract, so absence is inconclusive, not a contradiction. Only `Ast`-fidelity languages should be able to drive `Contradicted`. This fixes the verified bug without the full rewrite.
2. **Larger build:** port `py_symbol.rs` / `py_rel.rs` to tree-sitter (`grammar_for("Python")` + `extract_from_tree`), bringing Python to AST parity. Non-byte-identical (golden re-capture required) — a deliberate contract change.

## Limitations

- N=1, single vendor, **contaminated** (answer key in-repo) — no clean consume/bind conclusion. The next run must isolate `docs/evals/runs/`.
- Consumption was *instructed* (provided-brief workflow), so this run does not test spontaneous discovery (prior runs show agents do not spontaneously run `findings`/`brief`).
- The brief at run time reflected #93/#94/#95 (not #96).

> **Disclaimer.** Case-level controlled dogfood evidence, **not** product-level proof. The verified memory-grounding finding is a code-grounded product observation worth acting on; the consume/bind result is void this run due to contamination.
