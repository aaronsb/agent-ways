#!/usr/bin/env bash
# Drive each interactive screen of `ways` with real keys, at 80x25 and
# 120x35, and save a PNG and a text capture of each state. The screens run
# on the synthetic fixture of docs/images/ways/tools (env.sh, fixture.py).
#
#   cargo build --manifest-path tools/Cargo.toml -p ways -p ways-agent -p attend -p tui-harness
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
mod-states | ways settings mod --project $PS                |
mod-actions | ways settings mod --project $PS               | a
degraded   | ways settings --project $PS                    |
degraded-mod | ways settings mod --project $PS              | Left Down Down
healthy    | ways settings --project $PS                    |
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
theme-claude | ways settings theme --project $PS            | End
theme-claude-nights | ways settings theme --project $PS     | End c End Up Up
theme-claude-paths | ways settings theme --project $PS      | End c End
theme-claude-tampa | ways settings theme --project $PS      | End Up c Right | Left
flow-pick  | ways settings install --project $PS            | t
flow-other | ways settings install --project $PS            | t Down Enter text:~/work Enter
card-mod   | ways settings mod --project $PS                |
card-sensor | ways settings sensors --project $PS           | Down Down Down Down
card-command-sensor | ways settings sensors --project $PS   | End
card-mcp-sensor | ways settings sensors --project $PS       | End Up
card-theme | ways settings theme --project $PS              | Down
card-user-theme | ways settings theme --project $PS         | End
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
    printf '{"pid":%s,"sessionId":"%s","cwd":"%s/shop"}\n' "${pids[$i]}" "$id" "$h" >"$h/.claude/sessions/${pids[$i]}.json"
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

# The mod tab's node per mod (ADR-512 item 6): an app that ships both
# mods, attend installed from its folder, and ways-judge wanted under
# gate.engine: claude-code on a Claude Code below its floor, so pending. A
# stand-in `claude` answers the plugin lists and --version: mod_app takes
# the version it prints and the plugin list.
MODAPP=$F/home/.local/share/agent-ways
ATTEND_ONLY='[{"id":"attend@agent-ways","version":"0.29.0","scope":"user"}]'
setup_mod_actions() { mod_app 2.1.300 "$ATTEND_ONLY"; }
mod_app() {
  local h=$F/home stub=$WORK/stub/claude
  mkdir -p "$MODAPP/plugins/.claude-plugin" "$MODAPP/plugins/attend/.claude-plugin" "$MODAPP/plugins/ways-judge/.claude-plugin" "$WORK/stub"
  printf '{"name":"agent-ways","plugins":[{"name":"attend"},{"name":"ways-judge"}]}\n' >"$MODAPP/plugins/.claude-plugin/marketplace.json"
  printf '{"name":"attend","version":"0.29.0"}\n' >"$MODAPP/plugins/attend/.claude-plugin/plugin.json"
  printf '{"name":"ways-judge","version":"1.43.0"}\n' >"$MODAPP/plugins/ways-judge/.claude-plugin/plugin.json"
  printf 'engine: claude-code\n' >"$h/.config/agent-ways/agent.yaml"
  printf 'mods:\n  ways-judge:\n    install: true\n' >"$h/.config/agent-ways/config.yaml"
  cat >"$stub" <<STUB
#!/bin/sh
case "\$*" in
  "--version") echo "$1 (Claude Code)" ;;
  "plugin marketplace list --json") echo '[{"name":"agent-ways","source":"directory","path":"$MODAPP/plugins"}]' ;;
  "plugin list --json") echo '$2' ;;
  *) exit 2 ;;
esac
STUB
  chmod +x "$stub"
  export WAYS_CLAUDE_BIN=$stub
}
teardown_mod_actions() {
  rm -rf "$MODAPP" "$WORK/stub"
  rm -f "$F/home/.config/agent-ways/agent.yaml" "$F/home/.config/agent-ways/config.yaml"
  unset WAYS_CLAUDE_BIN
}

# A critical mod missing (the bottom bar's notice): Claude Code above every
# floor, and ways-judge wanted under gate.engine: claude-code and not
# installed. The healthy one has both mods installed, and no notice.
setup_degraded() { mod_app 2.1.310 "$ATTEND_ONLY"; }
teardown_degraded() { teardown_mod_actions; }
setup_degraded_mod() { setup_degraded; }
teardown_degraded_mod() { teardown_mod_actions; }
setup_healthy() { mod_app 2.1.310 '[{"id":"attend@agent-ways","version":"0.29.0","scope":"user"},{"id":"ways-judge@agent-ways","version":"1.43.0","scope":"user"}]'; }
teardown_healthy() { teardown_mod_actions; }

# The theme tab's Claude Code section (ADR-514): the ways-theme mod wanted
# and listed in the target's enabledPlugins, so installed and active.
setup_theme_claude() {
  local h=$F/home
  printf 'mods:\n  ways-theme:\n    install: true\n' >"$h/.config/agent-ways/config.yaml"
  printf '{"enabledPlugins":{"ways-theme@agent-ways":true}}\n' >"$h/.claude/settings.json"
}
teardown_theme_claude() { rm -f "$F/home/.config/agent-ways/config.yaml" "$F/home/.claude/settings.json"; }
setup_theme_claude_nights() { setup_theme_claude; }
teardown_theme_claude_nights() { teardown_theme_claude; }
setup_theme_claude_tampa() { setup_theme_claude; }
teardown_theme_claude_tampa() { teardown_theme_claude; }
setup_theme_claude_paths() { setup_theme_claude; }
teardown_theme_claude_paths() { teardown_theme_claude; }

setup_mod_states() { setup_mod_actions; }
teardown_mod_states() { teardown_mod_actions; }

# The module card (the detail pane's head on a mod, a sensor, a theme): the
# healthy mods; the shipped git sensor, described by the debug `attend` in
# $BIN; a command and an mcp sensor of the user's, each a file in attend's
# user sensor directory, last in name order; and a user theme beside the
# bundled ones.
setup_card_mod() { setup_healthy; }
teardown_card_mod() { teardown_mod_actions; }
setup_card_command_sensor() {
  local d=$F/home/.config/attend/sensors
  mkdir -p "$d"
  printf 'description: the wind at home, from Open-Meteo\nkind: command\nargv: [curl, -s, "https://api.open-meteo.com/v1/forecast?latitude=45.5&longitude=-122.7&current=wind_speed_10m"]\noutput: json\ninterval: 900\nmin_interval: 300\n' >"$d/weather.yaml"
  printf 'description: unread messages in Slack\nkind: mcp\nlane: message\nserver: slack\ntool: list_unread\nargs: { channel: general }\nstate:\n  unread: { type: number, from: "$.unread_count" }\n  user: { type: string, max: 64, from: "$.messages[0].user.name" }\ntriggers:\n  - name: new-message\n    state: { unread: rises }\n    author: user\n    magnitude: 1\n    description: "{state.unread} unread in Slack"\n' >"$d/slack-unread.yaml"
}
teardown_card_command_sensor() { rm -rf "$F/home/.config/attend/sensors"; }
setup_card_mcp_sensor() { setup_card_command_sensor; }
teardown_card_mcp_sensor() { teardown_card_command_sensor; }
setup_card_user_theme() {
  local t=$F/home/.config/agent-ways/themes
  mkdir -p "$t"
  sed -e 's/^THEME_NAME=.*/THEME_NAME="mine"/' -e 's/^THEME_LABEL=.*/THEME_LABEL="Mine"/' "$HERE/../../agent-theme/themes/nord.theme" >"$t/mine.theme"
}
teardown_card_user_theme() { rm -f "$F/home/.config/agent-ways/themes/mine.theme"; }

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
