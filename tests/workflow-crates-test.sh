#!/usr/bin/env bash
# Every crate a workflow names must exist. A crate merged away (#701 removed
# attend-session) otherwise fails CI only once a PR touches that workflow.
# Checked, in every .yml and .yaml under .github/workflows:
#   - packages after `-p` or `--package` on a cargo line, continuation
#     lines joined first;
#   - `component:` inputs, quoted or not, the crate reusable-build.yml
#     builds, except in a `with:` block that says `kind: cmake`;
#   - `paths:` entries under tools/: the named file or directory must
#     exist, or a name ending in `*` must match one.
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

  # Packages: only in the command segments that run cargo, so a later
  # `&& mkdir -p dist` is not read as one.
  while read -r pkg; do
    [[ -n $pkg ]] && check_pkg "$f" "cargo package" "$pkg"
  done < <(sed -E 's/(&&|\|\||;|\|)/\n/g' <<<"$text" \
    | grep -E '(^|[[:space:]])cargo[[:space:]]' \
    | grep -oE -- '(^|[[:space:]])(-p|--package)[[:space:]=]+[A-Za-z0-9_-]+' \
    | sed -E 's/^[[:space:]]*(-p|--package)[[:space:]=]+//' || true)

  # Components: each `with:` block's `component:`, quoted or not, unless
  # that same block says `kind: cmake`.
  while read -r comp; do
    [[ -n $comp ]] && check_pkg "$f" "component" "$comp"
  done < <(awk '
    function flush() { if (comp != "" && kind != "cmake") print comp; comp = ""; kind = ""; inwith = 0 }
    function value(line) { sub(/^[ \t]*[a-z-]+:[ \t]*/, "", line); gsub(/["\047 \t]/, "", line); return line }
    {
      match($0, /^[ ]*/); ind = RLENGTH
      if (inwith && ind <= wind && $0 !~ /^[ \t]*$/) flush()
      if ($0 ~ /^[ ]*with:[ \t]*$/) { flush(); inwith = 1; wind = ind; next }
      if (inwith && $0 ~ /^[ ]*component:/) comp = value($0)
      if (inwith && $0 ~ /^[ ]*kind:/) kind = value($0)
    }
    END { flush() }' <<<"$text")

  # Path entries under tools/: the named entry must exist; a name that
  # ends in a glob must match at least one.
  while read -r entry; do
    [[ -z $entry ]] && continue
    if [[ $entry == *'*' ]]; then
      found=$(compgen -G "tools/$entry" | head -1 || true)
    else
      found=$([[ -e "tools/$entry" ]] && echo yes || true)
    fi
    if [[ -n $found ]]; then
      pass=$((pass + 1))
    else
      echo "  FAIL: $f filters on tools/$entry, which matches nothing"
      fail=$((fail + 1))
    fi
  done < <(grep -E "^[[:space:]]*-[[:space:]]*['\"]?tools/" <<<"$text" \
    | grep -oE 'tools/[A-Za-z0-9_.-]+\*?' | sed -E 's#^tools/##' | sort -u || true)
done

echo "Results: $pass passed, $fail failed"
if (( pass == 0 )); then
  echo "  FAIL: nothing was checked"
  exit 1
fi
[[ $fail -eq 0 ]]
