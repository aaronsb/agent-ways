#!/usr/bin/env bash
# TaskCreated — guard for issue-backed work (ADR-180).
#
# Fires only when the new task references an issue number that already has a
# mirrored task gh-<n> in this session's store, and the subject lacks the
# [gh#n] prefix. Exit 2 rolls the creation back; stderr carries the reason.
# Everything else passes.

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
SUBJECT=$(echo "$INPUT" | jq -r '.task_input.subject // .task_subject // empty')
DESC=$(echo "$INPUT" | jq -r '.task_input.description // .task_description // empty')
[[ -n "$SESSION_ID" && -n "$SUBJECT" ]] || exit 0

GH_TASKS="$(dirname "$0")/softwaredev/delivery/issues/gh-tasks"
[[ -x "$GH_TASKS" ]] || exit 0
DIR="$("$GH_TASKS" --session "$SESSION_ID" dir 2>/dev/null)"
[[ -n "$DIR" && -d "$DIR" ]] || exit 0

[[ "$SUBJECT" =~ ^\[gh#[0-9]+\] ]] && exit 0

# Issue references. A bare #N counts only in the subject: a description that
# says "see #12 for context" is not a task about issue 12, and PRs share the
# number space.
REFS=$( { printf '%s\n' "$SUBJECT" | grep -oE '(\[gh#|#|/issues/)[0-9]+';
          printf '%s\n' "$DESC" | grep -oE '(\[gh#|/issues/)[0-9]+'; } | grep -oE '[0-9]+' | sort -u)
[[ -n "$REFS" ]] || exit 0

for n in $REFS; do
  [[ -f "$DIR/gh-$n.json" ]] || continue
  echo "Issue #$n is already mirrored as task gh-$n in this session (ADR-180). Update that task with TaskUpdate, or prefix the subject with [gh#$n] to add a linked sub-task." >&2
  exit 2
done
exit 0
