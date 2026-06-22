---
description: Sync this agent-ways clone into ~/.claude (subdirectory-clone topology)
---

# /sync-to-home

Sync this agent-ways repo clone into `~/.claude`. Run after pulling upstream changes or committing local edits.

## Steps

Run each step in order using the Bash tool. Print a short status line after each one.

### 1. Detect topology

```bash
SRC="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
DEST="$HOME/.claude"
echo "Source: $SRC"
echo "Target: $DEST"
```

If `$SRC == $DEST`, report "Already the canonical install — nothing to sync" and stop.

Verify `$SRC` is an agent-ways repo by checking for `hooks/check-config-updates.sh`. If missing, stop with an error.

### 2. Backup

```bash
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="$DEST/backups/sync-$STAMP"
mkdir -p "$BACKUP"
for item in settings.json commands hooks bin; do
  [[ -e "$DEST/$item" ]] && cp -r "$DEST/$item" "$BACKUP/"
done
```

### 3. Sync skills, agents, commands

```bash
mkdir -p "$DEST/skills" "$DEST/agents" "$DEST/commands"
cp -r "$SRC/skills/." "$DEST/skills/"
cp -r "$SRC/agents/." "$DEST/agents/"
cp -r "$SRC/commands/." "$DEST/commands/"
```

### 4. Sync hooks

Skip if the destination is already symlinked to the source:

```bash
_src_real="$(cd "$SRC/hooks" && pwd -P)"
_dst_real="$(cd "$DEST/hooks" 2>/dev/null && pwd -P || true)"
if [[ "$_dst_real" != "$_src_real" ]]; then
  mkdir -p "$DEST/hooks/ways"
  cp -r "$SRC/hooks/ways/." "$DEST/hooks/ways/"
  for h in check-config-updates.sh refresh-claude-md.sh; do
    [[ -f "$SRC/hooks/$h" ]] && cp -f "$SRC/hooks/$h" "$DEST/hooks/$h"
  done
fi
find "$DEST/hooks" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true
```

### 5. Rebuild and copy binaries

Skip the copy for any binary already symlinked to the source:

```bash
if command -v cargo >/dev/null && command -v make >/dev/null; then
  make -C "$SRC" update-binaries 2>&1 || echo "warn: binary rebuild had issues — copying existing bin/"
fi
mkdir -p "$DEST/bin"
for b in ways attend attend-chat way-embed; do
  if [[ -f "$SRC/bin/$b" ]]; then
    if [[ -L "$DEST/bin/$b" ]] && [[ "$(readlink "$DEST/bin/$b")" == "$SRC/bin/$b" ]]; then
      echo "ok $b already linked to source — skipping"
      continue
    fi
    cp -f "$SRC/bin/$b" "$DEST/bin/$b" && chmod +x "$DEST/bin/$b"
  fi
done
"$DEST/bin/ways" corpus --quiet 2>/dev/null || true
```

### 6. Merge settings.json

Merge the hooks block and ways permissions into the existing settings, quoting hook command paths for `$HOME` values that contain spaces:

```bash
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
```

### 7. Done

Print: "Sync complete. Restart Claude Code to activate changes."
