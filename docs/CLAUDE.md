# docs/ guidance

Loaded when working under `docs/`. Doc 07 is the canonical shipped-state reference; if docs conflict, it wins.

## Design Documents

`docs/` has three subtrees with distinct roles:
- `docs/philosophy/` — numbered chapters `00-07` + `notes/` retrospectives + long-form research. **`07-current-state-vs-direction.md` is the canonical "shipped today" reference** — read it (not docs 00–06) before making claims about what's live. `04-current-boundaries.md` is frozen 2026-04-04 and superseded by 07.
- `docs/superpowers/specs/` — one spec per feature, filename-dated. `specs/*-catalogue.md` are a distinct kind: compliance docs that bind observation families to named invariant IDs, not features to implement.
- `docs/evals/` — ACE eval cases + archived runs. **Blind eval runs must isolate the corpus via `scripts/dev/eval-blind-workspace.sh`** — `docs/evals/runs/` acts as an answer key (it contaminated ACE-011 v4).
- Narration discipline: don't describe aspirations as current state. If docs conflict, doc 07 wins.

### Doc Canon (CI-gated)

Changes under `docs/philosophy/`, `docs/product-canon.md`, or `docs/engineering-canon.md` must update `docs/doc-registry.yaml` in the same commit (verified by `scripts/verify-doc-canon.sh` in CI). Every tracked doc needs matching `Authority:` and `Lifecycle:` headers; historical docs need a `Historical — Frozen|Superseded YYYY-MM-DD …` banner. Dates in the registry (`frozen_as_of`, `superseded_as_of`) must be quoted strings, not bare YAML date literals — the verifier uses `YAML.safe_load`, which rejects `Date`.
