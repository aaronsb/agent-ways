#!/usr/bin/env bash
# Self-test for tests/workflow-crates-test.sh: it must go red on each stale
# form a workflow can carry, and on a tree where it would check nothing,
# and green on a clean tree. Each case runs the real script in a scratch
# repository: a one-crate workspace and a workflow directory.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
pass=0
fail=0

scratch() {
  local d
  d=$(mktemp -d)
  mkdir -p "$d/tests" "$d/.github/workflows" "$d/tools/attend-presence/src"
  cp "$here/workflow-crates-test.sh" "$d/tests/"
  printf '[workspace]\nmembers = ["attend-presence"]\nresolver = "2"\n' > "$d/tools/Cargo.toml"
  printf '[package]\nname = "attend-presence"\nversion = "0.1.0"\nedition = "2021"\n' \
    > "$d/tools/attend-presence/Cargo.toml"
  : > "$d/tools/attend-presence/src/lib.rs"
  echo "$d"
}

# expect <red|green> <label> <workflow file name> <workflow body, or empty for none>
expect() {
  local want=$1 label=$2 name=$3 body=$4 d got
  d=$(scratch)
  [[ -n $body ]] && printf '%s\n' "$body" > "$d/.github/workflows/$name"
  if bash "$d/tests/workflow-crates-test.sh" >/dev/null 2>&1; then got=green; else got=red; fi
  rm -rf "$d"
  if [[ $got == "$want" ]]; then
    pass=$((pass + 1))
  else
    echo "  FAIL: $label: expected $want, got $got"
    fail=$((fail + 1))
  fi
}

good='on:
  pull_request:
    paths:
      - '"'"'tools/attend-presence/**'"'"'
jobs:
  build:
    uses: ./.github/workflows/reusable-build.yml
    with:
      component: attend-presence
      test: |
        cargo test --manifest-path tools/Cargo.toml -p attend-presence'

expect green "a clean workflow" ok.yml "$good"
expect red "no workflows at all" none.yml ""
expect green "a clean .yaml workflow" ok.yaml "$good"
expect red "-p on a continued line" cont.yml 'jobs:
  t:
    steps:
      - run: cargo test --manifest-path tools/Cargo.toml \
          -p attend-session'
expect red "--package" pkg.yml 'jobs:
  t:
    steps:
      - run: cargo test --package attend-session'
expect red "component: input" comp.yml 'jobs:
  build:
    uses: ./.github/workflows/reusable-build.yml
    with:
      component: attend-session'
expect red "a tools/<dir>/ filter that is not /**" paths.yml 'on:
  pull_request:
    paths:
      - '"'"'tools/attend-session/src/**'"'"'
      - '"'"'tools/attend-session/Cargo.toml'"'"''
expect green "a cmake component is not a crate" cmake.yml 'jobs:
  build:
    uses: ./.github/workflows/reusable-build.yml
    with:
      component: way-embed
      kind: cmake
      test: |
        cargo test --manifest-path tools/Cargo.toml -p attend-presence'

echo "Results: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
