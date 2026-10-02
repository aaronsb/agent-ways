#!/usr/bin/env bash
# tools/scripts/download-prebuilt.sh against the fake `gh` (tests/fixtures/prebuilt):
# component validation, --release, the <COMPONENT>_RELEASE override, the
# way-embed output dir, and way-embed's `match --batch` gate. HOME and
# XDG_CACHE_HOME point into a temp dir, so the real cache is never touched.

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DL="$ROOT/tools/scripts/download-prebuilt.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0
export RETRY_MAX=2 RETRY_DELAY=0
export PATH="$ROOT/tests/fixtures/prebuilt:$PATH"
export HOME="$WORK/home" XDG_CACHE_HOME="$WORK/cache"
export FAKE="$WORK/fake"
mkdir -p "$FAKE/assets" "$FAKE/files" "$HOME"
unset WAYS_RELEASE WAYS_AUDIT_RELEASE WAY_EMBED_RELEASE

check() {  # name expected actual
    if [[ "$2" == "$3" ]]; then echo "  PASS: $1"; else echo "  FAIL: $1 — expected [$2], got [$3]"; fail=1; fi
}
yn() { if "$@"; then echo yes; else echo no; fi; }

# shellcheck source=/dev/null
platform=$(source "$ROOT/tools/scripts/prebuilt-lib.sh" && detect_platform)

# release TAG COMPONENT SCRIPT-BODY: one release holding a fake binary and its checksum.
release() {
    local d="$FAKE/files/$1" b="$2-$platform"
    mkdir -p "$d"
    printf '#!/bin/sh\n%s\n' "$3" > "$d/$b"
    ( cd "$d" && { sha256sum "$b" 2>/dev/null || shasum -a 256 "$b"; } ) > "$d/checksums.txt"
    printf '%s\n' "$b" checksums.txt > "$FAKE/assets/$1"
}
printf '[{"tag_name": "ways-audit-v1.0.0", "draft": false, "prerelease": false}, {"tag_name": "way-embed-v2.0.0", "draft": false, "prerelease": false}]\n' \
    > "$FAKE/releases-page-1.json"
release ways-audit-v1.0.0 ways-audit 'echo "ways-audit 1.0.0"'
release ways-audit-v0.9.0 ways-audit 'echo "ways-audit 0.9.0"'
# A current way-embed names --batch in its match usage; an old one rejects it.
release way-embed-v2.0.0 way-embed 'case "$1" in --version) echo "way-embed 2.0.0" ;; *) echo "usage: way-embed match --batch --corpus F --model M" >&2; exit 1 ;; esac'
release way-embed-v1.0.0 way-embed 'case "$1" in --version) echo "way-embed 1.0.0" ;; *) echo "unknown option: --batch" >&2; exit 1 ;; esac'

bash "$DL" no-such-thing >/dev/null 2>&1
check "an unknown component exits 2" "2" "$?"
bash "$DL" claude-projects >/dev/null 2>&1
check "a removed suite entry is unknown" "2" "$?"
err=$(bash "$DL" ways-audit "$WORK/out" --release 2>&1)
check "--release without a tag fails" "1" "$?"
check "and says it needs a tag" "yes" "$(yn grep -q -- '--release needs a tag' <<< "$err")"

: > "$FAKE/calls"
got=$(WAYS_AUDIT_RELEASE=ways-audit-v0.9.0 bash "$DL" ways-audit "$WORK/out-env" 2>/dev/null)
check "WAYS_AUDIT_RELEASE picks the tag" "ways-audit 0.9.0" "$("$got" --version 2>/dev/null)"
check "and skips the latest-tag lookup" "no" "$(yn grep -q '^gh api' "$FAKE/calls")"

got=$(bash "$DL" ways-audit --release ways-audit-v0.9.0 "$WORK/out-flag" 2>/dev/null)
check "--release picks the tag" "ways-audit 0.9.0" "$("$got" --version 2>/dev/null)"
got=$(bash "$DL" ways-audit "$WORK/out-latest" 2>/dev/null)
check "with neither, the newest ways-audit-v* release installs" "ways-audit 1.0.0" "$("$got" --version 2>/dev/null)"

cache="$XDG_CACHE_HOME/agent-ways/user"
got=$(bash "$DL" way-embed 2>/dev/null)
check "way-embed installs to the cache dir by default" "$cache/way-embed" "$got"
check "the newest way-embed passes the --batch gate" "way-embed 2.0.0" "$("$cache/way-embed" --version 2>/dev/null)"

rm -rf "$cache"
err=$(WAY_EMBED_RELEASE=way-embed-v1.0.0 bash "$DL" way-embed 2>&1 >/dev/null)
check "a way-embed without match --batch is refused" "1" "$?"
check "and removed" "no:no" "$(yn test -e "$cache/way-embed"):$(yn test -e "$cache/way-embed-$platform")"
check "and the refusal names the gate" "yes" "$(yn grep -q "predates the required 'match --batch'" <<< "$err")"

# A way-embed running at the latest release's version is kept without a download.
mkdir -p "$cache"
cp "$FAKE/files/way-embed-v2.0.0/way-embed-$platform" "$cache/way-embed"
chmod +x "$cache/way-embed"
: > "$FAKE/calls"
bash "$DL" way-embed >/dev/null 2>&1
check "a way-embed at the latest version is kept" "0:way-embed 2.0.0" "$?:$("$cache/way-embed" --version 2>/dev/null)"
check "without downloading" "no" "$(yn grep -q '^gh release' "$FAKE/calls")"

# One at an older version is replaced, and the replacement passes the gate (#772).
cp "$FAKE/files/way-embed-v1.0.0/way-embed-$platform" "$cache/way-embed"
got=$(bash "$DL" way-embed 2>/dev/null)
check "an older way-embed is replaced by the latest release" "0:way-embed 2.0.0" "$?:$("$got" --version 2>/dev/null)"

# A replacement that fails the gate is refused, and the working binary stays.
cp "$FAKE/files/way-embed-v2.0.0/way-embed-$platform" "$cache/way-embed"
WAY_EMBED_RELEASE=way-embed-v1.0.0 bash "$DL" way-embed >/dev/null 2>&1
check "a replacement without match --batch is refused, keeping the working binary" "1:way-embed 2.0.0" \
  "$?:$("$cache/way-embed" --version 2>/dev/null)"

exit $fail
