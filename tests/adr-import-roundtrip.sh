#!/usr/bin/env bash
# Round-trip test for `adr import` over this repo's own records (ADR-306 §7).
#
# Works on a copy of docs/architecture in a temporary git repo; the real tree
# is never written. One synthetic v0 record joins the copy: it has text above
# its H1 and a key with no v1 field, which no real record has yet. For every
# v0 record, archived and legacy ones included:
#
#   1. scan, then `apply --partial`. Sheets with a todo item lint could not
#      find again (a Deprecated record's note) are skipped; the test then
#      resolves those items, as a person would, and applies them.
#   2. The record is rewritten in place, and the text after its H1 is
#      byte-identical to the source's, with any text from above the H1
#      moved to just below it.
#   3. Every source frontmatter key is in the written record with the same
#      value, or under imported.unmapped with the same value. status is
#      checked against the ADR-304 §7 table.
#   4. Scanning the written v1 records and applying them again gives the
#      same files (idempotence).
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

# The synthetic record takes the first free number in the system range.
SYNTH=$(python3 - "$WORK/repo/docs/architecture" <<'PY'
import re, sys
from pathlib import Path
used = {int(m.group(1)) for p in Path(sys.argv[1]).rglob('ADR-*.md')
        if (m := re.match(r'ADR-(\d+)', p.name))}
print(next(n for n in range(100, 200) if n not in used))
PY
)
printf -- '---\nstatus: Accepted\ndate: 2025-01-01\ndeciders: [developer]\nrevised: 2025-02-01\nreviewers:\n  - someone\n---\n\n> A note written above the title.\n\n# ADR-%s: Synthetic record with a preamble\n\n## Context\n\nText.\n' "$SYNTH" \
  > "$WORK/repo/docs/architecture/system/ADR-$SYNTH-synthetic-record-with-a-preamble.md"

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
BLOCKING = ('status note', 'target.number', 'target.domain')
V0_STATUS = {'draft': 'proposed', 'proposed': 'proposed', 'accepted': 'accepted',
             'superseded': 'superseded', 'rejected': 'rejected'}
failures = []

def run(*args):
    result = subprocess.run([tool, 'import', *args], cwd=root, capture_output=True, text=True)
    if result.returncode != 0:
        failures.append(f"adr import {args[0]} exited {result.returncode}:\n{result.stdout}{result.stderr}")
    return result

def commit(message):
    subprocess.run(['git', 'add', '-A'], cwd=root, check=True)
    subprocess.run(['git', 'commit', '-qm', message], cwd=root, check=True)

def frontmatter(text):
    if not text.startswith('---\n'):
        return None
    end = text.find('\n---', 4)
    return yaml.safe_load(text[4:end]) or {}

def split(text):
    """(text between the frontmatter and the H1, text after the H1 line)."""
    match = TITLE.search(text)
    if match is None:
        return None, None
    start = text.find('\n---', 4) + 5 if text.startswith('---\n') else 0
    return text[start:match.start()], text[match.end() + 1:]

def expected_status(source):
    raw = str(source.get('status') or '').strip().lower()
    if raw == 'deprecated':
        return 'superseded' if source.get('superseded_by') else 'accepted'
    return V0_STATUS.get(raw)

records = sorted(arch.rglob('ADR-*.md'))
v0 = [p for p in records if (frontmatter(p.read_text()) or {}).get('contract') != 'adr/v1']
v1 = [p for p in records if p not in v0]
original = {p: p.read_text() for p in records}
rel = lambda p: p.relative_to(root)

# 1: scan and apply --partial; blocking items hold their sheets back.
run('scan', *map(str, v0))
blocked = []
for sheet_path in sorted(sheets_dir.glob('ADR-*.yaml')):
    sheet = yaml.safe_load(sheet_path.read_text())
    if any(str(t).split(':')[0] in BLOCKING for t in sheet['todo']):
        blocked.append(sheet_path)
run('apply', '--partial')
for sheet_path in blocked:
    if not sheet_path.exists():
        failures.append(f"{sheet_path.name}: --partial wrote past a blocking todo item")
        continue
    sheet = yaml.safe_load(sheet_path.read_text())
    sheet['todo'] = [t for t in sheet['todo'] if str(t).split(':')[0] not in BLOCKING]
    sheet_path.write_text(yaml.safe_dump(sheet, sort_keys=False, allow_unicode=True))
if blocked:
    run('apply', '--partial', *map(str, blocked))

# 2 and 3: bodies and keys, checked in the written files.
first = {}
bodies = keys = preambles = 0
for path in v0:
    text = path.read_text()
    first[path] = text
    written, source = frontmatter(text), frontmatter(original[path])
    if (written or {}).get('contract') != 'adr/v1':
        failures.append(f"{rel(path)}: not rewritten in place as adr/v1")
        continue
    before, after = split(original[path])
    written_before, written_after = split(text)
    if after is None or written_after is None:
        failures.append(f"{rel(path)}: no H1 found")
        continue
    preamble = before.strip('\n')
    expected = '\n' + preamble + '\n' + after if preamble.strip() else after
    preambles += bool(preamble.strip())
    if written_after != expected or written_before.strip():
        failures.append(f"{rel(path)}: text after the H1 differs from the source")
    else:
        bodies += 1
    unmapped = (written.get('imported') or {}).get('unmapped') or {}
    wrong = []
    for key, value in source.items():
        if key == 'status':
            ok = written.get('status') == expected_status(source)
        else:
            ok = (key in written and written[key] == value) or (key in unmapped and unmapped[key] == value)
        if not ok:
            wrong.append(key)
    if wrong:
        failures.append(f"{rel(path)}: source keys not carried with their values: {', '.join(wrong)}")
    else:
        keys += 1

# 4: the written records, scanned and applied again, come out the same.
commit('first import')
run('scan', *map(str, v0))
run('apply', '--partial')
stable = 0
for path in v0:
    if path.read_text() != first[path]:
        failures.append(f"{rel(path)}: a second scan and apply changed the record")
    else:
        stable += 1

# The v1 records reproduce byte-identically.
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
print(f"\nv0 records: {len(v0)} (1 synthetic); held back by --partial and resolved {len(blocked)}, "
      f"with a preamble {preambles}")
print(f"  keys carried with values {keys}, bodies identical {bodies}, idempotent {stable}")
print(f"v1 records: {len(v1)}; reproduced {reproduced}")
print(f"\n=== ADR Import Round Trip: {'passed' if not failures else f'{len(failures)} failure(s)'} ===")
sys.exit(1 if failures else 0)
PY
