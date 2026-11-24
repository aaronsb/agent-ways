#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Retake the screenshots of the README and docs/reference/ways-cli.md from
# a synthetic fixture. tools/tui-harness drives each screen in tmux, and
# tools/tui-harness/kitty-render.sh paints the final frame in kitty on a
# private Xvfb display (kitty, Xvfb, ImageMagick 7, JetBrains Mono and Noto
# Color Emoji needed).
#
#   cargo build --manifest-path tools/Cargo.toml -p ways -p ways-agent -p tui-harness
#   docs/images/ways/tools/shots.sh            # every shot
#   docs/images/ways/tools/shots.sh fires why  # some
#   docs/images/ways/tools/record.sh           # the README recording
#
# It rebuilds the fixture and takes each shot at 150x40 in the theme the
# shot names, so the docs show the themes side by side, and writes the PNGs
# over the committed ones. Read each image before committing it. See env.sh
# for the paths.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
OUT=$REPO/docs/images/ways
RENDER=$REPO/tools/tui-harness/kitty-render.sh
COLS=150 ROWS=40
python3 "$TOOLS/fixture.py" "$WORK" >/dev/null
bash "$TOOLS/markers.sh"
fixture_env

A=3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42
D=71d3e5a2-9f0b-4c86-a4d7-3e1b8c0f6a29
P=/home/dev/shop
# The settings screens take a project on disk: the fixture's ~/shop (fixture.py),
# where HOME is the link `~`. The tilde is literal, not expanded.
PS='~/shop'
CFG=$F/home/.config/agent-ways/config.yaml

# The theme a shot is drawn in, with any more of config.yaml after it.
theme() { # theme [yaml...]
  THEME=$1; shift
  { printf 'theme:\n  active: %s\n' "$THEME"; printf '%s\n' "$@"; } >"$CFG"
}
launch() { # name cmd... (at COLS x ROWS)
  local n=$1; shift
  $TH down "$n" >/dev/null 2>&1 || true
  $TH launch "$n" --cols "$COLS" --rows "$ROWS" -- "$@" >/dev/null
  sleep 2
}
keys() { # name key... (tmux key names; text:<chars> types literally)
  local n=$1; shift
  for k in "$@"; do
    case $k in
      text:*) $TH send "$n" -l "${k#text:}" ;;
      *) $TH send "$n" "$k" ;;
    esac
    sleep 0.6
  done
  sleep 1
}
shot() { # name path
  $TH text "$1" --ansi >"$WORK/$1.ansi"
  $TH down "$1" >/dev/null
  "$RENDER" "$WORK/$1.ansi" "$COLS" "$ROWS" "$2" "$THEME"
}
want() { [ -z "$WANT" ] || [[ " $WANT " == *" $1 "* ]]; }
WANT="${*:-}"

# The mod tab with the app's mods installed: a stand-in `claude` answers
# the plugin lists and --version (verify.sh's mod_app, healthy).
MODAPP=$F/home/.local/share/agent-ways
mods_installed() {
  local stub=$WORK/stub/claude
  mkdir -p "$MODAPP/plugins/.claude-plugin" "$MODAPP/plugins/attend/.claude-plugin" "$MODAPP/plugins/ways-judge/.claude-plugin" "$WORK/stub"
  printf '{"name":"agent-ways","plugins":[{"name":"attend"},{"name":"ways-judge"}]}\n' >"$MODAPP/plugins/.claude-plugin/marketplace.json"
  printf '{"name":"attend","version":"0.36.0"}\n' >"$MODAPP/plugins/attend/.claude-plugin/plugin.json"
  printf '{"name":"ways-judge","version":"1.55.0"}\n' >"$MODAPP/plugins/ways-judge/.claude-plugin/plugin.json"
  cat >"$stub" <<STUB
#!/bin/sh
case "\$*" in
  "--version") echo "2.1.330 (Claude Code)" ;;
  "plugin marketplace list --json") echo '[{"name":"agent-ways","source":"directory","path":"$MODAPP/plugins"}]' ;;
  "plugin list --json") echo '[{"id":"attend@agent-ways","version":"0.36.0","scope":"user"},{"id":"ways-judge@agent-ways","version":"1.55.0","scope":"user"}]' ;;
  *) exit 2 ;;
esac
STUB
  chmod +x "$stub"
  export WAYS_CLAUDE_BIN=$stub
}
mods_removed() { rm -rf "$MODAPP" "$WORK/stub"; unset WAYS_CLAUDE_BIN; }

# Two sensors of the user's beside the shipped ones: a command sensor and
# an MCP sensor, each a file in attend's user sensor directory.
user_sensors() {
  local d=$F/home/.config/attend/sensors
  mkdir -p "$d"
  printf 'description: the wind at home, from Open-Meteo\nkind: command\nargv: [curl, -s, "https://api.open-meteo.com/v1/forecast?latitude=45.5&longitude=-122.7&current=wind_speed_10m"]\noutput: json\ninterval: 900\nmin_interval: 300\n' >"$d/weather.yaml"
  printf 'description: unread messages in Slack\nkind: mcp\nlane: message\nserver: slack\ntool: list_unread\nargs: { channel: general }\nstate:\n  unread: { type: number, from: "$.unread_count" }\n  user: { type: string, max: 64, from: "$.messages[0].user.name" }\ntriggers:\n  - name: new-message\n    state: { unread: rises }\n    author: user\n    magnitude: 1\n    description: "{state.unread} unread in Slack"\n' >"$d/slack-unread.yaml"
}

if want help; then theme agent-ways; COLS=110 ROWS=44 launch help sh -c 'ways; sleep 600'; COLS=110 ROWS=44 shot help "$OUT/ways-help.png"; fi
if want sessions; then theme tokyonight-night; launch s ways session replay --project $P; keys s Down; shot s "$OUT/session-sessions.png"; fi
if want timeline; then theme miami-nights; launch t ways session replay --project $P --session $A; keys t End f; shot t "$OUT/session-timeline.png"; fi
if want why; then theme catppuccin-mocha; launch y ways session replay --project $P --session $A; keys y End Down Enter; shot y "$OUT/session-why.png"; fi
if want fires; then theme dracula; launch fr ways session replay --project $P --session $A; keys fr 2; shot fr "$OUT/session-fires.png"; fi
if want spend; then theme nord; launch sp ways session replay --project $P; keys sp 4; shot sp "$OUT/session-spend.png"; fi
if want projects; then theme gruvbox-dark; launch p ways projects; keys p / text:shop; shot p "$OUT/projects.png"; fi
if want settings; then theme terra; launch sw ways settings ways --project $PS; keys sw End Right Down Down Down Down Down Down Right Down; shot sw "$OUT/settings-ways.png"; fi
if want matching; then theme nord; launch st ways settings matching --project $PS; keys st Down Down e C-u text:0.2 Enter; shot st "$OUT/settings-matching.png"; fi
if want picker; then theme tokyonight-night; launch pk ways settings gate --project $PS; keys pk Down Down Down Down Right Down Right Down Down Enter; shot pk "$OUT/settings-model-picker.png"; fi
if want install; then theme paper; launch in ways settings install --project $PS; keys in a Down; shot in "$OUT/settings-install.png"; fi
if want attend; then theme catppuccin-mocha; launch at ways settings attend --project $PS; keys at Right; shot at "$OUT/settings-attend.png"; fi
if want sensors; then theme dracula; user_sensors; launch se ways settings sensors --project $PS; keys se Down Down Down Down Down Down; shot se "$OUT/settings-sensors.png"; fi
if want mod; then theme maximum-claude 'mods:' '  ways-judge:' '    install: true'; mods_installed; launch md ways settings mod --project $PS; shot md "$OUT/settings-mod.png"; mods_removed; fi
# The theme tab previews the theme under the cursor over the whole screen:
# agent-ways is in use, and the cursor rests on crystal-mountain.
if want theme; then theme agent-ways; launch th ways settings theme --project $PS; keys th Down Down Down Down Down Down Down Down Down Down Down; THEME=crystal-mountain; shot th "$OUT/settings-theme.png"; fi
if want sessionways; then theme agent-ways; COLS=110 ROWS=34 launch sl sh -c "ways session ways --session $D; sleep 600"; COLS=110 ROWS=34 shot sl "$REPO/docs/images/ways-list-session.png"; fi

# One screen in four themes, from plain to loud, joined two by two.
if want themes; then
  for t in paper nord tampa miami-nights; do
    theme $t; launch tm ways session replay --project $P --session $A; keys tm End f; shot tm "$WORK/themes-$t.png" >/dev/null
  done
  magick montage "$WORK"/themes-{paper,nord,tampa,miami-nights}.png -tile 2x2 -geometry +12+12 -background none "$OUT/themes.png"
  echo "$OUT/themes.png"
fi

# Last, as it moves session A to now in the fixture: the replay following
# the session as it writes, its newest frame seconds old.
if want live; then
  python3 - "$F/home/.local/state/agent-ways/events.jsonl" "$A" <<'EOF'
import datetime as dt, json, sys
path, sid = sys.argv[1], sys.argv[2]
rows = [json.loads(line) for line in open(path)]
fmt = "%Y-%m-%dT%H:%M:%SZ"
last = max(dt.datetime.strptime(r["ts"], fmt) for r in rows if r["session"] == sid)
now = dt.datetime.now(dt.timezone.utc).replace(tzinfo=None, microsecond=0)
shift = now - dt.timedelta(seconds=4) - last
for r in rows:
    if r["session"] == sid:
        r["ts"] = (dt.datetime.strptime(r["ts"], fmt) + shift).strftime(fmt)
with open(path, "w") as out:
    out.writelines(json.dumps(r) + "\n" for r in rows)
EOF
  theme agent-ways; launch lv ways session live --project $P --session $A; shot lv "$OUT/session-live.png"
fi
