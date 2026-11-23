#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Dynamic context for the attend way: this session's attend state, read
# through the attend CLI (never attend's cache). `attend listen status`
# resolves the session from the process that runs it, the Claude Code
# process this hook runs under. A subagent runs in its parent's process, so
# for a subagent (WAYS_SCOPE=subagent at dispatch, or CLAUDE_AGENT_ID set on
# its own tool call) the state is the parent's: it is reported as
# information, with no action, since listening and sending are the head
# agent's (#932). Any failure prints nothing rather than a guess.

if ! command -v attend &>/dev/null; then
  # A ways from before ADR-511 has no --commit and updates with the bare form.
  UPDATE="ways manage update"
  ways manage update --help 2>/dev/null | grep -q -- '--commit' && UPDATE="ways manage update --commit"
  echo "**Note**: attend is not installed. Run \`$UPDATE\` to install it."
  exit 0
fi

# One scalar field of a one-line JSON document: true, false, null, or a
# string with its quotes.
field() { grep -o "\"$2\":[^,}]*" <<<"$1" | head -1 | cut -d: -f2-; }

LISTEN=$(attend listen status --json 2>/dev/null) || exit 0
[[ $(field "$LISTEN" resolved) == true ]] || exit 0
LISTENING=$(field "$LISTEN" listening)
# Only the attend mod delivers anything: a session listening
# without it, from a shell's `attend listen on` or a join, gets nothing.
MOD_HOST=$([[ $LISTENING == true && $(field "$LISTEN" host) == '"mod"' ]] && echo true)

if [[ ${WAYS_SCOPE:-} == subagent || -n ${CLAUDE_AGENT_ID:-} ]]; then
  WHO="the parent session"
  if [[ $MOD_HOST == true ]]; then
    echo "**Status**: the parent session is listening through the attend mod."
  elif [[ $LISTENING == true ]]; then
    echo "**Status**: the parent session listens without the attend mod, so nothing is delivered to it."
  else
    echo "**Status**: the parent session is not listening. Listening is the head agent's call."
  fi
else
  WHO="this session"
  # Whether the attend mod may be loaded here. Claude Code enables the
  # plugin in its settings; whether it loaded is in the tool list, which a
  # hook cannot see, so the wording leaves that check to the agent.
  if grep -qx 'attend@agent-ways' <<<"${WAYS_ENABLED_PLUGINS:-}"; then
    START="call \`attend_listen\` when that tool is listed; without it the attend mod is not loaded, nothing reaches this session, and the attend CLI works by hand"
  else
    START="note that the attend mod is not enabled here, so nothing reaches this session and the attend CLI works by hand; \`ways manage mod install attend --commit\` installs the mod for the next session"
  fi
  if [[ $MOD_HOST == true ]]; then
    echo "**Status**: listening through the attend mod. Send with \`attend_send\` and \`attend_reply\`."
  elif [[ $LISTENING == true ]]; then
    echo "**Status**: listening without the attend mod, so nothing is delivered to this session. For delivery, $START."
  else
    echo "**Status**: not listening. If the user asks to use attend, $START."
  fi
fi

# Joined channels (read-only: no state cleanup on a way fire). A session the
# mod listens for also hears its project and #open.
JOINED=$(attend channel --joined --json 2>/dev/null | grep -o '"name":"[^"]*"' | cut -d'"' -f4 | sed 's/^/#/' | paste -sd, - | sed 's/,/, /g')
if [[ $MOD_HOST == true ]]; then
  echo "**Channels**: $WHO hears project, #open${JOINED:+, $JOINED}"
elif [[ -n $JOINED ]]; then
  echo "**Channels**: $WHO joined $JOINED; nothing is delivered until the attend mod listens for it"
fi

# Messages that wait while nothing delivers them.
if [[ $MOD_HOST != true ]]; then
  UNREAD=$(field "$(attend message unread --json 2>/dev/null)" unread)
  [[ $UNREAD =~ ^[1-9][0-9]*$ ]] && echo "**Unread**: $UNREAD for $WHO"
fi
exit 0
