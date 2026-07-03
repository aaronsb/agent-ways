#!/usr/bin/env bash
# Canonical telemetry events-log path — shared by every hook that appends events.
#
# MUST resolve to the same file as ways-core `paths::events_log()`, which the
# Rust writer (`session::log_event`) and every reader (`firing::load_events`)
# use. That path has XDG-vs-legacy fallback logic (ADR-142/ADR-153) that is
# awkward to reproduce faithfully in bash, so instead we ask the binary via
# `ways events-log-path` — the same delegation pattern as `ways
# response-topics-path`. Hardcoding `~/.claude/stats/events.jsonl` here is what
# orphaned post-migration `session_start` lines from `rethink` (ADR-153 §1).
#
# If the binary is unavailable we fall back to the legacy path so telemetry still
# lands somewhere; the reader's union recovers it later.
#
# Usage: source this file, then use $EVENTS_LOG
#   source "$(dirname "$0")/events-log.sh"

_ways_bin="${HOME}/.claude/bin/ways"
EVENTS_LOG=""
if [[ -x "$_ways_bin" ]]; then
  EVENTS_LOG=$("$_ways_bin" events-log-path 2>/dev/null)
fi
EVENTS_LOG="${EVENTS_LOG:-${HOME}/.claude/stats/events.jsonl}"
unset _ways_bin
