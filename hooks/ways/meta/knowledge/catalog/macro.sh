#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Ways catalog macro: every way path in the shipped corpus, one line per
# domain. The session-start table lists top-level ways only (#634).

WAYS_DIR="${HOME}/.claude/hooks/ways"

echo "## Ways Catalog"
echo ""
find -L "$WAYS_DIR" -mindepth 3 -name "*.md" ! -name "*.check.md" -type f 2>/dev/null \
  | sed "s|^${WAYS_DIR}/||; s|/[^/]*\.md\$||" \
  | sort -u \
  | awk -F/ '
      { d = $1; sub(/^[^\/]*\//, ""); w[d] = (w[d] == "" ? "" : w[d] ", ") $0 }
      END { cmd = "sort"; for (d in w) print "- **" d "**: " w[d] | cmd; close(cmd) }'
