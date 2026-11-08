#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Postcheck for softwaredev/code/overbuild (ADR-135, Tier 2).
#
# Reads the PostToolUse input on stdin. Exit 0 = "freshly written code matches a
# seed over-build pattern — fire the way." Exit 1 = no match, which is the
# default and the correct outcome on anything unfamiliar (ADR-135: silence on
# the unrecognized beats a confident-wrong flag).
#
# The seed set is deliberately tiny and high-precision. ADR-135 makes
# zero-false-positive a HARD constraint: missing an over-build is acceptable,
# firing on legitimate code is not. Grow the set only through the authoring loop
# (#7) from encounter telemetry — never by speculative addition.
set -euo pipefail

INPUT=$(cat)

# Tool name + target path in one jq pass (hot path: this runs on every
# Edit/Write/Bash/Task PostToolUse — keep forks minimal).
{ read -r tool; read -r path; } < <(printf '%s' "$INPUT" | jq -r '.tool_name // "", (.tool_input.file_path // "")')

# Only code writes. Ignore Bash/Task and any non-write tool.
case "$tool" in Write | Edit | MultiEdit) ;; *) exit 1 ;; esac

# Code files only — mirror code.check's allowlist so we never fire on docs,
# config, or a markdown file that merely contains an example.
[[ "$path" =~ \.(py|ts|tsx|js|jsx|mjs|vue|svelte|go|rs|rb|java|kt|scala|c|cc|cpp|h|hpp|cs|php|swift|sh|lua|ex|exs)$ ]] || exit 1

# Don't fire on the over-build way's own files: the detectors necessarily
# contain the pattern literals, and so do their tests. (Residual FP on
# pattern-literal code elsewhere — a linter, a teaching example — is the
# intrinsic limit of text matching, covered by the way's advisory "keep it if
# deliberate" design rather than a path guard.)
[[ "$path" == */overbuild/* ]] && exit 1

# The text actually being written: Write.content, Edit.new_string, or each
# MultiEdit edit's new_string. (`|| true`: malformed input -> empty -> silent exit.)
content=$(printf '%s' "$INPUT" \
  | jq -r '[.tool_input.content, .tool_input.new_string, (.tool_input.edits[]?.new_string)]
           | map(select(. != null)) | join("\n")' 2>/dev/null || true)
[[ -n "$content" ]] || exit 1

# 1. Hand-rolled LRU/TTL cache. `popitem(last=False` on an OrderedDict is the
#    FIFO-eviction tell (plain dict.popitem takes no args), and pairing it with
#    OrderedDict keeps it off legitimate ordered-dict reordering.
if grep -qE 'OrderedDict' <<<"$content" && grep -qE 'popitem\(\s*last\s*=\s*False' <<<"$content"; then
  exit 0
fi

# 2. Hand-rolled singleton. Require the guard form `_instance is None` (not a
#    bare `_instance` substring) so immutable `__new__` subclasses and unrelated
#    fields named *_instance* don't trip it.
if grep -qE '__new__' <<<"$content" && grep -qE '_instance\s+is\s+None' <<<"$content"; then
  exit 0
fi

exit 1
