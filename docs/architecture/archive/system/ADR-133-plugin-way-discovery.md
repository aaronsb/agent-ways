---
status: Rejected
date: 2026-04-25
deciders:
  - aaronsb
  - claude
related:
  - ADR-108
  - ADR-111
---

# ADR-133: Plugin Way Discovery

> **ARCHIVED — 2026-08-13.** No longer part of the active architecture set. Kept for history
> and so existing references still resolve.
>
> **Why:** Never implemented. Four months after proposal, no plugin way-path resolution exists; the only 'claude plugin list' call in the tree counts skills for the context-cost warning in hooks/ways/macro.sh.
>
> Nothing below this line has been edited.

> **Provenance.** This ADR was originally drafted on 2026-04-25 as ADR-129 on the
> exploratory `feat/plugin-way-discovery` branch (PR #76). That branch was discarded
> — it had drifted ~53 commits behind main and conflicted across the refactored
> ways-cli internals — but the design reasoning was sound and worth keeping. It is
> re-filed here as ADR-133 because the original ADR-129 number was reassigned on main
> to "instance suffix and heartbeat liveness." Status is **Proposed**: the design is
> captured and its load-bearing assumption (`claude plugin list --json`) is verified
> current, but it is not implemented. The **Implementation** section names specific
> code (`candidates.rs`, `cmd/corpus.rs`, the `SessionStart` hook chain) as it stood
> in April 2026; those internals have since been refactored, so treat that section as
> indicative of approach, not literal touch points.

## Context

Way discovery is currently hardcoded to two filesystem locations:

1. **Project-local**: `$PROJECT/.claude/ways/`
2. **Global**: `~/.claude/hooks/ways/`

Claude Code plugins can ship `ways/` directories inside their install paths (matching the project-local convention, demonstrated by the `x@tracer-plugins` plugin which contains `.claude/ways/fruity/way.md`). However, the ways system has no mechanism to discover or scan these. Plugin-shipped ways are invisible.

Claude Code provides a stable CLI interface for querying plugin state:

```
claude plugin list --json
```

Returns an array of installed plugins, each with:
- `id` — plugin identifier (`name@marketplace`)
- `installPath` — absolute path to the installed plugin on disk
- `enabled` — whether the plugin is currently active
- `scope` — `"user"` (global) or `"project"` (scoped to a specific project)
- `projectPath` — (project-scoped only) which project the plugin belongs to
- `version` — installed version
- `installedAt` / `lastUpdated` — timestamps

This is sufficient to resolve which plugins are active and where their files live.

### Design constraints

- **No per-invocation subprocess**: `ways scan` runs on every prompt and tool use. Shelling out to `claude plugin list --json` on each invocation adds unacceptable latency.
- **Don't couple to internal file formats**: Reading `installed_plugins.json` and `settings.json` directly is faster but couples to Claude Code's internal storage format, which may change without notice.
- **Use the official CLI**: `claude plugin list --json` is the stable public interface for plugin state.
- **Enabled means enabled**: The `enabled` field already reflects whether a plugin is active. No additional scope filtering is needed — if `enabled` is `true`, the plugin participates.
- **Version deduplication**: If the same plugin ID appears with multiple versions, use the latest (by `lastUpdated` timestamp). The CLI already resolves to the active version, but defensive dedup protects against edge cases.

## Decision

### Hybrid approach: resolve once, scan many

**At session start**, resolve enabled plugin way-paths via `claude plugin list --json` and write them to a session-scoped manifest. The `ways` binary reads this manifest during scans, adding plugin directories to the candidate collection alongside project-local and global ways.

### Session-start resolution

A new step in the `SessionStart` hook chain (after `ways init`, before `ways corpus --if-stale`):

1. Run `claude plugin list --json`
2. Filter to `enabled == true`
3. For each, check if `$installPath/ways/` exists on disk
4. Deduplicate by plugin name: if multiple versions, keep the one with the latest `lastUpdated`
5. Write the list of way-paths to `$SESSION_DIR/plugin-ways.json`

The manifest format:

```json
[
  {
    "id": "x@tracer-plugins",
    "path": "/Users/tracer/.claude/plugins/cache/tracer-plugins/x/1.0.0/ways"
  }
]
```

### Candidate collection

`collect_candidates()` gains a third source, inserted between project-local and global:

```
1. $PROJECT/.claude/ways/              — project-local (highest priority)
2. $PLUGIN/ways/                 — per enabled plugin (middle priority)
3. ~/.claude/hooks/ways/               — global (lowest priority)
```

The `ways` binary reads the session manifest (`plugin-ways.json`) and calls `collect_from_dir()` on each path. The existing `WalkDir`-based scanning, frontmatter parsing, domain filtering, and scope gating apply identically to plugin-sourced ways.

### ID namespacing

Plugin way IDs are prefixed with the plugin identifier to prevent collisions:

```
plugin:x@tracer-plugins/fruity       (from plugin)
softwaredev/code/security             (from global)
softwaredev/code/testing              (from project-local)
```

Same-ID ways across sources share a session marker (project-local overrides plugin overrides global). Plugin ways cannot shadow global ways unless they use the same domain/path structure intentionally.

### Corpus integration

`ways corpus --if-stale` must include plugin way directories so that semantic (embedding) matching works for plugin-shipped ways. The corpus generation reads the same session manifest to discover additional scan roots.

### Macro trust

Plugin macros (`macro.sh` files inside plugin ways) are third-party code. They follow the same trust model as project-local macros: disabled by default, enabled per-plugin via `~/.claude/trusted-plugin-macros` (or extending the existing `trusted-project-macros` mechanism).

## Consequences

### Benefits

- Plugins can ship ways alongside skills and hooks — a single plugin can provide guidance, tools, and workflows
- Plugin authors can use the full way authoring surface: frontmatter, semantic matching, macros, check curves, scope gating
- No coupling to Claude Code's internal plugin storage format — uses the stable CLI interface
- Session-start resolution means zero per-scan overhead from plugin discovery
- Existing way precedence model extends naturally (project > plugin > global)

### Costs

- Session-start adds one `claude plugin list --json` subprocess call (~100-200ms)
- Session manifest is a new file to manage (create on start, stale if plugins change mid-session)
- Corpus regeneration may take slightly longer with additional plugin way directories
- Plugin way authors must understand the ID namespacing scheme

### Risks

- **Mid-session plugin changes**: If a user installs/removes/toggles a plugin during a session, the manifest is stale until the next session or compaction. Acceptable — plugin changes are rare and a session restart is natural.
- **Manifest missing**: If the session manifest doesn't exist (e.g., older ways binary, failed resolution), `collect_candidates()` falls back to the current two-source behavior. No breakage.
- **Plugin path instability**: Plugin install paths include version strings that change on update. The session manifest captures the path at resolution time, so this is fine within a session. Cross-session, the next start re-resolves.

### Way file path convention

Each way lives in its own directory, named to match the way file. This enables sibling files (`.check.md`, `macro.sh`) alongside the way definition.

| Scope | Full path |
|-------|-----------|
| Global | `~/.claude/hooks/ways/{domain}/{way}/{way}.md` |
| Project-local | `$PROJECT/.claude/ways/{domain}/{way}/{way}.md` |
| Plugin | `$PLUGIN_INSTALL_PATH/ways/{domain}/{way}/{way}.md` |

Plugins use `ways/` at the plugin install root. Global ways use `hooks/ways/` under `~/.claude/` because they sit alongside other hook types.

## Implementation

> Indicative as of the April 2026 ways-cli structure; verify against current code before building.

### Touch points

1. **New hook script**: `hooks/ways/resolve-plugins.sh` — runs `claude plugin list --json`, filters, writes manifest
2. **`settings.json`**: Add `resolve-plugins.sh` to `SessionStart` hooks (after `ways init`)
3. **`candidates.rs`**: `collect_candidates()` reads session manifest and adds plugin dirs
4. **`candidates.rs`**: `collect_checks()` — same addition for check files
5. **`cmd/corpus.rs`**: Corpus generation reads manifest for additional scan roots
6. **Way ID derivation**: Prefix plugin-sourced IDs with `plugin:{id}/`
7. **Macro trust**: New trust file or extend existing mechanism for plugin macros
