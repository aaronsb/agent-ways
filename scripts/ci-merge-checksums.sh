#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Write the checksums.txt that goes up with a release upload, for the release
# and attach-best-effort jobs of .github/workflows/reusable-build.yml.
#
# The file holds a line for each file in DIR, and the lines of the release's
# current checksums.txt for every file this upload does not replace. An upload
# replaces checksums.txt as a whole, so a file written from DIR alone would
# drop the lines of the binaries already on the release, and the installer
# then refuses those binaries.
#
# Only "release not found" from `gh release view` means the release does not
# exist yet. Any other failure is retried, and after the last try the script
# exits 1 without writing checksums.txt or uploading, because a guess here
# would wipe the other platforms' lines.
#
# Usage:
#   scripts/ci-merge-checksums.sh TAG DIR            write DIR/checksums.txt
#   scripts/ci-merge-checksums.sh --upload TAG DIR   also upload DIR's files and
#                                                    checksums.txt with --clobber;
#                                                    the release must exist
#
# GITHUB_REPOSITORY names the repository. RETRY_MAX (default 4) and
# RETRY_DELAY (seconds, default 5, doubled after each try) tune the retries.

set -euo pipefail

upload=0
if [[ "${1:-}" == --upload ]]; then
  upload=1
  shift
fi
tag="${1:?usage: $0 [--upload] TAG DIR}"
dir="${2:?usage: $0 [--upload] TAG DIR}"
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"
max="${RETRY_MAX:-4}"
delay="${RETRY_DELAY:-5}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Run a gh command, retrying each failure except "release not found".
# Returns 0 on success, 2 when the release does not exist, 1 otherwise.
gh_retry() {
  local n=1 d="$delay"
  while true; do
    if "$@" 2>"$work/err"; then
      return 0
    fi
    if grep -qi 'release not found' "$work/err"; then
      return 2
    fi
    cat "$work/err" >&2
    if [[ "$n" -ge "$max" ]]; then
      return 1
    fi
    echo "  (attempt ${n}/${max} failed; retrying in ${d}s)" >&2
    sleep "$d"
    n=$((n + 1))
    d=$((d * 2))
  done
}

cd "$dir"
rm -f checksums.txt
shopt -s nullglob
files=(*)
if [[ ${#files[@]} -eq 0 ]]; then
  echo "error: no files to upload in ${dir}" >&2
  exit 1
fi
sha256sum -- "${files[@]}" > "$work/new"

rc=0
assets=$(gh_retry gh release view "$tag" --repo "$repo" --json assets --jq '.assets[].name') || rc=$?
case "$rc" in
  0)
    if grep -qx checksums.txt <<<"$assets"; then
      if ! gh_retry gh release download "$tag" --repo "$repo" --pattern checksums.txt --output "$work/old"; then
        echo "error: could not download checksums.txt of ${tag}; nothing written or uploaded." >&2
        exit 1
      fi
      # Keep each old line whose file this upload does not replace.
      # sha256sum writes `<hash>  <name>`, or `<hash> *<name>` in binary mode.
      awk 'NR == FNR { new[$2]; next } !($2 in new) && !(substr($2, 2) in new)' \
        "$work/new" "$work/old" >> "$work/new"
    fi
    ;;
  2)
    if [[ "$upload" -eq 1 ]]; then
      echo "error: release ${tag} not found; nothing to attach to." >&2
      exit 1
    fi
    echo "Release ${tag} does not exist yet; checksums.txt covers this upload only." >&2
    ;;
  *)
    echo "error: could not read release ${tag} after ${max} tries; nothing written or uploaded." >&2
    exit 1
    ;;
esac

sort -k2 "$work/new" > checksums.txt
cat checksums.txt

if [[ "$upload" -eq 1 ]]; then
  if ! gh_retry gh release upload "$tag" --repo "$repo" --clobber "${files[@]}" checksums.txt; then
    echo "error: upload to ${tag} failed." >&2
    exit 1
  fi
fi
