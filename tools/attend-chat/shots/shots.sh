#!/usr/bin/env bash
# Drive each state of attend-chat's left column and menus (ADR-405) with
# real keys and clicks, at 80x25 and 120x35, and save a PNG and a text
# capture of each, against a synthetic attend cache. Needs tmux and
# fontconfig.
#
#   cargo build --manifest-path tools/Cargo.toml -p attend-chat -p tui-harness
#   tools/attend-chat/shots/shots.sh                 # every scenario
#   tools/attend-chat/shots/shots.sh side click      # some
#
# Output goes to $SHOTS_DIR (default target/tui-verify/attend-chat), as
# <cols>x<rows>/<scenario>.png and .txt. Nothing committed is written; read
# each image, nothing here compares pixels. SIZES overrides the sizes, e.g.
# SIZES="100x30". FONT overrides the font (default JetBrains Mono, which
# has the Nerd Font fallback for the round lozenge caps).
#
# Each step waits on the screen, not on the clock: after a key or a click
# the script polls the pane's text until two captures agree, and wait:TEXT
# polls until TEXT is on the screen. A wait that runs out fails the run.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$REPO/tools/target}/debug
WORK=${SHOTS_DIR:-$REPO/target/tui-verify/attend-chat}
# The fixture lives outside the checkout: a cwd under a managed worktree
# (`.claude/worktrees/`) normalizes to the worktree's origin, and every
# fixture agent would collapse into that one identity.
F=$(mktemp -d "${TMPDIR:-/tmp}/attend-chat-shots.XXXXXX")
trap 'rm -rf "$F"' EXIT
FONT=${FONT:-JetBrains Mono}
SIZES=${SIZES:-80x25 120x35}
REALHOME=$HOME
TH="$BIN/tui-harness --dir $WORK/th"
mkdir -p "$WORK"

# The scenarios: name | fixture | steps. A step is a tmux key name, or
#   text:CHARS     typed literally
#   click:TEXT     a left click on the first cell showing TEXT
#   side:TEXT      the same, on TEXT in the left column only
#   wait:TEXT      until TEXT is on the screen
#   gone:TEXT      until TEXT is off the screen
#   resize:COLS    the terminal narrowed (or widened) to COLS columns
#   paste:TEXT     a bracketed paste of TEXT, \n for a newline
#   rawpaste:TEXT  TEXT sent as typed, newlines and all, with no brackets
# The fixture is base, crowd (fifteen more agents), many (24 more channels,
# so the last channel row sits at the foot), own (the operator in
# #deploy) or plain (no Nerd Font). An agent's name comes from the temp
# path, and a long one cuts its project, so the quiet agent is clicked by
# the column's first `○`, its row being the agents' last.
SCENARIOS="
side         | base  | wait:Agents
off          | base  | wait:Agents F3 gone:Agents
click        | base  | wait:#infra click:#infra click:#infra wait:pick
tab          | base  | wait:#deploy click:#deploy click:#deploy wait:pick
side-show    | base  | wait:Agents side:#infra
side-menu    | base  | wait:Agents side:#infra side:#infra wait:pick
key-dock     | base  | wait:Agents M-2 M-2 wait:pick
foot-keys    | many  | wait:Agents F2 Down End Up Enter Enter wait:pick
foot-click   | many  | wait:Agents F2 Down End Escape side:#infra side:#infra wait:pick
keys         | base  | wait:Agents F2 Down Down wait:↑↓_move
menu-keys    | base  | wait:Agents F2 Down Down Enter Enter wait:pick
focus-resize | base  | wait:Agents F2 Down Down wait:↑↓_move resize:70 gone:Agents
agent-menu   | base  | wait:Agents side:○ wait:Address
crowd        | crowd | wait:Agents F2 Down Up Up Up Up Up Up Up Up Up Up Up Up
common       | base  | wait:Agents click:≡ wait:Hide_column
clear-view   | base  | wait:merged click:merged wait:Clear_view Enter wait:clear_the_view?
paste        | base  | wait:Agents text:see: paste:line_one\\nline_two\\nline_three
rawpaste     | base  | wait:Agents rawpaste:one\\ntwo
plain        | plain | wait:Agents
own-row      | own   | wait:operator click:#deploy click:#deploy wait:pick Escape gone:pick side:operator wait:Leave_#deploy
quiet-send   | base  | wait:Agents side:○ wait:Address Enter text:are_you_there Enter wait:no_heartbeat
paste-asked  | base  | wait:merged click:merged wait:Clear_view Enter wait:clear_the_view? paste:yes_please\\nsecond_line wait:kept
ask          | base  | wait:merged click:merged wait:Clear_view Enter wait:y_clear
ask-yes      | base  | wait:merged click:merged wait:Clear_view Enter wait:y_clear click:y_clear wait:cleared
ask-no       | base  | wait:merged click:merged wait:Clear_view Enter wait:y_clear click:n_keep wait:kept
ask-feed     | base  | wait:merged click:merged wait:Clear_view Enter wait:y_clear click:signing wait:kept
ask-row      | base  | wait:merged click:merged wait:Clear_view Enter wait:y_clear side:#infra wait:kept
ask-long     | base  | wait:#open click:#open click:#open wait:Clear_history Down Enter wait:y_clear
menu-row     | base  | wait:Agents side:#infra side:#infra wait:pick side:#deploy gone:pick
menu-tab     | base  | wait:Agents side:#infra side:#infra wait:pick click:#open gone:pick
prompt-click | base  | wait:Agents text:draft C-n wait:new_channel text:abc click:signing gone:new_channel
lent-name    | base  | wait:Agents text:draft C-n wait:new_channel_name text:release
lent-desc    | base  | wait:Agents side:#deploy side:#deploy wait:pick click:Describe wait:describe_#deploy text:rollouts
lent-tab     | base  | wait:Agents text:draft C-n wait:new_channel_name text:abc M-3 gone:new_channel_name
lent-row     | base  | wait:Agents click:+ wait:new_channel_name side:#infra gone:new_channel_name Escape text:abc
esc-twice    | base  | wait:Agents side:#infra side:#infra wait:pick Escape Escape gone:pick
common-quit  | base  | wait:Agents click:≡ wait:Quit
quit-click   | base  | wait:Agents text:half click:≡ wait:Quit click:Quit wait:discards
"

# The fixture: three claudes (alpha and beta live, gamma quiet), #deploy
# with beta in it, an empty #infra, and #archive, which _groups.yaml still
# names after its directory was swept. The human is `operator`. `crowd`
# adds fifteen agents, every other one live; `own` puts the operator in
# #deploy, with a message there; `many` adds #ch01 to #ch24; `plain` the
# plain shape. No attend config is written: the mouse is on by default.
fixture() { # kind
  rm -rf "$F"
  local cache=$F/cache/attend sig=$F/cache/attend/signals now shape=round
  [ "$1" = plain ] && shape=plain
  mkdir -p "$sig/_broadcast" "$sig/@deploy" "$sig/@infra" "$cache/heartbeat" "$F/home/.claude/sessions" \
    "$F/config/agent-ways" "$F/config/attend" "$F/work/alpha" "$F/work/beta" "$F/work/gamma"
  A=aaaaaaaa-0000-4000-8000-000000000001
  B=bbbbbbbb-0000-4000-8000-000000000002
  C=cccccccc-0000-4000-8000-000000000003
  local i=1 s
  local sessions=("$A:$F/work/alpha" "$B:$F/work/beta" "$C:$F/work/gamma")
  if [ "$1" = crowd ]; then
    for n in $(seq -w 1 15); do
      sessions+=("dddddddd-0000-4000-8000-0000000000$n:$F/work/crowd-$n")
      mkdir -p "$F/work/crowd-$n"
      [ $((10#$n % 2)) -eq 1 ] && touch "$cache/heartbeat/dddddddd-0000-4000-8000-0000000000$n"
    done
  fi
  if [ "$1" = many ]; then
    for n in $(seq -w 1 24); do mkdir -p "$sig/@ch$n"; done
  fi
  for s in "${sessions[@]}"; do
    printf '{"pid":%d,"sessionId":"%s","cwd":"%s"}\n' "$i" "${s%%:*}" "${s#*:}" > "$F/home/.claude/sessions/$i.json"
    i=$((i + 1))
  done
  touch "$cache/heartbeat/$A" "$cache/heartbeat/$B"
  local own=
  [ "$1" = own ] && own="
    - operator"
  cat > "$sig/_groups.yaml" <<YAML
deploy:
  pinned: true
  members:
    - $B$own
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
  [ "$1" = own ] && msg "$sig/@deploy" s4 120 "external:operator@kitty" "$F/work/alpha" "watching the rollout"
  printf 'theme:\n  active: nord\n  shape: %s\n' "$shape" > "$F/config/agent-ways/config.yaml"
}

launch() { # name cols rows
  $TH down "$1" >/dev/null 2>&1 || true
  (cd "$F/work/alpha" && env -u CLAUDE_CONFIG_DIR -u CLAUDE_PROJECT_DIR -u CLAUDE_CODE_SESSION_ID HOME="$F/home" USER=operator \
    XDG_CACHE_HOME="$F/cache" XDG_CONFIG_HOME="$F/config" XDG_STATE_HOME="$F/state" XDG_DATA_HOME="$F/data" \
    TERM=xterm-256color COLORTERM=truecolor \
    $TH launch "$1" --cols "$2" --rows "$3" --font "$FONT" -- "$BIN/attend-chat" >/dev/null)
}

# Poll the pane's text every 0.1 s, up to 10 s, until `test` holds for it.
poll() { # name what test...
  local n=$1 what=$2 t
  shift 2
  for _ in $(seq 100); do
    t=$($TH text "$n")
    if "$@" "$t"; then return 0; fi
    sleep 0.1
  done
  echo "$n: timed out waiting for $what" >&2
  return 1
}
has() { [[ "$2" == *"$1"* ]]; }
lacks() { [[ "$2" != *"$1"* ]]; }
# Two captures 0.1 s apart that agree: the screen has taken the input.
PREV=
steady() {
  if [ "$1" = "$PREV" ]; then return 0; fi
  PREV=$1
  return 1
}
settle() { PREV=; poll "$1" "the screen to settle" steady; }

# The 1-based cell of the first TEXT on the screen; with `side`, the first
# left of the column's rule, so a name in the column and not in the feed.
cell() { # name text [side]
  $TH text "$1" | python3 -I -c '
import sys
want, side = sys.argv[1], sys.argv[2:] == ["side"]
for row, line in enumerate(sys.stdin.read().split("\n"), 1):
    col, rule = line.find(want), line.find("│")
    if col >= 0 and not (side and not 0 <= col < rule):
        print(col + 1, row)
        break
else:
    sys.exit(1)
' "${@:2}"
}

# The harness has no resize verb: its private tmux server resizes the window.
resize() { # name cols
  local s
  s=$(tmux -L agent-ways-tui list-sessions -F '#S' | grep -F "$1" | head -1)
  tmux -L agent-ways-tui resize-window -t "$s" -x "$2"
}

step() { # name step
  local n=$1 k=$2 at arg
  arg=${k#*:}
  arg=${arg//_/ }
  case $k in
    text:*) $TH send "$n" -l "$arg" ;;
    click:* | side:*)
      at=$(cell "$n" "$arg" "${k%%:*}") || { echo "$n: no '$arg' to click" >&2; return 1; }
      $TH send "$n" -l $'\e[<0;'"${at% *};${at#* }M"$'\e[<0;'"${at% *};${at#* }m" ;;
    wait:*) poll "$n" "'$arg'" has "$arg"; return ;;
    gone:*) poll "$n" "'$arg' to go" lacks "$arg"; return ;;
    resize:*) resize "$n" "$arg" ;;
    paste:*) $TH send "$n" -l $'\e[200~'"$(printf '%b' "$arg")"$'\e[201~' ;;
    rawpaste:*) $TH send "$n" -l "$(printf '%b' "$arg")" ;;
    *) $TH send "$n" "$k" ;;
  esac
  settle "$n"
}

want() { [ $# -eq 1 ] || [[ " ${*:2} " == *" $1 "* ]]; }
trim() { local s=$1; s=${s#"${s%%[![:space:]]*}"}; printf '%s' "${s%"${s##*[![:space:]]}"}"; }

n=0
while IFS='|' read -r name kind steps; do
  name=$(trim "$name")
  [ -n "$name" ] || continue
  want "$name" "$@" || continue
  fixture "$(trim "$kind")"
  for size in $SIZES; do
    c=${size%x*} r=${size#*x}
    out=$WORK/$size
    mkdir -p "$out"
    s=ac-$name
    launch "$s" "$c" "$r"
    for k in $steps; do
      step "$s" "$k"
    done
    $TH text "$s" >"$out/$name.txt"
    HOME=$REALHOME $TH shot "$s" --out "$out/$name.png" >/dev/null
    $TH down "$s" >/dev/null
    echo "$out/$name.png"
    n=$((n + 1))
  done
done <<<"$SCENARIOS"
[ "$n" -gt 0 ] || { echo "no scenario matched: $*" >&2; exit 1; }
