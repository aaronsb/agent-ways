#!/usr/bin/env bash
# The adr.yaml template, vendored into an empty repo as the adr skill does,
# lints clean before any record exists, and the blocks `adr contract
# --upgrade` appends match it.
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

# `adr contract --upgrade` appends the v1 blocks as the template writes them
# (#614): each block the tool carries must appear in the template verbatim.
if ! out=$(python3 - "$ADR_DIR/adr-tool" "$ADR_DIR/adr.yaml.template" 2>&1 <<'PY'
import runpy, sys
tool = runpy.run_path(sys.argv[1], run_name='adr_tool')
template = open(sys.argv[2], encoding='utf-8').read()
missing = [key for key, block in tool['CONTRACT_BLOCKS'][tool['CURRENT_CONTRACT']]
           if block not in template]
if missing:
    sys.exit(f"adr-tool's {tool['CURRENT_CONTRACT']} blocks differ from adr.yaml.template: {', '.join(missing)}")
PY
); then
  echo "$out"
  echo "=== ADR Template Test: FAILED ==="
  exit 1
fi

if out=$(cd "$TMP" && docs/scripts/adr lint --check 2>&1); then
  echo "=== ADR Template Test: passed ==="
else
  echo "$out"
  echo "=== ADR Template Test: FAILED ==="
  exit 1
fi
