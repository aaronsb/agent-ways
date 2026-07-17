#!/usr/bin/env bash
# PreToolUse (Bash): deny commit / PR / issue authoring commands that carry a
# Claude-Session transcript link in trailer/footer position. See ADR-167
# (supersedes ADR-162).
#
# The session link resolves to the FULL session transcript; publishing it in a
# commit trailer or PR/issue body is a thin wall in front of accidental secret
# disclosure.
#
# This hook is a BACKSTOP, not the primary control. `attribution.sessionUrl: false`
# suppresses the link at the source — the model is never instructed to emit it, so
# when that works this hook never fires. The key shipped in v2.1.183 and is
# projected to ~/.claude/settings.json from the settings fragment store (ADR-147 /
# ADR-163). A controlled experiment on 2026-07-16 (same repo, same v2.1.212, only
# the key differing) confirms it governs the trailer.
#
# It is retained because that setting is undocumented (upstream #69614), defaults
# to ON, and is scope-qualified upstream ("web and Remote Control sessions") — a
# host that never receives the fragment leaks by default. ADR-162 previously
# claimed no setting could govern the link at all, citing #18253; that was the
# Co-Authored-By/footer bug, not the session link. See ADR-167 for the correction.
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
