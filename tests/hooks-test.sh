#!/usr/bin/env bash
# Hook scripts under hooks/ways, run against a temp HOME, XDG dirs and
# sessions root. Each script is an adapter over `ways hook <event>`: the
# SessionStart clear and `ways session reset` clear only a plain session id, the Stop
# hook records the last response, the post-tool scan runs postchecks,
# inject-subagent.sh injects stashed ways from every ways root, and a macro
# gets its session, scope and sessions root. The hooks call the ways binary
# at $HOME/.claude/bin/ways; the test links the build named by
# $WAYS_TEST_BIN (default tools/target/debug/ways) there.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
# WAYS_TEST_HOOKS runs the suite against another checkout's hooks, with
# WAYS_TEST_BIN naming its binary: how a new check is shown to fail before.
HOOKS="${WAYS_TEST_HOOKS:-$ROOT/hooks/ways}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then
        echo "  PASS: $1"
    else
        echo "  FAIL: $1 — expected [$2], got [$3]"
        fail=1
    fi
}

WAYS_TEST_BIN="${WAYS_TEST_BIN:-$ROOT/tools/target/debug/ways}"
if [[ ! -x "$WAYS_TEST_BIN" ]]; then
    echo "  FAIL: no ways binary at $WAYS_TEST_BIN (cargo build -p ways)"
    exit 1
fi

# Every hook here resolves its state from HOME, XDG_* and XDG_RUNTIME_DIR.
export HOME="$WORK/home" XDG_RUNTIME_DIR="$WORK/run"
export XDG_CONFIG_HOME="$WORK/config" XDG_STATE_HOME="$WORK/state"
export XDG_CACHE_HOME="$WORK/cache" XDG_DATA_HOME="$WORK/data"
unset CLAUDE_PROJECT_DIR CLAUDE_AGENT_ID
mkdir -p "$HOME/.claude/bin"
ln -s "$WAYS_TEST_BIN" "$HOME/.claude/bin/ways"
SESSIONS="$XDG_RUNTIME_DIR/claude-sessions"

seed_sessions() {
    rm -rf "$SESSIONS"
    mkdir -p "$SESSIONS/sess-a/way-epochs" "$SESSIONS/sess-b/way-epochs"
}

# clear-markers.sh with a session id clears that session alone.
seed_sessions
echo '{"session_id":"sess-a","cwd":"/srv/p"}' | bash "$HOOKS/clear-markers.sh"
check "clear-markers removes its own session" "absent" "$([[ -e $SESSIONS/sess-a ]] && echo present || echo absent)"
check "clear-markers keeps another session" "present" "$([[ -e $SESSIONS/sess-b ]] && echo present || echo absent)"

# Without a session id it has nothing to clear, so every session survives.
seed_sessions
echo '{"cwd":"/srv/p"}' | bash "$HOOKS/clear-markers.sh"
check "clear-markers without a session id keeps every session" "2" "$(find "$SESSIONS" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')"

# A session id that climbs out of the sessions root never reaches rm: the
# directory beside the root survives.
seed_sessions
mkdir -p "$XDG_RUNTIME_DIR/victim"
echo '{"session_id":"../victim"}' | bash "$HOOKS/clear-markers.sh"
check "clear-markers ignores a session id that escapes the root" "present" "$([[ -d $XDG_RUNTIME_DIR/victim ]] && echo present || echo absent)"

# The memory macro reads MEMORY.md from the dir Claude Code names the project
# by: every non-alphanumeric character becomes '-'.
mkdir -p "$HOME/.claude/projects/-srv-mcp--prod-x-y/memory"
printf 'one\ntwo\n' > "$HOME/.claude/projects/-srv-mcp--prod-x-y/memory/MEMORY.md"
out=$(PATH="$HOME/.claude/bin:/usr/bin:/bin" CLAUDE_PROJECT_DIR="/srv/mcp/_prod/x.y" bash "$HOOKS/meta/memory/macro.sh")
check "memory macro finds an underscore project's MEMORY.md" "**MEMORY.md has 2 lines.**" "${out%% Review*}"

# `ways project-slug` is the rule the macro reads; non-ASCII takes UTF-16 units.
check "ways project-slug maps a path" "-srv-mcp--prod-x-y" "$("$WAYS_TEST_BIN" project-slug /srv/mcp/_prod/x.y)"
check "ways project-slug counts UTF-16 units" "-x---" "$("$WAYS_TEST_BIN" project-slug /x/🦀)"
check "ways project-slug defaults to CLAUDE_PROJECT_DIR" "-srv-p" "$(CLAUDE_PROJECT_DIR=/srv/p "$WAYS_TEST_BIN" project-slug)"

# inject-subagent.sh emits each stashed way, from the user root
# ($XDG_CONFIG_HOME/agent-ways/ways, ADR-143) as from the core root, with
# its frontmatter stripped and its macro output in place.
way() {  # root id scope body
    mkdir -p "$1/$2"
    printf -- '---\ndescription: test way\nscope: %s\nrefire: 0.15\n---\n%s\n' "$3" "$4" \
        > "$1/$2/$(basename "$2").md"
}
way "$XDG_CONFIG_HOME/agent-ways/ways" userdom/mine "subagent" "# User way body"
way "$HOME/.claude/hooks/ways" coredom/shipped "agent, subagent" "# Core way body"
# A way with a check file that sorts before it: the way body is what injects.
way "$HOME/.claude/hooks/ways" coredom/checked "subagent" "# Checked way body"
printf -- '---\ndescription: test check\n---\n# Check text\n' > "$HOME/.claude/hooks/ways/coredom/checked/checked.check.md"
printf -- '---\ndescription: test way\nscope: subagent\nrefire: 0.15\nmacro: append\n---\n# Macro way body\n' \
    > "$(mkdir -p "$HOME/.claude/hooks/ways/coredom/withmacro" && echo "$HOME/.claude/hooks/ways/coredom/withmacro")/withmacro.md"
printf '#!/bin/bash\necho macro-output\n' > "$HOME/.claude/hooks/ways/coredom/withmacro/macro.sh"
chmod +x "$HOME/.claude/hooks/ways/coredom/withmacro/macro.sh"

inject() {  # session -> additionalContext
    mkdir -p "$SESSIONS/$1/subagent-stash"
    echo '{"ways":["userdom/mine","coredom/shipped","coredom/withmacro","coredom/checked"],"channels":["prompt","prompt","prompt","prompt"]}' \
        > "$SESSIONS/$1/subagent-stash/001.json"
    echo "{\"session_id\":\"$1\",\"agent_id\":\"agent-1\",\"cwd\":\"$WORK/project\"}" \
        | bash "$HOOKS/inject-subagent.sh" | jq -r '.hookSpecificOutput.additionalContext // empty'
}
rm -rf "$SESSIONS"
ctx=$(inject sess-sub)
expected=$'# User way body\n\n# Core way body\n\n# Macro way body\nmacro-output\n\n# Checked way body'
check "inject-subagent injects user, core and macro ways" "$expected" "$ctx"
# A second subagent in the same session gets the same ways again.
check "inject-subagent ignores the parent's refire state" "$expected" "$(inject sess-sub)"
check "inject-subagent logs each injection" "8" \
    "$(jq -r 'select(.event=="way_fired" and .scope=="subagent") | .way' "$XDG_STATE_HOME/agent-ways/events.jsonl" 2>/dev/null | wc -l | tr -d ' ')"

# Hooks that test their assembled context must stay linear in its size
# (#705, #710). In a UTF-8 locale `${ctx// /}` took 150 ms on 13 KB of ways
# and grows with the square of the size: about 10 s on this 100 KB way.
# Timed with bash's `time` keyword, which needs no GNU `date +%N`.
TIMEFORMAT=%R
under() { awk -v s="$1" -v l="$2" 'BEGIN { print (s + 0 < l) ? "yes" : "no" }'; }
big_way() {  # dir scope
    mkdir -p "$1"
    { printf -- '---\ndescription: big way\nscope: %s\nrefire: 0.15\n---\n' "$2"
      for _ in $(seq 1 2000); do printf 'Le café est prêt, and the way body runs on.\n'; done
    } > "$1/$(basename "$1").md"
}
utf8=$(locale -a 2>/dev/null | grep -iE '^(C|en_US)\.utf-?8$' | head -1)
if [[ -n "$utf8" ]]; then
    big_way "$HOME/.claude/hooks/ways/coredom/big" subagent
    mkdir -p "$SESSIONS/sess-big/subagent-stash"
    echo '{"ways":["coredom/big"],"channels":["prompt"]}' > "$SESSIONS/sess-big/subagent-stash/001.json"
    payload="{\"session_id\":\"sess-big\",\"cwd\":\"$WORK/project\"}"
    secs=$( { time ( echo "$payload" | LC_ALL="$utf8" bash "$HOOKS/inject-subagent.sh" > "$WORK/big.json" 2>/dev/null ); } 2>&1 )
    check "inject-subagent injects a 100 KB way" "2000" \
        "$(jq -r '.hookSpecificOutput.additionalContext' "$WORK/big.json" | wc -l | tr -d ' ')"
    check "inject-subagent handles a 100 KB way in under 2 s (took ${secs} s)" "yes" "$(under "$secs" 2)"

    # check-post.sh (PostToolUse) fires a way whose postcheck exits 0; the
    # 10,000-character cap still admits an oversized first body.
    big_way "$HOME/.claude/hooks/ways/coredom/bigpost" agent
    printf '#!/bin/sh\nexit 0\n' > "$HOME/.claude/hooks/ways/coredom/bigpost/postcheck.sh"
    chmod +x "$HOME/.claude/hooks/ways/coredom/bigpost/postcheck.sh"
    payload="{\"session_id\":\"sess-post\",\"cwd\":\"$WORK/project\",\"tool_name\":\"Edit\"}"
    secs=$( { time ( echo "$payload" | LC_ALL="$utf8" bash "$HOOKS/check-post.sh" > "$WORK/post.json" 2>/dev/null ); } 2>&1 )
    check "check-post injects a 100 KB way" "2000" \
        "$(jq -r '.hookSpecificOutput.additionalContext' "$WORK/post.json" | wc -l | tr -d ' ')"
    check "check-post handles a 100 KB way in under 2 s (took ${secs} s)" "yes" "$(under "$secs" 2)"
    rm -rf "$HOME/.claude/hooks/ways/coredom/bigpost"
else
    echo "  SKIP: no UTF-8 locale for the context size checks"
fi

# The code-quality macro runs on every fire of its way, SubagentStart
# included. Its process count must not grow with the repo (#705): one spawn
# of `file` and of `wc` per tracked file cost 3.5 s on a 1,600-file repo.
# Shims on PATH count those spawns over a 300-file repo.
REPO="$WORK/quality-repo" SHIMS="$WORK/shims" SPAWNS="$WORK/spawns.log"
mkdir -p "$REPO" "$SHIMS"
for tool in file wc; do
    printf '#!/bin/sh\necho %s >> "%s"\nexec %s "$@"\n' "$tool" "$SPAWNS" "$(command -v $tool)" > "$SHIMS/$tool"
    chmod +x "$SHIMS/$tool"
done
for i in $(seq 1 300); do printf 'a\nb\nc\n' > "$REPO/short$i.rs"; done
seq 1 600 > "$REPO/long.rs"
seq 1 900 > "$REPO/has space.rs"
seq 1 700 > "$REPO/café.rs"                                    # non-ASCII name
seq 1 900 > "$REPO/notes.md"                                   # excluded by scan_exclude
{ printf '\211PNG\r\n\032\n\0\0\0\rIHDR'; seq 1 600; } > "$REPO/blob.png"  # binary
{ seq 1 500; printf 'tail'; } > "$REPO/edge.rs"                # 500 lines to wc
git -C "$REPO" init -q && git -C "$REPO" add -A
out=$(cd "$REPO" && PATH="$SHIMS:$PATH" bash "$HOOKS/softwaredev/code/quality/macro.sh")
check "quality macro spawns file once" "1" "$(grep -c '^file$' "$SPAWNS")"
check "quality macro spawns wc once" "1" "$(grep -c '^wc$' "$SPAWNS")"
check "quality macro lists only long text files" \
    $'  900  has space.rs\n  700  café.rs\n  600  long.rs' "$(grep -E '^ +[0-9]+  ' <<< "$out")"

# A file `wc` cannot read gets no line from it. The macro then says nothing
# rather than pair the remaining counts with the wrong files. The shim
# simulates that by dropping wc's first line.
mkdir -p "$WORK/shims-short"
printf '#!/bin/sh\n%s "$@" | sed 1d\n' "$(command -v wc)" > "$WORK/shims-short/wc"
chmod +x "$WORK/shims-short/wc"
out=$(cd "$REPO" && PATH="$WORK/shims-short:$PATH" bash "$HOOKS/softwaredev/code/quality/macro.sh")
check "quality macro is silent when wc skips a file" "" "$out"

# With submodule.recurse set (a common global setting) the scan stays out
# of submodules: their files are not this repo's and bypass scan_exclude.
SUBSRC="$WORK/sub-src" SUPER="$WORK/super-repo"
mkdir -p "$SUBSRC" "$SUPER"
seq 1 900 > "$SUBSRC/inner.rs"
seq 1 600 > "$SUPER/long.rs"
gitq() { git -c user.name=t -c user.email=t@t -c protocol.file.allow=always -c init.defaultBranch=main "$@"; }
gitq -C "$SUBSRC" init -q && gitq -C "$SUBSRC" add -A && gitq -C "$SUBSRC" commit -qm sub
gitq -C "$SUPER" init -q && gitq -C "$SUPER" submodule -q add "$SUBSRC" sub 2>/dev/null
gitq -C "$SUPER" add -A
check "submodule fixture has a gitlink" "sub" "$(git -C "$SUPER" ls-files -s | awk '$1 == "160000" { print $4 }')"
out=$(cd "$SUPER" && GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=submodule.recurse GIT_CONFIG_VALUE_0=true \
    bash "$HOOKS/softwaredev/code/quality/macro.sh")
check "quality macro skips submodule files under submodule.recurse" \
    '  600  long.rs' "$(grep -E '^ +[0-9]+  ' <<< "$out")"
# check-config-updates.sh: only the native XDG app source is checked. The
# cache path is fixed by the hook (and read by `ways show core`), so keep the
# user's real one and put it back.
CFG_CACHE="/tmp/.claude-config-update-state-$(id -u)"
CFG_BAK="$WORK/cfg-cache.bak"
[[ -f "$CFG_CACHE" ]] && cp "$CFG_CACHE" "$CFG_BAK"
restore_cfg_cache() { if [[ -f "$CFG_BAK" ]]; then cp "$CFG_BAK" "$CFG_CACHE"; else rm -f "$CFG_CACHE"; fi; }
# An abort between here and the end still puts the user's cache back.
trap 'restore_cfg_cache; rm -rf "$WORK"' EXIT

rm -f "$CFG_CACHE"
bash "$ROOT/hooks/check-config-updates.sh"
check "config-updates: no app source writes no cache" "absent" "$([[ -e $CFG_CACHE ]] && echo present || echo absent)"

APP="$XDG_DATA_HOME/agent-ways"
mkdir -p "$APP"
git -C "$APP" init -q
git -C "$APP" -c user.email=t@t -c user.name=t commit -q --allow-empty -m one
git -C "$APP" -c user.email=t@t -c user.name=t commit -q --allow-empty -m two
git -C "$APP" remote add origin https://github.com/aaronsb/agent-ways.git
git -C "$APP" update-ref refs/remotes/origin/main HEAD
git -C "$APP" reset -q --hard HEAD~1
# A fresh fetch stamp keeps the hook off the network.
printf 'fetched=%s\ntype=clone\nbehind=9\n' "$(date +%s)" > "$CFG_CACHE"
bash "$ROOT/hooks/check-config-updates.sh"
check "config-updates: native app source is stamped native" "native" "$(sed -n 's/^type=//p' "$CFG_CACHE")"
check "config-updates: behind count comes from the app source" "1" "$(sed -n 's/^behind=//p' "$CFG_CACHE")"
check "config-updates: the native cache names the app dir" "$APP" "$(sed -n 's/^repo=//p' "$CFG_CACHE")"
check "config-updates: show core nudges with ways manage update" "yes" \
    "$("$WAYS_TEST_BIN" show core --session cfg-sess 2>/dev/null | grep -q 'ways manage update' && echo yes || echo no)"

# An origin that is not upstream stamps behind=0: no nudge.
git -C "$APP" remote set-url origin https://example.com/fork/agent-ways.git
bash "$ROOT/hooks/check-config-updates.sh"
check "config-updates: a non-upstream origin stamps behind=0" "0" "$(sed -n 's/^behind=//p' "$CFG_CACHE")"
restore_cfg_cache
rm -rf "$APP"

# ── ways hook (#702): the hooks are adapters over the binary ──────────────

# `ways session reset` and the SessionStart clear share one rule: a session id that
# climbs out of the sessions root removes nothing.
seed_sessions
mkdir -p "$XDG_RUNTIME_DIR/victim"
"$WAYS_TEST_BIN" session reset --session ../victim --confirm >/dev/null 2>&1
check "ways session reset ignores a session id that escapes the root" "present" "$([[ -d $XDG_RUNTIME_DIR/victim ]] && echo present || echo absent)"
"$WAYS_TEST_BIN" session reset --session sess-a --confirm >/dev/null 2>&1
check "ways session reset clears the named session" "absent|present" \
    "$([[ -e $SESSIONS/sess-a ]] && echo present || echo absent)|$([[ -e $SESSIONS/sess-b ]] && echo present || echo absent)"

# Stop records Claude's last response in the session's state, where the
# SessionStart clear removes it; a stop the hook itself continued writes
# nothing, and a turn that ended without text clears the record.
assistant() { jq -cn --arg t "$1" '{type:"assistant",message:{content:[{type:"text",text:$t}]}}'; }
TRANSCRIPT="$WORK/transcript.jsonl"
{ assistant "older reply"; echo '{"type":"user","message":{"content":"q"}}'; assistant "The Reply"; } > "$TRANSCRIPT"
rm -rf "$SESSIONS"
stop() { jq -cn --arg s "$1" --arg t "$TRANSCRIPT" --argjson a "${2:-false}" \
    '{session_id:$s,transcript_path:$t,stop_hook_active:$a}' | bash "$HOOKS/check-response.sh"; }
stop sess-stop true
check "stop hook skips a stop it continued" "absent" "$([[ -e $SESSIONS/sess-stop/response-context.json ]] && echo present || echo absent)"
stop sess-stop
check "stop hook records the last response in the session" "The Reply" \
    "$(jq -r .context "$SESSIONS/sess-stop/response-context.json" 2>/dev/null)"
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}' >> "$TRANSCRIPT"
stop sess-stop
check "stop hook clears the record after a turn without text" "absent" "$([[ -e $SESSIONS/sess-stop/response-context.json ]] && echo present || echo absent)"

# The post-tool scan: a fired way's envelope names the event that ran it; a
# postcheck sees the sessions root; a project way's postcheck shadows the
# core way's of the same id, as the way itself does.
CORE="$HOME/.claude/hooks/ways"
PROJ="$WORK/project"
postcheck() {  # dir body
    printf '#!/bin/sh\n%s\n' "$2" > "$1/postcheck.sh"
    chmod +x "$1/postcheck.sh"
}
way "$CORE" postdom/fires agent "# Fires body"
postcheck "$CORE/postdom/fires" "echo \"\$WAYS_SESSIONS_ROOT\" > \"$WORK/postcheck-root\"; exit 0"
way "$CORE" postdom/shadowed agent "# Core shadowed body"
postcheck "$CORE/postdom/shadowed" "exit 0"
way "$PROJ/.claude/ways" postdom/shadowed agent "# Project shadowed body"
postcheck "$PROJ/.claude/ways/postdom/shadowed" "exit 1"
post() {  # session event
    jq -cn --arg s "$1" --arg e "$2" --arg c "$PROJ" '{session_id:$s,hook_event_name:$e,cwd:$c,tool_name:"Bash"}' \
        | bash "$HOOKS/check-post.sh"
}
out=$(post sess-post-1 PostToolUseFailure)
check "post-tool envelope names PostToolUseFailure" "PostToolUseFailure" "$(jq -r .hookSpecificOutput.hookEventName <<< "$out")"
check "post-tool fires the way whose postcheck exits 0" "yes" "$(grep -q 'Fires body' <<< "$out" && echo yes || echo no)"
check "a project postcheck shadows the core one" "no" "$(grep -q 'shadowed body' <<< "$out" && echo yes || echo no)"
check "a postcheck sees WAYS_SESSIONS_ROOT" "$SESSIONS" "$(cat "$WORK/postcheck-root" 2>/dev/null)"
# `ways manage repair` projects the core root as a symlink into the app; the
# scan walks through it (find(1) does not descend a symlinked start point).
mv "$CORE" "$WORK/core-real" && ln -s "$WORK/core-real" "$CORE"
out=$(post sess-post-2 PostToolUse)
check "post-tool finds postchecks under a symlinked core root" "yes" "$(grep -q 'Fires body' <<< "$out" && echo yes || echo no)"
rm "$CORE" && mv "$WORK/core-real" "$CORE"
rm -rf "$CORE/postdom" "$PROJ/.claude/ways/postdom"

# One broken way does not stop the scan: aa/broken has no `refire:` (an
# error on the fire path), and zz/ok after it still fires.
mkdir -p "$CORE/aa/broken"
printf -- '---\ndescription: broken way\nscope: agent\n---\n# Broken body\n' > "$CORE/aa/broken/broken.md"
postcheck "$CORE/aa/broken" "exit 0"
way "$CORE" zz/ok agent "# Ok body"
postcheck "$CORE/zz/ok" "exit 0"
out=$(post sess-post-5 PostToolUse)
check "post-tool fires zz/ok past a broken way" "yes" "$(grep -q 'Ok body' <<< "$out" && echo yes || echo no)"
rm -rf "$CORE/aa" "$CORE/zz"

# A hung postcheck does not hang the hook: the scan gives every postcheck one
# deadline, and the fast one's way still fires.
way "$CORE" hangdom/slow agent "# Slow body"
postcheck "$CORE/hangdom/slow" "sleep 8; exit 0"
way "$CORE" hangdom/fast agent "# Fast body"
postcheck "$CORE/hangdom/fast" "exit 0"
secs=$( { time ( post sess-post-6 PostToolUse > "$WORK/hang.json" ); } 2>&1 )
check "post-tool returns under 4 s beside a hung postcheck (took ${secs} s)" "yes" "$(under "$secs" 4)"
check "post-tool still fires the fast postcheck's way" "yes" "$(grep -q 'Fast body' "$WORK/hang.json" && echo yes || echo no)"
check "post-tool drops the hung postcheck's way" "no" "$(grep -q 'Slow body' "$WORK/hang.json" && echo yes || echo no)"
rm -rf "$CORE/hangdom"

# A project-local postcheck is project code, gated like a project macro: it
# runs only for a project listed in ~/.claude/trusted-project-macros.
way "$PROJ/.claude/ways" projdom/local agent "# Project local body"
postcheck "$PROJ/.claude/ways/projdom/local" "touch \"$WORK/project-postcheck-ran\"; exit 0"
rm -f "$WORK/project-postcheck-ran" "$HOME/.claude/trusted-project-macros"
out=$(post sess-post-3 PostToolUse)
check "an untrusted project's postcheck does not run" "absent" "$([[ -e $WORK/project-postcheck-ran ]] && echo present || echo absent)"
echo "$PROJ" > "$HOME/.claude/trusted-project-macros"
out=$(post sess-post-4 PostToolUse)
check "a trusted project's postcheck runs" "present" "$([[ -e $WORK/project-postcheck-ran ]] && echo present || echo absent)"
check "a trusted project's postcheck fires its way" "yes" "$(grep -q 'Project local body' <<< "$out" && echo yes || echo no)"
rm -rf "$PROJ/.claude/ways/projdom" "$HOME/.claude/trusted-project-macros" "$WORK/project-postcheck-ran"

# The tool lane (ADR-704): check-tool.sh fires a `tools:` way on its tool
# and phase, in the envelope of the real event, and skips a failed call.
mkdir -p "$CORE/tooldom/approved"
printf -- '---\ntools: ^ExitPlanMode$\ntool_phase: post\nscope: agent\nrefire: 0.15\n---\n# Approved body\n' \
    > "$CORE/tooldom/approved/approved.md"
tool() {  # session event tool
    jq -cn --arg s "$1" --arg e "$2" --arg t "$3" --arg c "$PROJ" '{session_id:$s,hook_event_name:$e,cwd:$c,tool_name:$t,tool_input:{}}' \
        | bash "$HOOKS/check-tool.sh"
}
check "check-tool.sh is silent before the tool runs on a post-only way" "" "$(tool sess-tool-1 PreToolUse ExitPlanMode)"
check "check-tool.sh skips a failed call" "" "$(tool sess-tool-1 PostToolUseFailure ExitPlanMode)"
check "check-tool.sh is silent for another tool" "" "$(tool sess-tool-1 PostToolUse Workflow)"
out=$(tool sess-tool-1 PostToolUse ExitPlanMode)
check "check-tool.sh envelope names PostToolUse" "PostToolUse" "$(jq -r .hookSpecificOutput.hookEventName <<< "$out")"
check "check-tool.sh fires the way on its tool" "yes" "$(grep -q 'Approved body' <<< "$out" && echo yes || echo no)"
rm -rf "$CORE/tooldom"

# #689: a macro run for a subagent gets the parent's session id, its own
# agent id, the scope it runs for and the sessions root, and the markdown
# queue macros leave the parent's queue for the parent.
mkdir -p "$CORE/envdom/env"
printf -- '---\ndescription: test way\nscope: subagent\nrefire: 0.15\nmacro: append\n---\n# Env way body\n' > "$CORE/envdom/env/env.md"
printf '#!/bin/bash\necho "env=$CLAUDE_SESSION_ID|$WAYS_SCOPE|$CLAUDE_AGENT_ID|$WAYS_SESSIONS_ROOT|$CLAUDE_PROJECT_DIR"\n' > "$CORE/envdom/env/macro.sh"
mkdir -p "$CORE/documentation/markdown"
cp -R "$HOOKS/documentation/markdown/density" "$CORE/documentation/markdown/density"
rm -rf "$SESSIONS"
mkdir -p "$SESSIONS/sess-env/subagent-stash" "$SESSIONS/sess-env/markdown-density"
printf '%s\tdocs/x.md\t500\t3\t6\t20\n' "$(date +%s)" > "$SESSIONS/sess-env/markdown-density/pending"
echo '{"ways":["envdom/env","documentation/markdown/density"],"channels":["prompt","prompt"]}' > "$SESSIONS/sess-env/subagent-stash/001.json"
ctx=$(jq -cn --arg c "$PROJ" '{session_id:"sess-env",agent_id:"agent-9",cwd:$c}' \
    | bash "$HOOKS/inject-subagent.sh" | jq -r '.hookSpecificOutput.additionalContext // empty')
check "a subagent macro gets session, scope, agent, root and project" \
    "env=sess-env|subagent|agent-9|$SESSIONS|$PROJ" "$(grep '^env=' <<< "$ctx")"
check "a subagent leaves the parent's density queue" "present" \
    "$([[ -s $SESSIONS/sess-env/markdown-density/pending ]] && echo present || echo absent)"
rm -rf "$CORE/envdom" "$CORE/documentation"

# #932: the attend macro reports this session's attend state through the
# CLI. A stand-in `attend` prints canned JSON from $FIX. PATH holds it and
# the system tools the macro needs, so no installed attend is reachable.
ATTEND_SHIMS="$WORK/attend-shims" FIX="$WORK/attend-fix"
mkdir -p "$ATTEND_SHIMS" "$FIX"
cat > "$ATTEND_SHIMS/attend" <<'SHIM'
#!/bin/sh
case "$*" in
    "listen status --json") f=listen ;;
    "channel --joined --json") f=channels ;;
    "message unread --json") f=unread ;;
    *) echo "$*" >> "$FIX/../attend-unexpected"; exit 2 ;;
esac
[ -e "$FIX/$f" ] || exit 1
cat "$FIX/$f"
SHIM
chmod +x "$ATTEND_SHIMS/attend"
# The system tools the macro and the shim use, and nothing else, so no
# attend installed on this host is reachable.
SYSBIN="$WORK/attend-sysbin" BASH_BIN=$(command -v bash)
mkdir -p "$SYSBIN"
for t in cat cut grep head paste sed; do ln -s "$(command -v "$t")" "$SYSBIN/$t"; done
attend_macro() {  # scope listen-json channels-json enabled-plugins [unread-json]
    rm -f "$FIX"/*
    [[ -n "$2" ]] && printf '%s\n' "$2" > "$FIX/listen"
    printf '%s\n' "$3" > "$FIX/channels"
    [[ -n "${5:-}" ]] && printf '%s\n' "$5" > "$FIX/unread"
    FIX="$FIX" WAYS_SCOPE="$1" CLAUDE_AGENT_ID="${AGENT_ID:-}" WAYS_ENABLED_PLUGINS="$4" PATH="$ATTEND_SHIMS:$SYSBIN" \
        "$BASH_BIN" "$HOOKS/softwaredev/environment/attend/macro.sh" 2>&1
}
listen() {  # listening host run_live
    printf '{"session_id":"s1","resolved":true,"listening":%s,"via":[],"host":%s,"run_live":%s}' "$1" "$2" "$3"
}
MOD_ON=$'other@x\nattend@agent-ways'
START_MOD='call `attend_listen` when that tool is listed; otherwise start `Monitor: attend run`'
START_MON='start `Monitor: attend run`'
DEPLOY='[{"name":"deploy","pinned":false}]'
check "attend macro: listening through the mod" \
    $'**Status**: listening through the attend mod. Send with `attend_send` and `attend_reply`; do not start Monitor.\n**Channels**: this session hears project, #open, #deploy' \
    "$(attend_macro agent "$(listen true '"mod"' false)" "$DEPLOY" "$MOD_ON")"
check "attend macro: attend run under Monitor" \
    $'**Status**: `attend run` is live under Monitor for this session.\n**Channels**: this session hears project, #open' \
    "$(attend_macro agent "$(listen true null true)" '[]' '')"
check "attend macro: attend run beside a listening mod" \
    $'**Status**: listening through the attend mod, and `attend run` is also live under Monitor. ADR-406 item 9 says not to run both: the mod is enough, so the Monitor can stop.\n**Channels**: this session hears project, #open' \
    "$(attend_macro agent "$(listen true '"mod"' true)" '[]' "$MOD_ON")"
check "attend macro: turn end only, attend plugin enabled" \
    "**Status**: listening at the end of each turn only; nothing wakes an idle session. For that, $START_MOD."$'\n**Channels**: this session hears project, #open' \
    "$(attend_macro agent "$(listen true null false)" '[]' "$MOD_ON")"
check "attend macro: turn end only, no plugins enabled" \
    "**Status**: listening at the end of each turn only; nothing wakes an idle session. For that, $START_MON."$'\n**Channels**: this session hears project, #open' \
    "$(attend_macro agent "$(listen true null false)" '[]' '')"
check "attend macro: turn end only, attend from another marketplace" \
    "**Status**: listening at the end of each turn only; nothing wakes an idle session. For that, $START_MON."$'\n**Channels**: this session hears project, #open' \
    "$(attend_macro agent "$(listen true null false)" '[]' 'attend@elsewhere')"
check "attend macro: not listening, attend plugin enabled" \
    "**Status**: not listening. If the user asks to use attend, $START_MOD."$'\n**Unread**: 3 for this session' \
    "$(attend_macro agent "$(listen false null false)" '[]' "$MOD_ON" '{"unread":3,"held_back":0,"listening":false,"rooms":[]}')"
check "attend macro: not listening, no attend plugin" \
    "**Status**: not listening. If the user asks to use attend, $START_MON."$'\n**Channels**: this session joined #deploy; nothing is delivered until it listens' \
    "$(attend_macro agent "$(listen false null false)" "$DEPLOY" '' '{"unread":0}')"
check "attend macro: a failing unread adds nothing" \
    "**Status**: not listening. If the user asks to use attend, $START_MOD." \
    "$(attend_macro agent "$(listen false null false)" '[]' "$MOD_ON")"
# Under a subagent the state is the parent's, reported with no action.
check "attend macro: subagent, parent listening through the mod" \
    $'**Status**: the parent session is listening through the attend mod.\n**Channels**: the parent session hears project, #open, #deploy' \
    "$(attend_macro subagent "$(listen true '"mod"' false)" "$DEPLOY" "$MOD_ON")"
check "attend macro: subagent, parent run beside the mod" \
    $'**Status**: the parent session is listening through the attend mod, and its `attend run` is also live under Monitor.\n**Channels**: the parent session hears project, #open' \
    "$(attend_macro subagent "$(listen true '"mod"' true)" '[]' "$MOD_ON")"
check "attend macro: subagent, parent run under Monitor" \
    $'**Status**: the parent session\'s `attend run` is live under Monitor.\n**Channels**: the parent session hears project, #open' \
    "$(attend_macro subagent "$(listen true null true)" '[]' '')"
check "attend macro: subagent, parent at turn end only" \
    $'**Status**: the parent session is listening at the end of each turn only.\n**Channels**: the parent session hears project, #open' \
    "$(attend_macro subagent "$(listen true null false)" '[]' "$MOD_ON")"
check "attend macro: subagent, parent not listening" \
    $'**Status**: the parent session is not listening. Listening is the head agent\'s call.\n**Unread**: 2 for the parent session' \
    "$(attend_macro subagent "$(listen false null false)" '[]' "$MOD_ON" '{"unread":2}')"
# A subagent's own tool call reaches the macro as WAYS_SCOPE=agent with its
# CLAUDE_AGENT_ID set: still the parent's state, still no action.
check "attend macro: a subagent's own call, parent not listening" \
    $'**Status**: the parent session is not listening. Listening is the head agent\'s call.' \
    "$(AGENT_ID=a123 attend_macro agent "$(listen false null false)" '[]' "$MOD_ON")"
check "attend macro: a subagent's own call, parent listening through the mod" \
    $'**Status**: the parent session is listening through the attend mod.\n**Channels**: the parent session hears project, #open' \
    "$(AGENT_ID=a123 attend_macro agent "$(listen true '"mod"' false)" '[]' "$MOD_ON")"
check "attend macro: says nothing for an unresolved session" "" \
    "$(attend_macro agent '{"session_id":"x","resolved":false,"listening":false,"via":[],"host":null,"run_live":false}' '[]' "$MOD_ON")"
check "attend macro: says nothing when listen status fails" "" "$(attend_macro agent '' '[]' "$MOD_ON")"
# The hint takes --commit only from a ways that has it (ADR-511); a release
# from before the flag, or no ways at all, gets the bare form.
mkdir -p "$WORK/ways-commit" "$WORK/ways-old"
printf '#!/bin/sh\necho "      --commit  Make the changes"\n' > "$WORK/ways-commit/ways"
printf '#!/bin/sh\necho "      --dry-run  Show what would run"\n' > "$WORK/ways-old/ways"
chmod +x "$WORK/ways-commit/ways" "$WORK/ways-old/ways"
check "attend macro: attend not installed, a ways with --commit" \
    '**Note**: attend is not installed. Run `ways manage update --commit` to install it.' \
    "$(PATH="$WORK/ways-commit:$SYSBIN" "$BASH_BIN" "$HOOKS/softwaredev/environment/attend/macro.sh" 2>&1)"
check "attend macro: attend not installed, a ways from before --commit" \
    '**Note**: attend is not installed. Run `ways manage update` to install it.' \
    "$(PATH="$WORK/ways-old:$SYSBIN" "$BASH_BIN" "$HOOKS/softwaredev/environment/attend/macro.sh" 2>&1)"
check "attend macro: calls attend only with the argv it expects" "" "$(cat "$WORK/attend-unexpected" 2>/dev/null)"

# The github macro fits under the 3 s macro deadline, and a check whose
# call timed out or failed is never shown as missing. A stand-in `gh`
# answers each endpoint with canned JSON and logs each network call to
# $GH_LOG. GH_SLOW names an endpoint that hangs (`all` hangs every call),
# GH_404 is a space-separated list that answers 404, GH_SECURITY is the
# security policy answer, and GH_NOT_REPO makes gh refuse the directory.
GH_SHIMS="$WORK/gh-shims"
mkdir -p "$GH_SHIMS"
cat > "$GH_SHIMS/gh" <<'SHIM'
#!/bin/sh
case "$1 $2" in
    "config get") echo me; exit 0 ;;
    "api repos/:owner/:repo") key=repo ;;
    "api repos/:owner/:repo/community/profile") key=community ;;
    "api repos/:owner/:repo/contributors") key=contributors ;;
    "api repos/:owner/:repo/labels") key=labels ;;
    "api graphql") key=security ;;
    "api repos/:owner/:repo/branches/"*) key=protection ;;
    *) echo "unexpected: $*" >> "$GH_LOG"; exit 2 ;;
esac
echo "$key" >> "$GH_LOG"
[ -n "$GH_NOT_REPO" ] && { echo "unable to expand placeholder in path: failed to run git: fatal: not a git repository" >&2; exit 1; }
[ "$GH_SLOW" = all ] || [ "$GH_SLOW" = "$key" ] && sleep 10
case " $GH_404 " in *" $key "*) echo "gh: Not Found (HTTP 404)" >&2; exit 1 ;; esac
case $key in
    repo) echo '{"description":"d","topics":["t"],"permissions":{"push":true,"admin":true},"full_name":"o/r"}' ;;
    community) echo '{"files":{"readme":{},"license":{},"code_of_conduct":{},"contributing":{},"issue_template":{},"pull_request_template":{}}}' ;;
    contributors) echo '[{"login":"a"}]' ;;
    labels) echo '[{"default":false}]' ;;
    security) echo "${GH_SECURITY:-true}" ;;
    protection) echo https://api.github.com/protection ;;
esac
exit 0
SHIM
chmod +x "$GH_SHIMS/gh"
export GH_LOG="$WORK/gh.log"
gh_macro() {  # env assignments... ; output in $WORK/gh.out, seconds in $secs
    secs=$( { time ( cd "$WORK" && env "$@" PATH="${GH_PATH:-$GH_SHIMS:$PATH}" \
        bash "$HOOKS/softwaredev/delivery/github/macro.sh" > "$WORK/gh.out" 2>/dev/null ); } 2>&1 )
}
health() { grep '^\*\*Repo health\*\*\|^| Branch protection' "$WORK/gh.out"; }
gh_macro GH_SLOW=all
check "github macro under a hung gh ends under 3 s (took ${secs} s)" "yes" "$(under "$secs" 3)"
check "github macro under a hung gh says the API is unreachable" "**Note**: Could not reach GitHub API" "$(cat "$WORK/gh.out")"
gh_macro GH_SLOW=repo
check "github macro: a hung repo check is an unreachable API" "**Note**: Could not reach GitHub API" "$(cat "$WORK/gh.out")"
gh_macro GH_NOT_REPO=1
check "github macro: a refused repo check is not a GitHub repo" \
    "**Note**: Not a GitHub repository - GitHub commands won't work" "$(cat "$WORK/gh.out")"
gh_macro GH_SLOW=protection
check "github macro: a hung check ends under 3 s (took ${secs} s)" "yes" "$(under "$secs" 3)"
check "github macro: a hung check is couldn't check, never missing" \
    $'**Repo health**: 10/12 — missing: README badges (you have push access)\n**Repo health**: couldn\'t check Branch protection (the GitHub call failed or timed out)' "$(health)"
gh_macro GH_404=protection
check "github macro: a 404 check is missing" \
    "**Repo health**: 10/12 — missing: Branch protection, README badges (you have push access)" "$(health)"
gh_macro GH_SLOW=community
check "github macro: a hung community profile leaves its six checks unknown" \
    $'**Repo health**: 5/12 — missing: README badges (you have push access)\n**Repo health**: couldn\'t check README, License, Code of conduct, Contributing guide, Issue templates, PR template (the GitHub call failed or timed out)' \
    "$(health)"
# In the full table (four or more missing), an unknown row says so.
gh_macro GH_SLOW=community GH_404="protection labels" GH_SECURITY=false
check "github macro: the full table shows a hung check as couldn't check" "| README | couldn't check | — |" \
    "$(grep '^| README |' "$WORK/gh.out")"
# Without `timeout` (stock macOS) the macro still runs, its calls unbounded.
GH_NOTIMEOUT="$WORK/gh-notimeout"
mkdir -p "$GH_NOTIMEOUT"
for t in bash cat cut git grep head jq mktemp rm sed sleep sort tr wc; do ln -s "$(command -v "$t")" "$GH_NOTIMEOUT/$t"; done
check "the no-timeout PATH has no timeout" "" "$(PATH="$GH_SHIMS:$GH_NOTIMEOUT" command -v timeout)"
rm -f "$GH_LOG"
GH_PATH="$GH_SHIMS:$GH_NOTIMEOUT" gh_macro GH_X=1
check "github macro without timeout reads the repo" \
    "**Repo health**: 11/12 — missing: README badges (you have push access)" "$(health)"
# Six network calls at most: more concurrent gh processes stall on some hosts.
check "github macro makes six calls, each one it expects" \
    "community contributors labels protection repo security" "$(sort "$GH_LOG" | tr '\n' ' ' | sed 's/ $//')"

# A macro past the 3 s deadline is killed: its way is delivered without the
# macro's output, and the timeout is logged by the way's name.
mkdir -p "$CORE/slowdom/slow"
printf -- '---\ndescription: test way\nscope: subagent\nrefire: 0.15\nmacro: append\n---\n# Slow way body\n' > "$CORE/slowdom/slow/slow.md"
printf '#!/bin/bash\necho early\nsleep 20\necho late\n' > "$CORE/slowdom/slow/macro.sh"
rm -rf "$SESSIONS"
mkdir -p "$SESSIONS/sess-slow/subagent-stash"
echo '{"ways":["slowdom/slow"],"channels":["prompt"]}' > "$SESSIONS/sess-slow/subagent-stash/001.json"
secs=$( { time ( jq -cn --arg c "$PROJ" '{session_id:"sess-slow",agent_id:"agent-s",cwd:$c}' \
    | bash "$HOOKS/inject-subagent.sh" | jq -r '.hookSpecificOutput.additionalContext // empty' > "$WORK/slow.ctx" ); } 2>&1 )
check "a macro past the deadline is cut off near 3 s (took ${secs} s)" "yes" "$(under "$secs" 5)"
check "a macro past the deadline adds nothing to its way" "# Slow way body" "$(cat "$WORK/slow.ctx")"
check "a macro past the deadline is logged by its way" "slowdom/slow" \
    "$(jq -r 'select(.event=="macro_timeout") | .way' "$XDG_STATE_HOME/agent-ways/events.jsonl" 2>/dev/null)"
rm -rf "$CORE/slowdom"

# attend-drain-stop.sh passes the drain's stdout and stdin through, and ends
# the turn normally (exit 0) on any failure from attend: Claude Code reads a
# Stop hook's exit 2 as a block, and clap exits 2 on a usage error (ADR-413).
drain_with() {  # stub-body -> "exit:stdout"
    local dir="$WORK/drain-bin"
    rm -rf "$dir"; mkdir -p "$dir"
    printf '#!/bin/bash\n%s\n' "$1" > "$dir/attend"; chmod +x "$dir/attend"
    local out rc=0
    out=$(echo '{"stop_hook_active":false}' | PATH="$dir:/usr/bin:/bin" bash "$HOOKS/attend-drain-stop.sh" 2>/dev/null) || rc=$?
    echo "$rc:$out"
}
check "the drain hook passes the drain's output and stdin through" '0:{"decision":"block","stdin":{"stop_hook_active":false}}' \
    "$(drain_with '[[ "$*" == "message inbox --drain --format hook" ]] || exit 9; echo "{\"decision\":\"block\",\"stdin\":$(cat)}"')"
check "a usage error from attend ends the turn normally" "0:" \
    "$(drain_with 'echo "error: unrecognized subcommand" >&2; exit 2')"
check "any other failure from attend ends the turn normally" "0:" "$(drain_with 'echo partial; exit 1')"
check "no attend on PATH ends the turn normally" "0:" \
    "$(rc=0; out=$(PATH="$WORK/empty-bin" /bin/bash "$HOOKS/attend-drain-stop.sh" </dev/null) || rc=$?; echo "$rc:$out")"

exit $fail
