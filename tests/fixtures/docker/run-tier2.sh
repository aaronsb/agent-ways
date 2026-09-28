#!/usr/bin/env bash
# Tier 2 runner (ADR-186 item 3): tier 1's install, then scenarios driven by a
# real model through `claude -p`. Runs inside the fixture container; see
# compose.yaml. Never runs on a pull request (ADR-186 item 4).
#
# Environment:
#   ANTHROPIC_API_KEY  required; read by claude, never printed
#   TIER2_MODEL        model for every scenario (default below)
#   TIER2_SCENARIOS    space-separated scenario names; empty runs them all
#   TIER2_MAX_TURNS    per-scenario turn cap (default 8)
#   plus everything run-tier1.sh reads
#
# Mounts: as tier 1, plus /out (writable) for transcripts and introspection.
#
# A scenario is a directory under scenarios/ holding:
#   prompt.txt   the prompt passed to `claude -p`
#   setup.sh     optional; runs in the scenario's fresh project before the prompt
#   max_turns    optional; this scenario's turn cap (default TIER2_MAX_TURNS)
#   prompt2.txt  optional; a second operator turn, run with `claude -p --resume`
#   check1.sh    optional, with prompt2.txt; sourced between the two turns
#   check.sh     sourced after the run; asserts with the helpers below
#
# check.sh sees $PROJ (the project dir), $ANSWER (the model's final text),
# $FIRED (fired way ids, one per line) and $OUT (this scenario's output dir).
# With a second turn, check1.sh sees the first turn's $ANSWER, check.sh the
# second's, and $FIRED holds the ways fired in either turn.

set -uo pipefail

FIX=/fixture
MODEL="${TIER2_MODEL:-claude-sonnet-5}"
MAX_TURNS="${TIER2_MAX_TURNS:-8}"
OUT_ROOT=/out

if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "ANTHROPIC_API_KEY is not set" >&2
  exit 2
fi

if ! touch "$OUT_ROOT/.writable" 2>/dev/null; then
  echo "/out is not writable by $(id -un); create TIER2_OUT on the host first" >&2
  exit 2
fi
rm -f "$OUT_ROOT/.writable"

# --- tier 1 is the precondition ----------------------------------------------
# It installs agent-ways into this home and asserts the install. A tier 1
# failure makes every scenario meaningless, so tier 2 stops there.

bash "$FIX/run-tier1.sh" || { echo "tier 1 failed; tier 2 not run" >&2; exit 1; }

PASS=0
FAIL=0
FAILED=()
SCENARIO=""

ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); FAILED+=("$SCENARIO: $1"); printf '  FAIL  %s\n' "$1"; [[ $# -gt 1 ]] && printf '        %s\n' "${@:2}"; }
section() { printf '\n== %s\n' "$1"; }

# fired WAY_ID — passes when the way fired during the run (a hard assertion)
fired() {
  if grep -qxF "$1" <<<"$FIRED"; then ok "way fired: $1"; else fail "way fired: $1" "fired: $(paste -sd' ' <<<"$FIRED")"; fi
}

# not_fired WAY_ID — passes when the way stayed silent
not_fired() {
  if grep -qxF "$1" <<<"$FIRED"; then fail "way stayed silent: $1"; else ok "way stayed silent: $1"; fi
}

# Rubric items score the answer. The model's wording varies run to run, so a
# scenario passes its rubric at a stated threshold rather than on every item.
RUBRIC_HIT=0
RUBRIC_TOTAL=0
# rubric DESCRIPTION REGEX — case-insensitive extended regex over $ANSWER
rubric() {
  RUBRIC_TOTAL=$((RUBRIC_TOTAL + 1))
  if grep -qiE -- "$2" <<<"$ANSWER"; then
    RUBRIC_HIT=$((RUBRIC_HIT + 1)); printf '  hit   %s\n' "$1"
  else
    printf '  miss  %s\n' "$1"
  fi
}
# rubric_threshold N — the scenario's rubric passes with at least N hits
rubric_threshold() {
  local name="rubric: $RUBRIC_HIT of $RUBRIC_TOTAL, need $1"
  if [[ $RUBRIC_HIT -ge $1 ]]; then ok "$name"; else fail "$name"; fi
}

# --- scenarios ----------------------------------------------------------------

run_scenario() {
  local dir="$1" name
  name=$(basename "$dir")
  SCENARIO="$name"
  section "scenario: $name ($MODEL)"

  PROJ="$HOME/scenarios/$name"
  OUT="$OUT_ROOT/$name"
  mkdir -p "$PROJ" "$OUT"
  (cd "$PROJ" && git init -q && printf '# %s\n' "$name" > README.md && git add -A && git commit -qm init)
  if [[ -f "$dir/setup.sh" ]]; then
    (cd "$PROJ" && bash "$dir/setup.sh") > "$OUT/setup.log" 2>&1 \
      || { fail "setup.sh ran" "see $OUT/setup.log"; return; }
  fi

  # The container is disposable and holds nothing but this run, so the model
  # gets tools without prompts. The turn cap bounds cost.
  # A scenario may raise the turn cap with a max_turns file.
  local turns="$MAX_TURNS"
  [[ -f "$dir/max_turns" ]] && turns=$(tr -dc '0-9' < "$dir/max_turns")
  turns=${turns:-$MAX_TURNS}
  SESSION=""
  FIRED=""
  run_turn "$dir/prompt.txt" "" "$turns"
  if [[ -f "$dir/prompt2.txt" ]]; then
    if [[ -f "$dir/check1.sh" ]]; then
      RUBRIC_HIT=0
      RUBRIC_TOTAL=0
      # shellcheck disable=SC1091
      source "$dir/check1.sh"
    fi
    if [[ -n "$SESSION" ]]; then
      run_turn "$dir/prompt2.txt" 2 "$turns"
    else
      fail "claude -p (turn 2) resumed turn 1" "turn 1 gave no session_id; see $OUT/result.json"
    fi
  fi

  RUBRIC_HIT=0
  RUBRIC_TOTAL=0
  # shellcheck disable=SC1091
  source "$dir/check.sh"
}

# run_turn PROMPT_FILE SUFFIX TURNS — one `claude -p` call capped at TURNS.
# An empty SUFFIX is the first turn; a second turn (SUFFIX 2) resumes
# $SESSION, writes its files with the suffix (result2.json, ...), and adds
# its fired ways to $FIRED.
run_turn() {
  local prompt="$1" sfx="$2" turns="$3" rc
  local resume=()
  [[ -n "$sfx" ]] && resume=(--resume "$SESSION")
  (cd "$PROJ" && claude -p "$(cat "$prompt")" ${resume[@]+"${resume[@]}"} \
      --model "$MODEL" \
      --max-turns "$turns" \
      --output-format json \
      --dangerously-skip-permissions) > "$OUT/result$sfx.json" 2> "$OUT/claude$sfx.err"
  rc=$?
  local label="claude -p exited 0${sfx:+ (turn $sfx)}"
  if [[ $rc -ne 0 ]]; then
    fail "$label" "exit $rc; see $OUT/claude$sfx.err"
  else
    ok "$label"
  fi

  ANSWER=$(jq -r '.result // empty' "$OUT/result$sfx.json" 2>/dev/null)
  SESSION=$(jq -r '.session_id // empty' "$OUT/result$sfx.json" 2>/dev/null)
  (cd "$PROJ" && ways introspect dump --session "$SESSION" --all) > "$OUT/introspect$sfx.json" 2>"$OUT/introspect$sfx.err"
  (cd "$PROJ" && git status --porcelain --untracked-files=all) > "$OUT/worktree$sfx.txt" 2>&1
  FIRED=$( { printf '%s\n' "$FIRED"; jq -r '[.turns[].fired_ways[]?.way_id] | unique | .[]' "$OUT/introspect$sfx.json" 2>/dev/null; } | sed '/^$/d' | sort -u)
  printf '%s\n' "$FIRED" > "$OUT/fired$sfx.txt"
}

for dir in "$FIX"/scenarios/*/; do
  name=$(basename "$dir")
  if [[ -n "${TIER2_SCENARIOS:-}" && " $TIER2_SCENARIOS " != *" $name "* ]]; then
    continue
  fi
  run_scenario "${dir%/}"
done

# --- summary ------------------------------------------------------------------

printf '\n== tier 2: %d passed, %d failed (transcripts under /out)\n' "$PASS" "$FAIL"
if [[ $FAIL -gt 0 ]]; then
  printf '  - %s\n' "${FAILED[@]}"
  exit 1
fi
