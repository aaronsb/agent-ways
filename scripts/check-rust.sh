#!/usr/bin/env bash
# Precheck: building agent-ways from source needs Rust/Cargo >= 1.89 (see MIN_MINOR).
#
# A transitive dependency (getrandom) uses Rust edition 2024, which was
# stabilized in 1.85. On older toolchains `cargo build` fails deep in dependency
# compilation with a cryptic "feature `edition2024` is required" error. Catch it
# up front with an actionable message instead.
#
# Called from the source-build branches of the Makefile. If cargo is absent the
# build target's own fallback handles it, so this exits 0 (don't double-report).

set -u
MIN_MAJOR=1
# 1.89: the ways binary locks per-way engagement state with std::fs::File::lock
# (stable since 1.89) so parallel hooks cannot both fire one way (#528).
MIN_MINOR=89

command -v cargo >/dev/null 2>&1 || exit 0   # no cargo → build target reports it

ver=$(cargo --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
[ -z "$ver" ] && exit 0                       # unparseable → don't block

maj=${ver%%.*}
min=${ver#*.}; min=${min%%.*}

if [ "$maj" -lt "$MIN_MAJOR" ] || { [ "$maj" -eq "$MIN_MAJOR" ] && [ "$min" -lt "$MIN_MINOR" ]; }; then
  # Tailor the fix to how Rust appears to be installed.
  if command -v rustup >/dev/null 2>&1; then
    fix="rustup update"
  elif command -v pacman >/dev/null 2>&1; then
    fix="sudo pacman -Syu rust     (or install rustup for the latest: https://rustup.rs)"
  elif command -v brew >/dev/null 2>&1; then
    fix="brew upgrade rust         (or install rustup: https://rustup.rs)"
  elif command -v dnf >/dev/null 2>&1; then
    fix="sudo dnf upgrade rust cargo   (or install rustup: https://rustup.rs)"
  elif command -v apt-get >/dev/null 2>&1; then
    fix="distro cargo is usually too old — install rustup: https://rustup.rs"
  else
    fix="install rustup: https://rustup.rs"
  fi

  cat >&2 <<EOF

  ──────────────────────────────────────────────────────────────────────────
  ERROR: agent-ways needs Rust/Cargo >= ${MIN_MAJOR}.${MIN_MINOR} to build from source.

    you have:  cargo ${ver}
    why:       std::fs::File::lock (stable in 1.89); a dependency uses edition 2024 (1.85)

    fix:       ${fix}

    (A pre-built binary may also be available — see the releases page.)
  ──────────────────────────────────────────────────────────────────────────
EOF
  exit 1
fi
exit 0
