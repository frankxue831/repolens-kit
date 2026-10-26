#!/usr/bin/env bash
# verify-all.sh — run the repo's non-cargo verification gates in one shot:
#   1. doc-canon registry ↔ tree check (scripts/verify-doc-canon.sh)
#   2. fixture scenario integrity     (scripts/verify-fixtures.sh)
#
# Complements gate.sh (fmt/clippy/test). Together they cover everything CI
# checks. Steps short-circuit on the first failure.
#
# Usage:
#   ./scripts/verify-all.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> [1/2] verify-doc-canon.sh"
bash "$ROOT/scripts/verify-doc-canon.sh"

echo "==> [2/2] verify-fixtures.sh"
bash "$ROOT/scripts/verify-fixtures.sh"

echo "✓ verify-all passed (doc-canon + fixtures)"
