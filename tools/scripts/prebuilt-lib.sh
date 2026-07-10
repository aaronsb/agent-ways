#!/usr/bin/env bash
# Shared helpers for the per-component pre-built download scripts
# (download-attend.sh, download-attend-chat.sh, download-ways.sh,
# download-ways-audit.sh). Sourced, not executed.
#
# Why this exists: a transient GitHub API / network blip on a `gh release`
# call used to be swallowed (the call was `... 2>/dev/null`), so an empty
# result read as "no release" and `ways update` silently degraded to a
# from-source build. These helpers retry transient failures and — critically —
# distinguish "the API call failed" from "the API succeeded but found nothing",
# so a real blip is retried and, if it persists, reported honestly instead of
# masquerading as a missing binary.

# Run a command, retrying on failure with exponential backoff. Returns the
# command's own exit status once it succeeds, or non-zero after the last try.
# Progress notes go to stderr so callers can capture stdout cleanly.
# Echo the platform slug used in release asset names (`<component>-<platform>`),
# e.g. `ways-darwin-arm64`.
#
# ARM64 has two spellings for one architecture, and `uname -m` reports whichever
# the kernel prefers: `arm64` on Darwin, `aarch64` on Linux. The release assets
# follow the same split — `darwin-arm64` but `linux-aarch64`. So the mapping is
# per-OS, not global: a blanket `arm64`→`aarch64` rewrite asks for
# `darwin-aarch64`, an asset that has never been published, and every Apple
# Silicon download 404s into a silent from-source build.
detect_platform() {
  local os arch
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)
  case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64)
      case "$os" in
        darwin) arch=arm64 ;;
        *) arch=aarch64 ;;
      esac
      ;;
  esac
  printf '%s-%s\n' "$os" "$arch"
}

retry() {
  local n=1 max="${RETRY_MAX:-3}" delay="${RETRY_DELAY:-2}"
  while true; do
    if "$@"; then
      return 0
    fi
    if [[ "$n" -ge "$max" ]]; then
      return 1
    fi
    echo "  (attempt ${n}/${max} failed; retrying in ${delay}s...)" >&2
    sleep "$delay"
    n=$((n + 1))
    delay=$((delay * 2))
  done
}
