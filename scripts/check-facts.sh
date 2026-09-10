#!/usr/bin/env bash
# Fact drift for edited markdown (issue #474).
#
# A register rewrite reshapes sentences. The facts those sentences carry have
# to survive it: a count, a path, an identifier, a heading, a link target. This
# script extracts each class as a multiset from both versions of every changed
# markdown file and prints what left. Sentences may change freely. These may not.
#
# Advisory by construction. Whether a dropped fact was deliberate is a judgement
# the editor makes, so this reports and exits 0 either way.
#
# Usage: scripts/check-facts.sh [REV]
#   REV defaults to HEAD, which compares the working tree against the last
#   commit. On a clean tree that finds nothing, so the comparison falls back to
#   HEAD~1 against HEAD and reports the commit just landed.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT" || exit 0
command -v python3 >/dev/null || { echo "check-facts: python3 not found"; exit 0; }

REV="${1:-HEAD}"
NEWREV=""   # empty means the working tree
# A mistyped revision would otherwise report a clean tree, which reads the same
# as a real pass. Say the rev is unknown instead.
git rev-parse --verify --quiet "$REV^{commit}" >/dev/null \
  || { echo "check-facts: unknown revision $REV"; exit 0; }
mapfile -t FILES < <(git diff --name-only "$REV" -- '*.md' 2>/dev/null)
if (( ${#FILES[@]} == 0 )) && [[ "$REV" == "HEAD" ]] \
   && git rev-parse --verify --quiet HEAD~1 >/dev/null; then
  REV="HEAD~1"; NEWREV="HEAD"
  mapfile -t FILES < <(git diff --name-only "$REV" HEAD -- '*.md' 2>/dev/null)
fi

echo "Fact drift since $REV (advisory, never fails)"
if (( ${#FILES[@]} == 0 )); then
  echo "  no markdown changed"
  exit 0
fi

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
cat > "$TMP/facts.py" <<'PY'
import re, sys
from collections import Counter

NUMBER = re.compile(r"\b\d+(?:[.,]\d+)*\b")
HEADING = re.compile(r"(?m)^[ ]{0,3}#{1,6} +.*$")
# One line only, so a fence opener never pairs with a fence closer below it.
CODE_SPAN = re.compile(r"(`+)(?:(?!\1).)+?\1")
LINK_TARGET = re.compile(r"(?<=\])\(([^)\n]+)\)")
FENCE = re.compile(r"^[ \t]*(`{3,}|~{3,})")

def unfenced(text):
    """The lines outside fenced blocks. A backtick run inside a code sample is
    part of the sample, so only prose contributes inline spans."""
    out, fence = [], None
    for line in text.splitlines():
        m = FENCE.match(line)
        if fence is None:
            if m:
                fence = m.group(1)[0]
                continue
            out.append(line)
        elif m and m.group(1)[0] == fence:
            fence = None
    return "\n".join(out)

def facts(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    return {"number": Counter(NUMBER.findall(text)),
            "heading": Counter(h.strip() for h in HEADING.findall(text)),
            "code span": Counter(m.group(0)
                                 for m in CODE_SPAN.finditer(unfenced(text))),
            "link target": Counter(LINK_TARGET.findall(text))}

def show(items, cap=6):
    out = []
    for value, n in sorted(items.items()):
        one = " ".join(value.split())
        if len(one) > 50:
            one = one[:47] + "..."
        out.append(one if n == 1 else f"{one} (x{n})")
    rest = len(out) - cap
    return ", ".join(out[:cap]) + (f", +{rest} more" if rest > 0 else "")

old, new = facts(sys.argv[1]), facts(sys.argv[2])
lines = []
for cls in old:
    gone = old[cls] - new[cls]
    if not gone:
        continue
    lines.append(f"    {cls}(s) gone: {show(gone)}")
    arrived = new[cls] - old[cls]
    if arrived:
        lines.append(f"      in place of them: {show(arrived)}")
print("\n".join(lines))
sys.exit(1 if lines else 0)
PY

DRIFTED=0
for f in "${FILES[@]}"; do
  git show "$REV:$f" > "$TMP/old.md" 2>/dev/null \
    || { echo "  $f: absent from $REV, skipped"; continue; }
  if [[ -n "$NEWREV" ]]; then
    git show "$NEWREV:$f" > "$TMP/new.md" 2>/dev/null || continue
  elif [[ -f "$f" ]]; then
    cp "$f" "$TMP/new.md"
  else
    echo "  $f: deleted, skipped"; continue
  fi
  if OUT="$(python3 "$TMP/facts.py" "$TMP/old.md" "$TMP/new.md")"; then
    continue
  fi
  DRIFTED=$(( DRIFTED + 1 ))
  echo "  $f"
  printf '%s\n' "$OUT"
done

(( DRIFTED == 0 )) \
  && echo "  ${#FILES[@]} file(s) checked, every fact survived" \
  || echo "  ${#FILES[@]} file(s) checked, $DRIFTED with drift — confirm each drop was meant"
exit 0
