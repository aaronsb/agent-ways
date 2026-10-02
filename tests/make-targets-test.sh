#!/usr/bin/env bash
# The suite list in tools/suite-bins drives the Makefile's get-or-build and
# -rebuild pattern rules, `make link`, scripts/install.sh, and one
# build-<name>.yml workflow per binary. Check each consumer against the list.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}
has() {  # haystack needle
    grep -qF -- "$2" <<< "$1" && echo yes || echo no
}

bins=$(awk '/^[a-z]/ { print $1 }' "$ROOT/tools/suite-bins")
check "tools/suite-bins lists binaries" "yes" "$([[ -n $bins ]] && echo yes || echo no)"

# Pattern rules, dry-run. Neither recipe calls $(MAKE) directly, so -n runs nothing.
for b in $bins; do
    get=$(make -s -n -C "$ROOT" "$b" 2>&1)
    check "make $b downloads $b first" "yes" "$(has "$get" "tools/scripts/download-prebuilt.sh $b;")"
    check "make $b falls back to cargo -p $b" "yes" "$(has "$get" "cargo build --release --manifest-path tools/Cargo.toml -p $b ")"
    check "make $b links bin/$b" "yes" "$(has "$get" "tools/target/release/$b\" bin/$b")"
    re=$(make -s -n -C "$ROOT" "$b-rebuild" 2>&1)
    check "make $b-rebuild builds with cargo -p $b" "yes" "$(has "$re" "cargo build --release --manifest-path tools/Cargo.toml -p $b")"
    check "make $b-rebuild skips the download" "no" "$(has "$re" "download-prebuilt")"
    check "build-$b.yml builds component $b" "yes" \
        "$(grep -qE "^ *component: $b\$" "$ROOT/.github/workflows/build-$b.yml" 2>/dev/null && echo yes || echo no)"
done

db=$(make -s -n -p -C "$ROOT" help 2>/dev/null)
expected_rebuilds="$(printf '%s-rebuild ' $bins)way-embed-rebuild"
check "update-binaries rebuilds every suite binary and way-embed" "$expected_rebuilds" \
    "$(sed -n 's/^update-binaries: //p' <<< "$db")"
check "relink installs from the same list" "$(echo $bins)" "$(sed -n 's/^SUITE_BINS := //p' <<< "$db")"

# `make link` and install.sh link the same names from one app dir.
APP="$WORK/app"
mkdir -p "$APP/bin" "$APP/tools"
cp "$ROOT/tools/suite-bins" "$APP/tools/"
for b in $bins; do printf '%s\n' "$b" > "$APP/bin/$b"; done
HOME="$WORK/home" XDG_BIN_HOME="$WORK/make-bin" make -s -f "$ROOT/Makefile" -C "$APP" link >/dev/null 2>&1
fns=$(awk '/^(link_path_binaries|suite_bins)\(\) \{/,/^\}/' "$ROOT/scripts/install.sh")
(
    # shellcheck disable=SC2034  # read by the eval'd install.sh functions
    APP_DIR="$APP" XDG_BIN="$WORK/install-bin"
    eval "$fns"
    link_path_binaries
)
expected=$(printf '%s\n' $bins | sort | tr '\n' ' ')
check "make link links every suite binary" "$expected" "$(ls "$WORK/make-bin" | sort | tr '\n' ' ')"
check "install.sh links every suite binary" "$expected" "$(ls "$WORK/install-bin" | sort | tr '\n' ' ')"

exit $fail
