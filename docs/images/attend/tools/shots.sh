#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Retake the attend-chat screenshots of the README and
# docs/attend-and-monitor/tui.md. tools/attend-chat/shots/shots.sh drives
# each scenario against its synthetic attend cache at 150x40, kitty paints
# the frame (RENDER=kitty), and the PNG is copied over the committed one.
# Each shot names its theme, so the docs show several.
#
#   cargo build --manifest-path tools/Cargo.toml -p attend-chat -p attend -p tui-harness
#   docs/images/attend/tools/shots.sh            # every shot
#   docs/images/attend/tools/shots.sh guard      # some, by scenario
#
# The fixture's agent names come from its temp path, so they change from
# one run to the next. Read each image before committing it.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../../../.." && pwd)
OUT=$REPO/docs/images/attend
WORK=$REPO/target/doc-shots/attend
WANT=" ${*:-} "

# scenario | theme | file under docs/images/attend
SHOTS="
header      | catppuccin-mocha | attend-chat-operator-view.png
fanout      | agent-ways       | attend-chat-fanout.png
header-chan | gruvbox-dark     | attend-chat-channel.png
slash       | nord             | attend-chat-slash.png
guard-draft | dracula          | attend-chat-guard.png
settings    | terra            | attend-chat-settings.png
"
while IFS='|' read -r scenario theme file; do
  scenario=$(echo $scenario) theme=$(echo $theme) file=$(echo $file)
  [ -n "$scenario" ] || continue
  [ "$WANT" = "  " ] || [[ "$WANT" == *" $scenario "* ]] || continue
  SIZES=150x40 RENDER=kitty THEME=$theme SHOTS_DIR=$WORK \
    "$REPO/tools/attend-chat/shots/shots.sh" "$scenario" >/dev/null
  cp "$WORK/150x40/$scenario.png" "$OUT/$file"
  echo "$OUT/$file"
done <<<"$SHOTS"
