#!/usr/bin/env bash
# statusline.sh: the agent segment reads `attend whoami --display`, falls
# back to the whoami table on older attend builds, and drops out without
# attend. The git segments parse remotes and mark a dirty tree. Sourcing
# prints nothing, and segments survive a strict-mode caller. Stub `attend`
# binaries stand in for a live session.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SCRIPT="$ROOT/statusline.sh"
WORK=$(mktemp -d)
BASH_BIN=$(command -v bash)
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

stub() {  # dir body
    mkdir -p "$WORK/$1"
    printf '#!%s\n%s\n' "$BASH_BIN" "$2" > "$WORK/$1/attend"
    chmod +x "$WORK/$1/attend"
}

# Current attend: --display prints the name alone.
stub current '[[ "$*" == "whoami --display" ]] && { echo "Chaucer-2"; exit 0; }; exit 1'

# attend before --display: clap rejects the flag; the table carries the name
# between ANSI styling.
stub table 'if [[ "$*" == "whoami" ]]; then
printf "  \033[1mValue\033[0m\n  session   abc\n  display   Chaucer\n"; exit 0; fi
echo "error: unexpected argument" >&2; exit 2'

# attend that hangs past SL_TIMEOUT.
stub hang 'sleep 5'

# PATH with no attend: the system directories, plus bash's own where it
# lives elsewhere (NixOS).
BARE="$(dirname "$BASH_BIN"):/usr/bin:/bin"

name_with() {  # PATH
    PATH="$1" bash -c "source '$SCRIPT'; sl_agent_name"
}

check "current attend uses --display" "Chaucer-2" "$(name_with "$WORK/current:$BARE")"
check "older attend falls back to the table" "Chaucer" "$(name_with "$WORK/table:$BARE")"
check "no attend gives no name" "" "$(name_with "$BARE")"
check "no attend drops the agent segment" "" \
    "$(PATH="$BARE" bash -c "source '$SCRIPT'; sl_agent")"
check "agent segment carries a trailing space" "🤖 Chaucer-2 " \
    "$(PATH="$WORK/current:$BARE" bash -c "source '$SCRIPT'; sl_agent")"
check "sourcing prints nothing" "" "$(bash -c "source '$SCRIPT'")"

check "a strict-mode caller survives a failing attend" "survived" \
    "$(cd "$WORK" && PATH="$WORK/table:$BARE" bash -c "set -euo pipefail; source '$SCRIPT'; sl_agent_name >/dev/null; sl_git; sl_remote; echo survived" 2>/dev/null)"

if command -v timeout > /dev/null 2>&1 || command -v gtimeout > /dev/null 2>&1; then
    start=$(date +%s%N)
    out=$(PATH="$WORK/hang:$BARE" SL_TIMEOUT=1 bash -c "source '$SCRIPT'; sl_agent_name")
    elapsed_ms=$(( ($(date +%s%N) - start) / 1000000 ))
    check "a hung attend gives no name" "" "$out"
    check "a hung attend costs one timeout (${elapsed_ms}ms)" "yes" \
        "$( (( elapsed_ms < 1800 )) && echo yes || echo no)"
fi

# zsh -f skips the user's startup files, which could put a real attend
# back on PATH.
if command -v zsh > /dev/null 2>&1; then
    check "sourcing from zsh prints nothing" "" "$(zsh -f -c "source '$SCRIPT'")"
    check "sourcing from zsh under nounset prints nothing" "" "$(zsh -f -u -c "source '$SCRIPT'" 2>&1)"
    check "segments run under zsh" "🤖 Chaucer-2 " \
        "$(PATH="$WORK/current:$BARE" zsh -f -c "source '$SCRIPT'; sl_agent")"
fi

# Git segments, in a throwaway repo.
REPO="$WORK/repo"
git init -q -b main "$REPO"
git -C "$REPO" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
in_repo() {  # function
    (cd "$REPO" && PATH="$BARE" bash -c "source '$SCRIPT'; $1")
}
check "clean branch" "🔀 main " "$(in_repo sl_git)"
touch "$REPO/dirty"
check "dirty tree is starred" "🔀 main* " "$(in_repo sl_git)"
rm "$REPO/dirty"
git -C "$REPO" checkout -q --detach
check "detached HEAD shows the short sha" "🔀 $(git -C "$REPO" rev-parse --short HEAD) " "$(in_repo sl_git)"
check "no origin, no remote segment" "" "$(in_repo sl_remote)"

git -C "$REPO" remote add origin placeholder
for pair in \
    "git@github.com:o/r.git|o/r" \
    "https://github.com/o/r.git|o/r" \
    "https://github.com/o/r|o/r" \
    "https://github.com/o/r/|o/r" \
    "ssh://git@github.com/o/r.git|o/r" \
    "ssh://git@github.com:22/o/r.git|o/r" \
    "https://x-access-token:SECRET@github.com/o/r.git|o/r"; do
    url=${pair%|*}
    git -C "$REPO" remote set-url origin "$url"
    check "remote $url" "📡 ${pair#*|} " "$(in_repo sl_remote)"
done
git -C "$REPO" remote set-url origin garbage
check "an unparseable remote shows nothing" "" "$(in_repo sl_remote)"

line=$(cd "$WORK" && PATH="$WORK/current:$BARE" bash "$SCRIPT")
if [[ "$line" =~ ^🤖\ Chaucer-2\ 📁\ [^\ ]+\ \|\ 🕐\ [0-9]{2}:[0-9]{2}$ ]]; then
    echo "  PASS: executing renders the full line outside a repo"
else
    echo "  FAIL: executing renders the full line outside a repo — got [$line]"
    fail=1
fi

exit $fail
