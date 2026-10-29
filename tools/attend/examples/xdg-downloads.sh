#!/usr/bin/env bash
#
# xdg-downloads.sh — example attend external sensor
#
# Observes new files landing in the user's XDG Downloads directory and
# emits attend signals when something arrives. Demonstrates:
#
#   1. The external-sensor protocol: one line per event on stdout as
#      `magnitude|description`, silent exit on quiet, 10s timeout.
#   2. Reading XDG user-dirs to locate a well-known folder, with a
#      sensible fallback if xdg-user-dirs isn't installed.
#   3. Tracking state across polls via a marker file under XDG_STATE_HOME
#      so attend restarts don't flood with old files.
#   4. Designing a magnitude hierarchy: one file is notable, a batch
#      of files is a clear burst worth surfacing more loudly.
#
# This is a pedagogical example. It runs on any typical Linux distro
# with no dependencies beyond bash and coreutils. attend runs it through
# a command sensor file with `output: lines` (ADR-403), from slice 2 of
# that record.
#
# Author's lever: magnitude. Everything else is attend's job.

set -euo pipefail

# --- Resolve the Downloads directory via xdg-user-dirs -----------------

# xdg-user-dirs writes ~/.config/user-dirs.dirs with lines like:
#   XDG_DOWNLOAD_DIR="$HOME/Downloads"
# We parse it manually instead of sourcing to avoid executing
# arbitrary shell from a config file.

USER_DIRS="${XDG_CONFIG_HOME:-$HOME/.config}/user-dirs.dirs"
DOWNLOADS="$HOME/Downloads"

if [[ -f "$USER_DIRS" ]]; then
  xdg_value=$(grep -E "^XDG_DOWNLOAD_DIR=" "$USER_DIRS" 2>/dev/null | cut -d'=' -f2- | tr -d '"' || true)
  xdg_value=${xdg_value/\$HOME/$HOME}
  [[ -n "$xdg_value" && -d "$xdg_value" ]] && DOWNLOADS="$xdg_value"
fi

# No downloads dir → nothing to observe. Exit silently; attend treats
# this as a quiet poll.
[[ -d "$DOWNLOADS" ]] || exit 0

# --- State marker ------------------------------------------------------

# The marker file records the last time we scanned. find -newer <marker>
# gives us files modified since then. We store it under XDG_STATE_HOME
# per the base directory spec.

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/attend"
mkdir -p "$STATE_DIR"
MARKER="$STATE_DIR/xdg-downloads.marker"

# First run: create the marker and emit nothing. Otherwise we would
# immediately report every existing file as "new" on the first poll.
if [[ ! -f "$MARKER" ]]; then
  touch "$MARKER"
  exit 0
fi

# --- Detect new files --------------------------------------------------

# Top-level files only (no recursion into subdirectories). Keeps the
# scan fast and avoids walking large archives users have extracted.
new_files=$(find "$DOWNLOADS" -maxdepth 1 -type f -newer "$MARKER" 2>/dev/null || true)

# Update the marker *after* the find so the next poll picks up from
# here. Do this whether or not there were new files — the marker is
# a rolling high-water mark.
touch "$MARKER"

# Quiet poll — return with no output. attend will see an empty
# Vec<(f64, String)> and treat it as "nothing changed this tick."
[[ -z "$new_files" ]] && exit 0

# --- Emit observations with magnitude tiers ----------------------------

count=$(echo "$new_files" | wc -l | tr -d ' ')

# Magnitude hierarchy:
#   1 file    → 2.0 (notable — a single arrival is worth surfacing)
#   2-3 files → 2.5 (small batch — something's happening)
#   4+ files  → 3.0 (clear burst — breaks through into notifications)
#
# These land above a sensor file's default threshold of 2.0, so a
# single file fires on its own. For more aggregation, lower the
# magnitudes here and raise `threshold` in the sensor file so several
# files accumulate before firing.

if (( count == 1 )); then
  name=$(basename "$(echo "$new_files" | head -1)")
  echo "2.0|new file in Downloads: ${name}"
elif (( count <= 3 )); then
  echo "2.5|${count} new files in Downloads"
else
  echo "3.0|${count} new files in Downloads (burst)"
fi
