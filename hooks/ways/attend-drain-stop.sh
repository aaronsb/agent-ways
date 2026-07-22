#!/usr/bin/env bash
# ADR-172: drain pending attend messages at the turn boundary.
#
# Dumb invoker by design — every decision (identity resolution, the
# seen-set, cold-start baselining, the re-entry ceiling, JSON output)
# lives in `attend inbox --drain`; the hook never reads attend-owned
# state (CLI-is-contract, ADR-124/136). The harness's Stop payload on
# stdin flows through to the verb, which reads `stop_hook_active` from
# it. No attend installed → no-op, turn ends normally.
command -v attend >/dev/null 2>&1 || exit 0
exec attend inbox --drain --format hook
