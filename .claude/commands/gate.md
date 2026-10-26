---
description: Run the pre-push gate (cargo fmt + clippy -D warnings + test) via scripts/gate.sh
allowed-tools: Bash(bash scripts/gate.sh:*), Bash(cargo fmt:*), Bash(cargo clippy:*), Bash(cargo test:*)
---
Run the full pre-push gate by executing `bash scripts/gate.sh` from the repo root.

It runs, in order and short-circuiting on the first failure:
1. `cargo fmt --all` (applies formatting)
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`

Report whether the gate passed. If any step failed, summarize the specific failures
(clippy lints with file:line, failing test names) and propose fixes — do not silently
continue past a red gate. If `cargo fmt` reformatted files, note which ones so they can
be re-staged.
