#!/usr/bin/env bash
# The conversion's guarantee, held against the live tree (ADR-306 §4, §7).
#
# Every record that was v0 in the frozen snapshot (tests/fixtures/adr/v0-corpus)
# and is v1 now must still carry what the conversion promised to keep:
#   - its raw source status, under imported.status
#   - its date and deciders, unchanged
#   - every other source frontmatter key, as a v1 field or under imported.unmapped
#   - its original body, which still opens the text after the H1, once an opening
#     Summary (which ADR-306 §4 lets anyone add) is set aside; text may be appended
# Records still v0, or archived, are skipped. A record whose body was edited on
# purpose is listed in BODY_EDITED, with the reason.
#
# A record keeps its number when `adr domain move` or `rename` changes its
# folder (ADR-306 §6), so a snapshot record is found by number wherever it
# sits now. Those commands rewrite paths to what moved, so the snapshot body
# gets the same rewrite, through the tool's own Relocation, before comparing.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

python3 - "$REPO_ROOT" <<'PY'
import re, subprocess, sys, types, yaml
from importlib.machinery import SourceFileLoader
from pathlib import Path

root = Path(sys.argv[1])
loader = SourceFileLoader('adr_tool', str(root / 'hooks/ways/documentation/adr/adr-tool'))
tool = types.ModuleType(loader.name)
loader.exec_module(tool)
snapshot = root / 'tests/fixtures/adr/v0-corpus/docs/architecture'
live = root / 'docs/architecture'

# Bodies edited on purpose after conversion: number -> reason.
BODY_EDITED = {
    '302': 'stray tool-call text removed from the end of the body',
    '113': 'a link to the archived ADR-112 now points into the archive',
    '114': 'a link to the archived ADR-112 now points into the archive',
}

TITLE = re.compile(r'^# ADR-[0-9.]+:.*$', re.M)
SUMMARY = re.compile(r'\A\s*## Summary[^\n]*\n.*?(?=^## |\Z)', re.M | re.S)

def split(text):
    if not text.startswith('---\n'):
        return None, None
    end = text.index('\n---\n', 4)
    front = yaml.safe_load(text[4:end]) or {}
    rest = text[end + 5:]
    m = TITLE.search(rest)
    return front, (rest[m.end():] if m else None)

def number(path):
    m = re.match(r'ADR-([0-9.]+)', path.name)
    return m.group(1).lstrip('0') or '0' if m else None

live_by_number = {number(p): p for p in live.rglob('ADR-*.md') if 'archive' not in p.parts}

# Where each snapshot record lives now, as the moves that took it there.
moves = {}
for src in snapshot.rglob('ADR-*.md'):
    dest = live_by_number.get(number(src))
    if dest is not None and 'archive' not in src.parts:
        was = 'docs/architecture/' + src.relative_to(snapshot).as_posix()
        now = dest.relative_to(root).as_posix()
        if was != now:
            moves[was] = now
listed = subprocess.run(['git', 'ls-files', '-z'], cwd=root, capture_output=True, text=True).stdout
known = {n for n in listed.split('\0') if n}
for name in list(known):
    while '/' in name:
        name = name.rsplit('/', 1)[0]
        known.add(name)
relocation = tool.Relocation(files=moves, known=known)

checked = failures = 0
for src in sorted(snapshot.rglob('ADR-*.md')):
    if 'archive' in src.parts:
        continue
    was = 'docs/architecture/' + src.relative_to(snapshot).as_posix()
    before, body_before = split(relocation.text(src.read_text(), was)[0])
    if before is None or before.get('contract'):
        continue
    n = number(src)
    dest = live_by_number.get(n)
    if dest is None:
        print(f"FAIL ADR-{n}: no live record")
        failures += 1
        continue
    after, body_after = split(dest.read_text())
    if not after or after.get('contract') != 'adr/v1':
        continue
    checked += 1
    problems = []
    imported = after.get('imported') or {}
    unmapped = imported.get('unmapped') or {}
    if 'status' in before and str(imported.get('status')) != str(before['status']):
        problems.append(f"imported.status is {imported.get('status')!r}, source had {before['status']!r}")
    for key in ('date', 'deciders'):
        if key in before and str(after.get(key)) != str(before[key]):
            problems.append(f"{key} changed")
    for key in before:
        if key in ('status',):
            continue
        if key not in after and key not in unmapped:
            problems.append(f"source key '{key}' is neither a field nor under imported.unmapped")
    if body_before is None or body_after is None:
        problems.append("no H1 on one side")
    elif n not in BODY_EDITED:
        opened = SUMMARY.sub('', body_after, count=1).lstrip('\n')
        if not opened.startswith(body_before.lstrip('\n')):
            problems.append("the original body no longer opens the record")
    for p in problems:
        print(f"FAIL ADR-{n}: {p}")
    failures += bool(problems)
print(f"converted records checked: {checked}, failing: {failures}")
sys.exit(1 if failures else 0)
PY
status=$?
echo ""
[[ $status -eq 0 ]] && echo "=== ADR Conversion Check: passed ===" || echo "=== ADR Conversion Check: FAILED ==="
exit $status
