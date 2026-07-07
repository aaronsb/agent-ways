#!/usr/bin/env bash
# PreToolUse (Bash): deny commit / PR / issue authoring commands that carry a
# Claude-Session transcript link in trailer/footer position. See ADR-162.
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
# commands (no pass-through — upstream FR #381). Deny bypasses nothing: the model
# re-issues WITHOUT the link and that clean command goes through normal permission
# evaluation. Cost is one retry, not a lost commit.
#
# Two-part gate, both must hold to deny:
#   (1) the command PUBLISHES to git/GitHub — matched on token presence, newline-
#       collapsed, so intervening flags (git -C <path> commit, gh <flags> pr
#       create) cannot evade the gate; and
#   (2) a session link sits in TRAILER/FOOTER position — a `Claude-Session:` line
#       or a bare session URL alone on its line. A URL mentioned INLINE in prose is
#       not a leak and passes, so this repo can discuss Claude-Session as subject
#       matter without a deny-loop.
#
# Fail posture: no-op paths (no command / not publishing / no trailer) allow. Once
# a trailer/footer link IS detected the hook fails CLOSED — on jq failure it still
# blocks (exit 2) rather than let the link through.

INPUT=$(cat)
CMD=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null)
[[ -z "$CMD" ]] && exit 0

# (1) Publishing command? (flag- and continuation-tolerant)
printf '%s' "$CMD" | tr '\n' ' ' \
  | grep -qiE '(\bgit\b.*\bcommit\b|\bgh\b.*\b(pr|issue)\b)' || exit 0

# (2) Session link in trailer/footer position? (anchored per line; inline mentions
#     fall through and are allowed)
# The URL must be line-LEADING (optionally behind the `Claude-Session:` key) — that
# is what makes it a footer/trailer rather than an inline mention. After the id we
# allow only non-alphanumeric terminators to end-of-line (closing "/'/)/} from the
# `-m "…"` and `--body "…"` forms), so `Claude-Session: <url>"` matches but
# `see <url> for context` does not.
printf '%s' "$CMD" \
  | grep -qiE '^[[:space:]]*(Claude-Session:[[:space:]]*)?https?://claude\.ai/code/session_[A-Za-z0-9_-]+[^A-Za-z0-9]*$' \
  || exit 0

REASON='Blocked (ADR-162): this command publishes a Claude-Session transcript link (a "Claude-Session:" trailer or a bare claude.ai/code/session_ URL on its own line) to git or GitHub. That link resolves to the full session transcript and must not be published. Remove the trailer/footer line from the commit message or PR/issue body and re-run. (A session URL mentioned inline in prose is allowed; only trailer/footer lines are blocked.)'

jq -cn --arg reason "$REASON" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: $reason
  }
}' 2>/dev/null || { printf '%s\n' "$REASON" >&2; exit 2; }
