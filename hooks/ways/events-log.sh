#!/usr/bin/env bash
# Canonical telemetry events-log path — shared by every hook that appends events.
#
# MUST resolve to the same file as ways-core `paths::events_log()`, which the
# Rust writer (`session::log_event`) and every reader (`firing::load_events`)
# use: `$XDG_STATE_HOME/agent-ways/events.jsonl`. Ask the binary via `ways
# events-log-path` — the same delegation pattern as `ways response-topics-path` —
# so the path cannot drift from the Rust side.
#
# If the binary is unavailable, use the same XDG default so telemetry still lands
# where the readers look.
#
# Usage: source this file, then use $EVENTS_LOG
#   source "$(dirname "$0")/events-log.sh"

_ways_bin="${HOME}/.claude/bin/ways"
EVENTS_LOG=""
if [[ -x "$_ways_bin" ]]; then
  EVENTS_LOG=$("$_ways_bin" events-log-path 2>/dev/null)
fi
if [[ -z "$EVENTS_LOG" ]]; then
  # An empty or relative XDG_STATE_HOME counts as unset (XDG spec; same as paths.rs).
  case "${XDG_STATE_HOME:-}" in
    /*) _state="$XDG_STATE_HOME" ;;
    *)  _state="${HOME}/.local/state" ;;
  esac
  EVENTS_LOG="${_state}/agent-ways/events.jsonl"
  unset _state
fi
unset _ways_bin
