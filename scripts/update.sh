#!/usr/bin/env bash
# Robust update front-half (run by `ways update`): pull + prune, surviving the machine-local
# edge cases that make a bare `git pull --ff-only` abort.
#
# Two recurring causes:
#   1. settings.json — tracked (shared hooks/permissions) but machine-mutable
#      (plugin enable-state, notification prefs, language). Diverges per machine.
#   2. Stale tracked build artifacts (e.g. tools/way-embed/build/Makefile on a
#      checkout from before they were gitignored) — modified locally by a build.
#
# Strategy: autostash everything, fast-forward, prune merged branches, then
# restore. On a restore conflict we DO NOT clobber — we preserve the stash and
# tell the operator exactly how to resolve. Build artifacts are safe to discard
# (the rebuild step regenerates them); settings.json is not, so it's surfaced.
#
# The caller rebuilds the binaries and relinks after this script returns.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

stashed=0
if [ -n "$(git status --porcelain)" ]; then
  echo "→ Local changes present; stashing before fast-forward..."
  if git stash push --include-untracked -m "make-update autostash" >/dev/null 2>&1; then
    stashed=1
  else
    echo "  (could not stash — continuing; pull may still abort)"
  fi
fi

echo "→ git pull --ff-only"
if ! git pull --ff-only; then
  echo "ERROR: fast-forward failed — local history has diverged from origin/main."
  echo "  Resolve manually (e.g. 'git log --oneline origin/main..HEAD')."
  [ "$stashed" = 1 ] && echo "  Your stashed changes are safe: 'git stash pop'."
  exit 1
fi

echo "→ git fetch --prune (drop deleted remote-tracking branches)"
git fetch --prune >/dev/null 2>&1 || true

if [ "$stashed" = 1 ]; then
  echo "→ Restoring local changes..."
  if ! git stash pop >/dev/null 2>&1; then
    echo ""
    echo "⚠  Local changes conflicted with upstream — safe in the stash, not lost."
    echo "   The working tree has conflict markers; resolve per file, then 'git stash drop':"
    echo "     • keep your machine-local version (usual for settings.json):"
    echo "         git checkout stash@{0} -- settings.json"
    echo "     • take upstream instead:"
    echo "         git checkout HEAD -- settings.json"
    echo "     • build artifacts under tools/*/build/ are safe to discard:"
    echo "         git checkout -- <path>"
    echo ""
  fi
fi

echo "→ Pruning local branches whose upstream is gone (merged + deleted on origin)..."
git for-each-ref --format '%(refname:short) %(upstream:track)' refs/heads \
  | awk '$2 == "[gone]" { print $1 }' \
  | while read -r b; do
      [ "$b" = "main" ] && continue
      if git branch -D "$b" >/dev/null 2>&1; then echo "  pruned: $b"; fi
    done

echo "✓ Synced with origin/main. Rebuilding binaries + reinstalling…"
