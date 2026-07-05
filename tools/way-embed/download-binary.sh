#!/usr/bin/env bash
# Download the pre-built way-embed binary for the current platform
#
# Detects OS/arch, downloads from GitHub Releases, verifies it runs.
# Falls back to build-from-source instructions if no pre-built binary exists.
#
# Usage:
#   download-binary.sh [--release TAG] [output-dir]
#
# The binary is placed at: ${XDG_CACHE_HOME:-~/.cache}/agent-ways/user/way-embed

set -euo pipefail

# Platform detection
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
PLATFORM="${OS}-${ARCH}"

GH_REPO="aaronsb/agent-ways"
RELEASE_TAG="${WAY_EMBED_RELEASE:-latest}"
BIN_NAME="way-embed-${PLATFORM}"
XDG_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"
OUTPUT_DIR="${XDG_CACHE}/agent-ways/user"
# App source dir (has the Makefile + way-embed source) — for build-from-source hints.
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"

# Parse args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE_TAG="$2"
      shift 2 ;;
    --help|-h)
      echo "Usage: $0 [--release TAG] [output-dir]"
      echo ""
      echo "  --release TAG  GitHub Release tag (default: latest way-embed-* release)"
      echo "  output-dir     Override output directory (default: \$XDG_CACHE_HOME/agent-ways/user/)"
      echo ""
      echo "Platform: ${PLATFORM}"
      echo "Available: linux-x86_64, linux-aarch64, darwin-x86_64, darwin-arm64"
      exit 0 ;;
    *)
      OUTPUT_DIR="$1"
      shift ;;
  esac
done

OUTPUT_FILE="${OUTPUT_DIR}/way-embed"
PLATFORM_FILE="${OUTPUT_DIR}/${BIN_NAME}"

# Check if already present and working
if [[ -x "$OUTPUT_FILE" ]] && "$OUTPUT_FILE" --version >/dev/null 2>&1; then
  echo "way-embed already installed and working: $OUTPUT_FILE" >&2
  "$OUTPUT_FILE" --version >&2
  echo "$OUTPUT_FILE"
  exit 0
fi

# Need gh CLI
if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh CLI not found — install it or build from source:" >&2
  echo "  cd $APP_DIR/tools/way-embed && make" >&2
  exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Find the latest way-embed release
if [[ "$RELEASE_TAG" == "latest" ]]; then
  RELEASE_TAG=$(gh release list --repo "$GH_REPO" --limit 20 --json tagName --jq '.[].tagName' 2>/dev/null \
    | grep '^way-embed-v' | head -1)
  if [[ -z "$RELEASE_TAG" ]]; then
    echo "No way-embed release found. Build from source:" >&2
    echo "  cd $APP_DIR && make setup" >&2
    exit 1
  fi
fi

echo "Platform: ${PLATFORM}" >&2
echo "Release:  ${RELEASE_TAG}" >&2

# Check if our platform binary exists in the release
if ! gh release view "$RELEASE_TAG" --repo "$GH_REPO" --json assets --jq '.assets[].name' 2>/dev/null | grep -q "^${BIN_NAME}$"; then
  echo "No pre-built binary for ${PLATFORM} in release ${RELEASE_TAG}." >&2
  echo "Available binaries:" >&2
  gh release view "$RELEASE_TAG" --repo "$GH_REPO" --json assets --jq '.assets[].name' 2>/dev/null | grep "way-embed-" | sed 's/^/  /' >&2
  echo "" >&2
  echo "Build from source instead:" >&2
  echo "  cd $APP_DIR && make setup" >&2
  exit 1
fi

# Download binary + checksums
echo "Downloading ${BIN_NAME}..." >&2
gh release download "$RELEASE_TAG" \
  --repo "$GH_REPO" \
  --pattern "$BIN_NAME" \
  --dir "$OUTPUT_DIR" \
  --clobber

# Verify checksum (if checksums.txt exists in release)
CHECKSUMS_FILE="${OUTPUT_DIR}/checksums.txt"
if gh release download "$RELEASE_TAG" \
    --repo "$GH_REPO" \
    --pattern "checksums.txt" \
    --dir "$OUTPUT_DIR" \
    --clobber 2>/dev/null; then
  expected_hash=$(grep "${BIN_NAME}" "$CHECKSUMS_FILE" | awk '{print $1}')
  if [[ -n "$expected_hash" ]]; then
    actual_hash=$(sha256sum "$PLATFORM_FILE" 2>/dev/null | cut -d' ' -f1 \
      || shasum -a 256 "$PLATFORM_FILE" 2>/dev/null | cut -d' ' -f1)
    if [[ "$actual_hash" != "$expected_hash" ]]; then
      echo "CHECKSUM MISMATCH for ${BIN_NAME}" >&2
      echo "  Expected: ${expected_hash}" >&2
      echo "  Got:      ${actual_hash}" >&2
      rm -f "$PLATFORM_FILE" "$CHECKSUMS_FILE"
      exit 1
    fi
    echo "Checksum verified: ${actual_hash:0:12}..." >&2
  fi
  rm -f "$CHECKSUMS_FILE"
fi

# Make executable and install
chmod +x "$PLATFORM_FILE"
cp "$PLATFORM_FILE" "$OUTPUT_FILE"
chmod +x "$OUTPUT_FILE"

# Verify it runs
if "$OUTPUT_FILE" --version >/dev/null 2>&1; then
  echo "Installed: $OUTPUT_FILE ($("$OUTPUT_FILE" --version))" >&2
  ls -lh "$OUTPUT_FILE" >&2
else
  echo "WARNING: binary downloaded but won't execute on this platform" >&2
  echo "Build from source instead:" >&2
  echo "  cd $APP_DIR/tools/way-embed && make" >&2
  rm -f "$OUTPUT_FILE" "$PLATFORM_FILE"
  exit 1
fi

# Capability gate: the late-interaction matcher (ADR-160) requires `match --batch`,
# added in #319. A release binary cut before that runs fine (`--version` passes) but
# lacks --batch — and the matcher then silently degrades to the single-vector
# fail-safe on every scan, so ADR-160 is effectively off. The version string does not
# distinguish them (both report 0.1.0), so probe the contract directly: the supporting
# binary names `--batch` in its `match` usage. Reject a binary that does not, so
# `setup-binary` falls through to a source build until a new release is cut.
if ! "$OUTPUT_FILE" match --batch </dev/null 2>&1 | grep -q -- "--batch"; then
  echo "Downloaded way-embed predates the required 'match --batch' primitive (ADR-160, #319)." >&2
  echo "Building from source instead." >&2
  rm -f "$OUTPUT_FILE" "$PLATFORM_FILE"
  exit 1
fi

# Output path for scripts to capture
echo "$OUTPUT_FILE"
