#!/bin/bash
# PreToolUse:Task — thin dispatcher to ways binary
#
# Phase 1 of two-phase subagent injection:
# 1. This script: ways scan task (matches ways, writes stash)
# 2. SubagentStart: inject-subagent.sh (reads stash, emits content)
#
# Skips dispatches to custom-defined agents (project, global, or plugin):
# their .md file IS the agent's constitution, so ways injection would be
# redundant — and their delegation prompts are large enough that embedding
# them against the corpus crashes the embedder on its position-embedding
# limit. The scan/embed exists for generic subagents that lack such a
# constitution (general-purpose, Explore, Plan, etc.).

source "$(dirname "$0")/require-ways.sh"

INPUT=$(cat)
TASK_PROMPT=$(echo "$INPUT" | jq -r '.tool_input.prompt // empty' | tr '[:upper:]' '[:lower:]')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
TEAM_NAME=$(echo "$INPUT" | jq -r '.tool_input.team_name // empty')
SUBAGENT_TYPE=$(echo "$INPUT" | jq -r '.tool_input.subagent_type // empty')

[[ -z "$TASK_PROMPT" || -z "$SESSION_ID" ]] && exit 0

# Skip custom agents — they have their own .md constitution.
#
# SUBAGENT_TYPE is model-emitted tool input and is used below as a path
# component. Validate the shape before letting it touch the filesystem
# — Claude Code's own agent names match `[a-zA-Z0-9_-]+` (general-purpose,
# vue-expert, code-reviewer, claude-code-guide). Without this guard,
# `subagent_type="*"` would glob-expand and match every plugin agent
# (spurious skip), and `"../foo"` would do shallow path traversal in
# the `-f` checks. Blast radius is bounded (only effect is whether we
# skip the scan, never an exec), but the input is untrusted so we keep
# the path-component contract honest.
if [[ "$SUBAGENT_TYPE" =~ ^[a-zA-Z0-9_-]+$ ]]; then
  if [[ -f "${PROJECT_DIR}/.claude/agents/${SUBAGENT_TYPE}.md" ]] \
     || [[ -f "${HOME}/.claude/agents/${SUBAGENT_TYPE}.md" ]]; then
    exit 0
  fi
  # Plugin-provided agents live under .../plugins/<plugin>/agents/.
  # `compgen -G` returns rc=0 iff at least one path matches, without
  # leaking shopt state back to a re-sourcing caller.
  if compgen -G "${HOME}/.claude/plugins/marketplaces/*/plugins/*/agents/${SUBAGENT_TYPE}.md" > /dev/null; then
    exit 0
  fi
fi

ARGS=(--query "$TASK_PROMPT" --session "$SESSION_ID" --project "$PROJECT_DIR")
[[ -n "$TEAM_NAME" ]] && ARGS+=(--team "$TEAM_NAME")

"${HOME}/.claude/bin/ways" scan task "${ARGS[@]}"

# Never block Task creation
exit 0
