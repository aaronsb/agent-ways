#!/usr/bin/env bash
# PostToolUse / PostToolUseFailure: reactive firing via postcheck.sh
# (ADR-123 Decision 5).
#
# Walks hooks/ways/**/postcheck.sh. For each, pipes the PostToolUse
# input on stdin and treats exit 0 as "this way requests firing for
# the observed post-state." Matching ways then flow through the same
# engine gate that predictive firing uses (`ways show way`), so
# reactive requests don't spam-fire during refractory.
#
# Output format: JSON with hookSpecificOutput.additionalContext for
# Claude Code to inject the combined way content into the session.

source "$(dirname "$0")/require-ways.sh"
source "$(dirname "$0")/sessions-root.sh"

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty')
[[ -n "$AGENT_ID" ]] && export CLAUDE_AGENT_ID="$AGENT_ID"
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(echo "$INPUT" | jq -r '.cwd // empty')}"
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')

[[ -z "$SESSION_ID" ]] && exit 0

export CLAUDE_PROJECT_DIR="${PROJECT_DIR}"

# Resolution order for way postchecks: project-local first, then global.
# Project-local is kept symmetric with predictive hooks; trust is
# enforced at the `ways show way` gate (which honors
# trusted-project-macros for project-local macros) rather than here.
# Precedence project > user > core (ADR-143). User root is the operator's own
# ways in $XDG_CONFIG; core is the shipped tree (projected at ~/.claude/hooks/ways).
WAYS_ROOTS=()
if [[ -d "${PROJECT_DIR}/.claude/ways" ]]; then
  WAYS_ROOTS+=("${PROJECT_DIR}/.claude/ways")
fi
_USER_WAYS="${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/ways"
if [[ -d "${_USER_WAYS}" ]]; then
  WAYS_ROOTS+=("${_USER_WAYS}")
fi
WAYS_ROOTS+=("${HOME}/.claude/hooks/ways")

CONTEXT=""
# Track way IDs already fired this tick so two postchecks under the
# same way directory don't double-fire.
declare -A FIRED=()

for WAYS_ROOT in "${WAYS_ROOTS[@]}"; do
  [[ -d "$WAYS_ROOT" ]] || continue

  while IFS= read -r postcheck; do
    [[ -x "$postcheck" ]] || continue

    # Derive the way id from the postcheck's parent directory, relative
    # to WAYS_ROOT. Example:
    #   .../hooks/ways/softwaredev/code/quality/postcheck.sh
    #   -> softwaredev/code/quality
    way_dir="$(dirname "$postcheck")"
    way_id="${way_dir#"$WAYS_ROOT"/}"

    [[ -n "${FIRED[$way_id]:-}" ]] && continue

    # Run the postcheck with the full PostToolUse input on stdin. Exit
    # 0 = "please fire"; anything else = "no match, move on." stderr
    # is swallowed to keep the hook output clean.
    if printf '%s' "$INPUT" | "$postcheck" >/dev/null 2>&1; then
      # Let the engine decide whether refractory permits firing.
      OUT=$("${HOME}/.claude/bin/ways" show way "$way_id" \
        --session "$SESSION_ID" \
        --trigger "postcheck" 2>/dev/null)
      if [[ -n "$OUT" ]]; then
        CONTEXT+="$OUT"$'\n\n'
        FIRED[$way_id]=1
      fi
    fi
  done < <(find "$WAYS_ROOT" -type f -name "postcheck.sh" 2>/dev/null)
done

# Suppress output if nothing fired or content is whitespace-only.
[[ -z "$CONTEXT" ]] && exit 0
TRIMMED="${CONTEXT%$'\n\n'}"
[[ -z "${TRIMMED// /}" ]] && exit 0

jq -n --arg ctx "$TRIMMED" --arg evt "PostToolUse" '{
  hookSpecificOutput: {
    hookEventName: $evt,
    additionalContext: $ctx
  }
}'
