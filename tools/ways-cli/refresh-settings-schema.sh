#!/usr/bin/env bash
# Refresh the vendored Claude Code settings JSON Schema (ADR-147).
#
# Maintainer action: re-vendors the schema, after which you rebuild so the
# `include_str!` in schema_doc.rs embeds the new copy. Normal operation never
# needs this — the schema ships bundled in the binary.
#
# Source resolution (mirrors cmd/settings/source.rs):
#   1. $WAYS_SETTINGS_SCHEMA_URL          — one-shot override
#   2. `ways settings schema --source`    — the configured/default URL, if a
#                                            built `ways` is on PATH
#   3. the SchemaStore default            — hardcoded fallback below
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HERE/src/cmd/settings/claude-code-settings.schema.json"
DEFAULT="https://json.schemastore.org/claude-code-settings.json"

if [[ -n "${WAYS_SETTINGS_SCHEMA_URL:-}" ]]; then
  URL="$WAYS_SETTINGS_SCHEMA_URL"
elif command -v ways >/dev/null 2>&1; then
  URL="$(ways settings schema --source 2>/dev/null || echo "$DEFAULT")"
else
  URL="$DEFAULT"
fi

echo "Refreshing settings schema from: $URL" >&2
curl -fsSL "$URL" -o "$OUT"

# Sanity: must be a JSON object with a properties map.
if ! grep -q '"properties"' "$OUT"; then
  echo "ERROR: fetched file has no \"properties\" — not a settings schema. Aborting." >&2
  exit 1
fi
echo "Vendored $(wc -c < "$OUT") bytes to $OUT" >&2
echo "Rebuild (make ways / cargo build) to embed the refreshed schema." >&2
