#!/usr/bin/env python3
"""sanitize-fixtures.py — Replace environment-specific absolute paths in
captured fixture JSON files with stable placeholders.

Replacements:
  - Absolute repo roots  -> __REPO_ROOT__
  - Absolute pack paths  -> __PACK_DIR__
  - Absolute report paths -> __REPORT_DIR__

Does NOT rewrite timestamps, drop fields, or reorder arrays.

Usage:
  python3 scripts/sanitize-fixtures.py --fixtures-dir repolens-rs/fixtures/scenarios
  python3 scripts/sanitize-fixtures.py --fixtures-dir repolens-rs/fixtures/scenarios --scenario java-small
"""

import argparse
import json
import os
import re
import sys


def find_repo_root_and_pack(scenario_dir: str) -> tuple[str | None, str | None]:
    """Detect the original absolute repo root and pack path from session.json."""
    session_path = os.path.join(scenario_dir, "repo", "session.json")
    if not os.path.isfile(session_path):
        return None, None

    with open(session_path, "r") as f:
        session = json.load(f)

    repo_root = None
    pack_path = None

    # session.json typically has repoRoot and packPath fields
    if isinstance(session, dict):
        repo_root = session.get("repoRoot") or session.get("root")
        pack_path = session.get("packPath") or session.get("activePack")

    return repo_root, pack_path


def sanitize_string(value: str, replacements: list[tuple[str, str]]) -> str:
    """Apply path replacements to a string value, longest-first."""
    for old, new in replacements:
        value = value.replace(old, new)
    return value


def sanitize_value(value, replacements: list[tuple[str, str]]):
    """Recursively sanitize JSON values."""
    if isinstance(value, str):
        return sanitize_string(value, replacements)
    elif isinstance(value, list):
        return [sanitize_value(item, replacements) for item in value]
    elif isinstance(value, dict):
        return {k: sanitize_value(v, replacements) for k, v in value.items()}
    else:
        return value


def sanitize_file(filepath: str, replacements: list[tuple[str, str]]) -> bool:
    """Sanitize a single JSON file in place. Returns True if modified."""
    try:
        with open(filepath, "r") as f:
            original = f.read()
    except (OSError, UnicodeDecodeError):
        return False

    # Check if any replacement is needed (fast path)
    needs_change = any(old in original for old, _ in replacements)
    if not needs_change:
        return False

    # Parse, transform, and write back preserving JSON structure
    try:
        data = json.loads(original)
        sanitized = sanitize_value(data, replacements)
        with open(filepath, "w") as f:
            json.dump(sanitized, f, indent=2, ensure_ascii=False)
            f.write("\n")
        return True
    except json.JSONDecodeError:
        # For non-JSON or malformed files, do raw string replacement
        modified = original
        for old, new in replacements:
            modified = modified.replace(old, new)
        if modified != original:
            with open(filepath, "w") as f:
                f.write(modified)
            return True
        return False


def sanitize_scenario(scenario_dir: str) -> int:
    """Sanitize all JSON files in a scenario directory. Returns count of modified files."""
    repo_root, pack_path = find_repo_root_and_pack(scenario_dir)

    if not repo_root:
        print(f"  WARNING: Could not detect repo root from {scenario_dir}/repo/session.json")
        return 0

    # Build replacements list, longest paths first to avoid partial matches
    replacements = []

    if pack_path:
        # Report path is pack path with -report suffix (convention)
        report_path = pack_path + "-report" if pack_path else None
        if report_path:
            replacements.append((report_path, "__REPORT_DIR__"))
        replacements.append((pack_path, "__PACK_DIR__"))

    replacements.append((repo_root, "__REPO_ROOT__"))

    # Sort by length of old string, longest first
    replacements.sort(key=lambda x: len(x[0]), reverse=True)

    modified_count = 0
    for root, _dirs, files in os.walk(scenario_dir):
        for fname in files:
            if fname.endswith(".json"):
                fpath = os.path.join(root, fname)
                if sanitize_file(fpath, replacements):
                    modified_count += 1
                    print(f"  Sanitized: {os.path.relpath(fpath, scenario_dir)}")

    return modified_count


def main():
    parser = argparse.ArgumentParser(description="Sanitize captured fixture JSON files")
    parser.add_argument("--fixtures-dir", required=True, help="Path to fixtures/scenarios directory")
    parser.add_argument("--scenario", help="Specific scenario to sanitize (default: all)")
    args = parser.parse_args()

    fixtures_dir = args.fixtures_dir
    if not os.path.isdir(fixtures_dir):
        print(f"ERROR: {fixtures_dir} is not a directory", file=sys.stderr)
        sys.exit(1)

    if args.scenario:
        scenarios = [args.scenario]
    else:
        scenarios = sorted(
            d for d in os.listdir(fixtures_dir)
            if os.path.isdir(os.path.join(fixtures_dir, d))
        )

    total_modified = 0
    for scenario in scenarios:
        scenario_dir = os.path.join(fixtures_dir, scenario)
        if not os.path.isdir(scenario_dir):
            print(f"WARNING: Scenario directory not found: {scenario_dir}")
            continue
        print(f"Sanitizing scenario: {scenario}")
        count = sanitize_scenario(scenario_dir)
        total_modified += count
        if count == 0:
            print("  No changes needed")

    print(f"\nTotal files modified: {total_modified}")


if __name__ == "__main__":
    main()
