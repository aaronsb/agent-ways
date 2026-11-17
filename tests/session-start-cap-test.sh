#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# The SessionStart payload of the shipped corpus fits Claude Code's
# 10,000-character hook additionalContext cap (#634). Over the cap, Claude
# Code saves the text to a file and shows the model a 2,000-character
# preview, so core.md never reaches the session. The scan runs against a
# temp HOME whose ~/.claude/hooks/ways is this checkout's corpus.
# WAYS_TEST_HOOKS points it at another corpus, WAYS_TEST_BIN at another
# binary: how the check is shown to fail before the fix.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOOKS="${WAYS_TEST_HOOKS:-$ROOT/hooks/ways}"
BIN="${WAYS_TEST_BIN:-$ROOT/tools/target/debug/ways}"
CAP=10000
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

if [[ ! -x "$BIN" ]]; then
    echo "  FAIL: no ways binary at $BIN (cargo build -p ways)"
    exit 1
fi

mkdir -p "$WORK/home/.claude/hooks" "$WORK/run" "$WORK/project"
ln -s "$HOOKS" "$WORK/home/.claude/hooks/ways"

env -i PATH="$PATH" HOME="$WORK/home" \
    XDG_CACHE_HOME="$WORK/home/.cache" XDG_STATE_HOME="$WORK/home/.local/state" \
    XDG_CONFIG_HOME="$WORK/home/.config" XDG_DATA_HOME="$WORK/home/.local/share" \
    XDG_RUNTIME_DIR="$WORK/run" CLAUDE_PROJECT_DIR="$WORK/project" \
    "$BIN" scan state --session cap-test --hook-event SessionStart --project "$WORK/project" \
    > "$WORK/out.json" 2>"$WORK/err.txt"

ctx=$(jq -r '.hookSpecificOutput.additionalContext // empty' "$WORK/out.json" 2>/dev/null)
# Claude Code counts a JavaScript string length: UTF-16 code units.
len=$(printf '%s' "$ctx" | iconv -f UTF-8 -t UTF-16LE | wc -c)
len=$((len / 2))

if [[ "$ctx" == *"# Core Ways of Working"* ]]; then
    echo "  PASS: core.md is in the SessionStart payload"
else
    echo "  FAIL: core.md missing from the SessionStart payload"
    fail=1
fi
if [[ "$ctx" == *"## Available Ways"* ]]; then
    echo "  PASS: the ways table is in the SessionStart payload"
else
    echo "  FAIL: the ways table is missing from the SessionStart payload"
    fail=1
fi
if (( len > 0 && len <= CAP )); then
    echo "  PASS: SessionStart payload is $len of $CAP characters"
else
    echo "  FAIL: SessionStart payload is $len characters, cap $CAP"
    fail=1
fi

exit $fail
