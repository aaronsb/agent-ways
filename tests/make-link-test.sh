#!/usr/bin/env bash
# `make link` must not replace a real file at ~/.claude/bin/way-embed: that is
# a pre-1.0 in-place clone's own file, and nothing under ~/.claude moves
# without `ways reconcile --force` (the same real-path rule reconcile uses).
# MAKEFILE overrides the Makefile under test.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
MAKEFILE="${MAKEFILE:-$ROOT/Makefile}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}

APP="$WORK/app"
mkdir -p "$APP/bin" "$WORK/home/.claude/bin"
printf 'app way-embed\n' > "$APP/bin/way-embed"
printf 'app ways\n' > "$APP/bin/ways"
export HOME="$WORK/home" XDG_BIN_HOME="$WORK/home/.local/bin"

# A real file at the destination survives, with a warning.
printf 'clone way-embed\n' > "$HOME/.claude/bin/way-embed"
out=$(make -s -f "$MAKEFILE" -C "$APP" link 2>&1)
check "make link keeps a real way-embed" "clone way-embed" "$(cat "$HOME/.claude/bin/way-embed")"
check "make link keeps it a regular file" "file" "$([[ -L $HOME/.claude/bin/way-embed ]] && echo link || echo file)"
check "make link says so" "yes" "$(grep -q 'not replacing' <<< "$out" && echo yes || echo no)"
check "make link still links the suite" "$APP/bin/ways" "$(readlink "$XDG_BIN_HOME/ways")"

# Control: with nothing there, the link is made.
rm -f "$HOME/.claude/bin/way-embed"
make -s -f "$MAKEFILE" -C "$APP" link >/dev/null 2>&1
check "make link creates the way-embed link when absent" "$APP/bin/way-embed" "$(readlink "$HOME/.claude/bin/way-embed")"

# Re-running over our own symlink is a no-op.
make -s -f "$MAKEFILE" -C "$APP" link >/dev/null 2>&1
check "make link is idempotent" "$APP/bin/way-embed" "$(readlink "$HOME/.claude/bin/way-embed")"

exit $fail
