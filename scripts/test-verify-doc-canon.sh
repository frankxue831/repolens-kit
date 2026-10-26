#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_SCRIPT="$REPO_ROOT/scripts/verify-doc-canon.sh"

make_sandbox() {
  local sandbox
  sandbox="$(mktemp -d)"
  mkdir -p "$sandbox/scripts" "$sandbox/docs/philosophy"
  cp "$SOURCE_SCRIPT" "$sandbox/scripts/verify-doc-canon.sh"
  chmod +x "$sandbox/scripts/verify-doc-canon.sh"
  printf '%s\n' "$sandbox"
}

write_file() {
  local path="$1"
  shift
  mkdir -p "$(dirname "$path")"
  cat >"$path" <<EOF
$*
EOF
}

run_expect_success() {
  local sandbox="$1"
  local output_file="$sandbox/output.txt"
  if ! bash "$sandbox/scripts/verify-doc-canon.sh" >"$output_file" 2>&1; then
    echo "expected success but verifier failed"
    cat "$output_file"
    return 1
  fi
}

run_expect_failure() {
  local sandbox="$1"
  local expected_fragment="$2"
  local output_file="$sandbox/output.txt"
  if bash "$sandbox/scripts/verify-doc-canon.sh" >"$output_file" 2>&1; then
    echo "expected verifier failure but it passed"
    return 1
  fi

  if ! grep -Fq "$expected_fragment" "$output_file"; then
    echo "verifier failed, but not with expected fragment: $expected_fragment"
    cat "$output_file"
    return 1
  fi
}

test_frozen_entry_may_carry_successor() {
  local sandbox
  sandbox="$(make_sandbox)"
  trap 'rm -rf "$sandbox"' RETURN

  write_file "$sandbox/docs/doc-registry.yaml" 'docs:
  - path: docs/philosophy/frozen.md
    authority: Historical
    lifecycle: Frozen
    frozen_as_of: "2026-04-04"
    successor: docs/philosophy/current.md
    summary: Frozen historical snapshot with an allowed successor pointer.
  - path: docs/philosophy/current.md
    authority: Canonical
    lifecycle: Living
    summary: Current guidance.'

  write_file "$sandbox/docs/philosophy/frozen.md" '# Frozen Doc

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-04. Preserved as the authoritative record of its topic as of that date.'

  write_file "$sandbox/docs/philosophy/current.md" '# Current Doc

Authority: Canonical
Lifecycle: Living'

  run_expect_success "$sandbox"
}

test_historical_banner_must_match_template() {
  local sandbox
  sandbox="$(make_sandbox)"
  trap 'rm -rf "$sandbox"' RETURN

  write_file "$sandbox/docs/doc-registry.yaml" 'docs:
  - path: docs/philosophy/frozen.md
    authority: Historical
    lifecycle: Frozen
    frozen_as_of: "2026-04-04"
    summary: Frozen historical snapshot.'

  write_file "$sandbox/docs/philosophy/frozen.md" '# Frozen Doc

Authority: Historical
Lifecycle: Frozen

Historical — Frozen sometime in April. Preserved for posterity.'

  run_expect_failure "$sandbox" "E014 historical doc missing standardized banner: docs/philosophy/frozen.md"
}

test_frozen_entry_may_carry_successor
test_historical_banner_must_match_template
