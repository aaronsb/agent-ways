#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Reactive firing for softwaredev/code/quality/versioning.
#
# Exits 0 (requesting fire) when the just-completed Edit/Write
# introduced a version-numbered identifier or version-reference
# docstring/comment in the *new* content — process_v2, _FOO_V0,
# `"""v0 seed ..."""`, `# v1 of this`. These are the moment the
# versioning way's guidance is load-bearing: a versioned twin is
# about to become permanent.
#
# Scoped to tool_input.new_string / tool_input.content (the diff
# being written), NOT the whole file — pre-existing version tokens
# the agent didn't author must not re-fire on unrelated edits.
#
# Receives the full PostToolUse input JSON on stdin. Exit 0 = "please
# fire"; non-zero = "no match, skip me." The engine's `ways show way`
# gate still applies refractory, so this won't spam-fire.

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')

case "$TOOL_NAME" in
  Edit|Write) ;;
  *) exit 1 ;;
esac

FP=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
[[ -z "$FP" ]] && exit 1

# Code only. Docs/specs/locks legitimately carry version strings, and
# the way files themselves quote the antipattern as examples.
case "$FP" in
  *.md|*.mdx|*.lock|*.jsonl|*.txt|*.csv) exit 1 ;;
  */node_modules/*|*/dist/*|*/build/*|*/vendor/*|*/__pycache__/*) exit 1 ;;
esac

# The diff being written — Edit carries new_string, Write carries content.
NEW=$(echo "$INPUT" | jq -r '.tool_input.new_string // .tool_input.content // empty')
[[ -z "$NEW" ]] && exit 1

# Discriminator: versioned identifiers the code OWNS, plus version-
# reference comments/docstrings. Deliberately does NOT match external
# version refs (/v1/ paths, apiVersion, __version__, schema_version)
# because those lack the _V<n> identifier shape or the digit-after-_v.
RE='(\b[A-Z][A-Z0-9]*_V[0-9]+\b|\b[a-zA-Z][a-zA-Z0-9]*_v[0-9]+\b|\b[A-Za-z]+V[0-9]+\b(?!ersion)|"""v[0-9]+ |# v[0-9]+ |\bv[0-9]+ (seed|impl|implementation|version of|rewrite|attempt)\b)'

if printf '%s' "$NEW" | grep -qP "$RE" 2>/dev/null; then
  exit 0
fi
exit 1
