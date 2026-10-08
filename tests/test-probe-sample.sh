#!/usr/bin/env bash
# Probe-sample drift check. tests/probes/tree-sample.tsv is the tree-sampled probe
# set (`ways author golden --probes`, ADR-701 §9). It is derived from the golden
# sidecars under hooks/ways, so it must track corpus edits. This fails when the
# committed file differs from what the current tree yields; regenerate with:
#
#   { printf '%s\n' "$(head -1 tests/probes/tree-sample.tsv)"; \
#     bin/ways author golden --ways-dir hooks/ways --probes; } > tests/probes/tree-sample.tsv
#
# Skips (exit 0) when no ways binary that knows --probes is built.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMMITTED="$SCRIPT_DIR/probes/tree-sample.tsv"
HEADER=$'prompt\texpected_way\tkind\trole\tmust_not'

WAYS_BIN=""
for c in "${WAYS_BIN_OVERRIDE:-}" "$REPO_ROOT/bin/ways" "$REPO_ROOT/tools/target/release/ways"; do
  [[ -n "$c" && -x "$c" ]] || continue
  if "$c" author golden --help 2>/dev/null | grep -q -- '--probes'; then WAYS_BIN="$c"; break; fi
done

if [[ -z "$WAYS_BIN" ]]; then
  echo "SKIP: no ways binary with 'author golden --probes' (run 'make setup')"
  exit 0
fi
[[ -f "$COMMITTED" ]] || { echo "FAIL: $COMMITTED is missing"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if ! "$WAYS_BIN" author golden --ways-dir "$REPO_ROOT/hooks/ways" --probes > "$TMP/body.tsv" 2> "$TMP/err"; then
  echo "FAIL: ways author golden --probes errored:"
  cat "$TMP/err"
  exit 1
fi
{ printf '%s\n' "$HEADER"; cat "$TMP/body.tsv"; } > "$TMP/now.tsv"

if diff -u "$COMMITTED" "$TMP/now.tsv" > "$TMP/diff"; then
  echo "PASS: tree-sample.tsv matches the corpus ($(($(wc -l < "$COMMITTED") - 1)) probes)"
  exit 0
fi

echo "FAIL: tests/probes/tree-sample.tsv is out of date with the golden sidecars"
head -40 "$TMP/diff"
echo "Regenerate it (see the header of tests/test-probe-sample.sh)."
exit 1
