#!/usr/bin/env bash
# Dynamic context for attend way
# Checks if attend is installed and running, emits live state

# Not installed — nothing to add
if ! command -v attend &>/dev/null; then
  echo "**Note**: attend is not installed. Run \`ways update\` to install it."
  exit 0
fi

# Check if attend is running for this session: an `attend run` process whose
# ancestry reaches the claude process that runs this hook. Another session's
# attend doesn't count. The claude pid comes from the session record that
# carries CLAUDE_SESSION_ID (set by `ways` for every macro); without one, the
# nearest ancestor named claude stands in.
session_pid() {
  local dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/sessions" f
  [[ -n "${CLAUDE_SESSION_ID:-}" ]] || return 0
  f=$(grep -l "\"sessionId\":\"$CLAUDE_SESSION_ID\"" "$dir"/*.json 2>/dev/null | head -1)
  [[ -n "$f" ]] && basename "$f" .json
}
attend_running() {
  ps -A -o pid=,ppid=,args= 2>/dev/null | awk -v claude="$(session_pid)" -v self="$$" '
    function base(w) { sub(/.*\//, "", w); return w }
    {
      parent[$1] = $2
      if (base($3) == "attend" && $4 == "run") attend[$1] = 1
      if (base($3) == "claude") named[$1] = 1
    }
    END {
      if (claude == "")
        for (p = self; p != "" && p != 0 && hops++ < 15; p = parent[p])
          if (p in named) { claude = p; break }
      if (claude == "") exit 1
      for (a in attend)
        for (p = a; p != "" && p != 0 && n[a]++ < 15; p = parent[p])
          if (p == claude) exit 0
      exit 1
    }'
}
if attend_running; then
  echo "**Status**: attend is running"
else
  echo "**Status**: attend is not running — start with \`/attend\` or \`Monitor: attend run\`"
fi

# Show this session's channels (read-only view: no state cleanup on a way fire)
CHANNELS_OUTPUT=$(attend channels --joined 2>/dev/null)
if [[ -n "$CHANNELS_OUTPUT" ]]; then
  echo "**Channels**:"
  echo "$CHANNELS_OUTPUT"
fi

# Show peer count
PEER_OUTPUT=$(attend peers 2>&1 | grep "baseline" | sed 's/\[attend\] peers: //')
if [[ -n "$PEER_OUTPUT" ]]; then
  echo "**Peers**: $PEER_OUTPUT"
fi

# Show pending signals
STATUS_OUTPUT=$(attend status 2>/dev/null)
PROJECT_SIGNALS=$(echo "$STATUS_OUTPUT" | grep "project:" | sed 's/.*project: *//')
BROADCAST_SIGNALS=$(echo "$STATUS_OUTPUT" | grep "broadcast:" | sed 's/.*broadcast: *//')
if [[ -n "$PROJECT_SIGNALS" ]] || [[ -n "$BROADCAST_SIGNALS" ]]; then
  echo "**Signals**: project: $PROJECT_SIGNALS | broadcast: $BROADCAST_SIGNALS"
fi
