#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Dynamic git branch context for branching way
# Output: terse one-line summary of current git state
# MUST complete in under 1 second — never block on failure

# Not a git repo? Say so and exit.
git rev-parse --is-inside-work-tree &>/dev/null || {
  echo "Git: not a repository"
  exit 0
}

# Current branch (or detached HEAD)
BRANCH=$(git branch --show-current 2>/dev/null)
if [[ -z "$BRANCH" ]]; then
  BRANCH="detached:$(git rev-parse --short HEAD 2>/dev/null)"
fi

# Clean or dirty
if [[ -z "$(git status --porcelain 2>/dev/null)" ]]; then
  STATE="clean"
else
  STATE="dirty"
fi

# Ahead/behind upstream (may not have upstream set)
AHEAD_BEHIND=""
if COUNTS=$(git rev-list --left-right --count HEAD...@{upstream} 2>/dev/null); then
  AHEAD=$(echo "$COUNTS" | cut -f1)
  BEHIND=$(echo "$COUNTS" | cut -f2)
  if [[ "$AHEAD" -gt 0 ]] || [[ "$BEHIND" -gt 0 ]]; then
    AHEAD_BEHIND=", ${AHEAD} ahead/${BEHIND} behind"
  fi
fi

# Remote repo: extract owner/repo from origin URL
REPO=""
if ORIGIN=$(git remote get-url origin 2>/dev/null); then
  # Handle both SSH and HTTPS URLs
  REPO=$(echo "$ORIGIN" | sed -E 's#^(https?://[^/]+/|git@[^:]+:)##; s#\.git$##')
fi

# Assemble output
if [[ -n "$REPO" ]]; then
  echo "Git: ${BRANCH} (${STATE}${AHEAD_BEHIND}) repo: ${REPO}"
else
  echo "Git: ${BRANCH} (${STATE}${AHEAD_BEHIND})"
fi

exit 0
