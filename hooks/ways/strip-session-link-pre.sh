#!/usr/bin/env bash
# PreToolUse (Bash): strip the Claude-Session transcript link from commit / PR /
# issue authoring commands before they run. See ADR-162.
#
# The session link resolves to the FULL session transcript; publishing it in a
# commit trailer or PR/issue body is a thin wall in front of accidental secret
# disclosure. Claude Code ships no mechanical strip — the `attribution` setting
# does not reliably reach the model (upstream #18253) and the system prompt keeps
# instructing the link — so enforcement lives here, at user scope, for every repo.
#
# Mechanism: rewrite the command in place via PreToolUse `updatedInput` and allow
# the cleaned command to proceed. The commit flow is never blocked; the link just
# never lands. Posture is FAIL-OPEN — on any internal error the original command
# proceeds unchanged, so a broken hook can never block a legitimate commit.

INPUT=$(cat)
CMD=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null)

# No command, or not a command that publishes to git/GitHub → nothing to do.
[[ -z "$CMD" ]] && exit 0
case "$CMD" in
  *"git commit"*|*"gh pr create"*|*"gh pr edit"*|*"gh pr comment"* \
  |*"gh issue create"*|*"gh issue edit"*|*"gh issue comment"*) ;;
  *) exit 0 ;;
esac

# Fast path: no session link present.
printf '%s' "$CMD" | grep -qiE 'claude\.ai/code/session_|Claude-Session:' || exit 0

# Strip, line-wise where possible, then any residual inline token.
CLEANED=$(printf '%s' "$CMD" | perl -0pe '
  # A Claude-Session: trailer line (drop the leading newline with it).
  s/\n[ \t]*Claude-Session:[ \t]*https?:\/\/claude\.ai\/code\/session_\S*[ \t]*(?=\n|$)//gi;
  # A line that is only a bare session URL.
  s/\n[ \t]*https?:\/\/claude\.ai\/code\/session_\S*[ \t]*(?=\n|$)//gi;
  # Any residual inline session URL token.
  s/https?:\/\/claude\.ai\/code\/session_\S+//gi;
' 2>/dev/null)

# Transform failed or was a no-op → allow the original command unchanged.
[[ -z "$CLEANED" || "$CLEANED" == "$CMD" ]] && exit 0

# Rewrite the command, preserving the rest of tool_input (e.g. description).
ORIG_INPUT=$(printf '%s' "$INPUT" | jq -c '.tool_input // {}' 2>/dev/null)
[[ -z "$ORIG_INPUT" ]] && exit 0

jq -cn --argjson orig "$ORIG_INPUT" --arg cmd "$CLEANED" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "allow",
    updatedInput: ($orig + { command: $cmd })
  }
}' 2>/dev/null || exit 0
