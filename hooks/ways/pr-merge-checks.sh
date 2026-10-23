#!/usr/bin/env bash
# PostToolUse(Bash gh pr merge) — after a merge, name any CI check that was
# not green (issue #1168). It never blocks: PostToolUse reports after the
# fact, and the exit status is always 0. It runs for subagents too, since a
# subagent may merge.
#
# Usage in settings.json: pr-merge-checks.sh
#
# PostToolUse needs hookSpecificOutput.additionalContext. Silence means the
# command was no merge, gh could not answer, or every check was pass/skipping.

# Stock macOS has no `timeout`; the stand-in runs the call unbounded.
command -v timeout >/dev/null 2>&1 || timeout() { shift; "$@"; }
command -v gh >/dev/null 2>&1 && command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null)
[[ "$CMD" == *"gh pr merge"* ]] || exit 0
CWD=$(echo "$INPUT" | jq -r '.cwd // empty' 2>/dev/null)
[[ -n "$CWD" && -d "$CWD" ]] && cd "$CWD"

# Arguments after `gh pr merge`, up to the next shell separator.
REST="${CMD#*gh pr merge}"
REST="${REST%%[;|&]*}"
SEL=""; REPO=""; skip=""
# shellcheck disable=SC2086
set -f; set -- $REST; set +f
for a in "$@"; do
  if [[ -n "$skip" ]]; then
    [[ "$skip" == repo ]] && REPO="$a"
    skip=""; continue
  fi
  case "$a" in
    -R|--repo) skip=repo ;;
    --repo=*) REPO="${a#--repo=}" ;;
    -R?*) REPO="${a#-R}" ;;
    -b|--body|-F|--body-file|-t|--subject|--match-head-commit|-A|--author-email) skip=other ;;
    -*) ;;
    *) [[ -n "$SEL" ]] || SEL="$a" ;;
  esac
done

ARGS=()
[[ -n "$SEL" ]] && ARGS+=("$SEL")
[[ -n "$REPO" ]] && ARGS+=(-R "$REPO")

CHECKS=$(timeout 10 gh pr checks "${ARGS[@]}" --json name,bucket 2>/dev/null) || exit 0
[[ -n "$CHECKS" ]] || exit 0

BAD=$(echo "$CHECKS" | jq -r '[.[] | select(.bucket != "pass" and .bucket != "skipping") | "\(.name) (\(.bucket))"] | join(", ")' 2>/dev/null) || exit 0
[[ -n "$BAD" ]] || exit 0

NUM=$(timeout 10 gh pr view "${ARGS[@]}" --json number -q .number 2>/dev/null)
LABEL="${NUM:+#$NUM}"
LABEL="${LABEL:-$SEL}"

MSG="PR${LABEL:+ $LABEL} merged with checks not green: ${BAD}. A grant that says green CI did not cover this merge; say so to the operator in your reply."
jq -n --arg ctx "$MSG" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
exit 0
