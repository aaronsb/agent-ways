#!/usr/bin/env bash
# statusline.sh: the agent segment reads `attend whoami --display`, falls
# back to the whoami table on older attend builds, and drops out without
# attend. Sourcing the script defines the segments and prints nothing.
# Stub `attend` binaries stand in for a live session.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SCRIPT="$ROOT/statusline.sh"
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

stub() {  # dir body
    mkdir -p "$WORK/$1"
    printf '#!/usr/bin/env bash\n%s\n' "$2" > "$WORK/$1/attend"
    chmod +x "$WORK/$1/attend"
}

# Current attend: --display prints the name alone.
stub current '[[ "$*" == "whoami --display" ]] && { echo "Chaucer-2"; exit 0; }; exit 1'

# attend before --display: clap rejects the flag; the table carries the name
# between ANSI styling.
stub table 'if [[ "$*" == "whoami" ]]; then
printf "  \033[1mValue\033[0m\n  session   abc\n  display   Chaucer\n"; exit 0; fi
echo "error: unexpected argument" >&2; exit 2'

# PATH with no attend at all: only the system directories.
BARE="/usr/bin:/bin"

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

if command -v zsh > /dev/null 2>&1; then
    check "sourcing from zsh prints nothing" "" "$(zsh -c "source '$SCRIPT'")"
fi

line=$(cd "$WORK" && PATH="$WORK/current:$BARE" bash "$SCRIPT")
if [[ "$line" =~ ^🤖\ Chaucer-2\ 📁\ [^\ ]+\ \|\ 🕐\ [0-9]{2}:[0-9]{2}$ ]]; then
    echo "  PASS: executing renders the full line outside a repo"
else
    echo "  FAIL: executing renders the full line outside a repo — got [$line]"
    fail=1
fi

exit $fail
