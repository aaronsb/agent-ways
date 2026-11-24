#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Paint a captured pane in kitty and save it as a PNG, for the screenshots
# in the docs. The pane is what `tui-harness text --ansi` prints: the
# program ran and was driven in tmux, and kitty draws the final frame with
# its own font rendering, truecolor and emoji, which the harness's built-in
# renderer does not match.
#
#   kitty-render.sh PANE.ansi COLS ROWS OUT.png [THEME]
#
# kitty runs on a private Xvfb display, so nothing opens on the desktop.
# THEME names a file in tools/agent-theme/themes; its background colours
# kitty's padding. FONT (default JetBrains Mono) and SIZE (default 20) set
# the face; the emoji fall back to Noto Color Emoji. Needs kitty, Xvfb and
# ImageMagick 7 (`magick`).
set -euo pipefail
pane=$1 cols=$2 rows=$3 out=$4 theme=${5:-}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
FONT=${FONT:-JetBrains Mono}
SIZE=${SIZE:-20}
PAD=14
for t in kitty Xvfb magick; do
  command -v "$t" >/dev/null || { echo "kitty-render: $t is not installed" >&2; exit 1; }
done

bg='#000000' fg='#d0d0d0'
if [ -n "$theme" ]; then
  tf=$REPO/tools/agent-theme/themes/$theme.theme
  [ -f "$tf" ] || { echo "kitty-render: no theme $theme" >&2; exit 1; }
  bg=$(sed -n 's/^THEME_BG="\(.*\)"/\1/p' "$tf")
  fg=$(sed -n 's/^THEME_FG="\(.*\)"/\1/p' "$tf")
fi

work=$(mktemp -d)
xpid= kpid=
cleanup() {
  [ -z "$kpid" ] || kill "$kpid" 2>/dev/null || true
  [ -z "$xpid" ] || kill "$xpid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

# The pane without its last newline, so the last row does not scroll the
# first away, and with the cursor hidden.
{ printf '\033[?25l'; head -c -1 "$pane"; } >"$work/pane"

# A screen far larger than the window; kitty opens at its top left.
Xvfb -displayfd 3 -screen 0 4000x3000x24 -nolisten tcp 3>"$work/display" >/dev/null 2>&1 &
xpid=$!
for _ in $(seq 50); do [ -s "$work/display" ] && break; sleep 0.1; done
display=:$(cat "$work/display")

sock=unix:@kitty-render-$$
DISPLAY=$display WAYLAND_DISPLAY= kitty --config NONE -o linux_display_server=x11 \
  -o font_family="$FONT" -o font_size="$SIZE" -o disable_ligatures=always \
  -o "symbol_map U+1F000-U+1FAFF Noto Color Emoji" \
  -o window_padding_width=$PAD -o remember_window_size=no \
  -o initial_window_width="${cols}c" -o initial_window_height="${rows}c" \
  -o background="$bg" -o foreground="$fg" -o cursor_blink_interval=0 \
  -o allow_remote_control=yes -o enable_audio_bell=no \
  --listen-on "$sock" sh -c "cat '$work/pane'; exec sleep 60" >/dev/null 2>&1 &
kpid=$!
for _ in $(seq 50); do kitty @ --to "$sock" ls >/dev/null 2>&1 && break; sleep 0.1; done
sleep 1

# The root window holds kitty on black; a black border lets -trim cut to
# the window whatever colour its corner is.
DISPLAY=$display magick import -window root "$work/root.png"
magick "$work/root.png" -bordercolor black -border 1 -trim +repage "$out"
echo "$out"
