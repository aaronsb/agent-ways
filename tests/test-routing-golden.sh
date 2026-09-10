#!/usr/bin/env bash
# Golden routing test. Score a committed prompt-to-way file and fail on a misroute.
#
# tests/routing-golden.tsv holds prompts a user would plausibly type, each with
# the one way that should win. This scores every row against a corpus built from
# hooks/ways in this checkout, so it measures the ways in the working tree rather
# than whatever is projected into ~/.claude.
#
# Two checks:
#   1. top-1 equals expected_way for at least TOP1_MIN of the targeted rows
#   2. every `none` row scores below NONE_MAX, so a novel domain stays quiet
#
# The corpus carries project-scoped ways from the operator's own registry. Their
# ids start with `-` (a mangled project path), and they belong to no committed
# way file, so scoring drops them.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GOLDEN="${1:-$SCRIPT_DIR/routing-golden.tsv}"   # an alternate TSV, for probing candidates

TOP1_MIN=90     # percent of targeted rows whose top-1 must equal expected_way
NONE_MAX=0.30   # a `none` row must score below this

# --- locate the engine -------------------------------------------------------

CACHE_BASE="${XDG_CACHE_HOME:-$HOME/.cache}"
CORPUS_DIR="$CACHE_BASE/agent-ways/user"
[[ -d "$CORPUS_DIR" ]] || CORPUS_DIR="$CACHE_BASE/claude-ways/user"

EMBED=""
for c in "$CORPUS_DIR/way-embed" "$HOME/.claude/bin/way-embed" \
         "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/bin/way-embed"; do
  [[ -x "$c" ]] && { EMBED="$c"; break; }
done

MODEL=""
for m in "$CORPUS_DIR/minilm-l6-v2.gguf" "$CORPUS_DIR/minilm-l6-v2-q5km.gguf"; do
  [[ -f "$m" ]] && { MODEL="$m"; break; }
done

WAYS_BIN="$REPO_ROOT/bin/ways"

skip() { echo "SKIP: $1"; exit 0; }

[[ -f "$GOLDEN" ]] || skip "$GOLDEN not found"
[[ -n "$EMBED" ]]  || skip "way-embed not found (run 'make setup')"
[[ -n "$MODEL" ]]  || skip "embedding model not found (run 'make setup')"
[[ -x "$WAYS_BIN" ]] || skip "bin/ways not found (run 'make setup')"

# --- build an isolated corpus from the working tree ---------------------------

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if ! "$WAYS_BIN" corpus --ways-dir "$REPO_ROOT/hooks/ways" --output "$TMP/corpus" -q >/dev/null 2>&1; then
  skip "corpus build failed (embedding engine unavailable)"
fi

CORPUS="$TMP/corpus/ways-corpus.jsonl"
[[ -s "$CORPUS" ]] || skip "corpus build produced no entries"

# --- score -------------------------------------------------------------------

# Top-scoring committed way for a prompt, as "id<TAB>score".
top_match() {
  "$EMBED" match --corpus "$CORPUS" --model "$MODEL" --query "$1" --threshold 0.0 2>/dev/null \
    | grep -v '^-' | head -1
}

printf '%-6s  %-38s  %-38s  %s\n' "RESULT" "EXPECTED" "TOP-1" "SCORE"
printf '%s\n' "$(printf '%.0s-' {1..100})"

targeted=0; targeted_pass=0; none_rows=0; none_fail=0; failures=()

while IFS=$'\t' read -r prompt expected; do
  [[ -z "${prompt:-}" || -z "${expected:-}" ]] && continue
  [[ "$prompt" == "prompt" ]] && continue

  row="$(top_match "$prompt")"
  top_id="$(cut -f1 <<<"$row")"
  top_score="$(cut -f2 <<<"$row")"

  if [[ "$expected" == "none" ]]; then
    none_rows=$((none_rows + 1))
    if awk -v s="${top_score:-0}" -v m="$NONE_MAX" 'BEGIN{exit !(s+0 < m)}'; then
      result=PASS
    else
      result=FAIL
      none_fail=$((none_fail + 1))
      failures+=("none row scored $top_score on $top_id: $prompt")
    fi
  else
    targeted=$((targeted + 1))
    if [[ "$top_id" == "$expected" ]]; then
      result=PASS
      targeted_pass=$((targeted_pass + 1))
    else
      result=FAIL
      failures+=("expected $expected, got $top_id: $prompt")
    fi
  fi

  printf '%-6s  %-38s  %-38s  %s\n' "$result" "$expected" "${top_id:-<none>}" "${top_score:-0}"
done < "$GOLDEN"

echo ""

if (( targeted == 0 )); then
  echo "FAIL: no targeted rows in $GOLDEN"
  exit 1
fi

rate=$(( targeted_pass * 100 / targeted ))
echo "top-1: $targeted_pass/$targeted (${rate}%, floor ${TOP1_MIN}%)"
echo "none:  $((none_rows - none_fail))/$none_rows below $NONE_MAX"

if (( ${#failures[@]} > 0 )); then
  echo ""
  echo "misroutes:"
  printf '  %s\n' "${failures[@]}"
fi

status=0
(( rate < TOP1_MIN )) && { echo ""; echo "FAILED: top-1 rate ${rate}% is below the ${TOP1_MIN}% floor"; status=1; }
(( none_fail > 0 )) && { echo ""; echo "FAILED: $none_fail novel-domain row(s) scored at or above $NONE_MAX"; status=1; }

exit $status
