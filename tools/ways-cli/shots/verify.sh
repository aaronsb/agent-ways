#!/usr/bin/env bash
# Drive each interactive screen of `ways` with real keys, at 80x25 and
# 120x35, and save a PNG and a text capture of each state. The screens run
# on the synthetic fixture of docs/images/ways/tools (env.sh, fixture.py).
#
#   cargo build --manifest-path tools/Cargo.toml -p ways -p ways-agent -p tui-harness
#   tools/ways-cli/shots/verify.sh                 # every scenario
#   tools/ways-cli/shots/verify.sh timeline why    # some
#
# Output goes to $SHOTS_DIR (default target/tui-verify/ways), as
# <cols>x<rows>/<scenario>.png and .txt. Nothing committed is written.
# SIZES overrides the sizes, e.g. SIZES="100x30".
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
export SHOTS_DIR=${SHOTS_DIR:-$HERE/../../../target/tui-verify/ways}
. "$HERE/../../../docs/images/ways/tools/env.sh"
FONT=${FONT:-Adwaita Mono}
SIZES=${SIZES:-80x25 120x35}

A=3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42
P=$FX_USER_HOME/shop
# The settings screens take a project on disk: the fixture's ~/shop, where
# HOME is the link `~` (env.sh). The tilde is literal, not expanded.
PS='~/shop'

# The scenarios: name | command | keys [| after]. Keys are tmux key names,
# and text:<chars> types literally. The keys after the shot, if any, put
# back what the scenario changed in the fixture, so the scenarios after it
# see it as it was. A scenario whose state needs files the fixture lacks
# has a setup_<name> function that writes them and a teardown_<name> that
# removes them (a `-` in the name is `_` there). A new screen state adds a
# line here.
SCENARIOS="
sessions   | ways session replay --project $P               | Down
timeline   | ways session replay --project $P --session $A  | End f
why        | ways session replay --project $P --session $A  | End Down Enter
fires      | ways session replay --project $P --session $A  | 2
spend      | ways session replay --project $P               | 4
stats      | ways session replay --project $P               | 5
precision  | ways session replay --project $P               | 6
projects   | ways projects                                  | / text:shop
settings   | ways settings --project $PS                    |
matching   | ways settings matching --project $PS           | Down Down e C-u text:0.2 Enter
picker     | ways settings gate --project $PS               | Down Down Down Right Down Right Down Down Enter
engine-picker | ways settings gate --project $PS            | Down Enter
judge      | ways settings gate --project $PS               | End
judge-readiness | ways settings gate --project $PS          | End Up Up Up Up
mod        | ways settings mod --project $PS                |
quit-guard | ways settings matching --project $PS           | Down Down e C-u text:0.2 Enter q
key-help   | ways settings matching --project $PS           | text:?
close-editor | ways settings theme --project $PS            | e text:mine Enter Escape
response   | ways settings install --project $PS            | p text:/nonexistent/claude Enter
confirm    | ways settings install --project $PS            | Down Down r
discard-tab | ways settings matching --project $PS          | Down Down e C-u text:0.2 Enter X
review-discard | ways settings matching --project $PS       | Down Down e C-u text:0.2 Enter w X
item-delete | ways settings theme --project $PS             | a Enter text:scratch Enter a Down Down Down Enter | y
action-menu | ways settings install --project $PS           | a Down
theme-menu | ways settings theme --project $PS              | a Down
flow-pick  | ways settings install --project $PS            | t
flow-other | ways settings install --project $PS            | t Down Enter text:~/work Enter
"

python3 "$TOOLS/fixture.py" "$WORK" >/dev/null
fixture_env

# The gate tab's judge section (ADR-708 §7, §8): claude-code named with
# neither the ways-judge mod nor a key, and two running sessions, one judging
# inside Claude Code and one left with no engine, which holds no resolution
# file and is resolved as the gate resolves it. A session record names a
# live process: this script's, or its parent's.
JUDGED="3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42 cccccccc-0000-4000-8000-000000000003"
setup_judge() {
  local h=$F/home pids=($$ $PPID) i=0 id
  printf 'engine: claude-code\n' >"$h/.config/agent-ways/agent.yaml"
  mkdir -p "$h/.claude/sessions"
  for id in $JUDGED; do
    printf '{"pid":%s,"sessionId":"%s","cwd":"/home/dev/shop"}\n' "${pids[$i]}" "$id" >"$h/.claude/sessions/${pids[$i]}.json"
    mkdir -p "$h/.local/state/agent-ways/judge/$id"
    i=$((i + 1))
  done
  printf '{"engine":"claude-code","reason":"explicit","provider":"claude-code"}\n' \
    >"$h/.local/state/agent-ways/judge/3f6c2a91-4b7e-4d10-9c55-2e8b7a1d0f42/resolution.json"
}
teardown_judge() {
  local h=$F/home id
  rm -f "$h/.config/agent-ways/agent.yaml" "$h/.claude/sessions/$$.json" "$h/.claude/sessions/$PPID.json"
  for id in $JUDGED; do rm -rf "$h/.local/state/agent-ways/judge/$id"; done
}
setup_judge_readiness() { setup_judge; }
teardown_judge_readiness() { teardown_judge; }

want() { [ $# -eq 1 ] || [[ " ${*:2} " == *" $1 "* ]]; }
trim() { local s=$1; s=${s#"${s%%[![:space:]]*}"}; printf '%s' "${s%"${s##*[![:space:]]}"}"; }

n=0
# Send each of a scenario's keys to session $1.
send_keys() {
  local s=$1 k
  for k in "${@:2}"; do
    case $k in
      text:*) $TH send "$s" -l "${k#text:}" ;;
      *) $TH send "$s" "$k" ;;
    esac
    sleep 0.6
  done
}

while IFS='|' read -r name cmd keys after; do
  name=$(trim "$name")
  [ -n "$name" ] || continue
  want "$name" "$@" || continue
  read -ra argv <<<"$cmd"
  hook=${name//-/_}
  if declare -F "setup_$hook" >/dev/null; then "setup_$hook"; fi
  for size in $SIZES; do
    c=${size%x*} r=${size#*x}
    out=$WORK/$size
    mkdir -p "$out"
    s=v-$name
    $TH down "$s" >/dev/null 2>&1 || true
    $TH launch "$s" --cols "$c" --rows "$r" --font "$FONT" -- "${argv[@]}" >/dev/null
    sleep 2
    send_keys "$s" $keys
    sleep 1
    $TH text "$s" >"$out/$name.txt"
    HOME=$REALHOME $TH shot "$s" --out "$out/$name.png" >/dev/null
    send_keys "$s" $after
    $TH down "$s" >/dev/null
    echo "$out/$name.png"
    n=$((n + 1))
  done
  if declare -F "teardown_$hook" >/dev/null; then "teardown_$hook"; fi
done <<<"$SCENARIOS"
[ "$n" -gt 0 ] || { echo "no scenario matched: $*" >&2; exit 1; }
