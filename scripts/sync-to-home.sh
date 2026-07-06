#!/usr/bin/env bash
# sync-to-home.sh — project a subdirectory agent-ways clone into ~/.claude.
#
# ADR-140: the deterministic mechanism behind `make sync-to-home`. The repo lives
# in a subdirectory (e.g. ~/.claude/directory) while ~/.claude stays the user's own
# config dir; this script projects the repo's outputs — skills, agents, commands,
# hooks, binaries — up into ~/.claude and merges only the hooks block + ways
# permissions into the existing settings.json, leaving model/theme/plugins/
# credentials untouched.
#
# Two projection modes (ADR-140):
#   copy    (default) — robust everywhere incl. Windows/low-privilege; re-run after
#                       every `git pull` (drift is detectable via the synced-HEAD stamp).
#   symlink (opt-in)  — ~/.claude/{hooks,bin,...} point at the repo; `git pull` IS the
#                       whole update, zero drift. Fragile where symlinks need admin.
#
# Overridable for testing: CLAUDE_HOME (target), SYNC_SRC (source repo root).
set -euo pipefail

# --- Resolve source (repo root) and destination ---
if [[ -n "${SYNC_SRC:-}" ]]; then
  SRC="$(cd "$SYNC_SRC" && pwd -P)"
else
  SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
fi
DEST="${CLAUDE_HOME:-$HOME/.claude}"
SYNC_MODE="${SYNC_MODE:-copy}"
KEEP_BACKUPS="${KEEP_BACKUPS:-5}"

log()  { printf '  %s\n' "$*"; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

case "$SYNC_MODE" in copy|symlink) ;; *) die "SYNC_MODE must be 'copy' or 'symlink' (got '$SYNC_MODE')";; esac

# --- 1. Topology guard ---------------------------------------------------------
DEST_REAL="$(cd "$DEST" 2>/dev/null && pwd -P || echo "$DEST")"
echo "Source: $SRC"
echo "Target: $DEST  (mode: $SYNC_MODE)"
if [[ "$SRC" == "$DEST_REAL" ]]; then
  log "Already the canonical in-place install — nothing to sync."
  exit 0
fi
[[ -f "$SRC/hooks/check-config-updates.sh" ]] || die "$SRC is not an agent-ways repo (no hooks/check-config-updates.sh)."
mkdir -p "$DEST"

# --- 2. Backup (then prune to the last $KEEP_BACKUPS) --------------------------
# This backup is the only safety net before the overwrite below, so a failed copy
# of an existing target must be loud — never reported as success. The $$ suffix
# keeps two syncs in the same second from sharing a backup dir.
STAMP="$(date +%Y%m%d-%H%M%S)-$$"
BACKUP="$DEST/backups/sync-$STAMP"
mkdir -p "$BACKUP"
for item in settings.json commands hooks bin skills agents; do
  [[ -e "$DEST/$item" ]] || continue
  cp -r "$DEST/$item" "$BACKUP/" || die "backup of $DEST/$item failed — aborting before overwrite"
done
log "Backed up to $BACKUP"
# Keep at least the backup just made: a KEEP_BACKUPS < 1 must not delete it.
keep=$(( KEEP_BACKUPS < 1 ? 1 : KEEP_BACKUPS ))
if [[ -d "$DEST/backups" ]]; then
  ls -1dt "$DEST"/backups/sync-* 2>/dev/null | tail -n +"$((keep + 1))" | while read -r old; do
    rm -rf "$old"
  done
fi

# --- projection helpers --------------------------------------------------------
# Project a whole directory subtree (skills, agents, commands, hooks/ways).
project_tree() {
  local rel="$1" src="$SRC/$1" dst="$DEST/$1"
  [[ -d "$src" ]] || return 0
  local src_real dst_real
  src_real="$(cd "$src" && pwd -P)"
  dst_real="$(cd "$dst" 2>/dev/null && pwd -P || true)"
  if [[ "$SYNC_MODE" == "symlink" ]]; then
    if [[ -L "$dst" && "$(readlink "$dst")" == "$src" ]]; then
      log "ok $rel already linked"; return 0
    fi
    rm -rf "$dst"
    mkdir -p "$(dirname "$dst")"
    ln -s "$src" "$dst"
    log "linked $rel"
  else
    if [[ -n "$dst_real" && "$dst_real" == "$src_real" ]]; then
      log "ok $rel already resolves to source — skipping"; return 0
    fi
    mkdir -p "$dst"
    cp -r "$src/." "$dst/"
    log "copied $rel"
  fi
}

# --- 3. Skills, agents, commands ----------------------------------------------
project_tree skills
project_tree agents
project_tree commands

# --- 4. Hooks ------------------------------------------------------------------
# hooks/ways/ carries the way content + per-event check scripts + macro.sh;
# hooks/ top-level carries the two runtime hooks settings.json references.
project_tree hooks/ways
if [[ "$SYNC_MODE" == "copy" ]]; then
  for h in check-config-updates.sh; do
    [[ -f "$SRC/hooks/$h" ]] && cp -f "$SRC/hooks/$h" "$DEST/hooks/$h"
  done
else
  mkdir -p "$DEST/hooks"
  for h in check-config-updates.sh; do
    if [[ -f "$SRC/hooks/$h" ]]; then
      [[ -L "$DEST/hooks/$h" && "$(readlink "$DEST/hooks/$h")" == "$SRC/hooks/$h" ]] && continue
      rm -f "$DEST/hooks/$h"; ln -s "$SRC/hooks/$h" "$DEST/hooks/$h"
    fi
  done
fi
find "$DEST/hooks" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true

# --- 5. Binaries ---------------------------------------------------------------
# Rebuild the source binaries first (in symlink mode the links then point at the
# freshly-built artifacts — the rebuild is still correct and worth doing).
if command -v cargo >/dev/null && command -v make >/dev/null; then
  make -C "$SRC" update-binaries >/dev/null 2>&1 || log "warn: binary rebuild had issues — using existing bin/"
fi
mkdir -p "$DEST/bin"
for b in ways attend attend-chat way-embed; do
  [[ -f "$SRC/bin/$b" ]] || continue
  if [[ -L "$DEST/bin/$b" && "$(readlink "$DEST/bin/$b")" == "$SRC/bin/$b" ]]; then
    log "ok $b already linked to source — skipping"; continue
  fi
  if [[ "$SYNC_MODE" == "symlink" ]]; then
    rm -f "$DEST/bin/$b"; ln -s "$SRC/bin/$b" "$DEST/bin/$b"
  else
    cp -f "$SRC/bin/$b" "$DEST/bin/$b" && chmod +x "$DEST/bin/$b"
  fi
done
[[ -x "$DEST/bin/ways" ]] && "$DEST/bin/ways" corpus --quiet 2>/dev/null || true

# --- 5b. Prune orphans (copy mode) --------------------------------------------
# A plain `cp` never deletes, so a way/skill/command removed or renamed upstream
# would linger in ~/.claude and keep loading while the synced-HEAD stamp reads
# green. Track exactly what the projection writes in a manifest, and on the next
# sync remove only files this projection created before but that no longer exist
# in source. User-owned files in the same shared dirs are never in the manifest,
# so they are never touched. (Symlink mode reflects source live — no orphans.)
MANIFEST="$DEST/.claude-source-manifest"
build_manifest() {
  local t
  for t in skills agents commands hooks/ways; do
    [[ -d "$SRC/$t" ]] && ( cd "$SRC" && find "$t" -type f )
  done
  for h in check-config-updates.sh; do
    [[ -f "$SRC/hooks/$h" ]] && echo "hooks/$h"
  done
  for b in ways attend attend-chat way-embed; do
    [[ -f "$SRC/bin/$b" ]] && echo "bin/$b"
  done
  return 0  # don't let a final failed -f test abort the caller under set -e/pipefail
}
if [[ "$SYNC_MODE" == "copy" ]]; then
  new_list="$(build_manifest | sort -u)"
  if [[ -f "$MANIFEST" ]]; then
    comm -23 <(sort -u "$MANIFEST") <(printf '%s\n' "$new_list") | while read -r orphan; do
      [[ -n "$orphan" && -e "$DEST/$orphan" ]] && rm -f "$DEST/$orphan" && log "removed orphan $orphan"
    done
  fi
  printf '%s\n' "$new_list" > "$MANIFEST"
fi

# --- 6. Merge settings.json ----------------------------------------------------
# Replace the hooks block + add ways permissions; leave everything else (model,
# theme, plugins, credentials) untouched. Hook command paths are quoted so a
# ${HOME} that expands to a path with spaces (Windows) stays a single token.
SETTINGS="$DEST/settings.json"
[[ -f "$SETTINGS" ]] || echo '{}' > "$SETTINGS"
ADD_PERMS='["Bash(ways:*)","Bash(attend:*)","Bash(attend-chat:*)","Bash(way-embed:*)","Edit(~/.claude/**)","Write(~/.claude/**)"]'
TMP="$SETTINGS.tmp.$$"
jq --slurpfile src "$SRC/settings.json" --argjson add "$ADD_PERMS" '
  .hooks = $src[0].hooks
  | .permissions = ((.permissions // {}) | .allow = ((.allow // []) + ($add - (.allow // []))))
  | .hooks |= (
      to_entries | map(.value |= map(.hooks |= map(.command |= (
        if startswith("\"") then .
        else ((index(" ")) as $i
          | if $i == null then "\"\(.)\"" else "\"\(.[0:$i])\"\(.[$i:])" end)
        end
      )))) | from_entries
    )
' "$SETTINGS" > "$TMP" && mv "$TMP" "$SETTINGS"
log "merged settings.json (hooks block + ways permissions)"

# --- 7. Source marker + synced-HEAD stamp (ADR-140 observability) --------------
# Lets check-config-updates.sh find the projecting repo (since ~/.claude is not a
# git repo in this topology) and detect "pulled but not synced" drift.
HEAD_SHA="$(git -C "$SRC" rev-parse HEAD 2>/dev/null || echo unknown)"
{
  echo "source=$SRC"
  echo "head=$HEAD_SHA"
  echo "mode=$SYNC_MODE"
  echo "synced_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$DEST/.claude-source"
log "stamped .claude-source (head ${HEAD_SHA:0:9}, mode $SYNC_MODE)"

echo "Sync complete. Restart Claude Code to activate changes."
