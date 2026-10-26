#!/usr/bin/env bash
# golden-pack-diff.sh — prove a code change is behaviour-preserving by showing
# the scan pack CONTENT is byte-identical between the current working tree and a
# baseline git ref.
#
# Diffs every file in the pack EXCEPT manifest.json (which carries the
# generation timestamp). That covers files-*.json, graph.json,
# graph-metrics.json, and project-profile.json.
#
# How it works:
#   * Builds the current `repolens` binary from the working tree.
#   * Builds a baseline binary from BASELINE_REF (default `main`) in a throwaway
#     git worktree with its own cached target dir (so it doesn't fight the main
#     target lock, per CLAUDE.md; first run does a full build, later runs are
#     incremental).
#   * For each fixture, scans the SAME copied root dir with both binaries
#     (same-dir A/B). Scanning the identical path with both means any absolute
#     paths embedded in the output are identical and cannot cause spurious
#     diffs — the environmental noise is cancelled, only the code change shows.
#
# Usage:
#   ./scripts/golden-pack-diff.sh [fixture-name ...]
#   BASELINE_REF=<ref> ./scripts/golden-pack-diff.sh
#
# Default fixtures: java-small rust-small mixed-small (under fixtures/inputs/).
# Exit status is non-zero if any fixture's pack content differs.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BASELINE_REF="${BASELINE_REF:-main}"
FIXTURES=("$@")
if [[ ${#FIXTURES[@]} -eq 0 ]]; then
  FIXTURES=(java-small rust-small mixed-small)
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/repolens-gpd.XXXXXX")"
BASE_WT="$WORK/baseline-src"
BASE_TARGET="${TMPDIR:-/tmp}/repolens-gpd-baseline-target" # cached across runs
CUR_BIN="$ROOT/target/debug/repolens"
BASE_BIN="$BASE_TARGET/debug/repolens"

# shellcheck disable=SC2329  # invoked indirectly via the EXIT trap below
cleanup() {
  git worktree remove --force "$BASE_WT" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "==> baseline ref: $BASELINE_REF"
echo "==> building current working-tree binary"
cargo build -p repolens-cli --quiet

echo "==> checking out baseline into throwaway worktree"
git worktree add --detach --force "$BASE_WT" "$BASELINE_REF" >/dev/null
echo "==> building baseline binary (cached target: $BASE_TARGET)"
(cd "$BASE_WT" && CARGO_TARGET_DIR="$BASE_TARGET" cargo build -p repolens-cli --quiet)

latest_pack() { # $1=root -> newest pack dir (no trailing slash), empty if none
  local d latest=""
  for d in "$1"/.repolens/pack-*/; do
    [[ -d "$d" ]] || continue
    latest="${d%/}" # glob expands sorted; pack-YYYYMMDD-HHMMSS sorts chronologically
  done
  printf '%s' "$latest"
}
snapshot_pack() { # $1=root  $2=dest — copy the newest pack's contents to dest
  local pack pid
  pack="$(latest_pack "$1")"
  pid="$(basename "$pack")"
  mkdir -p "$2"
  cp -R "$pack"/. "$2"/
  # Canonicalise this scan's OWN pack-id before diffing. Every scan mints a
  # fresh pack-YYYYMMDD-HHMMSS and embeds it in the content (e.g. the
  # evidence-scope `packId` fields in project-profile.json), so two scans taken
  # seconds apart would otherwise show a spurious id-only diff. Same-dir A/B
  # cancels absolute paths but not this self-reference; substituting a fixed
  # token does. A genuine content change elsewhere still diffs.
  find "$2" -type f -name '*.json' -exec sed -i.bak "s/${pid}/pack-CANONICAL/g" {} +
  find "$2" -type f -name '*.bak' -delete
}

OVERALL=0
for fx in "${FIXTURES[@]}"; do
  src="$ROOT/fixtures/inputs/$fx"
  if [[ ! -d "$src" ]]; then
    echo "  ! skip $fx — no such fixture at $src" >&2
    OVERALL=1
    continue
  fi
  work="$WORK/$fx"
  cp -R "$src" "$work"
  rm -rf "$work/.repolens"

  # current working-tree binary
  "$CUR_BIN" scan --root "$work" --full --keep 1 >/dev/null 2>&1
  snapshot_pack "$work" "$WORK/after-$fx"
  rm -rf "$work/.repolens"

  # baseline binary — same root dir, so any absolute paths are identical
  "$BASE_BIN" scan --root "$work" --full --keep 1 >/dev/null 2>&1
  snapshot_pack "$work" "$WORK/before-$fx"

  before="$WORK/before-$fx"
  after="$WORK/after-$fx"
  diffs=0
  for f in $({ ls "$before"; ls "$after"; } | sort -u); do
    [[ "$f" == "manifest.json" ]] && continue
    if ! diff -q "$before/$f" "$after/$f" >/dev/null 2>&1; then
      echo "  ✗ $fx: $f DIFFERS"
      diff -u "$before/$f" "$after/$f" 2>&1 | sed 's/^/      /' | head -40
      diffs=$((diffs + 1))
    fi
  done
  if [[ $diffs -eq 0 ]]; then
    echo "  ✓ $fx: pack content byte-identical"
  else
    echo "  ✗ $fx: $diffs file(s) differ"
    OVERALL=1
  fi
done

echo
if [[ $OVERALL -eq 0 ]]; then
  echo "✓ golden-pack-diff: all fixtures byte-identical vs $BASELINE_REF"
else
  echo "✗ golden-pack-diff: differences found vs $BASELINE_REF" >&2
fi
exit $OVERALL
