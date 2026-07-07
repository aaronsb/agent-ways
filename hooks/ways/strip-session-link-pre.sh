#!/usr/bin/env bash
# PreToolUse (Bash): deny commit / PR / issue authoring commands that carry the
# Claude-Session transcript link, instructing a clean re-issue. See ADR-162.
#
# The session link resolves to the FULL session transcript; publishing it in a
# commit trailer or PR/issue body is a thin wall in front of accidental secret
# disclosure. Claude Code ships no mechanical strip — the `attribution` setting
# does not reliably reach the model (upstream #18253) and the system prompt keeps
# instructing the link — so enforcement lives here, at user scope, for every repo.
#
# Mechanism: DENY, not rewrite. Rewriting in place (PreToolUse `updatedInput`) is
# honored only when the hook also forces `permissionDecision: allow`, which would
# bypass the operator's own Deny/Allow/Ask rules on outward-facing gh create/edit
# commands (no pass-through option — upstream FR #381). Deny bypasses nothing: the
# model re-issues the command WITHOUT the link, and that clean command goes through
# normal permission evaluation. Cost is one retry, not a lost commit.
#
# Fail posture: the no-op paths (no command / out of scope / no link) allow. Once a
# link IS detected the hook fails CLOSED — on any internal error it still blocks
# (exit 2) rather than let the link through.

INPUT=$(cat)
CMD=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null)

# No command, or not a command that publishes to git/GitHub → nothing to enforce.
[[ -z "$CMD" ]] && exit 0
case "$CMD" in
  *"git commit"*|*"gh pr create"*|*"gh pr edit"*|*"gh pr comment"* \
  |*"gh issue create"*|*"gh issue edit"*|*"gh issue comment"*) ;;
  *) exit 0 ;;
esac

# No session link present → allow, normal permission flow applies.
printf '%s' "$CMD" | grep -qiE 'claude\.ai/code/session_|Claude-Session:' || exit 0

REASON='Blocked (ADR-162): this command publishes a Claude-Session transcript link — a "Claude-Session:" trailer or a claude.ai/code/session_ URL — to git or GitHub. That link resolves to the full session transcript and must not be published. Re-run the command with the trailer/URL removed from the commit message or PR/issue body.'

# Deny via structured output; if jq fails, fall back to exit 2 + stderr (also a
# block). Either way the link does not land.
jq -cn --arg reason "$REASON" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: $reason
  }
}' 2>/dev/null || { printf '%s\n' "$REASON" >&2; exit 2; }
