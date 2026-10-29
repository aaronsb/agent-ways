#!/usr/bin/env bash
# Print, write or check the `paths:` filter of each Rust build-<component>.yml.
#
# A component's workflow must run when the component's crate changes, when any
# crate it depends on by path changes (directly or transitively, any dependency
# kind, since the workflow also runs the crate's tests), when a file outside
# tools/ that one of those crates embeds changes, when the workspace manifest
# or lock changes, and when its workflow or the shared reusable-build.yml
# changes. The crate set comes from the `path` dependencies that `cargo
# metadata --no-deps` reports, and the embedded files from each crate's
# include_str! and include_bytes! literals, so the filter follows the code
# instead of being kept by hand, and the walk needs no registry access.
#
# Usage:
#   scripts/workflow-paths.sh            print the expected paths per component
#   scripts/workflow-paths.sh --write    rewrite both paths: lists of each caller
#   scripts/workflow-paths.sh --check    exit 1 if a build-*.yml differs

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
WORKFLOWS="$ROOT/.github/workflows"
MODE="${1:-print}"
case "$MODE" in
  print | --write | --check) ;;
  *) echo "usage: $0 [--write|--check]" >&2; exit 2 ;;
esac

META="$(cargo metadata --manifest-path "$ROOT/tools/Cargo.toml" --format-version 1 --no-deps)"

# crates COMPONENT: the component's crate directory, then those of every
# crate it depends on by path, sorted; one absolute path per line.
crates() {
  jq -r --arg name "$1" '
    def dir: sub("/Cargo.toml$"; "");
    (.packages | map({key: (.manifest_path | dir), value: [.dependencies[] | select(.path) | .path]})
      | from_entries) as $deps
    | def closure: . as $s | ($s + [$s[] | $deps[.][]?] | unique) as $n
        | if ($n | length) == ($s | length) then $s else ($n | closure) end;
    (.packages[] | select(.name == $name) | .manifest_path | dir) as $own
    | [$own] + ([$own] | closure | map(select(. != $own)) | sort)
    | .[]' <<<"$META"
}

# embedded DIR...: the directories outside tools/ that the crates at DIR
# embed with include_str! or include_bytes!, such as attend/sensors/, as
# paths: entries. A change there changes the binary, as a crate's does.
embedded() {
  local dir file inc
  for dir in "$@"; do
    grep -rHoE --include='*.rs' 'include_(str|bytes)!\(\s*"[^"]+"' "$dir/src" 2>/dev/null || true
  done | while IFS= read -r line; do
    file=${line%%:*}
    inc=${line#*\"}
    inc=${inc%\"}
    realpath -m --relative-to="$ROOT" "$(dirname "$file")/$inc"
  done | { grep -v '^tools/' || true; } | xargs -r -n1 dirname | sort -u | sed 's|$|/**|'
}

# expected_paths COMPONENT: the paths: entries, one per line, unquoted.
expected_paths() {
  local dirs
  mapfile -t dirs < <(crates "$1")
  printf '%s\n' "${dirs[@]#"$ROOT/"}" | sed 's|$|/**|'
  embedded "${dirs[@]}"
  printf '%s\n' tools/Cargo.toml tools/Cargo.lock ".github/workflows/build-$1.yml" .github/workflows/reusable-build.yml
}

# block COMPONENT: the paths: list as the callers write it.
block() {
  expected_paths "$1" | sed "s/.*/      - '&'/"
}

# rewrite FILE BLOCK-FILE: FILE with every paths: list replaced by BLOCK-FILE.
rewrite() {
  awk -v blockfile="$2" '
    BEGIN { while ((getline l < blockfile) > 0) block = block l "\n" }
    skipping && /^      - / { next }
    { skipping = 0 }
    { print }
    $0 ~ /^[[:space:]]*paths:[[:space:]]*$/ { printf "%s", block; skipping = 1; n++ }
    END { exit (n == 2 ? 0 : 3) }
  ' "$1"
}

bad=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
while read -r comp _; do
  [[ "$comp" =~ ^[a-z] ]] || continue
  if [[ "$MODE" == print ]]; then
    echo "$comp:"
    block "$comp"
    continue
  fi
  file="$WORKFLOWS/build-$comp.yml"
  block "$comp" > "$tmp/block"
  status=0
  rewrite "$file" "$tmp/block" > "$tmp/new" || status=$?
  if [[ "$MODE" == --check ]]; then
    if [[ $status -ne 0 ]] || ! cmp -s "$file" "$tmp/new"; then
      echo "build-$comp.yml: paths differ from cargo metadata; run scripts/workflow-paths.sh --write"
      bad=1
    fi
  elif ! cmp -s "$file" "$tmp/new"; then
    cp "$tmp/new" "$file"
    echo "wrote build-$comp.yml"
  fi
done < "$ROOT/tools/suite-bins"
exit "$bad"
