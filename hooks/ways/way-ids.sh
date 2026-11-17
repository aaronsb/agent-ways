# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Sourced by the core macro and the ways catalog macro. way_ids prints the
# id of every way under a ways root, one per line, sorted: the directory of
# each non-check .md file that opens with a frontmatter fence, the rule the
# engine's scanner uses (ways-core scan_declared_ways). A directory with no
# way file of its own, or a .md with no frontmatter, is not a way.
#
# The root is entered with cd and paths are taken relative to it, so no part
# of the root's path reaches a regex.

way_ids() {  # way_ids <ways root>
  ( cd "$1" 2>/dev/null || exit 0
    find -L . -mindepth 2 -name "*.md" ! -name "*.check.md" -type f \
      -exec awk 'FNR == 1 { if ($0 == "---" || $0 == "---\r") print FILENAME; nextfile }' {} + \
      | sed 's|^\./||; s|/[^/]*$||' \
      | sort -u )
}
