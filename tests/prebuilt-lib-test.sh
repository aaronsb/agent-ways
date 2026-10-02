#!/usr/bin/env bash
# tools/scripts/prebuilt-lib.sh against a fake `gh`: tag resolution by prefix
# (#518), the release download call, and checksum handling. The suite then
# runs against deliberately broken copies of the library and must fail on each,
# so a check that cannot catch the bug it names fails this test.
# PREBUILT_LIB overrides the library under test.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
LIB="${PREBUILT_LIB:-$ROOT/tools/scripts/prebuilt-lib.sh}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
REPO="aaronsb/agent-ways"
export RETRY_MAX=2 RETRY_DELAY=0

# --- Fake gh and curl -------------------------------------------------------
# State lives under $FAKE: releases-page-N.json (the paginated releases API),
# assets/<tag> (asset names), files/<tag>/<name> (downloadable files), and
# calls (one line per invocation). FAKE_GH_FAIL=api|view|checksums fails that call.
mkdir -p "$WORK/fakebin"
cat > "$WORK/fakebin/gh" <<'GH'
#!/usr/bin/env bash
echo "gh $*" >> "$FAKE/calls"
case "$1 $2" in
  "api --paginate")
    [[ "${FAKE_GH_FAIL:-}" == api ]] && { echo "HTTP 502" >&2; exit 1; }
    jq_expr="${5:?}"
    for page in "$FAKE"/releases-page-*.json; do jq -r "$jq_expr" "$page"; done ;;
  "release view")
    [[ "${FAKE_GH_FAIL:-}" == view ]] && exit 1
    cat "$FAKE/assets/$3" ;;
  "release download")
    tag="$3"; shift 3; pattern="" dir=""
    while [[ $# -gt 0 ]]; do
      case "$1" in --pattern) pattern="$2"; shift 2 ;; --dir) dir="$2"; shift 2 ;; *) shift ;; esac
    done
    [[ "${FAKE_GH_FAIL:-}" == checksums && "$pattern" == checksums.txt ]] && exit 1
    [[ -f "$FAKE/files/$tag/$pattern" ]] || exit 1
    cp "$FAKE/files/$tag/$pattern" "$dir/$pattern" ;;
  *) echo "fake gh: unhandled: $*" >&2; exit 1 ;;
esac
GH
cat > "$WORK/fakebin/curl" <<'CURL'
#!/usr/bin/env bash
echo "curl $*" >> "$FAKE/calls"
exit 1
CURL
chmod +x "$WORK/fakebin/gh" "$WORK/fakebin/curl"
export PATH="$WORK/fakebin:$PATH"

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
  platform="$(detect_platform)"
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

  # A release of a fake binary with matching checksums.
  bin="ways-$platform"
  mkdir -p "$FAKE/files/ways-v1.10.0"
  printf '#!/bin/sh\necho "ways 1.10.0"\n' > "$FAKE/files/ways-v1.10.0/$bin"
  # Another platform's line comes first, so a lookup must match the name.
  { printf '%s  %s\n' 0000 ways-linux-other
    ( cd "$FAKE/files/ways-v1.10.0" && { sha256sum "$bin" 2>/dev/null || shasum -a 256 "$bin"; } ); } > "$FAKE/files/ways-v1.10.0/checksums.txt"
  printf '%s\n' "$bin" checksums.txt > "$FAKE/assets/ways-v1.10.0"

  out="$FAKE/out-ok"
  got=$(prebuilt_install ways latest "$out" "$REPO" "make ways" 2>"$FAKE/err")
  check "installs the verified binary and prints its path" "$out/ways" "$got"
  check "the installed binary runs" "ways 1.10.0" "$("$out/ways" --version 2>/dev/null)"
  check "downloads the platform asset of the resolved tag from the repo" "yes" \
    "$(grep -qx "gh release download ways-v1.10.0 --repo $REPO --pattern $bin --dir $out --clobber" "$FAKE/calls" && echo yes || echo no)"
  check "says the checksum was verified" "yes" "$(grep -q 'Checksum verified' "$FAKE/err" && echo yes || echo no)"
  check "removes checksums.txt after use" "no" "$([[ -e $out/checksums.txt ]] && echo yes || echo no)"

  : > "$FAKE/calls"
  prebuilt_install ways latest "$out" "$REPO" "make ways" >/dev/null 2>&1
  check "a working installed binary short-circuits without calling gh" "" "$(cat "$FAKE/calls")"

  # Mismatch: the binary changes after its checksum was published.
  mkdir -p "$FAKE/files/ways-v1.9.0"
  cp "$FAKE/files/ways-v1.10.0/checksums.txt" "$FAKE/files/ways-v1.9.0/"
  printf '#!/bin/sh\necho "ways 1.9.0 tampered"\n' > "$FAKE/files/ways-v1.9.0/$bin"
  cp "$FAKE/assets/ways-v1.10.0" "$FAKE/assets/ways-v1.9.0"
  out="$FAKE/out-mismatch"
  prebuilt_install ways ways-v1.9.0 "$out" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a checksum mismatch fails" "1" "$?"
  check "a checksum mismatch installs nothing" "no:no" \
    "$([[ -e $out/ways ]] && echo yes || echo no):$([[ -e $out/$bin ]] && echo yes || echo no)"
  check "a checksum mismatch says so" "yes" "$(grep -q 'CHECKSUM MISMATCH' "$FAKE/err" && echo yes || echo no)"

  # checksums.txt is listed but will not download: refuse, do not skip.
  out="$FAKE/out-nosums"
  FAKE_GH_FAIL=checksums prebuilt_install ways ways-v1.10.0 "$out" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a listed checksums.txt that fails to download refuses the install" "1" "$?"
  check "the refused binary is removed" "no:no" \
    "$([[ -e $out/ways ]] && echo yes || echo no):$([[ -e $out/$bin ]] && echo yes || echo no)"

  # A release without checksums.txt installs with a warning.
  mkdir -p "$FAKE/files/ways-v1.2.0"
  cp "$FAKE/files/ways-v1.10.0/$bin" "$FAKE/files/ways-v1.2.0/"
  printf '%s\n' "$bin" > "$FAKE/assets/ways-v1.2.0"
  out="$FAKE/out-unsigned"
  got=$(prebuilt_install ways ways-v1.2.0 "$out" "$REPO" "make ways" 2>"$FAKE/err")
  check "a release without checksums.txt installs" "$out/ways" "$got"
  check "and warns that it skipped verification" "yes" "$(grep -q 'no checksums.txt' "$FAKE/err" && echo yes || echo no)"

  # No asset for this platform; an unreadable release.
  printf '%s\n' ways-plan9-mips checksums.txt > "$FAKE/assets/ways-v1.2.0"
  prebuilt_install ways ways-v1.2.0 "$FAKE/out-noasset" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "a release without this platform's asset fails" "1" "$?"
  check "and names the build-from-source fallback" "yes" "$(grep -q 'make ways' "$FAKE/err" && echo yes || echo no)"
  FAKE_GH_FAIL=api prebuilt_install ways latest "$FAKE/out-noapi" "$REPO" "make ways" >/dev/null 2>"$FAKE/err"
  check "an unreachable API fails the install" "1" "$?"

  check "curl is never called" "no" "$(grep -q '^curl' "$FAKE/calls" && echo yes || echo no)"
  exit "$fails"
)

echo "prebuilt-lib against the fake gh:"
suite "$LIB"
status=$?

# --- Broken copies of the library -------------------------------------------
# Each must fail the suite. The first takes the first prefix match from the
# API, the pre-#518 resolver; the second never rejects a checksum mismatch;
# the third skips verification when checksums.txt will not download.
mutate() {  # name sed-expression
  sed -e "$2" "$LIB" > "$WORK/mutant-$1.sh"
  if cmp -s "$LIB" "$WORK/mutant-$1.sh"; then
    rm -f "$WORK/mutant-$1.sh"
    echo "  FAIL: mutant $1 left the library unchanged; update its expression"; status=1
  fi
}
mutate first-match 's/sort -V | tail -1/head -1/'
mutate no-mismatch 's/if \[\[ "$actual" != "$expected" \]\]; then/if false; then/'
mutate lenient-checksums '/refusing to install/{n;s/.*//;n;s/return 1/:/;}'

for m in first-match no-mismatch lenient-checksums; do
  [[ -f "$WORK/mutant-$m.sh" ]] || continue
  if suite "$WORK/mutant-$m.sh" >/dev/null 2>&1; then
    echo "  FAIL: the suite passes against the broken '$m' library"
    status=1
  else
    echo "  PASS: the suite fails against the broken '$m' library"
  fi
done

exit "$status"
