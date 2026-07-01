#!/usr/bin/env bash
# Download the pre-built mmaid binary for the current platform
#
# Detects OS/arch, downloads from GitHub Releases, verifies it runs.
# Binary is placed at: ${XDG_CACHE_HOME:-~/.cache}/agent-ways/user/mmaid
#
# Usage:
#   download-mmaid.sh [--release TAG]

set -euo pipefail

# Platform detection — mmaid uses GOOS-GOARCH naming
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)  GOARCH="amd64" ;;
  aarch64) GOARCH="arm64" ;;
  arm64)   GOARCH="arm64" ;;
  *)       echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac
PLATFORM="${OS}-${GOARCH}"

GH_REPO="aaronsb/mmaid-go"
RELEASE_TAG="${MMAID_RELEASE:-latest}"
BIN_NAME="mmaid-${PLATFORM}"
# Windows gets .exe suffix
[[ "$OS" == "windows" ]] && BIN_NAME="${BIN_NAME}.exe"

XDG_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"
OUTPUT_DIR="${XDG_CACHE}/agent-ways/user"
OUTPUT_FILE="${OUTPUT_DIR}/mmaid"

# Parse args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) RELEASE_TAG="$2"; shift 2 ;;
    --help|-h)
      B='' D='' C='' R=''
      if [[ -t 1 ]]; then B='\033[1m' D='\033[2m' C='\033[0;36m' R='\033[0m'; fi
      echo -e "${B}download-mmaid${R} — Install mmaid terminal diagram renderer"
      echo ""
      echo -e "  ${C}Usage:${R}  download-mmaid.sh [--release TAG]"
      echo ""
      echo -e "  ${D}--release TAG  GitHub Release tag (default: latest)${R}"
      echo -e "  ${D}Platform: ${PLATFORM}${R}"
      echo -e "  ${D}Source: github.com/${GH_REPO}${R}"
      exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# Check if already present and working
if [[ -x "$OUTPUT_FILE" ]] && "$OUTPUT_FILE" --version >/dev/null 2>&1; then
  echo "mmaid already installed and working: $OUTPUT_FILE" >&2
  "$OUTPUT_FILE" --version >&2
  echo "$OUTPUT_FILE"
  exit 0
fi

# Need gh CLI
if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh CLI not found — install it or download mmaid manually:" >&2
  echo "  https://github.com/${GH_REPO}/releases" >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"

# Find the latest release
if [[ "$RELEASE_TAG" == "latest" ]]; then
  RELEASE_TAG=$(gh release list --repo "$GH_REPO" --limit 1 --json tagName --jq '.[0].tagName' 2>/dev/null)
  if [[ -z "$RELEASE_TAG" ]]; then
    echo "No releases found in ${GH_REPO}" >&2
    exit 1
  fi
fi

echo "Platform: ${PLATFORM}" >&2
echo "Release:  ${RELEASE_TAG}" >&2

# Check if our platform binary exists in the release
if ! gh release view "$RELEASE_TAG" --repo "$GH_REPO" --json assets --jq '.assets[].name' 2>/dev/null | grep -q "^${BIN_NAME}$"; then
  echo "No pre-built binary for ${PLATFORM} in release ${RELEASE_TAG}." >&2
  echo "Available binaries:" >&2
  gh release view "$RELEASE_TAG" --repo "$GH_REPO" --json assets --jq '.assets[].name' 2>/dev/null | grep "mmaid-" | sed 's/^/  /' >&2
  echo "" >&2
  echo "Build from source: go install github.com/${GH_REPO}/cmd/mmaid@latest" >&2
  exit 1
fi

# Download
echo "Downloading ${BIN_NAME}..." >&2
gh release download "$RELEASE_TAG" \
  --repo "$GH_REPO" \
  --pattern "$BIN_NAME" \
  --dir "$OUTPUT_DIR" \
  --clobber

# Install
PLATFORM_FILE="${OUTPUT_DIR}/${BIN_NAME}"
chmod +x "$PLATFORM_FILE"
cp "$PLATFORM_FILE" "$OUTPUT_FILE"
chmod +x "$OUTPUT_FILE"

# Verify
if "$OUTPUT_FILE" --version >/dev/null 2>&1; then
  echo "Installed: $OUTPUT_FILE ($("$OUTPUT_FILE" --version))" >&2
  ls -lh "$OUTPUT_FILE" >&2
else
  echo "WARNING: binary downloaded but won't execute on this platform" >&2
  echo "Build from source: go install github.com/${GH_REPO}/cmd/mmaid@latest" >&2
  rm -f "$OUTPUT_FILE" "$PLATFORM_FILE"
  exit 1
fi

echo "$OUTPUT_FILE"
