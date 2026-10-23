#!/usr/bin/env bash
# PostToolUse(Bash gh pr merge) — after a merge, or an auto-merge request,
# name any CI check that was not green (issue #1168). It never blocks:
# PostToolUse reports after the fact, and the exit status is always 0. It
# runs for subagents too, since a subagent may merge.
#
# Usage in settings.json: pr-merge-checks.sh
#
# PostToolUse needs hookSpecificOutput.additionalContext. Silence means the
# command was no merge, the PR is neither freshly merged nor queued for
# auto-merge, gh could not answer, or every check was pass/skipping.

# Stock macOS has no `timeout`; the stand-in runs the call unbounded.
command -v timeout >/dev/null 2>&1 || timeout() { shift; "$@"; }
command -v gh >/dev/null 2>&1 && command -v jq >/dev/null 2>&1 || exit 0

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null)
[[ "$CMD" == *"gh pr merge"* ]] || exit 0
CWD=$(echo "$INPUT" | jq -r '.cwd // empty' 2>/dev/null)
[[ -n "$CWD" && -d "$CWD" ]] && { cd "$CWD" || exit 0; }

# A `cd` before the merge moves it to a directory this hook cannot see.
PREFIX="${CMD%%gh pr merge*}"
CD_RE='(^|[;&|(]|[[:space:]])cd([[:space:]]|$)'
[[ "$PREFIX" =~ $CD_RE ]] && exit 0
# A leading GH_REPO=x on the merge segment applies to the gh calls.
REPO_RE='GH_REPO=([^[:space:]]+)[[:space:]]*$'
if [[ "$PREFIX" =~ $REPO_RE ]]; then
  GH_REPO="${BASH_REMATCH[1]//[\"\']/}"; export GH_REPO
fi

# Tokenize what follows `gh pr merge`, quotes honoured, nothing evaluated.
REST="${CMD#*gh pr merge}"
REST="${REST%%$'\n'*}"
TOKENS=$(printf '%s' "$REST" | xargs printf '%s\n' 2>/dev/null) || exit 0

REDIR_BARE='^[0-9]*(>>?|<|>&|<&)$'
REDIR_ATTACHED='^[0-9]*[<>]'
SEL=""; REPO=""; skip=""
while IFS= read -r a; do
  if [[ -n "$skip" ]]; then
    [[ "$skip" == repo ]] && REPO="$a"
    skip=""; continue
  fi
  case "$a" in
    ';'|'&&'|'||'|'|'|'|&'|'&') break ;;
  esac
  stop=""
  case "$a" in
    *[\;\|\&\)\`]*) a="${a%%[;|&)\`]*}"; stop=1 ;;
  esac
  if [[ "$a" =~ $REDIR_BARE ]]; then skip=other
  elif [[ "$a" =~ $REDIR_ATTACHED ]]; then :
  else
    case "$a" in
      "") ;;
      -R|--repo) skip=repo ;;
      --repo=*) REPO="${a#--repo=}" ;;
      -R?*) REPO="${a#-R}" ;;
      -t|--subject|-b|--body|-F|--body-file|-A|--author-email|--match-head-commit) skip=other ;;
      -*) ;;
      *) [[ -n "$SEL" ]] || SEL="$a" ;;
    esac
  fi
  [[ -z "$stop" ]] || break
done <<< "$TOKENS"

# No selector: gh reads the current branch's PR. After a merge gh may have
# switched to the default branch, so look at the branch left behind.
if [[ -z "$SEL" ]]; then
  CUR=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
  DEF=$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)
  DEF="${DEF#origin/}"
  if [[ -n "$CUR" && "$CUR" == "$DEF" ]]; then
    SEL=$(git rev-parse --abbrev-ref '@{-1}' 2>/dev/null) || exit 0
  fi
fi

ARGS=()
[[ -n "$SEL" ]] && ARGS+=("$SEL")
[[ -n "$REPO" ]] && ARGS+=(-R "$REPO")

# One state query decides whether anything merged or is queued to merge.
VIEW=$(timeout 6 gh pr view "${ARGS[@]}" --json number,state,mergedAt,autoMergeRequest 2>/dev/null) || exit 0
[[ -n "$VIEW" ]] || exit 0
NUM=$(echo "$VIEW" | jq -r '.number // empty' 2>/dev/null)
STATE=$(echo "$VIEW" | jq -r '.state // empty' 2>/dev/null)
[[ -n "$NUM" ]] || exit 0

KIND=""
if [[ "$STATE" == MERGED ]]; then
  AGE=$(echo "$VIEW" | jq -r 'if .mergedAt then (now - (.mergedAt | fromdateiso8601) | floor) else empty end' 2>/dev/null)
  [[ "$AGE" =~ ^-?[0-9]+$ ]] && (( AGE <= 600 )) && KIND=merged
elif [[ "$STATE" == OPEN ]]; then
  [[ "$(echo "$VIEW" | jq -r 'if .autoMergeRequest then "yes" else empty end' 2>/dev/null)" == yes ]] && KIND=queued
fi
[[ -n "$KIND" ]] || exit 0

CARGS=("$NUM")
[[ -n "$REPO" ]] && CARGS+=(-R "$REPO")
CHECKS=$(timeout 6 gh pr checks "${CARGS[@]}" --json name,bucket 2>/dev/null) || exit 0
[[ -n "$CHECKS" ]] || exit 0

BAD=$(echo "$CHECKS" | jq -r '[.[] | select(.bucket != "pass" and .bucket != "skipping") | "\(.name) (\(.bucket))"] | join(", ")' 2>/dev/null) || exit 0
[[ -n "$BAD" ]] || exit 0

if [[ "$KIND" == merged ]]; then
  MSG="PR #$NUM merged while these checks were not green: $BAD. If you merged under a green-CI grant, this merge fell outside it; tell the operator."
else
  MSG="PR #$NUM is queued for auto-merge; GitHub merges it once required checks pass. Not green now: $BAD."
fi
jq -n --arg ctx "$MSG" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
exit 0
