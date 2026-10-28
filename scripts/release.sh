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
#   scripts/release.sh bump <ways|ways-audit|ways-mcp|ways-agent|attend|attend-chat> <patch|minor|major>
#   scripts/release.sh tag  <ways|ways-audit|ways-mcp|ways-agent|attend|attend-chat> [--push]
#   scripts/release.sh check <component>
#   scripts/release.sh mod-version <component> <version>   # MOD_VERSION alone, as bump moves it
#
# Both bump and tag refuse a release whose range, from the component's last tag
# to HEAD, changed plugins/attend/ without moving the plugin's version; `check`
# runs that check alone. Bumping attend moves the plugin's version with it;
# bumping ways moves the ways-judge and ways-theme plugins' versions.
#
# `tag` without --push creates the annotated tag locally and prints the push
# command (keeps the publish deliberate); `--push` pushes it. way-embed is out of
# scope — it versions via tools/way-embed/Makefile.
set -euo pipefail

die() { echo "release: $*" >&2; exit 1; }

manifest_for() {
  case "$1" in
    ways)        echo "tools/ways-cli/Cargo.toml" ;;
    ways-audit)  echo "tools/ways-audit/Cargo.toml" ;;
    ways-mcp)    echo "tools/ways-mcp/Cargo.toml" ;;
    ways-agent)  echo "tools/ways-agent/Cargo.toml" ;;
    attend)      echo "tools/attend/Cargo.toml" ;;
    attend-chat) echo "tools/attend-chat/Cargo.toml" ;;
    *) die "unknown component '$1' (ways|ways-audit|ways-mcp|ways-agent|attend|attend-chat)" ;;
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

# The attend mod (ADR-406) carries attend's version and is released with
# attend. Claude Code updates its plugin cache only when the version moves,
# so a change under plugins/attend/ with the same version never reaches an
# install that already has the mod.
PLUGIN_MANIFEST="plugins/attend/.claude-plugin/plugin.json"
# The mod cannot read its manifest, so it carries its version as a constant
# the beat sends to attend; the bump moves both.
PLUGIN_LIB="plugins/attend/hooks/lib.ts"
# The ways-judge mod (ADR-708) and the ways-theme mod (ADR-513) call ways
# verbs and carry ways' version; bumping ways moves them.
WAYS_MANIFESTS=("plugins/ways-judge/.claude-plugin/plugin.json" "plugins/ways-theme/.claude-plugin/plugin.json")

plugin_version_of() {
  # The first "version" field, wherever it sits on its line, so a manifest
  # written on one line reads the same.
  grep -oE '"version"[[:space:]]*:[[:space:]]*"[^"]*"' | head -n1 | sed -E 's/.*"([^"]*)"$/\1/'
}

# True when version $1 sorts before $2 (numeric fields, as sort -V orders them).
version_lt() {
  [[ "$1" != "$2" && "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n1)" == "$1" ]]
}

# Move a plugin manifest's version to $2, and fail unless it reads $2 after.
set_plugin_version() {
  awk -v ver="$2" '
    !done && /^[[:space:]]*"version"[[:space:]]*:/ { sub(/"version"[[:space:]]*:[[:space:]]*"[^"]+"/, "\"version\": \"" ver "\""); done=1 }
    { print }
  ' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
  [[ "$(plugin_version_of < "$1")" == "$2" ]] || die "could not move $1 to $2"
}

# Move the mod's MOD_VERSION constant to $1, and fail unless it reads $1 after.
set_mod_version() {
  sed -i.bak -E "s/^(export const MOD_VERSION = ')[^']*(')/\\1$1\\2/" "$PLUGIN_LIB" && rm -f "$PLUGIN_LIB.bak"
  grep -qF "export const MOD_VERSION = '$1'" "$PLUGIN_LIB" || die "could not move MOD_VERSION in $PLUGIN_LIB to $1"
}

# Refuse a release whose range changed plugins/attend/ without moving the
# plugin's version forward: the range runs from this component's last tag to
# HEAD. It fails closed: a version it cannot read on either side refuses,
# and so does one that moved backwards.
check_plugin_bump() {
  [[ -f "$PLUGIN_MANIFEST" ]] || return 0
  local prev old new
  prev="$(git describe --tags --abbrev=0 --match "${TAG_PREFIX}*" HEAD 2>/dev/null)" || return 0
  git diff --quiet "$prev" HEAD -- plugins/attend && return 0
  new="$(plugin_version_of < "$PLUGIN_MANIFEST")"
  [[ -n "$new" ]] || die "cannot read the plugin's version from $PLUGIN_MANIFEST"
  # A plugin that did not exist at the last tag is new: any version moves.
  git cat-file -e "$prev:$PLUGIN_MANIFEST" 2>/dev/null || return 0
  old="$(git show "$prev:$PLUGIN_MANIFEST" | plugin_version_of)"
  [[ -n "$old" ]] || die "cannot read the plugin's version from $PLUGIN_MANIFEST at $prev"
  version_lt "$new" "$old" && die "$PLUGIN_MANIFEST moved backwards since $prev: $old to $new"
  [[ "$old" != "$new" ]] || die "plugins/attend/ changed since $prev but $PLUGIN_MANIFEST is still $new; release attend first (make cut-release COMPONENT=attend), which moves the plugin's version with attend's"
  return 0
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="${1:-}"; COMPONENT="${2:-}"
[[ -n "$MODE" && -n "$COMPONENT" ]] || die "usage: release.sh <bump|tag> <ways|ways-audit|ways-mcp|ways-agent|attend|attend-chat> ..."
MANIFEST="$(manifest_for "$COMPONENT")"
PKG="$COMPONENT"   # package name == component name for every component
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

    # attend's own bump moves the plugin's version; any other component's
    # release must not ship a plugin change under an old version.
    [[ "$COMPONENT" == "attend" ]] || check_plugin_bump

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

    FILES=("$MANIFEST" tools/Cargo.lock)
    if [[ "$COMPONENT" == "attend" && -f "$PLUGIN_MANIFEST" ]]; then
      set_plugin_version "$PLUGIN_MANIFEST" "$NEXT"
      FILES+=("$PLUGIN_MANIFEST")
      set_mod_version "$NEXT"
      FILES+=("$PLUGIN_LIB")
    fi
    if [[ "$COMPONENT" == "ways" ]]; then
      for m in "${WAYS_MANIFESTS[@]}"; do
        [[ -f "$m" ]] || continue
        set_plugin_version "$m" "$NEXT"
        FILES+=("$m")
      done
    fi
    git add "${FILES[@]}"
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
    if [[ "$COMPONENT" == "attend" && -f "$PLUGIN_MANIFEST" ]]; then
      PV="$(plugin_version_of < "$PLUGIN_MANIFEST")"
      [[ "$PV" == "$VERSION" ]] || die "$PLUGIN_MANIFEST is $PV but attend is $VERSION; they release together"
    fi
    if [[ "$COMPONENT" == "ways" ]]; then
      for m in "${WAYS_MANIFESTS[@]}"; do
        [[ -f "$m" ]] || continue
        JV="$(plugin_version_of < "$m")"
        [[ "$JV" == "$VERSION" ]] || die "$m is $JV but ways is $VERSION; they release together"
      done
    fi
    check_plugin_bump

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

  check)
    # The plugin check alone, as bump and tag run it, on the checkout as it is.
    check_plugin_bump
    echo "release: plugins/attend/ is releasable with ${COMPONENT}"
    ;;

  mod-version)
    # The constant alone, as bump moves it, for the bump's test.
    [[ "$COMPONENT" == attend ]] || die "mod-version: only attend carries the mod's version, not '$COMPONENT'"
    [[ -n "${3:-}" ]] || die "usage: release.sh mod-version $COMPONENT <version>"
    set_mod_version "$3"
    ;;

  *) die "unknown mode '$MODE' (bump|tag|check|mod-version)" ;;
esac
