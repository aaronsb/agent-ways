#!/bin/bash
# Resolve enabled plugin way-paths and write session manifest.
# Delegates to `ways resolve-plugins` (single Rust implementation).
# ADR-129: Plugin Way Discovery

INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')

[[ -z "$SESSION_ID" ]] && exit 0

"${HOME}/.claude/bin/ways" resolve-plugins --session "$SESSION_ID"
