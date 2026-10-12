#!/usr/bin/env bash
# Dynamic context for the attend way: this session's attend state, read
# through the attend CLI (never attend's cache). `attend listen status`
# resolves the session from the process that runs it, the Claude Code
# process this hook runs under. A subagent runs in its parent's process, so
# under WAYS_SCOPE=subagent the state is the parent's: it is reported as
# information, with no action, since listening and sending are the head
# agent's (#932). Any failure prints nothing rather than a guess.

if ! command -v attend &>/dev/null; then
  echo "**Note**: attend is not installed. Run \`ways manage update\` to install it."
  exit 0
fi

# One scalar field of a one-line JSON document: true, false, null, or a
# string with its quotes.
field() { grep -o "\"$2\":[^,}]*" <<<"$1" | head -1 | cut -d: -f2-; }

LISTEN=$(attend listen status --json 2>/dev/null) || exit 0
[[ $(field "$LISTEN" resolved) == true ]] || exit 0
LISTENING=$(field "$LISTEN" listening)
MOD_HOST=$([[ $LISTENING == true && $(field "$LISTEN" host) == '"mod"' ]] && echo true)
RUN_LIVE=$(field "$LISTEN" run_live)

if [[ ${WAYS_SCOPE:-} == subagent ]]; then
  WHO="the parent session"
  if [[ $RUN_LIVE == true && $MOD_HOST == true ]]; then
    echo "**Status**: the parent session is listening through the attend mod, and its \`attend run\` is also live under Monitor."
  elif [[ $RUN_LIVE == true ]]; then
    echo "**Status**: the parent session's \`attend run\` is live under Monitor."
  elif [[ $MOD_HOST == true ]]; then
    echo "**Status**: the parent session is listening through the attend mod."
  elif [[ $LISTENING == true ]]; then
    echo "**Status**: the parent session is listening at the end of each turn only."
  else
    echo "**Status**: the parent session is not listening. Listening is the head agent's call."
  fi
else
  WHO="this session"
  # Whether the attend mod may be loaded here. Claude Code enables the
  # plugin in its settings; whether it loaded is in the tool list, which a
  # hook cannot see, so the wording leaves that check to the agent.
  if grep -qx 'attend@agent-ways' <<<"${WAYS_ENABLED_PLUGINS:-}"; then
    START="call \`attend_listen\` when that tool is listed; otherwise start \`/attend\` or \`Monitor: attend run\`"
  else
    START="start \`/attend\` or \`Monitor: attend run\`"
  fi
  if [[ $RUN_LIVE == true && $MOD_HOST == true ]]; then
    echo "**Status**: listening through the attend mod, and \`attend run\` is also live under Monitor. ADR-406 item 9 says not to run both: the mod is enough, so the Monitor can stop."
  elif [[ $RUN_LIVE == true ]]; then
    echo "**Status**: \`attend run\` is live under Monitor for this session."
  elif [[ $MOD_HOST == true ]]; then
    echo "**Status**: listening through the attend mod. Send with \`attend_send\` and \`attend_reply\`; do not start Monitor."
  elif [[ $LISTENING == true ]]; then
    echo "**Status**: listening at the end of each turn only; nothing wakes an idle session. For that, $START."
  else
    echo "**Status**: not listening. If the user asks to use attend, $START."
  fi
fi

# Joined channels (read-only: no state cleanup on a way fire). A listening
# session also hears its project and #open.
JOINED=$(attend channels --joined --json 2>/dev/null | grep -o '"name":"[^"]*"' | cut -d'"' -f4 | sed 's/^/#/' | paste -sd, - | sed 's/,/, /g')
if [[ $LISTENING == true ]]; then
  echo "**Channels**: $WHO hears project, #open${JOINED:+, $JOINED}"
elif [[ -n $JOINED ]]; then
  echo "**Channels**: $WHO joined $JOINED; nothing is delivered until it listens"
fi

# Messages that wait while the session does not listen.
if [[ $LISTENING != true ]]; then
  UNREAD=$(field "$(attend unread --json 2>/dev/null)" unread)
  [[ $UNREAD =~ ^[1-9][0-9]*$ ]] && echo "**Unread**: $UNREAD for $WHO"
fi
exit 0
