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

# The scenarios: name | command | keys. Keys are tmux key names, and
# text:<chars> types literally. A new screen state adds a line here.
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
mod        | ways settings mod --project $PS                |
"

python3 "$TOOLS/fixture.py" "$WORK" >/dev/null
fixture_env

want() { [ $# -eq 1 ] || [[ " ${*:2} " == *" $1 "* ]]; }
trim() { local s=$1; s=${s#"${s%%[![:space:]]*}"}; printf '%s' "${s%"${s##*[![:space:]]}"}"; }

n=0
while IFS='|' read -r name cmd keys; do
  name=$(trim "$name")
  [ -n "$name" ] || continue
  want "$name" "$@" || continue
  read -ra argv <<<"$cmd"
  for size in $SIZES; do
    c=${size%x*} r=${size#*x}
    out=$WORK/$size
    mkdir -p "$out"
    s=v-$name
    $TH down "$s" >/dev/null 2>&1 || true
    $TH launch "$s" --cols "$c" --rows "$r" --font "$FONT" -- "${argv[@]}" >/dev/null
    sleep 2
    for k in $keys; do
      case $k in
        text:*) $TH send "$s" -l "${k#text:}" ;;
        *) $TH send "$s" "$k" ;;
      esac
      sleep 0.6
    done
    sleep 1
    $TH text "$s" >"$out/$name.txt"
    HOME=$REALHOME $TH shot "$s" --out "$out/$name.png" >/dev/null
    $TH down "$s" >/dev/null
    echo "$out/$name.png"
    n=$((n + 1))
  done
done <<<"$SCENARIOS"
[ "$n" -gt 0 ] || { echo "no scenario matched: $*" >&2; exit 1; }
