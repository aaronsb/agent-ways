#!/usr/bin/env bash
# The installer prints the staged report every `ways manage` verb prints
# (ADR-511): one ruled header per stage, a mark per outcome, plain text in a
# pipe, under NO_COLOR and with TERM dumb or empty, colour when forced. Run
# against a sandboxed HOME and XDG tree with a stand-in app: its Makefile's
# `setup` writes a `bin/ways` stub, or fails when BUILD_FAILS is set. The stub's
# `reconcile` fails when RECONCILE_FAILS is set, its `status` reports no engine
# when ENGINE_OFF is set, and its `reconcile --help` lacks --commit when
# OLD_WAYS is set, as a release from before ADR-511 does.
#
# The bootstrap is checked under this bash, and under bash 3.2 when a local
# `bash:3.2` Docker image is present (stock macOS bash; nothing is pulled).

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0
ESC=$'\033'

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}
has() {  # name needle haystack
    if [[ "$3" == *"$2"* ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — [$2] not in:"; echo "$3" | sed 's/^/      /'; fail=1; fi
}
lacks() {  # name needle haystack
    if [[ "$3" != *"$2"* ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — [$2] found in:"; echo "$3" | sed 's/^/      /'; fail=1; fi
}

# Each header exactly once, in this order: "yes", or what went wrong.
stage_order() {  # output, then header names
    local out="$1" prev=0 h n count
    shift
    for h in "$@"; do
        count=$(grep -c "^── $h " <<<"$out")
        [[ "$count" == 1 ]] || { echo "\"$h\" appears $count times"; return; }
        n=$(grep -n "^── $h " <<<"$out" | cut -d: -f1)
        (( n > prev )) || { echo "\"$h\" is out of order"; return; }
        prev=$n
    done
    echo yes
}

# The stand-in source: the real installer, the marker file it checks for, a
# one-binary suite and a Makefile that "builds" it.
make_source() {
    local src="$1"
    mkdir -p "$src/scripts" "$src/hooks" "$src/tools"
    cp "$ROOT/scripts/install.sh" "$src/scripts/install.sh"
    printf '#!/bin/sh\n' > "$src/hooks/check-config-updates.sh"
    printf 'ways\n' > "$src/tools/suite-bins"
    cat > "$src/Makefile" <<'MK'
# agent-ways stand-in
setup:
	@echo "Pre-built binary installed."
	@[ -z "$$BUILD_FAILS" ] || { echo "cargo: build failed" >&2; exit 1; }
	@mkdir -p bin
	@printf '#!/bin/sh\ncase "$$1 $$2" in\n  "reconcile --help") [ -n "$$OLD_WAYS" ] || echo "      --commit  Make the changes"; i=0; while [ $$i -lt 4000 ]; do echo "      --option-$$i  more help text, enough to outlast a pipe buffer"; i=$$((i+1)); done ;;\n  reconcile*) echo "linked skills (absent)"; [ -z "$$RECONCILE_FAILS" ] || { echo "refused: skills is a real directory" >&2; exit 1; } ;;\n  status*) [ -n "$$ENGINE_OFF" ] || echo "Engine: embedding" ;;\nesac\nexit 0\n' > bin/ways
	@chmod +x bin/ways
MK
    git -C "$src" init -q
    git -C "$src" -c user.name=t -c user.email=t@e add -A
    git -C "$src" -c user.name=t -c user.email=t@e commit -qm one
}

# Run the installer from the source of sandbox `name` (made on first use);
# print its output. Extra arguments are env assignments.
install() {
    local name="$1"; shift
    local base="$WORK/$name"
    [[ -d "$base/src" ]] || make_source "$base/src"
    mkdir -p "$base/home"
    (cd "$base/src" && env -i PATH="${SHIM_PATH:+$SHIM_PATH:}$PATH" HOME="$base/home" XDG_DATA_HOME="$base/home/.local/share" \
        XDG_CONFIG_HOME="$base/home/.config" XDG_CACHE_HOME="$base/home/.cache" TERM=xterm "$@" \
        bash scripts/install.sh ${INSTALL_ARGS:-} </dev/null 2>&1)
}

echo "installer report:"
out=$(install plain); rc=$?
check "a fresh install exits 0" "0" "$rc"
check "one header per stage, in order" "yes" "$(stage_order "$out" "Stage" "Build" "Link" "Reproject" "Semantic matching" "Judge" "Done")"
has "the build's output sits under its stage" "    Pre-built binary installed." "$out"
has "the reconcile's output sits under its stage" "    linked skills (absent)" "$out"
has "a stage outcome is marked" "  ✓ projection reconciled" "$out"
has "the link stage counts what it linked" "  ✓ suite linked onto PATH in" "$out"
has "the engine check is marked" "  ✓ the embedding engine runs" "$out"
check "plain text in a pipe" "0" "$(grep -c "$ESC" <<<"$out")"

echo "colour:"
out=$(install forced CLICOLOR_FORCE=1)
check "colour when CLICOLOR_FORCE is set" "yes" "$(grep -q "$ESC\[" <<<"$out" && echo yes || echo no)"
out=$(install force-color FORCE_COLOR=1)
check "colour when FORCE_COLOR is set" "yes" "$(grep -q "$ESC\[" <<<"$out" && echo yes || echo no)"
out=$(install no-color CLICOLOR_FORCE=1 NO_COLOR=1)
check "NO_COLOR wins over a forced colour" "0" "$(grep -c "$ESC" <<<"$out")"
out=$(install dumb CLICOLOR_FORCE=1 TERM=dumb)
check "TERM=dumb wins over a forced colour" "0" "$(grep -c "$ESC" <<<"$out")"
out=$(install no-term CLICOLOR_FORCE=1 TERM=)
check "an empty TERM wins over a forced colour" "0" "$(grep -c "$ESC" <<<"$out")"

echo "degraded engine:"
out=$(install engine-off CLICOLOR_FORCE=1 ENGINE_OFF=1); rc=$?
check "a degraded engine still exits 0" "0" "$rc"
has "the recovery card is shown" "Semantic (meaning-based) way-matching is OFF" "$out"
lacks "the card prints colour, not escape text" '\033[' "$out"

echo "failed build:"
out=$(install build-fails BUILD_FAILS=1); rc=$?
check "a build with no binary exits 1" "1" "$rc"
has "the build stage warns" "  ⚠ the build had issues" "$out"
has "the link stage says nothing was linked" "  ⚠ no suite binary was built, so nothing was linked onto PATH" "$out"
has "the reproject stage fails" "  ✗ ways binary not built" "$out"

echo "refused projection:"
out=$(install refused RECONCILE_FAILS=1); rc=$?
check "a refused projection exits 3" "3" "$rc"
has "the refusal is a failed outcome" "  ✗ Projection stopped." "$out"
has "the fix says --commit when the app's ways has it" "ways manage repair --commit --force" "$out"
out=$(install refused-old RECONCILE_FAILS=1 OLD_WAYS=1)
has "an app ways without --commit gets the bare form" "ways manage repair --force" "$out"
lacks "and no --commit it would reject" "repair --commit" "$out"

echo "update in place:"
# A git shim answers the two calls that make the app an agent-ways clone of
# the upstream and pull it; everything else goes to the real git.
REAL_GIT=$(command -v git)
mkdir -p "$WORK/shim"
cat > "$WORK/shim/git" <<SHIM
#!/bin/sh
for a in "\$@"; do
  case "\$a" in
    get-url) echo "https://github.com/aaronsb/agent-ways"; exit 0 ;;
    pull) echo "Already up to date."; exit 0 ;;
  esac
done
exec "$REAL_GIT" "\$@"
SHIM
chmod +x "$WORK/shim/git"
install pulled >/dev/null
out=$(SHIM_PATH="$WORK/shim" install pulled); rc=$?
check "an update in place exits 0" "0" "$rc"
check "the update's stages, in order" "yes" "$(stage_order "$out" "Pull" "Build" "Link" "Reproject" "Semantic matching" "Judge" "Done")"
has "the pull's output sits under its stage" "    Already up to date." "$out"
has "it ends updated" "  ✓ Updated." "$out"

echo "probes under pipefail:"
# A ways whose help names --commit on its first line and then runs on past a
# pipe buffer. A probe that stops reading early (`grep -q`) kills it with
# SIGPIPE, and pipefail then reports "no --commit". Each probe runs 20 times:
# the outcome of the race depends on timing.
mkdir -p "$WORK/probe/app/bin" "$WORK/probe/sig/scripts" "$WORK/probe/sig/bin" "$WORK/probe/shim" "$WORK/probe/cache"
cat > "$WORK/probe/app/bin/ways" <<'STUB'
#!/bin/sh
echo "      --commit  Make the changes"
i=0; while [ $i -lt 4000 ]; do echo "      --option-$i  more help text, enough to outlast a pipe buffer"; i=$((i+1)); done
STUB
chmod +x "$WORK/probe/app/bin/ways"
repair_fn=$(sed -n '/^repair_cmd() {/,/^}/p' "$ROOT/scripts/install.sh")
misses=0
for _ in $(seq 20); do
    got=$(APP_DIR="$WORK/probe/app" bash -c 'set -euo pipefail; eval "$1"; repair_cmd' _ "$repair_fn")
    [[ "$got" == "ways manage repair --commit" ]] || misses=$((misses+1))
done
check "the installer's repair probe reads the whole help" "0" "$misses"
# fix-way-embed-signature.sh probes only on macOS and only with no engine to
# re-sign: a uname shim says Darwin and the cache holds no way-embed.
cp "$ROOT/scripts/fix-way-embed-signature.sh" "$WORK/probe/sig/scripts/"
cp "$WORK/probe/app/bin/ways" "$WORK/probe/sig/bin/ways"
printf '#!/bin/sh\necho Darwin\n' > "$WORK/probe/shim/uname"
printf '#!/bin/sh\nexit 0\n' > "$WORK/probe/shim/codesign"
chmod +x "$WORK/probe/shim/uname" "$WORK/probe/shim/codesign"
misses=0
for _ in $(seq 20); do
    got=$(PATH="$WORK/probe/shim:$PATH" XDG_CACHE_HOME="$WORK/probe/cache" bash "$WORK/probe/sig/scripts/fix-way-embed-signature.sh" 2>&1)
    [[ "$got" == *"run 'ways manage update --commit' first"* ]] || misses=$((misses+1))
done
check "the signature fix's update probe reads the whole help" "0" "$misses"

echo "bootstrap:"
make_source "$WORK/upstream/src"
out=$(INSTALL_ARGS=--bootstrap install bootstrap AGENT_WAYS_BOOTSTRAP_URL="$WORK/upstream/src"); rc=$?
check "the bootstrap with no other flag exits 0" "0" "$rc"
has "and runs the inner install to the end" "── Done" "$out"
if command -v docker >/dev/null 2>&1 && docker image inspect bash:3.2 >/dev/null 2>&1; then
    out=$(docker run --rm -v "$ROOT/scripts/install.sh:/install.sh:ro" -v "$WORK/upstream/src:/upstream:ro" bash:3.2 bash -c '
        apk add -q --no-cache git make jq >/dev/null 2>&1 || { echo "APK-NO-NETWORK"; exit 0; }
        export HOME=/h && mkdir -p /h && git config --global safe.directory "*"
        XDG_DATA_HOME=/h/.local/share XDG_CONFIG_HOME=/h/.config XDG_CACHE_HOME=/h/.cache \
          AGENT_WAYS_BOOTSTRAP_URL=/upstream bash /install.sh --bootstrap </dev/null 2>&1
        echo "rc=$?"; bash --version | head -1' 2>&1)
    if [[ "$out" == *APK-NO-NETWORK* ]]; then
        echo "  SKIP: bash 3.2 bootstrap (apk could not fetch git, make and jq)"
    else
    has "bash 3.2 runs the bootstrap" "GNU bash, version 3.2" "$out"
    lacks "with no unbound array" "unbound variable" "$out"
    has "to the end" "── Done" "$out"
    has "and exits 0" "rc=0" "$out"
    fi
else
    echo "  SKIP: bash 3.2 bootstrap (no local bash:3.2 image; docker pull bash:3.2 to run it)"
fi

exit $fail
