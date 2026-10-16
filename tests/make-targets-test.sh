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

# Every entry is a real package of the workspace, so a stale entry fails here.
packages=" $(cargo metadata --manifest-path "$ROOT/tools/Cargo.toml" --no-deps --offline --format-version 1 2>/dev/null \
    | jq -r '.packages[].name' | tr '\n' ' ')"
for b in $bins; do
    check "$b is a package in tools/Cargo.toml" "yes" "$([[ $packages == *" $b "* ]] && echo yes || echo no)"
done

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

# Each caller's paths: filter lists its crate's workspace dependencies.
paths_out=$(bash "$ROOT/scripts/workflow-paths.sh" --check 2>&1)
check "build-*.yml paths match cargo metadata" "0" "$?"
[[ -z $paths_out ]] || echo "$paths_out" | sed 's/^/    /'

# test.yml runs this test, so it must run when a build workflow changes.
for w in "build-*.yml" "reusable-build.yml"; do
    check "test.yml triggers on $w" "2" "$(grep -cF -- "- '.github/workflows/$w'" "$ROOT/.github/workflows/test.yml")"
done

# A failed cargo build stops the get-or-build rule and leaves no bin/attend.
# The app dir holds what the recipe reads; a fake gh finds no release and a
# fake cargo fails its build.
FAILAPP="$WORK/failapp"
mkdir -p "$FAILAPP/tools/scripts" "$FAILAPP/scripts" "$WORK/failbin" "$WORK/failfake"
cp "$ROOT/tools/suite-bins" "$FAILAPP/tools/"
cp "$ROOT/tools/scripts/download-prebuilt.sh" "$ROOT/tools/scripts/prebuilt-lib.sh" "$FAILAPP/tools/scripts/"
cp "$ROOT/scripts/check-rust.sh" "$FAILAPP/scripts/"
printf '#!/bin/sh\n[ "$1" = --version ] && { echo "cargo 1.95.0"; exit 0; }\nexit 101\n' > "$WORK/failbin/cargo"
chmod +x "$WORK/failbin/cargo"
PATH="$WORK/failbin:$ROOT/tests/fixtures/prebuilt:$PATH" FAKE="$WORK/failfake" FAKE_GH_FAIL=api RETRY_MAX=1 \
    HOME="$WORK/home" XDG_CACHE_HOME="$WORK/cache" \
    make -s -f "$ROOT/Makefile" -C "$FAILAPP" attend >/dev/null 2>&1
check "make attend fails when the download and the cargo build both fail" "yes" "$([[ $? -ne 0 ]] && echo yes || echo no)"
check "and leaves no bin/attend" "no" "$([[ -e $FAILAPP/bin/attend || -L $FAILAPP/bin/attend ]] && echo yes || echo no)"

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

# dev-install builds every suite binary in one cargo call.
dev=$(make -s -n -C "$ROOT" dev-install 2>&1)
check "dev-install builds every suite binary" "yes" \
    "$(has "$dev" "cargo build --release --manifest-path tools/Cargo.toml$(printf -- ' -p %s' $bins)")"

# dev-restore points existing links at the app and leaves real files alone.
# The links start out pointing at a dev checkout.
DEVB="$WORK/dev-xdg" DEVC="$WORK/dev-claude"
mkdir -p "$DEVB" "$DEVC"
for b in $bins; do ln -s "$WORK/checkout/bin/$b" "$DEVB/$b"; done
ln -s "$WORK/checkout/bin/ways" "$DEVC/ways"
printf 'mine\n' > "$DEVC/attend"
make -s -C "$ROOT" dev-restore APP_DIR="$APP" XDG_BIN="$DEVB" CLAUDE_BIN="$DEVC" >/dev/null 2>&1
check "dev-restore succeeds" "0" "$?"
for b in $bins; do
    check "dev-restore points $b at the app" "$APP/bin/$b" "$(readlink "$DEVB/$b")"
done
check "dev-restore repoints a projected link" "$APP/bin/ways" "$(readlink "$DEVC/ways")"
check "dev-restore leaves a real file" "mine" "$(cat "$DEVC/attend")"
check "dev-restore adds no link that was absent" "no" "$([[ -e $DEVC/ways-audit || -L $DEVC/ways-audit ]] && echo yes || echo no)"
make -s -C "$ROOT" dev-restore APP_DIR="$WORK/no-app" XDG_BIN="$DEVB" CLAUDE_BIN="$DEVC" >/dev/null 2>&1
check "dev-restore refuses a missing app" "yes" "$([[ $? -ne 0 ]] && echo yes || echo no)"
mkdir -p "$WORK/empty-xdg" "$WORK/empty-claude"
none=$(make -s -C "$ROOT" dev-restore APP_DIR="$APP" XDG_BIN="$WORK/empty-xdg" CLAUDE_BIN="$WORK/empty-claude" 2>&1)
check "dev-restore fails when it repoints nothing" "yes" "$([[ $? -ne 0 ]] && echo yes || echo no)"
check "and says so" "yes" "$(has "$none" "no link repointed")"

# purge-attend-state refuses while an `attend run` is live. Its pgrep must not
# match its own recipe, whose command line holds the pattern. The fake pgrep
# runs the real one and keeps only the matches in this test's process tree,
# so an `attend run` elsewhere on the machine decides nothing here. It
# leaves itself out, as the real pgrep does: its own arguments hold the
# pattern.
REAL_PGREP=$(command -v pgrep)
mkdir -p "$WORK/purgebin"
cat > "$WORK/purgebin/pgrep" <<PGREP
#!/usr/bin/env bash
found=1
for pid in \$("$REAL_PGREP" "\$@"); do
    [[ \$pid -eq \$\$ ]] && continue
    p=\$pid
    while [[ -n \$p && \$p -gt 1 ]]; do
        if [[ \$p -eq $$ ]]; then echo "\$pid"; found=0; break; fi
        p=\$(ps -o ppid= -p "\$p" | tr -d ' ')
    done
done
exit \$found
PGREP
chmod +x "$WORK/purgebin/pgrep"
purge() { PATH="$WORK/purgebin:$PATH" XDG_CACHE_HOME="$WORK/purge-cache" make -s -C "$ROOT" purge-attend-state >/dev/null 2>&1; }
mkdir -p "$WORK/purge-cache/attend/signals"
purge
check "purge-attend-state proceeds with no attend run" "0" "$?"
check "and wipes the attend cache" "no" "$([[ -e $WORK/purge-cache/attend ]] && echo yes || echo no)"
mkdir -p "$WORK/purge-cache/attend/signals"
(exec -a "attend run" sleep 30) &
live=$!
purge
check "purge-attend-state refuses while an attend run is live" "yes" "$([[ $? -ne 0 ]] && echo yes || echo no)"
check "and keeps the attend cache" "yes" "$([[ -e $WORK/purge-cache/attend ]] && echo yes || echo no)"
kill "$live" 2>/dev/null
wait "$live" 2>/dev/null

exit $fail
