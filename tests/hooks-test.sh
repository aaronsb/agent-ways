#!/usr/bin/env bash
# Hook scripts under hooks/ways, run against a temp HOME, XDG dirs and
# sessions root: clear-markers.sh clears only its own session's state, the
# memory macro finds MEMORY.md under Claude Code's project slug, and
# inject-subagent.sh injects stashed ways from every ways root. The hooks
# call the ways binary at $HOME/.claude/bin/ways; the test links the build
# named by $WAYS_TEST_BIN (default tools/target/debug/ways) there.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOOKS="$ROOT/hooks/ways"
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
seq 1 900 > "$REPO/notes.md"                                   # excluded by scan_exclude
{ printf '\211PNG\r\n\032\n\0\0\0\rIHDR'; seq 1 600; } > "$REPO/blob.png"  # binary
{ seq 1 500; printf 'tail'; } > "$REPO/edge.rs"                # 500 lines to wc
git -C "$REPO" init -q && git -C "$REPO" add -A
out=$(cd "$REPO" && PATH="$SHIMS:$PATH" bash "$HOOKS/softwaredev/code/quality/macro.sh")
check "quality macro spawns file once" "1" "$(grep -c '^file$' "$SPAWNS")"
check "quality macro spawns wc once" "1" "$(grep -c '^wc$' "$SPAWNS")"
check "quality macro lists only long text files" \
    $'  900  has space.rs\n  600  long.rs' "$(grep -E '^ +[0-9]+  ' <<< "$out")"

exit $fail
