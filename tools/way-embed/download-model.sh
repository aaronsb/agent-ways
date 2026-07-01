#!/usr/bin/env bash
# Download embedding models for way-embed
#
# Models:
#   English (default): all-MiniLM-L6-v2 Q5_K_M (21MB) — precise EN matching
#   Multilingual:      paraphrase-multilingual-MiniLM-L12-v2 Q8_0 (127MB) — 52 languages
#
# Usage:
#   download-model.sh [--upstream] [--f16] [--multilingual] [output-dir]
#
# Models are placed in the XDG cache directory by default:
#   ${XDG_CACHE_HOME:-$HOME/.cache}/agent-ways/user/

set -euo pipefail

# Default: English Q5_K_M (21MB)
MODEL_NAME="minilm-l6-v2.gguf"
QUANT="Q5_K_M"
EXPECTED_SHA256="60c7e141495321c7d303ec5ccc79296cfeb044263af840c583fed695d423aee8"
HF_FILENAME="all-MiniLM-L6-v2-Q5_K_M.gguf"
MODEL_SIZE="21MB"

# Source URLs
HF_BASE="https://huggingface.co/second-state/All-MiniLM-L6-v2-Embedding-GGUF/resolve/main"
MULTI_HF_BASE="https://huggingface.co/mykor/paraphrase-multilingual-MiniLM-L12-v2.gguf/resolve/main"
GH_REPO="aaronsb/agent-ways"
GH_RELEASE_TAG="v0.1.0-model"

# Defaults
SOURCE="github"
XDG_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"
OUTPUT_DIR="${XDG_CACHE}/agent-ways/user"

# Parse args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --upstream)
      SOURCE="huggingface"
      shift ;;
    --f16)
      QUANT="F16"
      EXPECTED_SHA256="797b70c4edf85907fe0a49eb85811256f65fa0f7bf52166b147fd16be2be4662"
      HF_FILENAME="all-MiniLM-L6-v2-ggml-model-f16.gguf"
      MODEL_SIZE="44MB"
      shift ;;
    --multilingual)
      MODEL_NAME="multilingual-minilm-l12-v2-q8.gguf"
      QUANT="Q8_0"
      HF_FILENAME="paraphrase-multilingual-MiniLM-L12-118M-v2-Q8_0.gguf"
      HF_BASE="$MULTI_HF_BASE"
      # No GitHub release yet for this model — HuggingFace only
      SOURCE="huggingface"
      EXPECTED_SHA256="1bcc1fd3f65d1269f8e733218d77ac5a479796fade0e87257606c2c4f2854662"
      MODEL_SIZE="127MB"
      shift ;;
    --help|-h)
      echo "Usage: $0 [--upstream] [--f16] [--multilingual] [output-dir]"
      echo ""
      echo "  --upstream      Download directly from HuggingFace (verify provenance)"
      echo "  --f16           Download full-precision F16 (44MB) instead of Q5_K_M (21MB)"
      echo "  --multilingual  Download multilingual model (127MB, 52 languages)"
      echo "  output-dir      Override output directory (default: \$XDG_CACHE_HOME/agent-ways/user/)"
      echo ""
      echo "Models:"
      echo "  English:      all-MiniLM-L6-v2 Q5_K_M (21MB)"
      echo "  Multilingual: paraphrase-multilingual-MiniLM-L12-v2 Q8_0 (127MB)"
      exit 0 ;;
    *)
      OUTPUT_DIR="$1"
      shift ;;
  esac
done

HF_URL="${HF_BASE}/${HF_FILENAME}"
GH_URL="https://github.com/${GH_REPO}/releases/download/${GH_RELEASE_TAG}/${MODEL_NAME}"
OUTPUT_FILE="${OUTPUT_DIR}/${MODEL_NAME}"

# Check if already present and valid
if [[ -f "$OUTPUT_FILE" ]]; then
  if [[ -n "$EXPECTED_SHA256" ]]; then
    existing_hash=$(sha256sum "$OUTPUT_FILE" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$OUTPUT_FILE" 2>/dev/null | cut -d' ' -f1)
    if [[ "$existing_hash" == "$EXPECTED_SHA256" ]]; then
      echo "Model already present and verified: $OUTPUT_FILE" >&2
      echo "$OUTPUT_FILE"
      exit 0
    else
      echo "WARNING: existing model has wrong checksum (maybe different quant), re-downloading" >&2
    fi
  else
    echo "Model already present: $OUTPUT_FILE (no pinned checksum)" >&2
    echo "$OUTPUT_FILE"
    exit 0
  fi
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Select URL
if [[ "$SOURCE" == "huggingface" ]]; then
  URL="$HF_URL"
  echo "Downloading from HuggingFace (upstream)..." >&2
else
  URL="$GH_URL"
  echo "Downloading from GitHub Release..." >&2
  # Fall back to HuggingFace if GitHub release doesn't exist yet
  if ! curl -fsSL --head "$URL" >/dev/null 2>&1; then
    echo "GitHub release not found, falling back to HuggingFace upstream..." >&2
    URL="$HF_URL"
    SOURCE="huggingface"
  fi
fi

# Download
TMPFILE="${OUTPUT_FILE}.tmp.$$"
trap 'rm -f "$TMPFILE"' EXIT

echo "Downloading ${MODEL_NAME} (${QUANT}, ${MODEL_SIZE})..." >&2
if command -v curl >/dev/null 2>&1; then
  curl -fSL --progress-bar -o "$TMPFILE" "$URL"
elif command -v wget >/dev/null 2>&1; then
  wget -q --show-progress -O "$TMPFILE" "$URL"
else
  echo "error: need curl or wget to download the model" >&2
  exit 1
fi

# Verify checksum
actual_hash=$(sha256sum "$TMPFILE" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$TMPFILE" 2>/dev/null | cut -d' ' -f1)

if [[ -n "$EXPECTED_SHA256" ]]; then
  echo "Verifying checksum..." >&2
  if [[ "$actual_hash" != "$EXPECTED_SHA256" ]]; then
    echo "CHECKSUM MISMATCH" >&2
    echo "  Expected: ${EXPECTED_SHA256}" >&2
    echo "  Got:      ${actual_hash}" >&2
    echo "" >&2
    echo "The downloaded file does not match the expected hash." >&2
    echo "If using --upstream, the model may have been updated on HuggingFace." >&2
    echo "If using GitHub release, the release artifact may be corrupt." >&2
    rm -f "$TMPFILE"
    exit 1
  fi
else
  echo "No pinned checksum — recording hash for future verification:" >&2
  echo "  SHA-256: ${actual_hash}" >&2
fi

# Atomic move
mv "$TMPFILE" "$OUTPUT_FILE"
echo "Verified and installed: $OUTPUT_FILE" >&2
echo "  Source: ${SOURCE} (${URL})" >&2
echo "  Quant: ${QUANT}" >&2
echo "  SHA-256: ${actual_hash}" >&2

# Output path for scripts to capture
echo "$OUTPUT_FILE"
