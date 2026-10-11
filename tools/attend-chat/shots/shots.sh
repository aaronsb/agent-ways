#!/usr/bin/env bash
# Screenshots of attend-chat's left column and channel menus (ADR-405),
# driven through tools/tui-harness against a synthetic attend cache.
# Needs tmux and fontconfig.
#
#   cargo build --manifest-path tools/Cargo.toml -p attend-chat -p tui-harness
#   tools/attend-chat/shots/shots.sh                 # every scenario
#   tools/attend-chat/shots/shots.sh side-80 click   # some
#
# PNGs land in $SHOTS_DIR (default target/attend-chat-shots, git-ignored),
# one per scenario. Read each image; nothing here compares pixels.
# FONT overrides the font (default JetBrains Mono, which has the Nerd Font
# fallback for the round lozenge caps).
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$REPO/tools/target}/debug
WORK=${SHOTS_DIR:-$REPO/target/attend-chat-shots}
F=$WORK/fx
FONT=${FONT:-JetBrains Mono}
REALHOME=$HOME
TH="$BIN/tui-harness --dir $WORK/th"
mkdir -p "$WORK"

# The fixture: three claudes (alpha and beta live, gamma quiet), #deploy
# with beta in it, an empty #infra, and #archive, which _groups.yaml still
# names after its directory was swept. The human is `operator`.
fixture() {
  rm -rf "$F"
  local cache=$F/cache/attend sig=$F/cache/attend/signals now
  mkdir -p "$sig/_broadcast" "$sig/@deploy" "$sig/@infra" "$cache/heartbeat" "$F/home/.claude/sessions" \
    "$F/config/agent-ways" "$F/config/attend" "$F/work/alpha" "$F/work/beta" "$F/work/gamma"
  A=aaaaaaaa-0000-4000-8000-000000000001
  B=bbbbbbbb-0000-4000-8000-000000000002
  C=cccccccc-0000-4000-8000-000000000003
  local i=1
  for s in "$A:$F/work/alpha" "$B:$F/work/beta" "$C:$F/work/gamma"; do
    printf '{"pid":%d,"sessionId":"%s","cwd":"%s"}\n' "$i" "${s%%:*}" "${s#*:}" > "$F/home/.claude/sessions/$i.json"
    i=$((i + 1))
  done
  touch "$cache/heartbeat/$A" "$cache/heartbeat/$B"
  cat > "$sig/_groups.yaml" <<YAML
deploy:
  pinned: true
  members:
    - $B
  description: rollout coordination
infra:
  pinned: true
  members: []
archive:
  pinned: false
  members:
    - $A
YAML
  now=$(date +%s)
  msg() { # dir id age from cwd text
    printf '%s|%s|%s|%s\n' "$4" "$(basename "$5")" "$5" "$6" > "$1/$2.signal"
    touch -d "@$((now - $3))" "$1/$2.signal"
  }
  msg "$sig/_broadcast" s1 900 "claude:$C" "$F/work/gamma" "signing off for today"
  msg "$sig/_broadcast" s2 600 "claude:$A" "$F/work/alpha" "starting the theme port; the legend moves first"
  msg "$sig/@deploy" s3 300 "claude:$B" "$F/work/beta" "rollout is green on staging"
  printf 'theme:\n  active: nord\n  shape: %s\n' "${SHAPE:-round}" > "$F/config/agent-ways/config.yaml"
  printf 'chat:\n  mouse: true\n' > "$F/config/attend/config.yaml"
}

launch() { # name cols rows
  $TH down "$1" >/dev/null 2>&1 || true
  (cd "$F/work/alpha" && env -u CLAUDE_CONFIG_DIR -u CLAUDE_PROJECT_DIR -u CLAUDE_CODE_SESSION_ID HOME="$F/home" USER=operator \
    XDG_CACHE_HOME="$F/cache" XDG_CONFIG_HOME="$F/config" XDG_STATE_HOME="$F/state" XDG_DATA_HOME="$F/data" \
    TERM=xterm-256color COLORTERM=truecolor \
    $TH launch "$1" --cols "$2" --rows "$3" --font "$FONT" -- "$BIN/attend-chat" >/dev/null)
  sleep 2
}
keys() { # name key... (tmux key names; text:<chars> types; click:COL,ROW from 1)
  local n=$1; shift
  for k in "$@"; do
    case $k in
      text:*) $TH send "$n" -l "${k#text:}" ;;
      click:*) local at=${k#click:}; $TH send "$n" -l $'\e[<0;'"${at%,*};${at#*,}M"$'\e[<0;'"${at%,*};${at#*,}m" ;;
      *) $TH send "$n" "$k" ;;
    esac
    sleep 0.5
  done
  sleep 1
}
shot() { # name
  HOME=$REALHOME $TH shot "$1" --out "$WORK/$1.png" >/dev/null
  $TH text "$1" > "$WORK/$1.txt"
  $TH down "$1" >/dev/null
  echo "$WORK/$1.png"
}
want() { [ -z "$WANT" ] || [[ " $WANT " == *" $1 "* ]]; }
WANT="${*:-}"

fixture
# The column beside the feed, at the two sizes users run.
if want side-80; then launch side-80 80 25; shot side-80; fi
if want side-120; then launch side-120 120 35; shot side-120; fi
# Ctrl+B hides it and the legend row comes back.
if want off-80; then launch off-80 80 25; keys off-80 C-b; shot off-80; fi
# Too narrow for the column: it hides itself.
if want narrow; then launch narrow 70 25; shot narrow; fi
# One click on a channel row: the channel shows and its menu opens.
if want click; then launch click 80 25; keys click click:6,8; shot click; fi
# One click on a tab does the same.
if want tab; then launch tab 80 25; keys tab click:38,1; shot tab; fi
# The keyboard: F2, Down into the column, Down to #deploy.
if want keys; then launch keys 80 25; keys keys F2 Down Down; shot keys; fi
# Enter on the row opens the menu; the human is not in #deploy, so Join.
if want menu-keys; then launch menu-keys 80 25; keys menu-keys F2 Down Down Enter; shot menu-keys; fi
# The plain shape, for terminals without a Nerd Font.
if want plain; then SHAPE=plain fixture; launch plain 80 25; shot plain; fi
