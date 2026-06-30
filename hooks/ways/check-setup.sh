#!/usr/bin/env bash
# SessionStart: Check if ways installation is complete.
# Runs as the first startup hook. If setup is incomplete, emits a
# diagnostic and exits cleanly so other hooks don't error.
#
# Checks: ways binary → corpus → embedding engine (functional probe)

WAYS_BIN="${HOME}/.claude/bin/ways"
# Prefer the 1.0 cache name; fall back to the legacy one for un-migrated installs
# (must match paths::cache_root() in the binary so the probe checks where the
# binary actually reads).
_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"
if [[ -d "${_CACHE}/agent-ways/user" ]]; then XDG_WAY="${_CACHE}/agent-ways/user"
elif [[ -d "${_CACHE}/claude-ways/user" ]]; then XDG_WAY="${_CACHE}/claude-ways/user"
else XDG_WAY="${_CACHE}/agent-ways/user"; fi

# Nothing to check if this isn't a ways-enabled install
[[ ! -d "${HOME}/.claude/hooks/ways" ]] && exit 0

if [[ ! -x "$WAYS_BIN" ]]; then
  cat <<'MSG'

⚠️  Ways setup incomplete — the `ways` binary is not installed.

Hooks will be inactive until setup completes. Run:

    cd ~/.claude && make setup

This downloads the ways binary, embedding model, and generates
the matching corpus. If you don't have a Rust toolchain, pre-built
binaries are downloaded automatically.

MSG
  exit 0
fi

# Binary exists — check corpus
CORPUS="${XDG_WAY}/ways-corpus.jsonl"
if [[ ! -f "$CORPUS" ]]; then
  cat <<'MSG'

⚠️  Ways corpus not generated — semantic matching is inactive.

Run:

    cd ~/.claude && make setup

MSG
  exit 0
fi

# Embedding engine — a hard dependency (ADR-125). Probe it *functionally* rather
# than checking file paths. Two reasons:
#   1. Path checks go stale. The old check looked for way-embed at $XDG_WAY/way-embed,
#      but it actually lives at ~/.claude/bin/way-embed — so on a healthy install it
#      false-positived "not installed" (masked by a once-a-day marker).
#   2. ADR-140 symlink projection makes ~/.claude/{bin,hooks,...} symlinks into a
#      subdir repo. Asking the binary to actually embed a query resolves identically
#      across all topologies (in-place / copy-subdir / symlink-subdir) and also
#      catches a corrupt model or a binary that loads-but-errors — which existence
#      checks miss entirely.
# Loud EVERY session while broken (no per-day suppression): a degraded engine means
# only coarse pattern:/commands:/files: triggers fire, and that must not be skimmable.
# Silent when it works.
probe_embed() {
  if command -v timeout >/dev/null 2>&1; then
    timeout 10 "$WAYS_BIN" match "$1" 2>/dev/null
  else
    "$WAYS_BIN" match "$1" 2>/dev/null
  fi
}
if ! probe_embed "embedding engine health probe" | grep -qE '[0-9]\.[0-9]'; then
  cat <<'MSG'

⚠  Embedding engine is NOT functional — semantic way matching is OFF.
   Only explicit pattern:/commands:/files: triggers will fire (coarse — expect ways
   to over- and under-fire until this is fixed). It is a hard dependency (ADR-125).

   Repair:
     in-place install:      cd ~/.claude && make setup
     subdirectory install:  cd <repo-subdir> && make setup && make sync-to-home

MSG
fi
