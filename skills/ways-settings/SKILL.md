---
name: ways-settings
description: Interview-driven authoring of Claude Code's settings.json as composable, lintable fragments — scaffold from the settings schema, lint, compile, and project into ~/.claude. Synthesizes Claude Code's own config knowledge and its /insights usage report with the operator's history. Use when the operator wants to set up, review, or change their Claude Code settings or configuration. Not for editing individual ways (that is the ways skill), CLAUDE.md memory, or updating the agent-ways install (ways-update).
allowed-tools: Bash, Read, Edit, Write
---

# ways-settings: interview the operator to author Claude Code config

This is the **conductor** for the `ways settings` CLI (ADR-147, ADR-149). The CLI
is deterministic mechanism — `lint`, `new`, `schema`, `compile`, `project`. This
skill is the intelligence on top: it *interviews* the operator, authors config
**fragments** (markdown files with a `settings:` YAML block + a rationale body),
then lints, compiles, and projects them into `~/.claude/settings.json`.

## The one rule: synthesize with Claude Code, don't rebuild it

You do not re-teach Claude Code its own settings. You **compose four sources**:

1. **Your own config knowledge** — you already know what `statusLine`,
   `permissions`, `cleanupPeriodDays`, etc. do. Use it.
2. **The schema** — `ways settings schema` and `ways settings new <key>` give the
   authoritative key set, types, and descriptions. This keeps every fragment valid
   by construction; never hand-invent a key.
3. **The operator's history** — what they say in the interview, and (v2) ways
   telemetry (`ways permissions audit`, firing stats).
4. **The `/insights` report** — Claude Code's own analysis of the operator's last
   30 days, at `~/.claude/usage-data/report-<timestamp>.html`. **Read it, don't
   parse it** — open the newest report and interpret it the way a human would.

The operator's answer to *"why do you want this?"* becomes the fragment's markdown
body. The interview **is** the documentation.

## Resolve the store first (this skill is global — assume no cwd)

```bash
STORE="${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/settings"
mkdir -p "$STORE"
command -v ways >/dev/null 2>&1 || { echo "the 'ways' binary is not on PATH — is agent-ways installed?"; exit 1; }
ways settings schema >/dev/null 2>&1 || echo "note: settings schema unavailable — run 'ways settings schema --refresh' (lint/scaffold need it)"
```

All `ways settings` commands below take the store path explicitly; never rely on
the current directory.

## Sub-functions

Pick the one that matches the request; a full "set up my config" runs author →
check → rebuild → project in order.

### author — the interview

1. Find out what the operator wants (a permission, a model default, a status line,
   cleanup policy…). If they're vague, offer a few concrete options drawn from the
   schema and from `/insights` (see *suggest*).
2. Scaffold the fragment — **let the CLI create it** so the key is schema-valid:
   ```bash
   ways settings new <key> --scope <user|project|managed> --dir "$STORE"
   ```
   (Omit `--scope` to let it default: `managed` for managed-only keys, else `user`.)
3. **Fill it in.** `Edit` the generated `NN-<key>.md`: replace the placeholder value
   with the real one, and rewrite the body with the operator's *rationale* — their
   "why", in their words. That body is the config's `git blame`.
4. Refuse to invent keys the schema doesn't know; if a key is missing, it may be
   newer than the vendored schema — offer `ways settings schema --refresh`.

### check
```bash
ways settings lint "$STORE"          # add --json for machine output
```
Fix errors before compiling; warnings (managed-overridable, duplicate) are advice.

### rebuild (compile)
```bash
ways settings compile "$STORE" --scope user        # baked settings.json to stdout
ways settings compile "$STORE" --out "$STORE/../build"   # per-scope files + provenance.json
```
Compile is lint-gated — it refuses a store with errors.

### project (writes the live config — be careful)
```bash
ways settings project "$STORE" --dry-run           # ALWAYS preview first
ways settings project "$STORE"                     # apply (keeps a .ways-project.bak)
```
- **Always dry-run and show the operator the change summary before applying.**
  Projecting writes `~/.claude/settings.json` (user) or `$CLAUDE_PROJECT_DIR/.claude`
  (project). Confirm before the real run.
- `project` coexists with the reconciler: it owns the fragment keys and preserves
  `hooks`/`permissions` (the reconciler's) — it skips those with a warning.
- Managed scope is never auto-written; it prints a blob to paste into the console.

### pull-schema
```bash
ways settings schema --refresh       # fetch the latest schema (no rebuild needed)
```
Do this when a key the operator wants isn't recognized, or the schema looks stale.

### suggest — mine what Claude Code already knows (the high-value move)

Read the newest `/insights` report and turn its **settings-relevant** findings into
fragment proposals:

```bash
ls -t "$HOME/.claude/usage-data/report-"*.html 2>/dev/null | head -1
```
- If a report exists, `Read` it and focus on **"Where Things Go Wrong"** (friction
  → permission or tool-timeout settings), **"How You Use Claude Code"** (tool/model
  usage → defaults), and **"Existing CC Features to Try"** (→ settings toggles).
- Map those to *settings* fragments. **"Suggested CLAUDE.md Additions" is memory,
  not settings** — a different surface; mention it, but don't turn it into a
  settings fragment.
- A skill **cannot run `/insights` itself** (skills can't invoke slash commands).
  If there's no report or it's stale, ask the operator to run `/insights`, then
  read the fresh file.
- v2: also mine `ways permissions audit` and firing stats for repeated manual
  grants worth a fragment.

## Close out

- Settings changes land in `~/.claude/settings.json`; some take effect on the next
  Claude Code session — tell the operator if a restart is needed.
- The store (`$STORE`) is git-friendly — suggest they version it; the fragment
  bodies make the history self-documenting.

## See Also

- ADR-147 (composable settings.json fragments) — the CLI shape contract this drives.
- ADR-149 (this skill's decision record) — the synthesize-with-CC thesis.
- `ways-update` — updating the agent-ways install (not this skill's job).
