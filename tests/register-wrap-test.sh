#!/usr/bin/env bash
# Runs the wrapped-paragraph check in scripts/check-register.sh over the
# fixtures in tests/fixtures/register. Each wrapped-*.md must fail on the line
# its `<!-- expect: N -->` comment names. Each clean-*.md must pass.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHECK="$REPO_ROOT/scripts/check-register.sh"
DIR="$REPO_ROOT/tests/fixtures/register"
pass=0; fail=0

for f in "$DIR"/wrapped-*.md; do
  [[ -e "$f" ]] || continue
  want="$(sed -nE '1s/^<!-- expect: ([0-9]+) -->$/\1/p' "$f")"
  out="$("$CHECK" --wrapped "$f")"; rc=$?
  if (( rc == 1 )) && [[ -n "$want" ]] && grep -q ":$want: " <<<"$out"; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1)); echo "FAIL ${f##*/}: want line $want flagged, got rc=$rc: $out"
  fi
done

for f in "$DIR"/clean-*.md; do
  [[ -e "$f" ]] || continue
  out="$("$CHECK" --wrapped "$f")"; rc=$?
  if (( rc == 0 )); then
    pass=$((pass + 1))
  else
    fail=$((fail + 1)); echo "FAIL ${f##*/}: want clean, got: $out"
  fi
done

if (( pass + fail == 0 )); then
  echo "FAIL no fixtures found in $DIR"; exit 1
fi
echo "register wrap fixtures: $pass passed, $fail failed"
(( fail == 0 ))
