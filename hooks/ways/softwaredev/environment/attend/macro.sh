#!/usr/bin/env bash
# Dynamic context for the attend way: this session's attend state, read
# through the attend CLI (never attend's cache). `attend listen status`
# resolves the session from the process that runs it: the Claude Code
# process this hook runs under, a subagent's macro included (#932).
# Any failure prints nothing rather than a guess.

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
HOST=$(field "$LISTEN" host)
RUN_LIVE=$(field "$LISTEN" run_live)

# Whether the attend mod is in this host. The hook cannot see the session's
# tool list, so this reads the setting that installs the mod: true, false, or
# empty when `ways` cannot say.
MOD=$(ways settings get attend_mod.install 2>/dev/null)

if [[ $RUN_LIVE == true ]]; then
  echo "**Status**: \`attend run\` is live under Monitor for this session."
elif [[ $LISTENING == true && $HOST == '"mod"' ]]; then
  echo "**Status**: listening through the attend mod. Send with \`attend_send\` and \`attend_reply\`; do not start Monitor."
elif [[ $LISTENING == true ]]; then
  case $MOD in
    true) wake="call \`attend_listen\`" ;;
    false) wake="start \`/attend\` or \`Monitor: attend run\`" ;;
    *) wake="call \`attend_listen\` when that tool is listed, else start \`/attend\` or \`Monitor: attend run\`" ;;
  esac
  echo "**Status**: listening at the end of each turn only; nothing wakes an idle session. For that, $wake."
else
  case $MOD in
    true) echo "**Status**: dormant. If the user asks to use attend, call \`attend_listen\`; do not start Monitor." ;;
    false) echo "**Status**: not listening. Start with \`/attend\` or \`Monitor: attend run\`." ;;
    *) echo "**Status**: not listening. If the user asks to use attend, call \`attend_listen\` when that tool is listed, else start \`/attend\` or \`Monitor: attend run\`." ;;
  esac
fi

# Joined channels (read-only: no state cleanup on a way fire). A listening
# session also hears its project and #open.
JOINED=$(attend channels --joined --json 2>/dev/null | grep -o '"name":"[^"]*"' | cut -d'"' -f4 | sed 's/^/#/' | paste -sd, - | sed 's/,/, /g')
if [[ $LISTENING == true ]]; then
  echo "**Channels**: project, #open${JOINED:+, $JOINED}"
elif [[ -n $JOINED ]]; then
  echo "**Channels**: joined $JOINED; nothing is delivered until the session listens"
fi

# Messages that wait while the session does not listen.
if [[ $LISTENING != true ]]; then
  UNREAD=$(field "$(attend unread --json 2>/dev/null)" unread)
  [[ $UNREAD =~ ^[1-9][0-9]*$ ]] && echo "**Unread**: $UNREAD (\`attend unread\`)"
fi
exit 0
