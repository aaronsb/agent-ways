#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# scripts/ci-merge-checksums.sh against a fake `gh`: the attach of a
# best-effort binary, a first release, a rerun over a release that holds a
# best-effort line, a transient `gh release view` failure that recovers, one
# that does not, and an attach to a release that does not exist.
# CI_MERGE_SCRIPT overrides the script under test.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SCRIPT="${CI_MERGE_SCRIPT:-$ROOT/scripts/ci-merge-checksums.sh}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
export RETRY_MAX=3 RETRY_DELAY=0 GITHUB_REPOSITORY=o/r
passes=0 fails=0

check() {  # name expected actual
  if [[ "$2" == "$3" ]]; then
    echo "  PASS: $1"; passes=$((passes + 1))
  else
    echo "  FAIL: $1 — expected [$2], got [$3]"; fails=$((fails + 1))
  fi
}
yn() { if "$@"; then echo yes; else echo no; fi; }

# The fake gh. $FAKE/release holds the release's checksums.txt; without it the
# release does not exist. $FAKE/view-fails holds how many `release view` calls
# fail with an HTTP 502 before one succeeds. Each call is logged to $FAKE/calls.
mkdir -p "$WORK/bin"
cat > "$WORK/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >> "$FAKE/calls"
case "$1 $2" in
  "release view")
    n=$(cat "$FAKE/view-fails" 2>/dev/null || echo 0)
    if [[ "$n" -gt 0 ]]; then
      echo $((n - 1)) > "$FAKE/view-fails"
      echo "HTTP 502: Bad Gateway" >&2
      exit 1
    fi
    [[ -f "$FAKE/release" ]] || { echo "release not found" >&2; exit 1; }
    echo x-linux-x86_64
    echo checksums.txt ;;
  "release download")
    out=""
    while [[ $# -gt 0 ]]; do [[ "$1" == --output ]] && out="$2"; shift; done
    cp "$FAKE/release" "$out" ;;
  "release upload") ;;
  *) echo "fake gh: unhandled: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$WORK/bin/gh"
export PATH="$WORK/bin:$PATH"

# case NAME: a fresh fake and a binaries dir; sets FAKE and DIR.
case_dir() {
  export FAKE="$WORK/$1"
  DIR="$FAKE/binaries"
  mkdir -p "$DIR"
  : > "$FAKE/calls"
}
sum() { (cd "$DIR" && sha256sum "$1"); }

echo "ci-merge-checksums against the fake gh:"

# Attach: the release holds the gating lines (one in binary mode) and a stale
# darwin-arm64 line, which the new line replaces.
case_dir attach
echo arm > "$DIR/x-darwin-arm64"
printf '%s  x-linux-x86_64\n%s *x-darwin-x86_64\n%s  x-darwin-arm64\n' aaaa bbbb stale > "$FAKE/release"
bash "$SCRIPT" --upload x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "attach succeeds" "0" "$?"
check "attach keeps the gating lines and replaces the stale one" \
  "$(sum x-darwin-arm64)|bbbb *x-darwin-x86_64|aaaa  x-linux-x86_64" "$(paste -sd'|' "$DIR/checksums.txt")"
check "attach uploads the binary and checksums.txt with --clobber" "yes" \
  "$(yn grep -qx 'gh release upload x-v1.0.0 --repo o/r --clobber x-darwin-arm64 checksums.txt' "$FAKE/calls")"

# First release: no release yet, so checksums.txt covers this upload only.
case_dir first
echo l > "$DIR/x-linux-x86_64"
echo stale > "$DIR/checksums.txt"
bash "$SCRIPT" x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "a first release succeeds" "0" "$?"
check "and checksums.txt holds this upload's line only" "$(sum x-linux-x86_64)" "$(cat "$DIR/checksums.txt")"
check "and nothing is uploaded without --upload" "no" "$(yn grep -q 'release upload' "$FAKE/calls")"

# Rerun: the release already holds a darwin-arm64 line, which stays.
case_dir rerun
echo l > "$DIR/x-linux-x86_64"
printf '%s  x-linux-x86_64\n%s  x-darwin-arm64\n' oldlinux armhash > "$FAKE/release"
bash "$SCRIPT" x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "a rerun keeps the attached line and refreshes its own" \
  "armhash  x-darwin-arm64|$(sum x-linux-x86_64)" "$(paste -sd'|' "$DIR/checksums.txt")"

# A transient failure that recovers within the retries.
case_dir recovers
echo arm > "$DIR/x-darwin-arm64"
printf '%s  x-linux-x86_64\n' aaaa > "$FAKE/release"
echo 2 > "$FAKE/view-fails"
bash "$SCRIPT" --upload x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "a 502 that clears on retry still merges and uploads" "0:aaaa  x-linux-x86_64:yes" \
  "$?:$(grep linux "$DIR/checksums.txt"):$(yn grep -q 'release upload' "$FAKE/calls")"

# A failure that persists: exit non-zero, write nothing, upload nothing.
case_dir transient
echo arm > "$DIR/x-darwin-arm64"
printf '%s  x-linux-x86_64\n' aaaa > "$FAKE/release"
echo 99 > "$FAKE/view-fails"
bash "$SCRIPT" --upload x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "a persistent 502 on release view exits non-zero" "1" "$?"
check "after every retry" "3" "$(grep -c 'release view' "$FAKE/calls")"
check "and uploads nothing" "no" "$(yn grep -q 'release upload' "$FAKE/calls")"
check "and writes no checksums.txt" "no" "$(yn test -e "$DIR/checksums.txt")"

# The same failure in the release job's mode, on a rerun over a release that
# holds a darwin-arm64 line: a checksums.txt written now would drop that line.
case_dir transient-rerun
echo l > "$DIR/x-linux-x86_64"
printf '%s  x-linux-x86_64\n%s  x-darwin-arm64\n' oldlinux armhash > "$FAKE/release"
echo 99 > "$FAKE/view-fails"
bash "$SCRIPT" x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "a persistent 502 without --upload exits non-zero" "1" "$?"
check "and writes no checksums.txt for the next step to upload" "no" "$(yn test -e "$DIR/checksums.txt")"

# Attach to a release that does not exist.
case_dir missing
echo arm > "$DIR/x-darwin-arm64"
bash "$SCRIPT" --upload x-v1.0.0 "$DIR" >/dev/null 2>"$FAKE/err"
check "attaching to a missing release exits non-zero" "1" "$?"
check "and uploads nothing" "no" "$(yn grep -q 'release upload' "$FAKE/calls")"

# An empty dir is an error, not an empty checksums.txt.
case_dir empty
bash "$SCRIPT" x-v1.0.0 "$DIR" >/dev/null 2>&1
check "an empty dir exits non-zero" "1" "$?"

echo "Results: $passes passed, $fails failed"
exit "$fails"
