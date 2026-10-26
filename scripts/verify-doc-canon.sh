#!/usr/bin/env bash
set -euo pipefail

# mapfile and associative arrays below need bash 4+; macOS ships 3.2.
if (( BASH_VERSINFO[0] < 4 )); then
  echo "verify-doc-canon.sh needs bash 4 or newer (this is $BASH_VERSION); on macOS: brew install bash" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGISTRY="$ROOT/docs/doc-registry.yaml"

if [[ ! -f "$REGISTRY" ]]; then
  echo "E001 docs/doc-registry.yaml is missing"
  exit 1
fi

if ! registry_rows_output="$(
  ruby - "$REGISTRY" <<'RUBY'
require 'yaml'

registry = YAML.safe_load(File.read(ARGV[0]))
docs = registry.is_a?(Hash) ? registry['docs'] : nil
abort 'E002 registry root key docs: missing' unless docs.is_a?(Array)

seen_paths = {}

docs.each do |row|
  unless row.is_a?(Hash)
    abort 'E003 registry row missing path'
  end

  path = row['path']
  if path.nil? || path.to_s.strip.empty?
    abort 'E003 registry row missing path'
  end

  if seen_paths.key?(path)
    abort "E016 duplicate registry row path: #{path}"
  end
  seen_paths[path] = true

  authority = row['authority']
  abort "E004 #{path} missing authority" if authority.nil? || authority.to_s.strip.empty?

  lifecycle = row['lifecycle']
  abort "E005 #{path} missing lifecycle" if lifecycle.nil? || lifecycle.to_s.strip.empty?

  summary = row['summary']
  abort "E006 #{path} missing summary" if summary.nil? || summary.to_s.strip.empty?

  if lifecycle == 'Superseded'
    successor = row['successor']
    abort "E007 #{path} superseded entry missing successor" if successor.nil? || successor.to_s.strip.empty?

    superseded_as_of = row['superseded_as_of']
    abort "E008 #{path} superseded entry missing superseded_as_of" if superseded_as_of.nil? || superseded_as_of.to_s.strip.empty?
  end

  if lifecycle == 'Frozen'
    frozen_as_of = row['frozen_as_of']
    abort "E009 #{path} frozen entry missing frozen_as_of" if frozen_as_of.nil? || frozen_as_of.to_s.strip.empty?
  end

  if ['Canonical', 'Supporting'].include?(authority) && row.key?('successor')
    abort "E010 #{path} canonical/supporting entry must not carry successor"
  end

  puts [path, authority, lifecycle].join("\t")
end
RUBY
)"; then
  exit 1
fi

mapfile -t registry_rows <<< "$registry_rows_output"

declare -A registry_paths=()
failures=0

report_error() {
  echo "$1"
  failures=$((failures + 1))
}

header_value() {
  local header="$1"
  local file="$2"
  sed -n "s/^${header}: //p" "$file" | head -n 1
}

historical_banner_matches() {
  local lifecycle="$1"
  local file="$2"
  local date_re='[0-9]{4}-[0-9]{2}-[0-9]{2}'
  local link_re='\[[^][]+\](\([^)]+\))?'
  local file_level_pattern
  local section_level_pattern

  case "$lifecycle" in
    Frozen)
      file_level_pattern="^Historical — Frozen ${date_re}\\. Preserved as the authoritative record of its topic as of that date\\.\$"
      section_level_pattern="^> Historical — Frozen ${date_re}\\. This section is preserved for historical context; the rest of this doc remains canonical\\.\$"
      ;;
    Superseded)
      file_level_pattern="^Historical — Superseded ${date_re} by ${link_re}\\. Read ${link_re} for current guidance\\.\$"
      section_level_pattern="^> Historical — Superseded ${date_re} by ${link_re}\\. Read ${link_re} for current guidance on this section's topic; the rest of this doc remains canonical\\.\$"
      ;;
    *)
      return 1
      ;;
  esac

  grep -Eq "$file_level_pattern|$section_level_pattern" "$file"
}

for row in "${registry_rows[@]}"; do
  IFS=$'\t' read -r relpath authority lifecycle <<< "$row"
  registry_paths["$relpath"]=1

  if [[ ! -f "$ROOT/$relpath" ]]; then
    report_error "E011 missing file: $relpath"
    continue
  fi

  actual_authority="$(header_value Authority "$ROOT/$relpath")"
  if [[ -z "$actual_authority" ]]; then
    report_error "E012 missing Authority header: $relpath"
  elif [[ "$actual_authority" != "$authority" ]]; then
    report_error "E012 authority header mismatch: $relpath (expected $authority, got $actual_authority)"
  fi

  actual_lifecycle="$(header_value Lifecycle "$ROOT/$relpath")"
  if [[ -z "$actual_lifecycle" ]]; then
    report_error "E013 missing Lifecycle header: $relpath"
  elif [[ "$actual_lifecycle" != "$lifecycle" ]]; then
    report_error "E013 lifecycle header mismatch: $relpath (expected $lifecycle, got $actual_lifecycle)"
  fi

  if [[ "$authority" == "Historical" ]] && ! historical_banner_matches "$lifecycle" "$ROOT/$relpath"; then
    report_error "E014 historical doc missing standardized banner: $relpath"
  fi
done

if [[ -f "$ROOT/docs/product-canon.md" ]] && [[ -z "${registry_paths[docs/product-canon.md]:-}" ]]; then
  report_error "E015 missing registry row for in-scope doc: docs/product-canon.md"
fi

if [[ -f "$ROOT/docs/engineering-canon.md" ]] && [[ -z "${registry_paths[docs/engineering-canon.md]:-}" ]]; then
  report_error "E015 missing registry row for in-scope doc: docs/engineering-canon.md"
fi

if [[ -d "$ROOT/docs/philosophy" ]]; then
  while IFS= read -r -d '' relpath; do
    relpath="${relpath#"$ROOT/"}"
    if [[ -z "${registry_paths[$relpath]:-}" ]]; then
      report_error "E015 missing registry row for in-scope doc: $relpath"
    fi
  done < <(find "$ROOT/docs/philosophy" -type f -name '*.md' -print0)
fi

if [[ "$failures" -ne 0 ]]; then
  exit 1
fi
