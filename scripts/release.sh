#!/usr/bin/env bash
# Cut a release for one Cargo-versioned component (ADR-150): bump its version,
# refresh the lockfile, commit, and tag — all LOCAL. Publishing (pushing the
# commit + tag, which triggers CI to build every platform and create the GitHub
# Release) is left as an explicit, printed step so the outward action stays a
# deliberate choice, never a side effect.
#
# Usage:
#   scripts/release.sh <component> <patch|minor|major> [--push]
#     component : ways | attend | attend-chat
#     --push    : also push main + the tag (publishes). Omit to stop local and
#                 print the push commands.
#
# way-embed is NOT handled here — it versions through its own
# tools/way-embed/Makefile (`make release VERSION=...`).
set -euo pipefail

die() { echo "release: $*" >&2; exit 1; }

[[ $# -ge 2 ]] || die "usage: release.sh <ways|attend|attend-chat> <patch|minor|major> [--push]"
COMPONENT="$1"; LEVEL="$2"; PUSH="${3:-}"

case "$COMPONENT" in
  ways)        MANIFEST="tools/ways-cli/Cargo.toml"; PKG="ways" ;;
  attend)      MANIFEST="tools/attend/Cargo.toml"; PKG="attend" ;;
  attend-chat) MANIFEST="tools/attend-chat/Cargo.toml"; PKG="attend-chat" ;;
  *) die "unknown component '$COMPONENT' (ways|attend|attend-chat)" ;;
esac
TAG_PREFIX="${COMPONENT}-v"

# Resolve repo root so the script works from anywhere.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# --- Guards: releases are cut from a clean, up-to-date main. ---
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[[ "$BRANCH" == "main" ]] || die "must be on main to cut a release (on '$BRANCH')"
[[ -z "$(git status --porcelain)" ]] || die "working tree not clean — commit or stash first"
git fetch --quiet origin main
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
  || die "local main is not in sync with origin/main — pull/push first"

# --- Compute the next version from the current one. ---
CURRENT="$(grep -m1 -E '^version *= *"' "$MANIFEST" | sed -E 's/.*"([^"]+)".*/\1/')"
[[ "$CURRENT" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || die "unparseable version '$CURRENT' in $MANIFEST"
MAJOR="${BASH_REMATCH[1]}"; MINOR="${BASH_REMATCH[2]}"; PATCH="${BASH_REMATCH[3]}"
case "$LEVEL" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) die "level must be patch|minor|major (got '$LEVEL')" ;;
esac
NEXT="${MAJOR}.${MINOR}.${PATCH}"
TAG="${TAG_PREFIX}${NEXT}"

git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null && die "tag ${TAG} already exists"

echo "release: $COMPONENT $CURRENT -> $NEXT  (tag ${TAG})"

# --- Bump the manifest version (only the first `version = "..."` — the package
#     stanza), then refresh the lockfile entry for this package. ---
#     BSD/GNU sed differ on -i; write via a temp file to stay portable.
awk -v ver="$NEXT" '
  !done && /^version *= *"/ { sub(/"[^"]+"/, "\"" ver "\""); done=1 }
  { print }
' "$MANIFEST" > "$MANIFEST.tmp" && mv "$MANIFEST.tmp" "$MANIFEST"

# Sync Cargo.lock (best effort; a build would otherwise do it). --offline keeps
# it from touching the network; ignore failure so a lock-less checkout still cuts.
cargo update -p "$PKG" --precise "$NEXT" --manifest-path tools/Cargo.toml --offline >/dev/null 2>&1 \
  || cargo update -p "$PKG" --manifest-path tools/Cargo.toml --offline >/dev/null 2>&1 || true

git add "$MANIFEST" tools/Cargo.lock
git commit -q -m "release(${COMPONENT}): bump to ${NEXT}"
git tag -a "$TAG" -m "${COMPONENT} v${NEXT}"
echo "release: committed and tagged ${TAG} locally."

if [[ "$PUSH" == "--push" ]]; then
  echo "release: publishing (push main + tag)…"
  git push origin main
  git push origin "$TAG"
  echo "release: pushed. CI (build-${COMPONENT}.yml) will build all platforms and create the GitHub Release."
else
  cat <<EOF

Not pushed. To PUBLISH (triggers CI build of every platform + a GitHub Release):
  git push origin main
  git push origin ${TAG}

To ABORT this local cut:
  git tag -d ${TAG} && git reset --hard HEAD~1
EOF
fi
