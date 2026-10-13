#!/usr/bin/env bash
# ADR-172: drain pending attend messages at the turn boundary.
#
# Dumb invoker by design — every decision (identity resolution, enrollment, the
# seen-set, cold-start baselining, the re-entry ceiling, JSON output)
# lives in `attend message inbox --drain`; the hook never reads attend-owned
# state (ADR-124/136). The harness's Stop payload on stdin flows through to
# the verb, which reads `stop_hook_active` from it. No attend installed → no-op,
# turn ends normally.
#
# Any exit other than 0 from attend also ends the turn normally (ADR-413
# item 6). Claude Code reads a Stop hook's exit 2 as a block, and clap exits 2
# on a usage error, so an attend that lacks this command must not pass it
# through.
command -v attend >/dev/null 2>&1 || exit 0
out=$(attend message inbox --drain --format hook) || exit 0
[[ -n $out ]] && printf '%s\n' "$out"
exit 0
