#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Ways catalog macro: the id of every way in the shipped corpus, one line per
# domain. The session-start list names top-level ways only (#634).

HERE="${BASH_SOURCE[0]%/*}"
source "${HERE}/../../../way-ids.sh"

echo "## Ways Catalog"
echo ""
way_ids "${HERE}/../../.." | awk -F/ '
  NF == 1 { w[$1] = w[$1]; root[$1] = 1; next }
  { d = $1; sub(/^[^\/]*\//, ""); w[d] = (w[d] == "" ? "" : w[d] ", ") $0 }
  END { cmd = "sort"
        for (d in w) print "- **" d "**" (d in root ? " (also a way)" : "") ": " w[d] | cmd
        close(cmd) }'
