#!/usr/bin/env bash
# Download the pre-built binary of one agent-ways component for this platform.
#
# Usage:
#   download-prebuilt.sh COMPONENT [--release TAG] [output-dir]
#
# COMPONENT is a suite binary (see tools/suite-bins) or way-embed. The newest
# `<COMPONENT>-v*` release is used unless --release or <COMPONENT>_RELEASE
# (upper case, `-` as `_`, e.g. WAYS_AUDIT_RELEASE) names a tag.
#
# Output: suite binaries go to bin/ in the repo; way-embed goes to
# ${XDG_CACHE_HOME:-~/.cache}/agent-ways/user. Prints the installed path on
# stdout. Exits non-zero when no usable binary was installed, so the Makefile
# falls back to a source build. The logic lives in prebuilt-lib.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$SCRIPT_DIR/prebuilt-lib.sh"

GH_REPO="aaronsb/agent-ways"

usage() {
  echo "Usage: $0 COMPONENT [--release TAG] [output-dir]"
  echo ""
  echo "  COMPONENT      $(awk '/^[a-z]/ { printf "%s ", $1 }' "$REPO_ROOT/tools/suite-bins")way-embed"
  echo "  --release TAG  GitHub Release tag (default: latest COMPONENT-v* release)"
  echo "  output-dir     Override output directory"
  echo ""
  echo "Platform: $(detect_platform)"
  echo "Available: linux-x86_64, linux-aarch64, darwin-x86_64, darwin-arm64"
}

COMPONENT="${1:-}"
case "$COMPONENT" in
  "" | -h | --help) usage; exit 0 ;;
esac
shift

if [[ "$COMPONENT" == "way-embed" ]]; then
  OUTPUT_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/agent-ways/user"
  BUILD_HINT="cd \"$REPO_ROOT\" && make setup"
elif awk -v c="$COMPONENT" '$1 == c { found = 1 } END { exit !found }' "$REPO_ROOT/tools/suite-bins"; then
  OUTPUT_DIR="$REPO_ROOT/bin"
  BUILD_HINT="cd \"$REPO_ROOT\" && make $COMPONENT"
else
  echo "error: unknown component '$COMPONENT'" >&2
  usage >&2
  exit 2
fi

release_var="$(tr '[:lower:]' '[:upper:]' <<<"${COMPONENT//-/_}")_RELEASE"
RELEASE_TAG="${!release_var:-latest}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) RELEASE_TAG="${2:?--release needs a tag}"; shift 2 ;;
    -h | --help) usage; exit 0 ;;
    *) OUTPUT_DIR="$1"; shift ;;
  esac
done

# A way-embed that was already installed and running is kept as it is, as the
# capability gate below applies only to a fresh download.
preinstalled=false
[[ -x "$OUTPUT_DIR/$COMPONENT" ]] && "$OUTPUT_DIR/$COMPONENT" --version >/dev/null 2>&1 && preinstalled=true

installed="$(prebuilt_install "$COMPONENT" "$RELEASE_TAG" "$OUTPUT_DIR" "$GH_REPO" "$BUILD_HINT")"

if [[ "$COMPONENT" == "way-embed" && "$preinstalled" == false ]]; then
  # Capability gate: the late-interaction matcher (ADR-160) requires `match --batch`,
  # added in #319. A release binary cut before that runs fine (`--version` passes)
  # but lacks --batch, and the matcher then silently degrades to the single-vector
  # fail-safe on every scan. The version string does not distinguish them (both
  # report 0.1.0), so probe the contract directly. The binary exits 1 either way
  # (a supporting one wants --corpus and --model; an old one rejects the flag), so
  # only the text decides: a supporting binary names `--batch` in its usage line,
  # an old one prints `unknown option`.
  probe="$("$installed" match --batch </dev/null 2>&1 || true)"
  if [[ "$probe" == *"unknown option"* ]] || [[ "$probe" != *"--batch"* ]]; then
    echo "Downloaded way-embed predates the required 'match --batch' primitive (ADR-160, #319)." >&2
    echo "Building from source instead." >&2
    rm -f "$installed" "$OUTPUT_DIR/way-embed-$(detect_platform)"
    exit 1
  fi
fi

echo "$installed"
