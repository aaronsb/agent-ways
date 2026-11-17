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

# scan <ways root> <event> <session>: the state lane's additionalContext.
scan() {
    local home="$WORK/home-$3"
    if [[ ! -e "$home/.claude/hooks/ways" ]]; then
        mkdir -p "$home/.claude/hooks" "$home/run" "$WORK/project"
        ln -s "$1" "$home/.claude/hooks/ways"
    fi
    env -i PATH="$PATH" HOME="$home" \
        XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state" \
        XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$home/.local/share" \
        XDG_RUNTIME_DIR="$home/run" CLAUDE_PROJECT_DIR="$WORK/project" \
        "$BIN" scan state --session "$3" --hook-event "$2" --project "$WORK/project" 2>/dev/null \
        | jq -r '.hookSpecificOutput.additionalContext // empty' 2>/dev/null
}

has() {  # name needle haystack want(yes|no)
    local got=no
    [[ "$3" == *"$2"* ]] && got=yes
    if [[ "$got" == "$4" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1"; fail=1; fi
}

# ── The shipped corpus fits ───────────────────────────────────────

ctx=$(scan "$HOOKS" SessionStart shipped)
# Claude Code counts a JavaScript string length: UTF-16 code units.
len=$(printf '%s' "$ctx" | iconv -f UTF-8 -t UTF-16LE | wc -c)
len=$((len / 2))

has "core.md is in the SessionStart payload" "# Core Ways of Working" "$ctx" yes
has "the ways list is in the SessionStart payload" "## Available Ways" "$ctx" yes
if (( len > 0 && len <= CAP )); then
    echo "  PASS: SessionStart payload is $len of $CAP characters"
else
    echo "  FAIL: SessionStart payload is $len characters, cap $CAP"
    fail=1
fi

# ── A state way that does not fit waits for the next prompt ───────
# A corpus whose core leaves less room than its one session-start way
# needs. Core is admitted first and whole; the way is withheld and not
# stamped, so the UserPromptSubmit run of the same lane shows it, once.

SYN="$WORK/synthetic"
mkdir -p "$SYN/lab/late"
{ printf -- '---\nrefire: 0.15\n---\n# Core Ways of Working\n\n'
  for _ in $(seq 1 90); do printf 'Core filler line that takes up room in the budget, one hundred characters long, more or less ok.\n'; done
} > "$SYN/core.md"
{ printf -- '---\ndescription: a late state way\ntrigger: session-start\nscope: agent\nrefire: 0.15\n---\n# Late State Way\n\n'
  for _ in $(seq 1 20); do printf 'State way filler line, one hundred characters long, so the way cannot fit after the core text.\n'; done
} > "$SYN/lab/late/late.md"

first=$(scan "$SYN" SessionStart synthetic)
has "core is admitted first" "# Core Ways of Working" "$first" yes
has "the state way that does not fit is withheld" "# Late State Way" "$first" no
second=$(scan "$SYN" UserPromptSubmit synthetic)
has "the withheld way shows on the next prompt" "# Late State Way" "$second" yes
third=$(scan "$SYN" UserPromptSubmit synthetic)
has "once shown, it does not show again" "# Late State Way" "$third" no

exit $fail
