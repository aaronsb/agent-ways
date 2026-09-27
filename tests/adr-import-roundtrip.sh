#!/usr/bin/env bash
# Round-trip test for `adr import` over this repo's own records (ADR-306 §7).
#
# Works on a copy of docs/architecture in a temporary git repo; the real tree
# is never written. For every v0 record, archived and legacy ones included:
#
#   1. scan, then `apply --partial`: the record is written in place, and the
#      text after its H1 is byte-identical to the source's;
#   2. every source frontmatter key is in the written record or in the
#      sheet's `unmapped`;
#   3. scanning the written v1 record and applying it again gives the same
#      file (idempotence).
#
# The repo's v1 records are scanned and applied as well, and each comes back
# byte-identical.
#
# ADR_TOOL overrides the tool under test (default: docs/scripts/adr).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ADR_TOOL="${ADR_TOOL:-$REPO_ROOT/docs/scripts/adr}"
[[ "$ADR_TOOL" = /* ]] || ADR_TOOL="$PWD/$ADR_TOOL"
[[ -x "$ADR_TOOL" ]] || { echo "ADR_TOOL is not executable: $ADR_TOOL" >&2; exit 2; }

WORK="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$WORK"' EXIT

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=fixture GIT_AUTHOR_EMAIL=fixture@example.invalid
export GIT_COMMITTER_NAME=fixture GIT_COMMITTER_EMAIL=fixture@example.invalid

mkdir -p "$WORK/repo/docs"
cp -r "$REPO_ROOT/docs/architecture" "$WORK/repo/docs/architecture"
rm -rf "$WORK/repo/docs/architecture/.import"
(cd "$WORK/repo" && git init -q && git add -A && git commit -qm corpus) \
  || { echo "could not set up the corpus copy" >&2; exit 2; }

python3 - "$ADR_TOOL" "$WORK/repo" <<'PY'
import re
import subprocess
import sys
from pathlib import Path

import yaml

tool, root = sys.argv[1], Path(sys.argv[2])
arch = root / 'docs' / 'architecture'
sheets_dir = arch / '.import'
TITLE = re.compile(r'^# ADR-\d+(?:\.\d+)?: .+$', re.M)
failures = []

def run(*args):
    result = subprocess.run([tool, 'import', *args], cwd=root, capture_output=True, text=True)
    if result.returncode != 0:
        failures.append(f"adr import {args[0]} exited {result.returncode}:\n{result.stdout}{result.stderr}")
    return result

def frontmatter(text):
    if not text.startswith('---\n'):
        return None
    end = text.find('\n---', 4)
    return yaml.safe_load(text[4:end]) or {}

def after_title(text):
    match = TITLE.search(text)
    return text[match.end() + 1:] if match else None

def load_sheets():
    return {Path(s['source']['path']): s for s in
            (yaml.safe_load(p.read_text()) for p in sorted(sheets_dir.glob('ADR-*.yaml')))}

records = sorted(arch.rglob('ADR-*.md'))
v0 = [p for p in records if (frontmatter(p.read_text()) or {}).get('contract') != 'adr/v1']
v1 = [p for p in records if p not in v0]
original = {p: p.read_text() for p in records}
rel = lambda p: p.relative_to(root)

# 1 and 2: scan, check the sheets cover every source key, apply --partial.
run('scan', *map(str, v0))
sheets = load_sheets()
covered = bodies = 0
for path in v0:
    sheet = sheets.get(rel(path))
    if sheet is None:
        failures.append(f"{rel(path)}: no sheet")
        continue
    missing = [k for k in frontmatter(original[path])
               if k not in sheet['record'] and k not in (sheet.get('unmapped') or {})]
    if missing:
        failures.append(f"{rel(path)}: source keys dropped: {', '.join(missing)}")
    else:
        covered += 1
run('apply', '--partial')
first = {}
for path in v0:
    text = path.read_text()
    first[path] = text
    if (frontmatter(text) or {}).get('contract') != 'adr/v1':
        failures.append(f"{rel(path)}: not rewritten in place as adr/v1")
    elif after_title(text) != after_title(original[path]):
        failures.append(f"{rel(path)}: text after the H1 differs from the source")
    else:
        bodies += 1

# 3: the written records, scanned and applied again, come out the same.
run('scan', *map(str, v0))
run('apply', '--partial')
stable = 0
for path in v0:
    if path.read_text() != first[path]:
        failures.append(f"{rel(path)}: a second scan and apply changed the record")
    else:
        stable += 1

# The v1 records reproduce as the same data and the same text after the H1.
run('scan', *map(str, v1))
run('apply', '--partial')
reproduced = 0
for path in v1:
    if path.read_text() != original[path]:
        failures.append(f"{rel(path)}: the v1 record does not reproduce")
    else:
        reproduced += 1

leftover = sorted(p.name for p in sheets_dir.glob('ADR-*.yaml'))
if leftover:
    failures.append(f"sheets left unapplied: {', '.join(leftover)}")

for failure in failures:
    print(f"  FAIL: {failure}")
print(f"\nv0 records: {len(v0)}; keys covered {covered}, bodies identical {bodies}, idempotent {stable}")
print(f"v1 records: {len(v1)}; reproduced {reproduced}")
print(f"\n=== ADR Import Round Trip: {'passed' if not failures else f'{len(failures)} failure(s)'} ===")
sys.exit(1 if failures else 0)
PY
