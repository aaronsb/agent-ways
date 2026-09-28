#!/usr/bin/env bash
# The adr.yaml template, vendored into an empty repo as the adr skill does,
# lints clean before any record exists.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ADR_DIR="$REPO_ROOT/hooks/ways/documentation/adr"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

git -C "$TMP" init -q
mkdir -p "$TMP/docs/scripts" "$TMP/docs/architecture"
cp "$ADR_DIR/adr-tool" "$TMP/docs/scripts/adr"
cp "$ADR_DIR/adr.yaml.template" "$TMP/docs/architecture/adr.yaml"
chmod +x "$TMP/docs/scripts/adr"
sed -i.bak "s/^  adopted: .*/  adopted: $(date +%F)/" "$TMP/docs/architecture/adr.yaml"

if out=$(cd "$TMP" && docs/scripts/adr lint --check 2>&1); then
  echo "=== ADR Template Test: passed ==="
else
  echo "$out"
  echo "=== ADR Template Test: FAILED ==="
  exit 1
fi
