---
description: Run the non-cargo verification gates (doc-canon registry + fixture integrity) via scripts/verify-all.sh
allowed-tools: Bash(bash scripts/verify-all.sh:*)
---
Run `bash scripts/verify-all.sh` from the repo root. It runs, short-circuiting on the
first failure:
1. `scripts/verify-doc-canon.sh` — doc-registry.yaml ↔ docs tree consistency
2. `scripts/verify-fixtures.sh` — fixture scenario integrity (required files + valid JSON)

Report whether both passed. On failure, summarize which check failed (a doc-registry
mismatch with the offending doc, or a fixture with missing files / invalid JSON) and
exactly what needs updating.
