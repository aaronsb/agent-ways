#!/usr/bin/env bash
# tools/scripts/prebuilt-lib.sh against a fake `gh` (tests/fixtures/prebuilt):
# tag resolution by prefix (#518), the release download call, checksum
# handling, parallel calls into one directory, and the atomic install. The
# suite then runs against deliberately broken copies of the library and must
# fail on each, so a check that cannot catch the bug it names fails this test.
# PREBUILT_LIB overrides the library under test.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
LIB="${PREBUILT_LIB:-$ROOT/tools/scripts/prebuilt-lib.sh}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
REPO="aaronsb/agent-ways"
export RETRY_MAX=2 RETRY_DELAY=0
export PATH="$ROOT/tests/fixtures/prebuilt:$PATH"

sha() { ( cd "$(dirname "$1")" && { sha256sum "$(basename "$1")" 2>/dev/null || shasum -a 256 "$(basename "$1")"; } ); }
yn() { if "$@"; then echo yes; else echo no; fi; }

# --- The suite --------------------------------------------------------------
# Runs every check against one library in a subshell, printing PASS/FAIL lines;
# exits with the number of failures.
suite() (
  lib="$1"
  # shellcheck source=/dev/null
  source "$lib"
  fails=0
  check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fails=$((fails + 1)); fi
  }
  installed_nothing() {  # dir — neither binary copy nor a staging dir is left
    [[ ! -e $1/ways && ! -e $1/$bin ]] && ! compgen -G "$1/.ways.*" >/dev/null
  }
  platform="$(detect_platform)"
  bin="ways-$platform"
  export FAKE="$WORK/fake.$RANDOM"
  mkdir -p "$FAKE/assets" "$FAKE/files"

  # Releases across two API pages. ways-v1.9.0 comes first and sorts after
  # ways-v1.10.0 as a string; drafts and prereleases are not install targets;
  # ways-mcp exists only on the second page.
  cat > "$FAKE/releases-page-1.json" <<'JSON'
[
  {"tag_name": "ways-v1.9.0", "draft": false, "prerelease": false},
  {"tag_name": "ways-audit-v9.0.0", "draft": false, "prerelease": false},
  {"tag_name": "ways-v1.10.0", "draft": false, "prerelease": false},
  {"tag_name": "ways-v2.0.0", "draft": true, "prerelease": false},
  {"tag_name": "ways-v1.11.0-rc1", "draft": false, "prerelease": true}
]
JSON
  cat > "$FAKE/releases-page-2.json" <<'JSON'
[
  {"tag_name": "ways-v1.2.0", "draft": false, "prerelease": false},
  {"tag_name": "ways-mcp-v3.0.0", "draft": false, "prerelease": false}
]
JSON

  check "resolves the highest ways-v tag, not the first or a string-sorted one" \
    "ways-v1.10.0" "$(latest_tag_for_prefix "$REPO" ways-v)"
  check "resolves a component found only on a later page" \
    "ways-mcp-v3.0.0" "$(latest_tag_for_prefix "$REPO" ways-mcp-v)"
  check "a prefix with no release resolves to nothing, successfully" \
    "0:" "$(t=$(latest_tag_for_prefix "$REPO" attend-v); echo "$?:$t")"
  check "an API failure is an error, not an empty answer" \
    "1" "$(FAKE_GH_FAIL=api latest_tag_for_prefix "$REPO" ways-v >/dev/null 2>&1; echo $?)"

  # release_of TAG COMPONENT VERSION-TEXT: a release with a fake binary and
  # a checksums.txt whose first line is another platform's, so a lookup must
  # match the name.
  release_of() {
    local d="$FAKE/files/$1" b="$2-$platform"
    mkdir -p "$d"
    printf '#!/bin/sh\necho "%s"\n' "$3" > "$d/$b"
    { printf '%s  %s\n' 0000 "$2-linux-other"; sha "$d/$b"; } > "$d/checksums.txt"
    printf '%s\n' "$b" checksums.txt > "$FAKE/assets/$1"
  }
  release_of ways-v1.10.0 ways "ways 1.10.0"

  out="$FAKE/out-ok"
  got=$(prebuilt_install ways latest "$out" "$REPO" "make ways" 2>"$FAKE/err")
  check "installs the verified binary and prints its path" "$out/ways" "$got"
  check "the installed binary runs" "ways 1.10.0" "$("$out/ways" --version 2>/dev/null)"
  check "keeps the platform-named copy beside it" "yes" "$(yn test -x "$out/$bin")"
  check "downloads the platform asset of the resolved tag into a staging dir" "yes" \
    "$(yn grep -qE "^gh release download ways-v1\.10\.0 --repo $REPO --pattern $bin --dir $out/\.ways\.[A-Za-z0-9]+ --clobber\$" "$FAKE/calls")"
  check "says the checksum was verified" "yes" "$(yn grep -q 'Checksum verified' "$FAKE/err")"
  check "leaves no checksums.txt or staging dir" "no:no" \
    "$(yn test -e "$out/checksums.txt"):$(yn compgen -G "$out/.ways.*")"

  : > "$FAKE/calls"
  prebuilt_install ways ways-v1.10.0 "$out" "$REPO" "make ways" >/dev/null 2>&1
  check "a binary at the named tag's version short-circuits without calling gh" "" "$(cat "$FAKE/calls")"
  : > "$FAKE/calls"
  got=$(prebuilt_install ways latest "$out" "$REPO" "make ways" 2>/dev/null)
  check "a binary at the latest release's version is kept" "$out/ways" "$got"
  check "and nothing is downloaded" "no" "$(yn grep -q '^gh release' "$FAKE/calls")"

  # A working binary at another version is replaced (#772).
  out="$FAKE/out-stale"
  mkdir -p "$out"
  printf '#!/bin/sh\necho "ways 1.2.0 (old)"\n' > "$out/ways"
  chmod +x "$out/ways"
  got=$(prebuilt_install ways latest "$out" "$REPO" "make ways" 2>"$FAKE/err")
  check "a working binary at an older version is replaced by the latest release" "$out/ways:ways 1.10.0" \
    "$got:$("$out/ways" --version 2>/dev/null)"
  check "and the replacement is named" "yes" "$(yn grep -q 'Replacing ways 1.2.0 with ways-v1.10.0' "$FAKE/err")"
  printf '#!/bin/sh\necho "ways 1.2.0 (old)"\n' > "$out/ways"
  got=$(FAKE_GH_FAIL=api prebuilt_install ways latest "$out" "$REPO" "make ways" 2>/dev/null)
  check "an unreachable API keeps the working binary" "0:ways 1.2.0 (old)" "$?:$("$out/ways" --version)"

  # Mismatch: the binary changes after its checksum was published.
  release_of ways-v1.9.0 ways "ways 1.9.0"
  printf '#!/bin/sh\necho "ways 1.9.0 tampered"\n' > "$FAKE/files/ways-v1.9.0/$bin"
  out="$FAKE/out-mismatch"
  prebuilt_install ways ways-v1.9.0 "$out" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a checksum mismatch fails" "1" "$?"
  check "a checksum mismatch installs nothing" "yes" "$(yn installed_nothing "$out")"
  check "a checksum mismatch says so" "yes" "$(yn grep -q 'CHECKSUM MISMATCH' "$FAKE/err")"

  # checksums.txt is listed but has no line for this binary.
  release_of ways-v1.0.1 ways "ways 1.0.1"
  printf '%s  %s\n' 0000 ways-linux-other > "$FAKE/files/ways-v1.0.1/checksums.txt"
  out="$FAKE/out-noline"
  prebuilt_install ways ways-v1.0.1 "$out" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a checksums.txt without the binary's line refuses the install" "1" "$?"
  check "and installs nothing" "yes" "$(yn installed_nothing "$out")"
  check "and says the line is missing" "yes" "$(yn grep -q "has no line for $bin" "$FAKE/err")"

  # checksums.txt is listed but will not download: refuse, do not skip.
  out="$FAKE/out-nosums"
  FAKE_GH_FAIL=checksums prebuilt_install ways ways-v1.10.0 "$out" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a listed checksums.txt that fails to download refuses the install" "1" "$?"
  check "the refused binary is removed" "yes" "$(yn installed_nothing "$out")"

  # A release without checksums.txt installs with a warning.
  mkdir -p "$FAKE/files/ways-v1.2.0"
  cp "$FAKE/files/ways-v1.10.0/$bin" "$FAKE/files/ways-v1.2.0/"
  printf '%s\n' "$bin" > "$FAKE/assets/ways-v1.2.0"
  out="$FAKE/out-unsigned"
  got=$(prebuilt_install ways ways-v1.2.0 "$out" "$REPO" "make ways" 2>"$FAKE/err")
  check "a release without checksums.txt installs" "$out/ways" "$got"
  check "and warns that it skipped verification" "yes" "$(yn grep -q 'no checksums.txt' "$FAKE/err")"

  # A dangling bin/ways symlink, as `cargo clean` leaves after a source build.
  out="$FAKE/out-dangling"
  mkdir -p "$out"
  ln -s "$FAKE/no-such-target/ways" "$out/ways"
  got=$(prebuilt_install ways ways-v1.10.0 "$out" "$REPO" "make ways" 2>"$FAKE/err")
  check "replaces a dangling symlink with the verified binary" "$out/ways" "$got"
  check "the replacement is a regular file that runs" "file:ways 1.10.0" \
    "$([[ -L $out/ways ]] && echo link || echo file):$("$out/ways" --version 2>/dev/null)"

  # Two components into one directory at once, as `make -j setup` does.
  release_of ways-audit-v9.0.0 ways-audit "ways-audit 9.0.0"
  out="$FAKE/out-parallel"
  FAKE_GH_DELAY=0.3 prebuilt_install ways ways-v1.10.0 "$out" "$REPO" "make ways" >"$FAKE/p1.out" 2>"$FAKE/p1.err" &
  p1=$!
  FAKE_GH_DELAY=0.3 prebuilt_install ways-audit ways-audit-v9.0.0 "$out" "$REPO" "make ways-audit" >"$FAKE/p2.out" 2>"$FAKE/p2.err" &
  p2=$!
  wait "$p1"; r1=$?
  wait "$p2"; r2=$?
  check "parallel installs into one directory both succeed" "0:0" "$r1:$r2"
  check "and both verify their checksum" "yes:yes" \
    "$(yn grep -q 'Checksum verified' "$FAKE/p1.err"):$(yn grep -q 'Checksum verified' "$FAKE/p2.err")"
  check "and leave no staging dirs" "no" "$(yn compgen -G "$out/.*.*")"

  # No asset for this platform; an unreadable release.
  printf '%s\n' ways-plan9-mips checksums.txt > "$FAKE/assets/ways-v1.2.0"
  prebuilt_install ways ways-v1.2.0 "$FAKE/out-noasset" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a release without this platform's asset fails" "1" "$?"
  check "and names the build-from-source fallback" "yes" "$(yn grep -q 'make ways' "$FAKE/err")"
  FAKE_GH_FAIL=api prebuilt_install ways latest "$FAKE/out-noapi" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "an unreachable API fails the install" "1" "$?"

  check "curl is never called" "no" "$(yn grep -q '^curl' "$FAKE/calls")"
  exit "$fails"
)

echo "prebuilt-lib against the fake gh:"
suite "$LIB"
status=$?

# --- Broken copies of the library -------------------------------------------
# Each must fail the suite:
#   first-match     takes the first prefix match from the API (pre-#518)
#   no-mismatch     never rejects a checksum mismatch
#   skip-checksums  never verifies, even when checksums.txt is listed
#   missing-line    installs unverified when checksums.txt lacks the binary
#   shared-stage    stages every call in one directory
#   copy-install    copies over the target instead of renaming
mutate() {  # name sed-expression...
  local name="$1"; shift
  local args=() e
  for e in "$@"; do args+=(-e "$e"); done
  sed "${args[@]}" "$LIB" > "$WORK/mutant-$name.sh"
  if cmp -s "$LIB" "$WORK/mutant-$name.sh"; then
    rm -f "$WORK/mutant-$name.sh"
    echo "  FAIL: mutant $name left the library unchanged; update its expression"; status=1
  fi
}
mutants=(first-match no-mismatch skip-checksums missing-line shared-stage copy-install)
mutate first-match 's/sort -V | tail -1/head -1/'
mutate no-mismatch 's/if \[\[ "$actual" != "$expected" \]\]; then/if false; then/'
mutate skip-checksums "s/if grep -qx 'checksums.txt' <<<\"\$assets\"; then/if false; then/"
mutate missing-line 's/if \[\[ -z "$expected" \]\]; then/if false; then/' \
  's/if \[\[ "$actual" != "$expected" \]\]; then/if [[ -n "$expected" \&\& "$actual" != "$expected" ]]; then/'
mutate shared-stage 's|stage=$(mktemp -d "${out_dir}/.${comp}.XXXXXX")|stage="${out_dir}/.shared.stage"; mkdir -p "$stage"|'
mutate copy-install 's|mv -f "$stage/$comp" "$out_file"|cp "$stage/$comp" "$out_file"|'

for m in "${mutants[@]}"; do
  [[ -f "$WORK/mutant-$m.sh" ]] || continue
  if suite "$WORK/mutant-$m.sh" >/dev/null 2>&1; then
    echo "  FAIL: the suite passes against the broken '$m' library"
    status=1
  else
    echo "  PASS: the suite fails against the broken '$m' library"
  fi
done

exit "$status"
