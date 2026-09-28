#!/usr/bin/env bash
# Host-side entry point for the live fixture (ADR-186): `make test-live TIER=1`.
#
# Builds the tier image and runs the tier's runner in a fresh container.
#
#   TIER              1 (install and configure, no key) or 2 (tier 1, then
#                     model scenarios through `claude -p`; needs a key)
#   FLAVOR            branch (default) or release
#   CLAUDE_VERSION    Claude Code version for the image. The pin below is the
#                     one place it is set; compose.yaml and the Dockerfile
#                     take it from here.
#   CLAUDE_INSTALLER  native (default) or npm
#   WAYS_BINARIES     dir with ways, ways-audit, attend, attend-chat (branch flavor)
#   GH_TOKEN          taken from `gh auth token` when unset
#   ANTHROPIC_API_KEY       tier 2 key; or set ANTHROPIC_API_KEY_FILE to a file
#   ANTHROPIC_API_KEY_FILE  holding it. The key is exported, never printed.
#   TIER2_OUT         host dir for tier 2 transcripts (default: a fresh temp dir)

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
TIER="${TIER:-1}"
FLAVOR="${FLAVOR:-branch}"
CLAUDE_VERSION_PIN="2.1.275"

case "$TIER" in
  1) SERVICE=tier1 ;;
  2) SERVICE=tier2 ;;
  *) echo "TIER must be 1 or 2" >&2; exit 2 ;;
esac

command -v docker >/dev/null || { echo "docker is required" >&2; exit 2; }
docker compose version >/dev/null 2>&1 || { echo "docker compose (v2) is required" >&2; exit 2; }

export FLAVOR
export CLAUDE_VERSION="${CLAUDE_VERSION:-$CLAUDE_VERSION_PIN}"
export CLAUDE_INSTALLER="${CLAUDE_INSTALLER:-native}"
# `latest` names a moving target, so the installer layer must not stay cached
# on a workstation. The date rebuilds it once a day. A pinned version leaves
# CACHE_BUST empty and keeps full caching.
if [[ "$CLAUDE_VERSION" == "latest" ]]; then
  export CACHE_BUST="$(date -u +%F)"
else
  export CACHE_BUST=""
fi
if [[ -z "${GH_TOKEN:-}" ]] && command -v gh >/dev/null; then
  GH_TOKEN="$(gh auth token 2>/dev/null || true)"
fi
export GH_TOKEN="${GH_TOKEN:-}"

if [[ "$FLAVOR" == "branch" ]]; then
  export WAYS_BINARIES="${WAYS_BINARIES:-$REPO_ROOT/tools/target/release}"
  for b in ways ways-audit attend attend-chat; do
    if [[ ! -x "$WAYS_BINARIES/$b" ]]; then
      echo "missing $WAYS_BINARIES/$b" >&2
      echo "build the suite first:" >&2
      echo "  cargo build --release --manifest-path tools/Cargo.toml -p ways -p ways-audit -p attend -p attend-chat" >&2
      exit 2
    fi
  done
else
  # The release flavor downloads its binaries. Mount an empty scratch dir so
  # compose never creates tools/target/release on the host (root-owned under
  # a rootful daemon, which then breaks the next cargo build).
  export WAYS_BINARIES="$(mktemp -d)"
fi

if [[ "$TIER" == "2" ]]; then
  if [[ -z "${ANTHROPIC_API_KEY:-}" && -n "${ANTHROPIC_API_KEY_FILE:-}" ]]; then
    [[ -r "$ANTHROPIC_API_KEY_FILE" ]] || { echo "cannot read ANTHROPIC_API_KEY_FILE" >&2; exit 2; }
    ANTHROPIC_API_KEY="$(tr -d '[:space:]' < "$ANTHROPIC_API_KEY_FILE")"
  fi
  [[ -n "${ANTHROPIC_API_KEY:-}" ]] || { echo "tier 2 needs ANTHROPIC_API_KEY or ANTHROPIC_API_KEY_FILE" >&2; exit 2; }
  export ANTHROPIC_API_KEY
  export TIER2_OUT="${TIER2_OUT:-$(mktemp -d -t agent-ways-tier2.XXXXXX)}"
  # Create it as the host user. A rootful daemon creates a missing bind
  # source as root, and the container user then cannot write to it.
  mkdir -p "$TIER2_OUT"
  [[ -w "$TIER2_OUT" ]] || { echo "TIER2_OUT is not writable: $TIER2_OUT" >&2; exit 2; }
  # The container user is uid 1000, and the host user may not be (a CI runner
  # is 1001). The dir holds only this run's transcripts, so open it to all.
  chmod 0777 "$TIER2_OUT"
  echo "tier 2 transcripts: $TIER2_OUT"
fi

cd "$HERE"
docker compose build "$SERVICE"
exec docker compose run --rm "$SERVICE"
