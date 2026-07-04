#!/usr/bin/env bash
# Download the pre-built attend binary for the current platform
#
# Detects OS/arch, downloads from GitHub Releases, verifies it runs.
# Falls back to build-from-source instructions if no pre-built binary exists.
#
# Usage:
#   download-attend.sh [--release TAG] [output-dir]
#
# The binary is placed at: bin/attend (relative to repo root)

set -euo pipefail

# Shared retry/backoff helpers — a transient gh/API blip must not silently
# degrade `ways update` to a from-source build.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../scripts" && pwd)/prebuilt-lib.sh"

# Platform detection
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m | sed 's/arm64/aarch64/')
PLATFORM="${OS}-${ARCH}"

GH_REPO="aaronsb/agent-ways"
RELEASE_TAG="${ATTEND_RELEASE:-latest}"
BIN_NAME="attend-${PLATFORM}"

# Default output: repo bin/ directory
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_DIR="${REPO_ROOT}/bin"

# Parse args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE_TAG="$2"
      shift 2 ;;
    --help|-h)
      echo "Usage: $0 [--release TAG] [output-dir]"
      echo ""
      echo "  --release TAG  GitHub Release tag (default: latest attend-* release)"
      echo "  output-dir     Override output directory (default: bin/)"
      echo ""
      echo "Platform: ${PLATFORM}"
      echo "Available: linux-x86_64, linux-aarch64, darwin-x86_64, darwin-arm64"
      exit 0 ;;
    *)
      OUTPUT_DIR="$1"
      shift ;;
  esac
done

OUTPUT_FILE="${OUTPUT_DIR}/attend"
PLATFORM_FILE="${OUTPUT_DIR}/${BIN_NAME}"

# Check if already present and working
if [[ -x "$OUTPUT_FILE" ]] && "$OUTPUT_FILE" --version >/dev/null 2>&1; then
  echo "attend already installed and working: $OUTPUT_FILE" >&2
  "$OUTPUT_FILE" --version >&2
  echo "$OUTPUT_FILE"
  exit 0
fi

# Need gh CLI
if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh CLI not found — build from source instead:" >&2
  echo "  cd \"$REPO_ROOT\" && make attend" >&2
  exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Find the latest attend release
if [[ "$RELEASE_TAG" == "latest" ]]; then
  # Retry transient API failures, then distinguish "couldn't reach the API"
  # (retries exhausted → honest error) from "reached it, no matching release".
  if ! release_tags=$(retry gh release list --repo "$GH_REPO" --limit 30 --json tagName --jq '.[].tagName'); then
    echo "error: could not reach GitHub Releases after retries (network/gh/auth?)." >&2
    echo "  Falling back to build-from-source: cd \"$REPO_ROOT\" && make attend" >&2
    exit 1
  fi
  RELEASE_TAG=$(echo "$release_tags" | grep '^attend-v' | head -1)
  if [[ -z "$RELEASE_TAG" ]]; then
    echo "No attend release found. Build from source:" >&2
    echo "  cd \"$REPO_ROOT\" && make attend" >&2
    exit 1
  fi
fi

echo "Platform: ${PLATFORM}" >&2
echo "Release:  ${RELEASE_TAG}" >&2

# Check if our platform binary exists in the release
if ! release_assets=$(retry gh release view "$RELEASE_TAG" --repo "$GH_REPO" --json assets --jq '.assets[].name'); then
  echo "error: could not read assets of ${RELEASE_TAG} after retries (network/gh/auth?)." >&2
  echo "  Falling back to build-from-source: cd \"$REPO_ROOT\" && make attend" >&2
  exit 1
fi
if ! echo "$release_assets" | grep -q "^${BIN_NAME}$"; then
  echo "No pre-built binary for ${PLATFORM} in release ${RELEASE_TAG}." >&2
  echo "Available binaries:" >&2
  echo "$release_assets" | grep "^attend-" | sed 's/^/  /' >&2
  echo "" >&2
  echo "Build from source instead:" >&2
  echo "  cd \"$REPO_ROOT\" && make attend" >&2
  exit 1
fi

# Download binary + checksums
echo "Downloading ${BIN_NAME}..." >&2
if ! retry gh release download "$RELEASE_TAG" \
    --repo "$GH_REPO" \
    --pattern "$BIN_NAME" \
    --dir "$OUTPUT_DIR" \
    --clobber; then
  echo "error: download of ${BIN_NAME} failed after retries — building from source instead." >&2
  echo "  cd \"$REPO_ROOT\" && make attend" >&2
  exit 1
fi

# Verify checksum
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
else
  echo "WARNING: checksums.txt not found in release — skipping verification" >&2
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
  echo "  cd \"$REPO_ROOT\" && make attend" >&2
  rm -f "$OUTPUT_FILE" "$PLATFORM_FILE"
  exit 1
fi

# Output path for scripts to capture
echo "$OUTPUT_FILE"
