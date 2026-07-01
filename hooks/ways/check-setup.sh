#!/usr/bin/env bash
# SessionStart: Check if ways installation is complete.
# Runs as the first startup hook. If setup is incomplete, emits a
# diagnostic and exits cleanly so other hooks don't error.
#
# Checks: ways binary → corpus → embedding engine (functional probe)

WAYS_BIN="${HOME}/.claude/bin/ways"
# The app source (with the Makefile) lives in $XDG_DATA, not in the ~/.claude
# projection — repairs run there, not in ~/.claude (ADR-142).
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
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
  cat <<MSG

⚠️  Ways setup incomplete — the \`ways\` binary is not installed.

Hooks will be inactive until setup completes. Build the app source and
reproject (the Makefile lives in the app dir, not in ~/.claude):

    cd "$APP_DIR" && make setup && ways reconcile

If the app dir doesn't exist, re-run the installer one-liner. Setup
downloads the ways binary, embedding model, and generates the matching
corpus; if you don't have a Rust toolchain, pre-built binaries are
downloaded automatically.

MSG
  exit 0
fi

# Binary exists — check corpus
CORPUS="${XDG_WAY}/ways-corpus.jsonl"
if [[ ! -f "$CORPUS" ]]; then
  cat <<MSG

⚠️  Ways corpus not generated — semantic matching is inactive.

Rebuild it from the app source:

    cd "$APP_DIR" && make setup

MSG
  exit 0
fi

# Embedding engine — a hard dependency (ADR-125). Probe it *functionally* rather
# than checking file paths. Two reasons:
#   1. Path checks go stale. The old check looked for way-embed at $XDG_WAY/way-embed,
#      but it actually lives at ~/.claude/bin/way-embed — so on a healthy install it
#      false-positived "not installed" (masked by a once-a-day marker).
#   2. The ADR-142 projection makes ~/.claude/{bin,hooks,...} symlinks into the app
#      dir in $XDG_DATA. Asking the binary to actually embed a query resolves
#      identically whether the projection is fresh, repaired, or a legacy in-place
#      clone, and also catches a corrupt model or a binary that loads-but-errors —
#      which existence checks miss entirely.
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
  cat <<MSG

⚠  Embedding engine is NOT functional — semantic way matching is OFF.
   Only explicit pattern:/commands:/files: triggers will fire (coarse — expect ways
   to over- and under-fire until this is fixed). It is a hard dependency (ADR-125).

   Repair (rebuild the binary, model, and corpus from the app source, then reproject):
     cd "$APP_DIR" && make setup && ways reconcile

MSG
fi
