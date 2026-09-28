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
# sits now. Those commands rewrite paths to what moved, and ADR-309 turned
# design notes into records. The check builds its own map of those moves,
# from the snapshot, the live tree and the NOTES table below, and applies it
# to the snapshot body before comparing. It shares no code with the tool, so
# a fault in the tool's rewrite shows here as a failing record.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

python3 - "$REPO_ROOT" <<'PY'
import posixpath, re, sys, yaml
from pathlib import Path

root = Path(sys.argv[1])
snapshot = root / 'tests/fixtures/adr/v0-corpus/docs/architecture'
live = root / 'docs/architecture'

# Bodies edited on purpose after conversion: number -> reason.
BODY_EDITED = {
    '302': 'stray tool-call text removed from the end of the body',
}

# Design notes a snapshot body names that became records (ADR-309): the
# note's path -> the record's number.
NOTES = {
    'docs/design-notes/attend-envelope-fields.md': '401',
    'docs/design-notes/attend-messaging-disclosure-reheat.md': '400',
    'docs/design-notes/cognitive-loop-and-awareness-layer.md': '600',
    'docs/design-notes/cypress-survey.md': '603',
    'docs/design-notes/settings-json-merge-spec-and-peer-writer-contract.md': '500',
    'docs/design-notes/tool-use-channel-lookbehind-chunk-matching.md': '191',
}

# Links that named no file in the snapshot and were repaired after
# conversion: the record's number -> (the folder the links named it in,
# the reason). The links now point to where the record is.
REPAIRED = {
    '112': ('docs/architecture/system',
            'the session-ledger record was already archived; links named it as a sibling'),
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
# An archived record is still a place a link can point to.
anywhere = {**{number(p): p for p in live.rglob('ADR-*.md') if 'archive' in p.parts}, **live_by_number}

def repo_path(path):
    return path.relative_to(root).as_posix()

# Each file a snapshot body may name by path -> where that file is now: every
# snapshot record, found by its number, each design note in NOTES, and each
# broken link in REPAIRED.
moved = {}
for src in snapshot.rglob('ADR-*.md'):
    dest = anywhere.get(number(src))
    if dest is not None:
        moved['docs/architecture/' + src.relative_to(snapshot).as_posix()] = repo_path(dest)
for note, n in NOTES.items():
    moved[note] = repo_path(anywhere[n])
for n, (folder, _) in REPAIRED.items():
    moved[f'{folder}/{anywhere[n].name}'] = repo_path(anywhere[n])

PATH = re.compile(r'[\w./-]+')

def relocate(body, was, now):
    """body, written at `was`, with each path to a moved file rewritten to
    that file's place now: a path from the repo root stays one, and a path
    relative to the record is written relative to where the record is now."""
    was_dir, now_dir = posixpath.dirname(was), posixpath.dirname(now)
    def one(m):
        token = m.group(0)
        path = token.rstrip('.')
        if path in moved:
            return moved[path] + token[len(path):]
        resolved = posixpath.normpath(posixpath.join(was_dir, path))
        target = moved.get(resolved)
        if target is None or posixpath.normpath(posixpath.join(now_dir, path)) == target:
            return token
        new = posixpath.relpath(target, now_dir)
        if path.startswith('./') and not new.startswith('.'):
            new = './' + new
        return new + token[len(path):]
    return PATH.sub(one, body)

checked = failures = 0
for src in sorted(snapshot.rglob('ADR-*.md')):
    if 'archive' in src.parts:
        continue
    was = 'docs/architecture/' + src.relative_to(snapshot).as_posix()
    before, body_before = split(src.read_text())
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
        expected = relocate(body_before, was, repo_path(dest))
        if not opened.startswith(expected.lstrip('\n')):
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
