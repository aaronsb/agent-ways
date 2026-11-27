#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Release helper for the Cargo-versioned components. Two steps, because
# `main` is branch-protected and the repo is PR-first:
#
#   bump  — create one release branch with the version bump of every listed
#           component and a verified Cargo.lock, push it, and open one PR. No
#           tag yet; the bump is reviewed and merged like any other change.
#   tag   — AFTER that PR merges to main: tag `<comp>-vX.Y.Z` on main for every
#           listed component and push the tags. Pushing a tag is the one outward
#           step — CI (build-<comp>.yml) then builds every platform and creates
#           the GitHub Release.
#
# Usage:
#   scripts/release.sh bump  <list> [patch|minor|major]
#   scripts/release.sh tag   <list> [--push]
#   scripts/release.sh check <component>
#   scripts/release.sh mod-version <component> <version>   # MOD_VERSION alone, as bump moves it
#
# <list> names components from ways, ways-audit, ways-mcp, ways-agent, attend
# and attend-chat, separated by spaces or commas. For bump, an entry may name
# its own level as <comp>:<level>; the level argument is the default for an
# entry that names none. One component gets the branch `release/<comp>-vX.Y.Z`;
# several get `release/<date>-<n>components`.
#
# Both bump and tag refuse a release whose range, from the component's last tag
# to HEAD, changed plugins/attend/ without moving the plugin's version; `check`
# runs that check alone. Bumping attend moves the plugin's version with it, so a
# batch that includes attend passes the check for the others; bumping ways
# moves the ways-judge, ways-theme and images plugins' versions.
#
# `tag` without --push creates the annotated tags locally and prints the push
# commands (keeps the publish deliberate); `--push` pushes them. way-embed is out
# of scope — it versions via tools/way-embed/Makefile.
#
# References:
# - ADR-150: main takes changes only through a PR, so the version bump merges
#   first and the tag lands on main after it.
set -euo pipefail

die() { echo "release: $*" >&2; exit 1; }

# Every list is worked in this order. attend comes first: its bump sets the
# plugin version that the plugin check reads for the others.
ORDER=(attend ways ways-audit ways-mcp ways-agent attend-chat)

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
# The package name is the component name for every component.
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
# The ways-judge mod (ADR-708), the ways-theme mod (ADR-514) and the images
# mod (ADR-517) call ways verbs and carry ways' version; bumping ways moves them.
WAYS_MANIFESTS=("plugins/ways-judge/.claude-plugin/plugin.json" "plugins/ways-theme/.claude-plugin/plugin.json" "plugins/images/.claude-plugin/plugin.json")

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

# Refuse a release of component $1 whose range changed plugins/attend/ without
# moving the plugin's version forward: the range runs from the component's last
# tag to HEAD. $2, when given, is the version the release commit gives the
# plugin, which attend's bump in the same batch sets; without it the manifest
# in the working tree is read. It fails closed: a version it cannot read on
# either side refuses, and so does one that moved backwards.
check_plugin_bump() {
  [[ -f "$PLUGIN_MANIFEST" ]] || return 0
  local prev old new
  prev="$(git describe --tags --abbrev=0 --match "${1}-v*" HEAD 2>/dev/null)" || return 0
  git diff --quiet "$prev" HEAD -- plugins/attend && return 0
  new="${2:-$(plugin_version_of < "$PLUGIN_MANIFEST")}"
  [[ -n "$new" ]] || die "cannot read the plugin's version from $PLUGIN_MANIFEST"
  # A plugin that did not exist at the last tag is new: any version moves.
  git cat-file -e "$prev:$PLUGIN_MANIFEST" 2>/dev/null || return 0
  old="$(git show "$prev:$PLUGIN_MANIFEST" | plugin_version_of)"
  [[ -n "$old" ]] || die "cannot read the plugin's version from $PLUGIN_MANIFEST at $prev"
  version_lt "$new" "$old" && die "$PLUGIN_MANIFEST moved backwards since $prev: $old to $new"
  [[ "$old" != "$new" ]] || die "plugins/attend/ changed since $prev but $PLUGIN_MANIFEST is still $new; release attend with $1 (make cut-release COMPONENT=\"attend $1\"), which moves the plugin's version with attend's"
  return 0
}

# Split list $1 into COMPS and LEVELS, in ORDER. For bump ($3 = bump) an entry
# may name its level as <comp>:<level>, and $2 is the level of an entry that
# names none; tag takes names alone.
parse_list() {
  local mode="$3" entry c l n i
  local -a entries=() names=() levels=()
  IFS=', ' read -r -a entries <<< "$1"
  for entry in ${entries[@]+"${entries[@]}"}; do
    [[ -n "$entry" ]] || continue
    c="${entry%%:*}"; l=""
    [[ "$entry" == *:* ]] && l="${entry#*:}"
    manifest_for "$c" >/dev/null
    for n in ${names[@]+"${names[@]}"}; do
      [[ "$n" != "$c" ]] || die "$c is listed twice"
    done
    if [[ "$mode" == bump ]]; then
      l="${l:-$2}"
      [[ -n "$l" ]] || die "no level for $c: name it as $c:<patch|minor|major> or pass a default level"
      case "$l" in patch|minor|major) ;; *) die "level for $c must be patch|minor|major (got '$l')" ;; esac
    else
      [[ -z "$l" ]] || die "$mode takes component names alone (got '$entry')"
    fi
    names+=("$c"); levels+=("$l")
  done
  [[ ${#names[@]} -gt 0 ]] || die "no component in '$1'"
  COMPS=(); LEVELS=()
  for c in "${ORDER[@]}"; do
    for i in "${!names[@]}"; do
      [[ "${names[$i]}" == "$c" ]] && { COMPS+=("$c"); LEVELS+=("${levels[$i]}"); }
    done
  done
  return 0
}

# Both steps run on a clean main in sync with origin. $1 names the step.
require_clean_main() {
  [[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]] || die "run $1 from main"
  [[ -z "$(git status --porcelain)" ]] || die "working tree not clean"
  git fetch --quiet origin main
  [[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
    || die "local main is not in sync with origin/main — pull/push first"
}

# The version after a bump of version $1 by level $2.
next_version() {
  [[ "$1" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] || die "unparseable version '$1'"
  local major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}" patch="${BASH_REMATCH[3]}"
  case "$2" in
    patch) patch=$((patch + 1)) ;;
    minor) minor=$((minor + 1)); patch=0 ;;
    major) major=$((major + 1)); minor=0; patch=0 ;;
  esac
  echo "${major}.${minor}.${patch}"
}

# One component's bump, in the working tree: move component $1 to version $2
# in its manifest, Cargo.lock and the plugin manifests that carry its version,
# and add each file it moved to FILES.
bump_component() {
  local comp="$1" next="$2" manifest got m
  manifest="$(manifest_for "$comp")"
  # Bump ONLY the package's own version line (column-0 `version = "..."`), never a
  # dependency's `version` field. `!done` makes it a one-shot on the first match.
  awk -v ver="$next" '
    !done && /^version *= *"/ { sub(/"[^"]+"/, "\"" ver "\""); done=1 }
    { print }
  ' "$manifest" > "$manifest.tmp" && mv "$manifest.tmp" "$manifest"
  FILES+=("$manifest")

  # Sync Cargo.lock, then VERIFY it actually records the new version — a stale
  # lock in the release commit is the exact drift this helper exists to prevent.
  cargo update -p "$comp" --manifest-path tools/Cargo.toml --offline >/dev/null 2>&1 \
    || cargo update -p "$comp" --manifest-path tools/Cargo.toml >/dev/null 2>&1 || true
  got="$(lock_version "$comp")"
  [[ "$got" == "$next" ]] \
    || die "Cargo.lock still records $comp=${got:-none}, not $next — refusing to commit a stale lockfile. Run 'cargo update -p $comp --manifest-path tools/Cargo.toml' and retry."

  if [[ "$comp" == "attend" && -f "$PLUGIN_MANIFEST" ]]; then
    set_plugin_version "$PLUGIN_MANIFEST" "$next"
    set_mod_version "$next"
    FILES+=("$PLUGIN_MANIFEST" "$PLUGIN_LIB")
  fi
  if [[ "$comp" == "ways" ]]; then
    for m in "${WAYS_MANIFESTS[@]}"; do
      [[ -f "$m" ]] || continue
      set_plugin_version "$m" "$next"
      FILES+=("$m")
    done
  fi
}

# Refuse to tag component $1 unless main carries its merged bump and its tag
# is new; print the tag.
tag_for() {
  local comp="$1" manifest version got pv m
  manifest="$(manifest_for "$comp")"
  version="$(current_version "$manifest")"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "unparseable version '$version' in $manifest"
  # The lock must agree — otherwise the bump PR wasn't merged (or is inconsistent).
  got="$(lock_version "$comp")"
  [[ "$got" == "$version" ]] || die "Cargo.lock records $comp=${got:-none} but manifest is $version — is the bump PR merged?"
  if [[ "$comp" == "attend" && -f "$PLUGIN_MANIFEST" ]]; then
    pv="$(plugin_version_of < "$PLUGIN_MANIFEST")"
    [[ "$pv" == "$version" ]] || die "$PLUGIN_MANIFEST is $pv but attend is $version; they release together"
  fi
  if [[ "$comp" == "ways" ]]; then
    for m in "${WAYS_MANIFESTS[@]}"; do
      [[ -f "$m" ]] || continue
      pv="$(plugin_version_of < "$m")"
      [[ "$pv" == "$version" ]] || die "$m is $pv but ways is $version; they release together"
    done
  fi
  check_plugin_bump "$comp"
  local tag="${comp}-v${version}"
  git rev-parse -q --verify "refs/tags/${tag}" >/dev/null && die "tag ${tag} already exists locally"
  git ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1 && die "tag ${tag} already exists on origin"
  echo "$tag"
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="${1:-}"; LIST="${2:-}"
[[ -n "$MODE" && -n "$LIST" ]] || die "usage: release.sh <bump|tag|check|mod-version> <components> ..."

case "$MODE" in
  bump)
    parse_list "$LIST" "${3:-}" bump
    require_clean_main bump

    # Every guard runs before the branch exists. attend is first in COMPS, so
    # its next version is known when the others' plugin check runs.
    CURS=(); NEXTS=(); PLUGIN_NEXT=""
    for i in "${!COMPS[@]}"; do
      c="${COMPS[$i]}"
      cur="$(current_version "$(manifest_for "$c")")"
      next="$(next_version "$cur" "${LEVELS[$i]}")" || exit 1
      CURS+=("$cur"); NEXTS+=("$next")
      git rev-parse -q --verify "refs/tags/${c}-v${next}" >/dev/null && die "tag ${c}-v${next} already exists"
      if [[ "$c" == attend ]]; then PLUGIN_NEXT="$next"; else check_plugin_bump "$c" "$PLUGIN_NEXT"; fi
    done

    SUMMARY=""; PUBLISH=""; TAGS=""
    for i in "${!COMPS[@]}"; do
      SUMMARY+="${SUMMARY:+, }${COMPS[$i]} v${NEXTS[$i]}"
      PUBLISH+="${PUBLISH:+ }${COMPS[$i]}"
      TAGS+="${TAGS:+ }${COMPS[$i]}-v${NEXTS[$i]}"
    done
    if [[ ${#COMPS[@]} -eq 1 ]]; then
      BRANCH="release/${TAGS}"
      SUBJECT="release(${COMPS[0]}): bump to ${NEXTS[0]}"
      TITLE="release(${COMPS[0]}): v${NEXTS[0]}"
    else
      BRANCH="release/$(date +%Y-%m-%d)-${#COMPS[@]}components"
      SUBJECT="release: ${SUMMARY}"
      TITLE="$SUBJECT"
      PUBLISH="\"${PUBLISH}\""
    fi
    git rev-parse -q --verify "refs/heads/${BRANCH}" >/dev/null && die "branch ${BRANCH} already exists"

    for i in "${!COMPS[@]}"; do
      echo "release: bump ${COMPS[$i]} ${CURS[$i]} -> ${NEXTS[$i]}  (tag-to-be ${COMPS[$i]}-v${NEXTS[$i]})"
    done
    echo "release: branch $BRANCH"
    git checkout -q -b "$BRANCH"
    FILES=()
    for i in "${!COMPS[@]}"; do
      bump_component "${COMPS[$i]}" "${NEXTS[$i]}"
    done
    git add tools/Cargo.lock "${FILES[@]}"
    git commit -q -m "$SUBJECT"
    git push -q -u origin "$BRANCH"
    gh pr create --title "$TITLE" \
      --body "Version bump for ${SUMMARY}. main takes changes only through a PR, so the tags come after this merges. After merge, tag it: \`make publish-release COMPONENT=${PUBLISH}\` — that pushes \`${TAGS}\` and CI builds all platforms + creates the GitHub Release."
    cat <<EOF

Bump PR opened on branch ${BRANCH}. Next:
  1. review + merge the PR
  2. git checkout main && git pull
  3. make publish-release COMPONENT=${PUBLISH}   (tags ${TAGS} on main + publishes)
EOF
    ;;

  tag)
    PUSH="${3:-}"
    parse_list "$LIST" "" tag
    require_clean_main "tag (after the bump PR merged)"

    # Every component passes its checks before any tag is made.
    TAGS=()
    for c in "${COMPS[@]}"; do t="$(tag_for "$c")" || exit 1; TAGS+=("$t"); done
    for i in "${!COMPS[@]}"; do
      git tag -a "${TAGS[$i]}" -m "${COMPS[$i]} v${TAGS[$i]##*-v}"
      echo "release: tagged ${TAGS[$i]} on $(git rev-parse --short HEAD)."
    done
    if [[ "$PUSH" == "--push" ]]; then
      # One push per tag: GitHub starts no workflow for a push that creates
      # more than three tags.
      for i in "${!COMPS[@]}"; do
        git push origin "${TAGS[$i]}"
        echo "release: pushed ${TAGS[$i]}. CI (build-${COMPS[$i]}.yml) is building platforms + creating the GitHub Release."
      done
    else
      echo
      echo "Tags created locally, NOT pushed. To PUBLISH (CI builds platforms + GitHub Release):"
      for t in "${TAGS[@]}"; do echo "  git push origin ${t}"; done
      echo "To abort:"
      echo "  git tag -d ${TAGS[*]}"
    fi
    ;;

  check)
    # The plugin check alone, as bump and tag run it, on the checkout as it is.
    manifest_for "$LIST" >/dev/null
    check_plugin_bump "$LIST"
    echo "release: plugins/attend/ is releasable with ${LIST}"
    ;;

  mod-version)
    # The constant alone, as bump moves it, for the bump's test.
    [[ "$LIST" == attend ]] || die "mod-version: only attend carries the mod's version, not '$LIST'"
    [[ -n "${3:-}" ]] || die "usage: release.sh mod-version $LIST <version>"
    set_mod_version "$3"
    ;;

  *) die "unknown mode '$MODE' (bump|tag|check|mod-version)" ;;
esac
