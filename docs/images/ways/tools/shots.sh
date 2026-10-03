#!/usr/bin/env bash
# Retake the screenshots of docs/reference/ways-cli.md from a synthetic
# fixture, with tools/tui-harness (tmux and fontconfig needed).
#
#   cargo build --manifest-path tools/Cargo.toml -p ways -p ways-agent -p tui-harness
#   docs/images/ways/tools/shots.sh            # every shot
#   docs/images/ways/tools/shots.sh fires why  # some
#   docs/images/ways/tools/record.sh           # the README recording
#
# It rebuilds the fixture, takes each shot at 100x30 in Adwaita Mono (it
# has ↩ and ↻; FONT overrides), and writes the PNGs over the committed ones.
# Read each image before committing it. See env.sh for the paths.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
OUT=$REPO/docs/images/ways
FONT=${FONT:-Adwaita Mono}
python3 "$TOOLS/fixture.py" "$WORK" >/dev/null
bash "$TOOLS/markers.sh"
fixture_env

A=3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42
D=71d3e5a2-9f0b-4c86-a4d7-3e1b8c0f6a29
P=/home/dev/shop

launch() { # name cols rows cmd...
  local n=$1 c=$2 r=$3; shift 3
  $TH down "$n" >/dev/null 2>&1 || true
  $TH launch "$n" --cols "$c" --rows "$r" --font "$FONT" -- "$@" >/dev/null
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
  HOME=$REALHOME $TH shot "$1" --out "$2" >/dev/null
  $TH down "$1" >/dev/null
  echo "$2"
}
want() { [ -z "$WANT" ] || [[ " $WANT " == *" $1 "* ]]; }
WANT="${*:-}"

if want help; then launch help 100 44 sh -c 'ways; sleep 600'; shot help "$OUT/ways-help.png"; fi
if want sessions; then launch s 100 30 ways session replay --project $P; keys s Down; shot s "$OUT/session-sessions.png"; fi
if want timeline; then launch t 100 30 ways session replay --project $P --session $A; keys t End f; shot t "$OUT/session-timeline.png"; fi
if want why; then launch y 100 30 ways session replay --project $P --session $A; keys y End Down Enter; shot y "$OUT/session-why.png"; fi
if want fires; then launch fi 100 30 ways session replay --project $P --session $A; keys fi 2; shot fi "$OUT/session-fires.png"; fi
if want spend; then launch sp 100 30 ways session replay --project $P; keys sp 4; shot sp "$OUT/session-spend.png"; fi
if want projects; then launch p 100 30 ways projects; keys p / text:shop; shot p "$OUT/projects.png"; fi
if want settings; then launch st 100 30 ways settings matching --project $P; keys st Down Down e C-u text:0.2 Enter; shot st "$OUT/settings-matching.png"; fi
if want picker; then launch pk 100 30 ways settings gate --project $P; keys pk Down Down Down Right Down Right Down Down Enter; shot pk "$OUT/settings-model-picker.png"; fi
if want sessionways; then launch sw 100 34 sh -c "ways session ways --session $D; sleep 600"; shot sw "$REPO/docs/images/ways-list-session.png"; fi
