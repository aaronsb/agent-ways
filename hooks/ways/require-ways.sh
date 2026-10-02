#!/usr/bin/env bash
# Sourced by every hook adapter under hooks/ways. A hook script is a transport
# adapter (ADR-504 §11): `ways hook <event>` reads Claude Code's payload on
# stdin, decides, and prints what the hook returns.
#
# Usage: source "$(dirname "$0")/require-ways.sh"; ways_hook <event>
#
# If the binary is missing, the hook exits 0 with no output. The SessionStart
# check-setup.sh hook handles the user-facing diagnostic.

WAYS_BIN="${HOME}/.claude/bin/ways"
if [[ ! -x "$WAYS_BIN" ]]; then
  exit 0
fi

# Avoid racing with foreground git commits. `git status` / `describe --dirty`
# and similar read-ish operations normally take .git/index.lock to refresh the
# stat cache; GIT_OPTIONAL_LOCKS=0 tells git to skip that lock. Hooks run
# opportunistically alongside user git activity, so optional locks are always
# safe here — we never rely on the cache being rewritten.
export GIT_OPTIONAL_LOCKS=0

# Run the event and end the hook. These hooks guide and never block, so the
# hook exits 0 whatever the binary returns: exit 2 would read as "block the
# prompt" or "block the tool".
ways_hook() {
  "$WAYS_BIN" hook "$1"
  exit 0
}
