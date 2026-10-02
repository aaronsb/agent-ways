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
check "config-updates: show core nudges with ways update" "yes" \
    "$("$WAYS_TEST_BIN" show core --session cfg-sess 2>/dev/null | grep -q 'ways update' && echo yes || echo no)"

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
# `ways reconcile` projects the core root as a symlink into the app; the
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

exit $fail
