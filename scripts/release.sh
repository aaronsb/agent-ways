#!/usr/bin/env bash
# ADR-150 release helper for a Cargo-versioned component. Two steps, because
# `main` is branch-protected and the repo is PR-first:
#
#   bump  — create a `release/<comp>-vX.Y.Z` branch with the version bump + a
#           verified Cargo.lock, push it, and open a PR. No tag yet; the bump is
#           reviewed and merged like any other change.
#   tag   — AFTER that PR merges to main: tag `<comp>-vX.Y.Z` on main and push the
#           tag. Pushing the tag is the one outward step — CI (build-<comp>.yml)
#           then builds every platform and creates the GitHub Release.
#
# Usage:
#   scripts/release.sh bump <ways|attend|attend-chat> <patch|minor|major>
#   scripts/release.sh tag  <ways|attend|attend-chat> [--push]
#
# `tag` without --push creates the annotated tag locally and prints the push
# command (keeps the publish deliberate); `--push` pushes it. way-embed is out of
# scope — it versions via tools/way-embed/Makefile.
set -euo pipefail

die() { echo "release: $*" >&2; exit 1; }

manifest_for() {
  case "$1" in
    ways)        echo "tools/ways-cli/Cargo.toml" ;;
    attend)      echo "tools/attend/Cargo.toml" ;;
    attend-chat) echo "tools/attend-chat/Cargo.toml" ;;
    *) die "unknown component '$1' (ways|attend|attend-chat)" ;;
  esac
}

current_version() {
  grep -m1 -E '^version *= *"' "$1" | sed -E 's/.*"([^"]+)".*/\1/'
}

# Read the recorded version of a package from Cargo.lock (empty if absent).
lock_version() {
  awk -v p="\"$1\"" '
    /^\[\[package\]\]/ { name="" }
    $1=="name" { name=$3 }
    $1=="version" && name==p { gsub(/"/,"",$3); print $3; exit }
  ' tools/Cargo.lock 2>/dev/null
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="${1:-}"; COMPONENT="${2:-}"
[[ -n "$MODE" && -n "$COMPONENT" ]] || die "usage: release.sh <bump|tag> <ways|attend|attend-chat> ..."
MANIFEST="$(manifest_for "$COMPONENT")"
PKG="$COMPONENT"   # package name == component name for all three
TAG_PREFIX="${COMPONENT}-v"

case "$MODE" in
  bump)
    LEVEL="${3:-}"
    [[ -n "$LEVEL" ]] || die "usage: release.sh bump $COMPONENT <patch|minor|major>"

    # Guards: bump is cut from a clean, in-sync main.
    [[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]] || die "run bump from main"
    [[ -z "$(git status --porcelain)" ]] || die "working tree not clean"
    git fetch --quiet origin main
    [[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
      || die "local main is not in sync with origin/main — pull/push first"

    CURRENT="$(current_version "$MANIFEST")"
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
    BRANCH="release/${TAG}"

    git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null && die "tag ${TAG} already exists"
    git rev-parse -q --verify "refs/heads/${BRANCH}" >/dev/null && die "branch ${BRANCH} already exists"

    echo "release: bump $COMPONENT $CURRENT -> $NEXT  (branch $BRANCH, tag-to-be $TAG)"
    git checkout -q -b "$BRANCH"

    # Bump ONLY the package's own version line (column-0 `version = "..."`), never a
    # dependency's `version` field. `!done` makes it a one-shot on the first match.
    awk -v ver="$NEXT" '
      !done && /^version *= *"/ { sub(/"[^"]+"/, "\"" ver "\""); done=1 }
      { print }
    ' "$MANIFEST" > "$MANIFEST.tmp" && mv "$MANIFEST.tmp" "$MANIFEST"

    # Sync Cargo.lock, then VERIFY it actually records the new version — a stale
    # lock in the release commit is the exact drift this helper exists to prevent.
    cargo update -p "$PKG" --manifest-path tools/Cargo.toml --offline >/dev/null 2>&1 \
      || cargo update -p "$PKG" --manifest-path tools/Cargo.toml >/dev/null 2>&1 || true
    GOT="$(lock_version "$PKG")"
    [[ "$GOT" == "$NEXT" ]] \
      || die "Cargo.lock still records $PKG=${GOT:-none}, not $NEXT — refusing to commit a stale lockfile. Run 'cargo update -p $PKG --manifest-path tools/Cargo.toml' and retry."

    git add "$MANIFEST" tools/Cargo.lock
    git commit -q -m "release(${COMPONENT}): bump to ${NEXT}"
    git push -q -u origin "$BRANCH"
    gh pr create --title "release(${COMPONENT}): v${NEXT}" \
      --body "Version bump for the ${COMPONENT} ${NEXT} release (ADR-150). After merge, tag it: \`make publish-release COMPONENT=${COMPONENT}\` — that pushes \`${TAG}\` and CI builds all platforms + creates the GitHub Release."
    cat <<EOF

Bump PR opened on branch ${BRANCH}. Next:
  1. review + merge the PR
  2. git checkout main && git pull
  3. make publish-release COMPONENT=${COMPONENT}   (tags ${TAG} on main + publishes)
EOF
    ;;

  tag)
    PUSH="${3:-}"
    # Guards: tag the MERGED bump on a clean, in-sync main.
    [[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]] || die "run tag from main (after the bump PR merged)"
    [[ -z "$(git status --porcelain)" ]] || die "working tree not clean"
    git fetch --quiet origin main
    [[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
      || die "local main is not in sync with origin/main — pull first"

    VERSION="$(current_version "$MANIFEST")"
    [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "unparseable version '$VERSION' in $MANIFEST"
    # The lock must agree — otherwise the bump PR wasn't merged (or is inconsistent).
    GOT="$(lock_version "$PKG")"
    [[ "$GOT" == "$VERSION" ]] || die "Cargo.lock records $PKG=${GOT:-none} but manifest is $VERSION — is the bump PR merged?"
    TAG="${TAG_PREFIX}${VERSION}"

    git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null && die "tag ${TAG} already exists locally"
    git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1 && die "tag ${TAG} already exists on origin"

    git tag -a "$TAG" -m "${COMPONENT} v${VERSION}"
    echo "release: tagged ${TAG} on $(git rev-parse --short HEAD)."
    if [[ "$PUSH" == "--push" ]]; then
      git push origin "$TAG"
      echo "release: pushed ${TAG}. CI (build-${COMPONENT}.yml) is building platforms + creating the GitHub Release."
    else
      cat <<EOF

Tag created locally, NOT pushed. To PUBLISH (CI builds platforms + GitHub Release):
  git push origin ${TAG}
To abort:
  git tag -d ${TAG}
EOF
    fi
    ;;

  *) die "unknown mode '$MODE' (bump|tag)" ;;
esac
