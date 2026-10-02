#!/usr/bin/env bash
# Every crate a workflow names must exist. A crate merged away (#701 removed
# attend-session) otherwise fails CI only once a PR touches that workflow.
# Checked, in every .yml and .yaml under .github/workflows:
#   - packages after `-p` or `--package` on a cargo line, continuation
#     lines joined first;
#   - `component:` inputs, the crate reusable-build.yml builds, except in a
#     `kind: cmake` caller;
#   - `paths:` entries under tools/: the directory must exist.
# Fails when there is nothing to check: no workflow files, or no item.
# tests/workflow-crates-selftest.sh plants each stale form and expects red.
set -euo pipefail
cd "$(dirname "$0")/.."
shopt -s nullglob

files=(.github/workflows/*.yml .github/workflows/*.yaml)
if (( ${#files[@]} == 0 )); then
  echo "  FAIL: no workflow files under .github/workflows"
  exit 1
fi

members=$(cargo metadata --manifest-path tools/Cargo.toml --no-deps --format-version 1 \
  | jq -r '.packages[].name')
pass=0
fail=0

check_pkg() { # file what name
  if grep -qx -- "$3" <<<"$members"; then
    pass=$((pass + 1))
  else
    echo "  FAIL: $1 names $2 '$3', which is not a workspace package"
    fail=$((fail + 1))
  fi
}

for f in "${files[@]}"; do
  # One logical line per backslash-continued command.
  text=$(sed -e ':a' -e '/\\$/N; s/\\\n[[:space:]]*/ /; ta' "$f")

  while read -r pkg; do
    [[ -n $pkg ]] && check_pkg "$f" "cargo package" "$pkg"
  done < <(grep -E 'cargo[[:space:]]' <<<"$text" \
    | grep -oE -- '(^|[[:space:]])(-p|--package)[[:space:]=]+[A-Za-z0-9_-]+' \
    | sed -E 's/^[[:space:]]*(-p|--package)[[:space:]=]+//' || true)

  if ! grep -qE '^[[:space:]]*kind:[[:space:]]*cmake' <<<"$text"; then
    while read -r comp; do
      [[ -n $comp ]] && check_pkg "$f" "component" "$comp"
    done < <(grep -oE '^[[:space:]]*component:[[:space:]]*[A-Za-z0-9_-]+' <<<"$text" \
      | sed -E 's/.*component:[[:space:]]*//' || true)
  fi

  while read -r dir; do
    [[ -z $dir ]] && continue
    if [[ -d "tools/$dir" ]]; then
      pass=$((pass + 1))
    else
      echo "  FAIL: $f filters on tools/$dir/, which does not exist"
      fail=$((fail + 1))
    fi
  done < <(grep -E "^[[:space:]]*-[[:space:]]*['\"]?tools/" <<<"$text" \
    | grep -oE 'tools/[A-Za-z0-9_.-]+/' | sed -E 's#tools/([^/]+)/#\1#' | sort -u || true)
done

echo "Results: $pass passed, $fail failed"
if (( pass == 0 )); then
  echo "  FAIL: nothing was checked"
  exit 1
fi
[[ $fail -eq 0 ]]
