#!/usr/bin/env bash
# Host-side entry point for the live fixture (ADR-186): `make test-live TIER=1`.
#
# Builds the tier image and runs the tier's runner in a fresh container.
#
#   TIER              1 (install and configure, no key). 2 is not built yet.
#   FLAVOR            branch (default) or release
#   CLAUDE_VERSION    Claude Code version for the image. The pin below is the
#                     one place it is set; compose.yaml and the Dockerfile
#                     take it from here.
#   CLAUDE_INSTALLER  native (default) or npm
#   WAYS_BINARIES     dir with ways, ways-audit, attend, attend-chat (branch flavor)
#   GH_TOKEN          taken from `gh auth token` when unset

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
TIER="${TIER:-1}"
FLAVOR="${FLAVOR:-branch}"
CLAUDE_VERSION_PIN="2.1.275"

case "$TIER" in
  1) SERVICE=tier1 ;;
  2) echo "tier 2 is not built yet (ADR-186 item 3)" >&2; exit 2 ;;
  *) echo "TIER must be 1 or 2" >&2; exit 2 ;;
esac

command -v docker >/dev/null || { echo "docker is required" >&2; exit 2; }
docker compose version >/dev/null 2>&1 || { echo "docker compose (v2) is required" >&2; exit 2; }

export FLAVOR
export CLAUDE_VERSION="${CLAUDE_VERSION:-$CLAUDE_VERSION_PIN}"
export CLAUDE_INSTALLER="${CLAUDE_INSTALLER:-native}"
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

cd "$HERE"
docker compose build "$SERVICE"
exec docker compose run --rm "$SERVICE"
