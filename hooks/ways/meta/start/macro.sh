#!/usr/bin/env bash
# Dynamic context for the start way: whisper the current context-window gauge so
# /start can immediately judge whether this really is a beginning-of-session moment.
# Best-effort and fast — if the gauge can't be read, emit nothing.

command -v ways >/dev/null 2>&1 || exit 0

gauge=$(ways context 2>/dev/null) || exit 0
[ -z "$gauge" ] && exit 0

echo "## Context gauge at start"
echo ""
echo '```'
echo "$gauge"
echo '```'
echo ""
echo "Starting is a beginning-of-session act. If the window is already half or more spent, dissuade a fresh start — prefer \`/wrap\` or \`/clear\` + a new window (see the start skill, Step 0)."
echo ""
