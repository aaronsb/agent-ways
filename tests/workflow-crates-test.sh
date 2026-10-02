#!/usr/bin/env bash
# Every crate a workflow names must exist: each `cargo ... -p <pkg>` must be a
# workspace package, and each `tools/<dir>/**` path filter must be a directory.
# A crate merged away (#701 removed attend-session) otherwise fails CI only
# once a PR touches that workflow's paths.
set -euo pipefail
cd "$(dirname "$0")/.."

members=$(cargo metadata --manifest-path tools/Cargo.toml --no-deps --format-version 1 \
  | jq -r '.packages[].name')
fail=0
pass=0

while IFS=: read -r file _ line; do
  for pkg in $(grep -oE -- '-p [a-z][a-z0-9_-]+' <<<"$line" | cut -d' ' -f2); do
    if grep -qx -- "$pkg" <<<"$members"; then
      pass=$((pass + 1))
    else
      echo "  FAIL: $file runs cargo for package '$pkg', which is not in the workspace"
      fail=$((fail + 1))
    fi
  done
done < <(grep -n 'cargo ' .github/workflows/*.yml)

while IFS=: read -r file dir; do
  if [[ -d "tools/$dir" ]]; then
    pass=$((pass + 1))
  else
    echo "  FAIL: $file filters on tools/$dir/**, which does not exist"
    fail=$((fail + 1))
  fi
done < <(grep -oE "tools/[a-z][a-z0-9_-]*/\*\*" .github/workflows/*.yml \
  | sed -E 's#^([^:]+):tools/([^/]+)/\*\*#\1:\2#' | grep -v ':sensor-\*$' | sort -u)

echo "Results: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
