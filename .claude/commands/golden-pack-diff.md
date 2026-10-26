---
description: Prove the working tree's scan pack output is byte-identical to a baseline ref (default main)
argument-hint: "[fixture-names...]  (default: java-small rust-small mixed-small)"
allowed-tools: Bash(bash scripts/golden-pack-diff.sh:*), Bash(git worktree:*), Bash(cargo build:*)
---
Run `bash scripts/golden-pack-diff.sh $ARGUMENTS` from the repo root.

This is the contract gate for "behavior-preserving" refactors. It builds the current
working-tree binary and a baseline binary (from `main`, or `BASELINE_REF=<ref>`), then
scans the same copied fixture root with both and diffs the pack content — every file
except `manifest.json` (timestamps): `files-*.json`, `graph.json`, `graph-metrics.json`,
`project-profile.json`.

- `$ARGUMENTS` overrides the default fixtures (java-small rust-small mixed-small). Names
  resolve under `fixtures/inputs/`.
- The first run does a full baseline build (slow); the baseline target dir is cached for
  later runs.

Report per-fixture whether output was byte-identical. If ANY fixture differs, show the
diff and treat it as a real behavior change that must be explained or fixed before the
slice can claim to be behavior-preserving — do not wave it away.
