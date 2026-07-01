#!/usr/bin/env bash
# Refresh the Claude Code settings JSON Schema (ADR-147).
#
# The schema is read at runtime (not compiled in), so this takes effect
# immediately — NO rebuild. By default it writes the durable user copy in
# $XDG_CONFIG/agent-ways, which outranks the shipped copy and survives
# `make update`. Pass --shipped to update the repo's shipped copy instead
# (maintainer action; that copy rides along with `make update`).
#
# Source URL resolution (mirrors cmd/settings/source.rs):
#   1. $WAYS_SETTINGS_SCHEMA_URL        — one-shot override
#   2. `ways settings schema --source`  — configured/default URL, if ways is on PATH
#   3. the SchemaStore default          — hardcoded fallback below
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DEFAULT="https://json.schemastore.org/claude-code-settings.json"
FILE="claude-code-settings.schema.json"

if [[ "${1:-}" == "--shipped" ]]; then
  OUT="$HERE/../../share/$FILE"          # repo's shipped copy (maintainer)
else
  OUT="${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/$FILE"   # durable user copy
fi

if [[ -n "${WAYS_SETTINGS_SCHEMA_URL:-}" ]]; then
  URL="$WAYS_SETTINGS_SCHEMA_URL"
elif command -v ways >/dev/null 2>&1; then
  URL="$(ways settings schema --source 2>/dev/null || echo "$DEFAULT")"
else
  URL="$DEFAULT"
fi

mkdir -p "$(dirname "$OUT")"
echo "Refreshing settings schema from: $URL" >&2
curl -fsSL "$URL" -o "$OUT"

# Sanity: must be a JSON object with a properties map.
if ! grep -q '"properties"' "$OUT"; then
  echo "ERROR: fetched file has no \"properties\" — not a settings schema. Aborting." >&2
  rm -f "$OUT"
  exit 1
fi
echo "Wrote $(wc -c < "$OUT") bytes to $OUT" >&2
echo "Takes effect immediately — no rebuild needed." >&2
