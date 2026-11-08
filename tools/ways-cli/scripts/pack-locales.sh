#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Pack individual .lang.md locale stubs into .locales.jsonl files.
# Usage: pack-locales.sh [--dry-run] [ways-dir]
set -euo pipefail

DRY_RUN=false
WAYS_DIR="${HOME}/.claude/hooks/ways"

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    *) WAYS_DIR="$arg" ;;
  esac
done

packed=0
deleted=0

# Find all locale stub files (*.lang.md where lang is 2-5 lowercase chars)
find -L "$WAYS_DIR" -name '*.*.md' -not -name '*.check.md' -type f | sort | while IFS= read -r stubfile; do
  fname=$(basename "$stubfile")
  stem="${fname%.md}"

  # Extract candidate locale code (last dot-separated segment)
  lang="${stem##*.}"
  wayname="${stem%.*}"

  # Validate: 2-5 lowercase ascii chars (with optional hyphen for codes like pt-br)
  if ! echo "$lang" | grep -qE '^[a-z]{2,5}(-[a-z]{2,5})?$'; then
    continue
  fi

  # Skip inactive languages
  LANGUAGES_JSON="${WAYS_DIR}/../../tools/ways-cli/languages.json"
  if [ -f "$LANGUAGES_JSON" ]; then
    if ! jq -e ".languages.\"${lang}\".active == true" "$LANGUAGES_JSON" >/dev/null 2>&1; then
      echo "SKIP (inactive language): $stubfile" >&2
      continue
    fi
  fi

  dir=$(dirname "$stubfile")
  jsonl_file="${dir}/${wayname}.locales.jsonl"

  # Parse YAML frontmatter
  desc=""
  vocab=""
  in_fm=false
  while IFS= read -r line; do
    if [ "$line" = "---" ]; then
      if $in_fm; then break; fi
      in_fm=true
      continue
    fi
    if $in_fm; then
      case "$line" in
        description:*) desc="${line#description: }" ;;
        vocabulary:*) vocab="${line#vocabulary: }" ;;
      esac
    fi
  done < "$stubfile"

  if [ -z "$desc" ]; then
    echo "SKIP (no description): $stubfile" >&2
    continue
  fi

  # Build JSON line (escape for JSON)
  json_line=$(printf '{"lang":"%s","description":"%s","vocabulary":"%s"}' \
    "$lang" \
    "$(echo "$desc" | sed 's/"/\\"/g')" \
    "$(echo "$vocab" | sed 's/"/\\"/g')")

  if $DRY_RUN; then
    echo "PACK: $stubfile -> $jsonl_file"
    echo "  $json_line"
  else
    echo "$json_line" >> "$jsonl_file"
    rm "$stubfile"
    deleted=$((deleted + 1))
  fi
done

if ! $DRY_RUN; then
  # Sort each .locales.jsonl by lang for determinism
  find -L "$WAYS_DIR" -name '*.locales.jsonl' -type f | while IFS= read -r jf; do
    sort -o "$jf" "$jf"
    packed=$((packed + 1))
  done
  echo "Packed locale stubs into $(find -L "$WAYS_DIR" -name '*.locales.jsonl' | wc -l) .locales.jsonl files"
  echo "Deleted $(find -L "$WAYS_DIR" -name '*.*.md' -not -name '*.check.md' -not -name '*.locales.jsonl' | wc -l) remaining stub files"
fi
