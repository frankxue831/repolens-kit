---
description: Run the extract criterion benchmark (I/O vs parser-alloc vs parse/walk component shares)
allowed-tools: Bash(cargo bench:*)
---
Run `cargo bench -p repolens-extract --bench extract` from the repo root.

The bench runs four variants per language — full `extract` / read-only / parse-with-fresh-parser
/ parse-with-reused-parser — isolating I/O vs `Parser` allocation vs parse+walk, as
documented in `crates/repolens-extract/benches/README.md`.

Report the per-language component shares. Use this to re-baseline before and after any
change to the extraction path (e.g. the parse-once / read-once slice), so a claimed
speedup is backed by a measured before/after delta rather than asserted.
