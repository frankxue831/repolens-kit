#!/usr/bin/env bash
# gate.sh — the pre-push gate: format, lint (deny warnings), test.
#
# Mirrors the canonical gate documented in CLAUDE.md and enforced in CI:
#   cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
#
# Note: this APPLIES formatting (`cargo fmt --all`, not `--check`) — the same as
# the CLAUDE.md pre-push line. If it reformats anything, re-stage those files
# before committing. Steps short-circuit: a failure stops the gate immediately.
#
# Usage:
#   ./scripts/gate.sh
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> [1/3] cargo fmt --all"
cargo fmt --all

echo "==> [2/3] cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [3/3] cargo test --workspace"
cargo test --workspace

echo "✓ gate passed (fmt + clippy + test)"
