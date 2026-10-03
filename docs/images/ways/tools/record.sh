#!/usr/bin/env bash
# Record the README's replay: the timeline of the fixture's session A stepped
# frame by frame, then the matched view and why a blocked way was kept out,
# joined at one frame a second into docs/images/ways-introspect.mp4 and .gif.
# Needs ffmpeg. Run shots.sh first (or fixture.py) so the fixture exists.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
REC=$WORK/rec
OUT=$REPO/docs/images
rm -rf "$REC"; mkdir -p "$REC"
fixture_env
A=3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42
$TH down rec >/dev/null 2>&1 || true
$TH launch rec --cols 100 --rows 30 --font "${FONT:-Adwaita Mono}" -- ways session replay --project /home/dev/shop --session $A >/dev/null
sleep 2
n=0
snap() { n=$((n + 1)); HOME=$REALHOME $TH shot rec --out "$REC/$(printf %03d $n).png" >/dev/null; }
$TH send rec Home; sleep 1; snap
for _ in 1 2 3 4 5 6 7 8; do $TH send rec Right; sleep 0.8; snap; done
snap
$TH send rec f; sleep 1; snap; snap
$TH send rec Down; sleep 0.5; $TH send rec Enter; sleep 1; snap; snap; snap
$TH down rec >/dev/null
ffmpeg -loglevel error -y -framerate 1 -i "$REC/%03d.png" -c:v libx264 -pix_fmt yuv420p -r 10 "$OUT/ways-introspect.mp4"
ffmpeg -loglevel error -y -framerate 1 -i "$REC/%03d.png" -vf "split[a][b];[a]palettegen=max_colors=64[p];[b][p]paletteuse=dither=none" -loop 0 "$OUT/ways-introspect.gif"
ls -l "$OUT/ways-introspect."*
