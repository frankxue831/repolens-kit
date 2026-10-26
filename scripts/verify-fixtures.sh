#!/usr/bin/env bash
# verify-fixtures.sh — Verify that captured fixture scenarios have
# all required files and valid JSON.
#
# Usage:
#   ./scripts/verify-fixtures.sh [fixtures-dir]
#
# Default fixtures-dir: repolens-rs/fixtures/scenarios (relative to repo root)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FIXTURES_DIR="${1:-$(cd "$SCRIPT_DIR/../fixtures/scenarios" && pwd)}"

if [[ ! -d "$FIXTURES_DIR" ]]; then
  echo "ERROR: Fixtures directory not found: $FIXTURES_DIR" >&2
  exit 1
fi

ERRORS=0
WARNINGS=0
SCENARIOS=0

check_json() {
  local file="$1"
  if ! python3 -c "import json, sys; json.load(open(sys.argv[1]))" "$file" 2>/dev/null; then
    echo "  FAIL: Invalid JSON: $file"
    ERRORS=$((ERRORS + 1))
    return 1
  fi
  return 0
}

for scenario_dir in "$FIXTURES_DIR"/*/; do
  scenario=$(basename "$scenario_dir")
  echo "=== Verifying scenario: $scenario ==="
  SCENARIOS=$((SCENARIOS + 1))

  # Check pack/manifest.json
  if [[ -f "$scenario_dir/pack/manifest.json" ]]; then
    check_json "$scenario_dir/pack/manifest.json"
    echo "  OK: pack/manifest.json"
  else
    echo "  FAIL: Missing pack/manifest.json"
    ERRORS=$((ERRORS + 1))
  fi

  # Check at least one pack/files-*.json
  chunk_count=$(ls "$scenario_dir"/pack/files-*.json 2>/dev/null | wc -l | tr -d ' ')
  if [[ "$chunk_count" -gt 0 ]]; then
    echo "  OK: $chunk_count chunk file(s) found"
    # Validate each chunk file
    for chunk in "$scenario_dir"/pack/files-*.json; do
      check_json "$chunk"
    done
  else
    echo "  FAIL: No pack/files-*.json chunk files found"
    ERRORS=$((ERRORS + 1))
  fi

  # Check repo/session.json
  if [[ -f "$scenario_dir/repo/session.json" ]]; then
    check_json "$scenario_dir/repo/session.json"
    echo "  OK: repo/session.json"
  else
    echo "  FAIL: Missing repo/session.json"
    ERRORS=$((ERRORS + 1))
  fi

  # Check pack/files/ directory for small scenarios
  if [[ -d "$scenario_dir/pack/files" ]]; then
    file_count=$(find "$scenario_dir/pack/files" -type f | wc -l | tr -d ' ')
    echo "  OK: pack/files/ has $file_count content file(s)"
  else
    echo "  WARN: No pack/files/ directory (content copies)"
    WARNINGS=$((WARNINGS + 1))
  fi

  # Check graph.json if present
  if [[ -f "$scenario_dir/pack/graph.json" ]]; then
    check_json "$scenario_dir/pack/graph.json"
    echo "  OK: pack/graph.json"
  fi

  # Check graph-metrics.json if present
  if [[ -f "$scenario_dir/pack/graph-metrics.json" ]]; then
    check_json "$scenario_dir/pack/graph-metrics.json"
    echo "  OK: pack/graph-metrics.json"
  fi

  # Check project-profile.json if present
  if [[ -f "$scenario_dir/pack/project-profile.json" ]]; then
    check_json "$scenario_dir/pack/project-profile.json"
    echo "  OK: pack/project-profile.json"
  fi

  # Check MCP captures if present
  if [[ -d "$scenario_dir/mcp" ]]; then
    for mcp_file in "$scenario_dir"/mcp/*.json; do
      [[ -f "$mcp_file" ]] || continue
      check_json "$mcp_file"
      echo "  OK: mcp/$(basename "$mcp_file")"
    done
  fi

  # Validate ALL .json files parse
  while IFS= read -r json_file; do
    check_json "$json_file"
  done < <(find "$scenario_dir" -name "*.json" -type f)

  echo ""
done

echo "=== Summary ==="
echo "  Scenarios: $SCENARIOS"
echo "  Errors:    $ERRORS"
echo "  Warnings:  $WARNINGS"

if [[ "$ERRORS" -gt 0 ]]; then
  echo "VERIFICATION FAILED"
  exit 1
else
  echo "ALL SCENARIOS VALID"
  exit 0
fi
