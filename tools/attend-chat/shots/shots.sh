#!/usr/bin/env bash
# Drive each state of attend-chat's left column and menus (ADR-405) with
# real keys and clicks, at 80x25 and 120x35, and save a PNG and a text
# capture of each, against a synthetic attend cache. Needs tmux and
# fontconfig.
#
#   cargo build --manifest-path tools/Cargo.toml -p attend-chat -p attend -p tui-harness
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
#
# The harness's tmux server is shared by every harness root on the machine,
# so a session name is global. Each run names its sessions ac-<pid>-<scenario>:
# two runs, in one checkout or two, never share a name, and a run never
# downs or launches onto another's. The run downs its own session on every
# exit (a failed wait, an error, ^C, a TERM), and at the start downs the
# leftovers of earlier runs of this root whose pid is gone (a KILL or a
# power cut leaves one). Nothing else on the server is touched.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$REPO/tools/target}/debug
WORK=${SHOTS_DIR:-$REPO/target/tui-verify/attend-chat}
# The fixture lives outside the checkout: a cwd under a managed worktree
# (`.claude/worktrees/`) normalizes to the worktree's origin, and every
# fixture agent would collapse into that one identity.
F=$(mktemp -d "${TMPDIR:-/tmp}/attend-chat-shots.XXXXXX")
SESSION=
cleanup() {
  [ -z "$SESSION" ] || $TH down "$SESSION" >/dev/null 2>&1 || true
  rm -rf "$F"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
FONT=${FONT:-JetBrains Mono}
SIZES=${SIZES:-80x25 120x35}
REALHOME=$HOME
TH="$BIN/tui-harness --dir $WORK/th"
mkdir -p "$WORK"

# Down the sessions of earlier runs of this root that no process owns now.
sweep() {
  local d name pid
  for d in "$WORK"/th/sessions/ac-[0-9]*-*; do
    [ -d "$d" ] || continue
    name=${d##*/}
    pid=${name#ac-}
    pid=${pid%%-*}
    kill -0 "$pid" 2>/dev/null || $TH down "$name" >/dev/null 2>&1 || true
  done
}
sweep

# The scenarios: name | fixture | steps. A step is a tmux key name, or
#   text:CHARS     typed literally
#   click:TEXT     a left click on the first cell showing TEXT
#   side:TEXT      the same, on TEXT in the left column only
#   feed:TEXT      the same, on TEXT right of the left column
#   last:TEXT      the same, on the last cell showing TEXT
#   rclick: rside: rfeed: rlast:  the same with the right button
#   wait:TEXT      until TEXT is on the screen
#   gone:TEXT      until TEXT is off the screen
#   resize:COLS    the terminal narrowed (or widened) to COLS columns
#   paste:TEXT     a bracketed paste of TEXT, \n for a newline
#   rawpaste:TEXT  TEXT sent as typed, newlines and all, with no brackets
#   bigpaste:N     a bracketed paste of N x characters
# The fixture is base, crowd (fifteen more agents), many (24 more channels,
# so the last channel row sits at the foot), own (the operator in
# #deploy), long (a fourth agent whose project name outruns any row),
# dm (a direct message from alpha to the operator), trays (direct messages
# in every project's tray: one the operator sent beta in an earlier
# session, and the agents' DMs with each other), proj (alpha, where the
# chat runs, a project whose .claude/ways.yaml sets
# attend_mod.listening: active, and beta a git checkout) or plain (no
# Nerd Font). An agent's name comes from the temp
# path, and a long one cuts its project, so the quiet agent is clicked by
# the column's first `○`, its row being the agents' last.
SCENARIOS="
side         | base  | wait:Agents
off          | base  | wait:Agents F3 gone:Agents
click        | base  | wait:#infra click:#infra click:#infra wait:pick
tab          | base  | wait:#deploy click:#deploy click:#deploy wait:pick
side-show    | base  | wait:Agents side:#infra
side-arrows  | base  | wait:Agents side:#open Down Down Up wait:_column_
side-enter   | base  | wait:Agents side:#infra Enter wait:pick_one:_#infra
side-menu    | base  | wait:Agents side:#infra side:#infra wait:pick
key-dock     | base  | wait:Agents M-2 M-2 wait:pick
foot-keys    | many  | wait:Agents F2 Down End Up Enter wait:pick
foot-click   | many  | wait:Agents F2 Down End side:#infra side:#infra wait:pick
keys         | base  | wait:Agents F2 Down Down wait:↑_↓_move
menu-keys    | base  | wait:Agents F2 Down Down Enter wait:pick
focus-resize | base  | wait:Agents F2 Down Down wait:↑_↓_move resize:70 gone:Agents
agent-menu   | base  | wait:Agents side:○ wait:DMs side:○ wait:Address
crowd        | crowd | wait:Agents F2 Down Up Up Up Up Up Up Up Up Up Up Up Up
common       | base  | wait:Agents click:≡ wait:Hide_column
hide-here    | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:hide_the_messages_in_⋯all?
hide-done    | base  | wait:#open click:#open click:#open wait:Hide_messages_here Enter wait:y_hide click:y_hide wait:the_bus_keeps_them
hide-chan    | base  | wait:Agents side:#deploy side:#deploy wait:pick click:Hide_messages_here wait:hide_the_messages_in_#deploy?
paste        | base  | wait:Agents text:see: paste:line_one\\nline_two\\nline_three
rawpaste     | base  | wait:Agents rawpaste:one\\ntwo
plain        | plain | wait:Agents
own-row      | own   | wait:operator click:#deploy click:#deploy wait:pick Escape gone:pick side:operator wait:Leave_#deploy
quiet-send   | base  | wait:Agents rside:○ wait:Address Down Enter text:are_you_there Enter wait:no_heartbeat
paste-asked  | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:hide_the_messages_in_⋯all? paste:yes_please\\nsecond_line wait:kept
ask          | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:y_hide
ask-yes      | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:y_hide click:y_hide wait:the_bus_keeps_them
ask-no       | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:y_hide click:n_keep wait:kept
ask-feed     | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:y_hide click:signing wait:kept
ask-row      | base  | wait:⋯all click:⋯all wait:Hide_messages_here Enter wait:y_hide side:#infra wait:kept
delete-older | base  | wait:#open click:#open click:#open wait:Delete_read Down Enter wait:delete_older_than
ask-long     | base  | wait:#open click:#open click:#open wait:Delete_read Down Enter wait:delete_older_than click:All wait:y_delete
delete-done  | base  | wait:#open click:#open click:#open wait:Delete_read Down Enter wait:delete_older_than click:All wait:y_delete click:y_delete wait:Undo_delete_on_its_menu
delete-undo  | base  | wait:#open click:#open click:#open wait:Delete_read Down Enter wait:delete_older_than click:All wait:y_delete click:y_delete wait:Undo_delete_on_its_menu click:#open wait:Undo_delete click:Undo_delete wait:restored
delete-custom | base | wait:#open click:#open click:#open wait:Delete_read Down Enter wait:delete_older_than click:Custom… wait:older_than: text:soon Enter wait:not_a_duration
dm-delete    | dm    | wait:→_you rfeed:→_you wait:Show_DMs Enter wait:DMs rside:DMs wait:Delete_read click:Delete_read wait:delete_older_than
tomb-menu    | base  | wait:Agents rside:○ wait:Tombstone…
tomb-ask     | base  | wait:Agents rside:○ wait:Tombstone… click:Tombstone… wait:y_tombstone
tomb-said    | base  | wait:Agents rside:○ wait:Tombstone… click:Tombstone… wait:y_tombstone click:y_tombstone wait:tombstoned
review-tomb  | base  | wait:Agents rside:○ wait:Review_DMs click:Review_DMs wait:read_only rside:DMs wait:Tombstone…
quit-ask     | base  | wait:Agents F2 q wait:quit?
menu-row     | base  | wait:Agents side:#infra side:#infra wait:pick side:#deploy gone:pick
menu-tab     | base  | wait:Agents side:#infra side:#infra wait:pick click:#open gone:pick
prompt-click | base  | wait:Agents text:draft C-n wait:new_channel text:abc click:Agents gone:new_channel
entry-name   | base  | wait:Agents text:draft C-n wait:new_channel_name text:release
entry-bad    | base  | wait:Agents text:draft C-n wait:new_channel_name text:no_spaces Enter wait:no_spaces
entry-desc   | base  | wait:Agents side:#deploy side:#deploy wait:pick click:Describe wait:describe_#deploy text:_for_staging
entry-tab    | base  | wait:Agents text:draft C-n wait:new_channel_name text:abc click:#open gone:new_channel_name
entry-row    | base  | wait:Agents click:+ wait:new_channel_name side:#infra gone:new_channel_name text:abc
esc-twice    | base  | wait:Agents side:#infra side:#infra wait:pick Escape Escape wait:pick_one:_attend-chat
common-quit  | base  | wait:Agents click:≡ wait:Quit
quit-click   | base  | wait:Agents text:half click:≡ wait:Quit click:Quit wait:Quit_and_discard
menu-waits   | base  | wait:Agents side:#deploy side:#deploy wait:needs_ADR-404_D4 Down
theme-sub    | base  | wait:Agents click:≡ wait:Theme click:Theme wait:theme_for
keys-sub     | base  | wait:Agents click:≡ wait:Keybinding click:Keybinding wait:keybinding_set
remove-sub   | base  | wait:Agents side:#deploy side:#deploy wait:pick click:Remove_agent wait:remove_from
settings     | base  | wait:Agents click:≡ wait:Settings click:Settings wait:listening_=
settings-layer | proj | wait:Agents click:≡ wait:Settings click:Settings wait:listening_= click:listening_= wait:write_to
settings-value | proj | wait:Agents click:≡ wait:Settings click:Settings wait:listening_= click:listening_= wait:write_to click:project_ wait:current
settings-set | proj  | wait:Agents click:≡ wait:Settings click:Settings wait:listening_= click:listening_= wait:write_to click:project_ wait:current click:dormant wait:dormant_·_alpha/.claude/ways.yaml
settings-chat | base | wait:Agents click:≡ wait:Settings click:Settings wait:listening_= click:mouse_= wait:current
tab-closes   | base  | wait:Agents M-2 M-2 wait:pick click:rollout gone:pick
guard-name   | base  | wait:Agents C-n wait:new_channel_name text:abc C-c wait:being_typed
guard-draft  | base  | wait:Agents text:agg C-c wait:Quit_and_discard
guard-typed  | base  | wait:Agents text:agg C-c wait:Quit_and_discard text:xyz C-c
guard-back   | base  | wait:Agents text:agg C-c wait:Quit_and_discard Escape gone:Quit_and_discard wait:>_agg
rclick-row   | base  | wait:Agents rside:#infra wait:pick_one:_#infra
rclick-agent | base  | wait:Agents rside:beta wait:Address
rclick-name  | base  | wait:signing rfeed:gamma wait:Address
flag-click   | base  | wait:Agents M-2 wait:rollout_is_green last:#deploy wait:pick_one:_#deploy
flag-merged  | base  | wait:Agents last:#open wait:pick_one:_#open
key-help     | base  | wait:Agents F1 wait:any_other_key
key-help-nav | base  | wait:Agents BTab BTab wait:↑_↓_move F1 wait:any_other_key
header       | base  | wait:signing_off
header-long  | long  | wait:keeps_the_time
header-human | own   | wait:watching_the
header-pick  | base  | wait:signing_off click:signing_off
header-chan  | base  | wait:Agents M-2 wait:rollout_is_green
quit-entry   | base  | wait:Agents text:half C-n wait:new_channel_name text:abc click:≡ wait:Quit click:Quit wait:Quit_and_discard
too-long     | base  | wait:Agents bigpaste:65537 wait:xxxxxxxxxx Enter wait:over_attend's_limit
focus-compose | base  | wait:Agents wait:Enter_send
focus-view   | base  | wait:Agents F2 wait:▸⋯all
focus-open   | base  | wait:Agents F2 Left wait:▸#open
focus-plus   | base  | wait:Agents F2 Left Left wait:▸+
focus-tab    | base  | wait:Agents M-2 F2 wait:▸#deploy
focus-common | base  | wait:Agents text:x Down wait:▸≡
focus-agent  | base  | wait:Agents F2 Down wait:_column_
focus-row    | base  | wait:Agents F2 Down End Up wait:_column_
focus-feed   | base  | wait:Agents BTab wait:_feed_
focus-msg    | base  | wait:Agents BTab Up Up wait:select
enter-view   | base  | wait:Agents F2 Enter wait:pick_one:_⋯all
enter-row    | base  | wait:Agents F2 Down End Up Enter wait:pick_one:_#infra
enter-agent  | base  | wait:Agents F2 Down Enter wait:Add_agent_to_chat
enter-feed   | base  | wait:Agents BTab Enter wait:message_from
enter-plus   | base  | wait:Agents F2 Left Left Enter wait:new_channel_name
add-menu     | base  | wait:Agents rside:beta wait:Add_agent_to_chat
add-pick     | base  | wait:Agents rside:beta wait:Add_agent_to_chat click:Add_agent_to_chat wait:add_to_channel
add-done     | base  | wait:Agents rside:● wait:Add_agent_to_chat click:Add_agent_to_chat wait:add_to_channel Enter wait:invite
narrow-bar   | many  | wait:Agents resize:60 wait:⋯all
arrow-empty  | base  | wait:Agents Up text:1x wait:>_1x
q-asks       | base  | wait:Agents BTab BTab q wait:quit?
walk-back    | base  | wait:Agents text:x Down Tab Tab Tab Tab wait:_column_
bar-active   | base  | wait:Agents M-2 wait:rollout
tab-numbers  | base  | wait:Agents M-2 wait:rollout F3 gone:Agents wait:1_#open
tab-narrow   | base  | wait:Agents M-2 wait:rollout resize:70 gone:Agents wait:1_#open
digit-agent  | base  | wait:Agents F2 Down wait:_column_ text:2 wait:>_2
esc-tab-out  | base  | wait:Agents BTab BTab wait:_column_ text:hello Tab wait:▸≡ Escape wait:pick_one:_attend-chat Escape gone:pick_one wait:_column_
bar-narrow   | base  | wait:Agents M-2 wait:rollout resize:60 wait:⋯all
digit-row    | base  | wait:Agents F2 Down End 2 wait:rollout
digit-typed  | base  | wait:Agents F2 2 wait:>_2
tab-leaves   | base  | wait:Agents text:hello Tab wait:▸≡
feed-type    | base  | wait:Agents BTab wait:_feed_ q wait:>_q
esc-feed     | base  | wait:Agents text:draft Escape wait:_feed_
esc-menu     | base  | wait:Agents Escape wait:_feed_ Escape wait:pick_one:_attend-chat
esc-back     | base  | wait:Agents BTab BTab Escape wait:pick_one:_attend-chat Escape gone:pick wait:_column_
esc-feed-back | base | wait:Agents Escape Escape wait:pick_one:_attend-chat Escape gone:pick wait:_feed_
msg-menu     | base  | wait:Agents BTab Up wait:select Enter wait:message_from
msg-quote    | base  | wait:Agents BTab Up wait:select Enter wait:message_from click:Quote wait:>_>
msg-copy     | base  | wait:Agents BTab Up wait:select Enter wait:message_from click:Copy wait:clipboard
tab-nomatch  | base  | wait:Agents text:hi_@zzz Tab wait:▸≡
tab-once     | base  | wait:Agents text:#arch Tab wait:#archive Tab wait:▸≡
esc-hide     | base  | wait:Agents BTab BTab Escape wait:Hide_column Down Down Down Enter gone:pick wait:_feed_
add-chan     | base  | wait:Agents side:#infra side:#infra wait:pick click:Add_agent wait:add_to_#infra
dm-open      | dm    | wait:→_you rfeed:→_you wait:Show_DMs Enter wait:DMs
dm-compose   | dm    | wait:→_you rfeed:→_you wait:Show_DMs Enter wait:DMs text:looks_good,_push_it Enter wait:sent_→
dm-menu      | dm    | wait:→_you rfeed:→_you wait:Show_DMs Enter wait:DMs rfeed:→_you wait:Hide_DMs
dm-hide      | dm    | wait:→_you rfeed:→_you wait:Show_DMs Enter wait:DMs side:DMs wait:Hide_DMs click:Hide_DMs gone:pick_one
dm-quiet     | base  | wait:Agents side:○ wait:DMs text:are_you_there Enter wait:no_heartbeat
dm-earlier   | trays | wait:Agents_3/4 side:beta wait:sent_before_this_chat
review-open  | trays | wait:Agents_3/4 rside:alpha wait:Review_DMs click:Review_DMs wait:read_only
review-type  | trays | wait:Agents_3/4 rside:alpha wait:Review_DMs click:Review_DMs wait:read_only text:hello BTab
review-menu  | trays | wait:Agents_3/4 rside:alpha wait:Review_DMs click:Review_DMs wait:read_only side:DMs wait:Hide_review
review-both  | trays | wait:Agents_3/4 side:alpha wait:DMs rside:alpha wait:Review_DMs click:Review_DMs wait:read_only
"

# The fixture: three claudes (alpha and beta live, gamma quiet), #deploy
# with beta in it, an empty #infra, and #archive, which _groups.yaml still
# names after its directory was swept. The human is `operator`. `crowd`
# adds fifteen agents, every other one live; `own` puts the operator in
# #deploy, with a message there; `many` adds #ch01 to #ch24; `plain` the
# plain shape; `dm` a direct message from alpha to the operator; `trays`
# that, a direct message the operator sent beta in an earlier session, in
# beta's tray, and direct messages between the agents in their trays. No
# attend config is written: the mouse is on by default.
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
  if [ "$1" = long ]; then
    sessions+=("eeeeeeee-0000-4000-8000-000000000005:$F/work/the-delta-project-with-a-name-far-too-long-for-a-row")
    mkdir -p "$F/work/the-delta-project-with-a-name-far-too-long-for-a-row"
    touch "$cache/heartbeat/eeeeeeee-0000-4000-8000-000000000005"
  fi
  if [ "$1" = many ]; then
    for n in $(seq -w 1 24); do mkdir -p "$sig/@ch$n"; done
  fi
  # gamma's record names a pid no process holds: it has exited, so its
  # row offers Tombstone… and attend accepts it.
  for s in "${sessions[@]}"; do
    local pid=$i
    [ "${s%%:*}" = "$C" ] && pid=4194301
    printf '{"pid":%d,"sessionId":"%s","cwd":"%s"}\n' "$pid" "${s%%:*}" "${s#*:}" > "$F/home/.claude/sessions/$i.json"
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
  dm() { # dir id age from cwd kind sender-id to text: a direct message, with its envelope
    printf '%s|%s|%s|from_kind:%s|from_id:%s|to:%s|%s\n' "$4" "$(basename "$5")" "$5" "$6" "$7" "$8" "$9" > "$1/$2.signal"
    touch -d "@$((now - $3))" "$1/$2.signal"
  }
  msg "$sig/_broadcast" s1 900 "claude:$C" "$F/work/gamma" "signing off for today"
  msg "$sig/_broadcast" s2 600 "claude:$A" "$F/work/alpha" "starting the theme port; the legend moves first"
  msg "$sig/@deploy" s3 300 "claude:$B" "$F/work/beta" "rollout is green on staging"
  [ "$1" = own ] && msg "$sig/@deploy" s4 120 "external:operator@kitty" "$F/work/alpha" "watching the rollout"
  if [ "$1" = dm ] || [ "$1" = trays ]; then
    mkdir -p "$sig/_people/operator"
    msg "$sig/_people/operator" s6 450 "claude:$A" "$F/work/alpha" "the legend moved; want a look before I push?"
  fi
  if [ "$1" = trays ]; then
    local ta tb
    ta=$sig/$(tray "$F/work/alpha") tb=$sig/$(tray "$F/work/beta")
    mkdir -p "$ta" "$tb"
    dm "$tb" s7 800 "external:operator@kitty" "$F/work/alpha" human operator "$B" "sent before this chat: is staging green?"
    dm "$tb" s8 400 "claude:$A" "$F/work/alpha" agent "$A" "$B" "the legend moved; can you check the tab bar?"
    dm "$ta" s9 350 "claude:$B" "$F/work/beta" agent "$B" "$A" "checked: the bar holds"
    dm "$tb" s10 300 "claude:$C" "$F/work/gamma" agent "$C" "$B" "gamma to beta, not alpha's"
  fi
  [ "$1" = long ] && msg "$sig/_broadcast" s5 60 "claude:eeeeeeee-0000-4000-8000-000000000005" "$F/work/the-delta-project-with-a-name-far-too-long-for-a-row" "a project name longer than any row: the band cuts it and keeps the time"
  printf 'theme:\n  active: nord\n  shape: %s\n' "$shape" > "$F/config/agent-ways/config.yaml"
  if [ "$1" = proj ]; then
    mkdir -p "$F/work/alpha/.claude" "$F/work/beta/.git"
    printf 'attend_mod:\n  listening: active\n' > "$F/work/alpha/.claude/ways.yaml"
  fi
}

# The tray of the project at PATH: its attend key, `claude_sessions::attend_key`.
tray() { # path
  python3 -I -c '
import sys
p = sys.argv[1]
h = 0
for u in p.encode("utf-16-le").decode("utf-16-le"):
    for unit in [ord(u)] if ord(u) < 0x10000 else [0xD800 + ((ord(u) - 0x10000) >> 10), 0xDC00 + ((ord(u) - 0x10000) & 0x3FF)]:
        h = ((h << 5) - h + unit) & 0xFFFFFFFF
if h >= 1 << 31:
    h -= 1 << 32
n, d = abs(h), "0123456789abcdefghijklmnopqrstuvwxyz"
b = d[0] if n == 0 else ""
while n:
    b, n = d[n % 36] + b, n // 36
slug = "".join(c if c.isascii() and c.isalnum() else "-" * (2 if ord(c) >= 0x10000 else 1) for c in p)
assert len(slug) <= 200, "a fixture path this long needs the slug cut"
print(f"{slug}-{b}")
' "$1"
}

# The build's own attend leads PATH, so the cleanup items (Delete read
# messages for everyone…, Tombstone…) run the verbs this checkout has.
launch() { # name cols rows
  (cd "$F/work/alpha" && env -u CLAUDE_CONFIG_DIR -u CLAUDE_PROJECT_DIR -u CLAUDE_CODE_SESSION_ID HOME="$F/home" USER=operator \
    XDG_CACHE_HOME="$F/cache" XDG_CONFIG_HOME="$F/config" XDG_STATE_HOME="$F/state" XDG_DATA_HOME="$F/data" \
    PATH="$BIN:$PATH" \
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
# left of the column's rule, so a name in the column and not in the feed;
# with `feed`, the first right of it; with `last`, the last on the screen.
cell() { # name text [side|feed|last]
  $TH text "$1" | python3 -I -c '
import sys
want, where = sys.argv[1], (sys.argv[2:] or ["any"])[0]
lines = sys.stdin.read().split("\n")
rows = list(enumerate(lines, 1))
if where == "last":
    rows.reverse()
for row, line in rows:
    rule = line.find("│")
    start = rule + 1 if where == "feed" and rule >= 0 else 0
    col = line.rfind(want) if where == "last" else line.find(want, start)
    if col < 0 or (where == "side" and not 0 <= col < rule) or (where == "feed" and rule < 0):
        continue
    print(col + 1, row)
    break
else:
    sys.exit(1)
' "${@:2}"
}

# The harness has no resize verb: its private tmux server resizes the window.
resize() { # name cols
  local s
  s=$(tmux -L agent-ways-tui list-sessions -F '#S' | grep -Fx "tui-$1" | head -1)
  tmux -L agent-ways-tui resize-window -t "$s" -x "$2"
}

step() { # name step
  local n=$1 k=$2 at arg
  arg=${k#*:}
  arg=${arg//_/ }
  case $k in
    text:*) $TH send "$n" -l "$arg" ;;
    click:* | side:* | feed:* | last:* | rclick:* | rside:* | rfeed:* | rlast:*)
      local kind=${k%%:*} b=0
      case $kind in r*) b=2; kind=${kind#r} ;; esac
      at=$(cell "$n" "$arg" "$kind") || { echo "$n: no '$arg' to click" >&2; return 1; }
      $TH send "$n" -l $'\e[<'"$b;${at% *};${at#* }M"$'\e[<'"$b;${at% *};${at#* }m" ;;
    wait:*) poll "$n" "'$arg'" has "$arg"; return ;;
    gone:*) poll "$n" "'$arg' to go" lacks "$arg"; return ;;
    resize:*) resize "$n" "$arg" ;;
    paste:*) $TH send "$n" -l $'\e[200~'"$(printf '%b' "$arg")"$'\e[201~' ;;
    rawpaste:*) $TH send "$n" -l "$(printf '%b' "$arg")" ;;
    bigpaste:*) # in pieces: tmux refuses one send-keys this long
      local left=$arg piece
      $TH send "$n" -l $'\e[200~'
      while [ "$left" -gt 0 ]; do
        piece=$((left < 4000 ? left : 4000))
        $TH send "$n" -l "$(head -c "$piece" /dev/zero | tr '\0' x)"
        left=$((left - piece))
      done
      $TH send "$n" -l $'\e[201~' ;;
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
  for size in $SIZES; do
    # Each size from the same state: a scenario that sends writes the cache.
    fixture "$(trim "$kind")"
    c=${size%x*} r=${size#*x}
    out=$WORK/$size
    mkdir -p "$out"
    s=ac-$$-$name
    SESSION=$s
    launch "$s" "$c" "$r"
    for k in $steps; do
      step "$s" "$k"
    done
    $TH text "$s" >"$out/$name.txt"
    HOME=$REALHOME $TH shot "$s" --out "$out/$name.png" >/dev/null
    $TH down "$s" >/dev/null
    SESSION=
    echo "$out/$name.png"
    n=$((n + 1))
  done
done <<<"$SCENARIOS"
[ "$n" -gt 0 ] || { echo "no scenario matched: $*" >&2; exit 1; }
