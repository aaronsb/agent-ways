#!/usr/bin/env bash
# The installer prints the staged report every `ways manage` verb prints
# (ADR-511): one ruled header per stage, a mark per outcome, plain text in a
# pipe or under NO_COLOR, colour when forced. Run against a sandboxed HOME and
# XDG tree with a stand-in app: its Makefile's `setup` writes a `bin/ways`
# whose `reconcile` succeeds, or fails when RECONCILE_FAILS is set.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}
has() {  # name needle haystack
    if [[ "$3" == *"$2"* ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — [$2] not in:"; echo "$3" | sed 's/^/      /'; fail=1; fi
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
	@mkdir -p bin
	@printf '#!/bin/sh\ncase "$$1" in\n  reconcile) echo "linked skills (absent)"; [ -z "$$RECONCILE_FAILS" ] || { echo "refused: skills is a real directory" >&2; exit 1; } ;;\n  status) echo "Engine: embedding" ;;\nesac\nexit 0\n' > bin/ways
	@chmod +x bin/ways
MK
    git -C "$src" init -q
    git -C "$src" -c user.name=t -c user.email=t@e add -A
    git -C "$src" -c user.name=t -c user.email=t@e commit -qm one
}

# Run the installer from a fresh source into a fresh home; print its output.
install() {  # name, then extra env assignments
    local name="$1"; shift
    local base="$WORK/$name"
    make_source "$base/src"
    mkdir -p "$base/home"
    (cd "$base/src" && env -i PATH="$PATH" HOME="$base/home" XDG_DATA_HOME="$base/home/.local/share" \
        XDG_CONFIG_HOME="$base/home/.config" XDG_CACHE_HOME="$base/home/.cache" TERM=xterm "$@" \
        bash scripts/install.sh </dev/null 2>&1)
}

echo "installer report:"
out=$(install plain); rc=$?
check "a fresh install exits 0" "0" "$rc"
order=""
for h in "Stage" "Build" "Link" "Reproject" "Semantic matching" "Judge" "Done"; do
    n=$(grep -n "^── $h " <<<"$out" | head -1 | cut -d: -f1)
    order+="${n:-x} "
done
check "one header per stage, in order" "yes" "$(awk '{ for (i = 2; i <= NF; i++) if ($i == "x" || $i + 0 <= $(i - 1) + 0) { print "no"; exit } print "yes" }' <<<"$order")"
has "the build's output sits under its stage" "    Pre-built binary installed." "$out"
has "the reconcile's output sits under its stage" "    linked skills (absent)" "$out"
has "a stage outcome is marked" "  ✓ projection reconciled" "$out"
has "the engine check is marked" "  ✓ the embedding engine runs" "$out"
check "plain text in a pipe" "0" "$(grep -c $'\x1b' <<<"$out")"

out=$(install forced CLICOLOR_FORCE=1)
check "colour when forced" "yes" "$(grep -q $'\x1b\[' <<<"$out" && echo yes || echo no)"
out=$(install no-color CLICOLOR_FORCE=1 NO_COLOR=1)
check "NO_COLOR wins over a forced colour" "0" "$(grep -c $'\x1b' <<<"$out")"

out=$(install refused RECONCILE_FAILS=1); rc=$?
check "a refused projection exits 3" "3" "$rc"
has "the refusal is a failed outcome" "  ✗ Projection stopped." "$out"
has "the fix says --commit" "ways manage repair --commit --force" "$out"

exit $fail
