---
contract: adr/v1
kind: evidence
capability: config
status: proposed
date: 2026-10-29
deciders:
  - aaronsb
related: [ADR-516, ADR-503, ADR-185, ADR-143, ADR-403, ADR-512, ADR-514, ADR-184]
---

# ADR-515: How themes, mods, sensors, ways and judge profiles are kept and shown across the store, the settings screens and the CLI, audited 2026-10-29

## Summary

A read-only audit on 2026-10-29 at `main` `a7d8099f` looked at five item families that have an origin: an item agent-ways ships, or one a person or a project adds. These are themes, Claude Code mods, attend sensors, ways and judge profiles. For each family it records three things: where the store keeps the item and how it encodes the origin, what the `ways settings` screens show, and what each listing command prints as text and under `--json`. The families describe the same six facts (kind, origin, path, source, version, state) in different words and fields. The built-in origin alone has five names. No family stores its origin as a field, and every family implies it by directory, by a compiled-in list or by a key's presence. Three listing commands have no `--json`, or have one that leaves the origin out. Twelve inconsistencies are listed at the end. ADR-516 decides one descriptor for them.

## Method

- The CLI commands were run from debug builds of `main`: `ways 1.47.1 (ways-v1.47.1-10-ga7d8099f)` and `attend 0.33.0 (a7d8099f)`. Only commands that read were run, against the live install of this host, which has no user themes, user ways or user profiles.
- The screens were read from `tools/ways-cli/shots/verify.sh mod theme-menu settings` at 120x35 and from the golden frames under `tools/ways-cli/tests/fixtures/settings-tui/` (`sensors-browse`, `theme-browse`, `theme-menu-user`, `ways-project-way`, `gate-pick-engine`, `gate-browse`, `install-browse`).
- The store was read from source: `tools/agent-theme/src/bundled.rs`, `tools/ways-cli/src/cmd/settings/themes.rs`, `tools/ways-cli/src/cmd/attend_mod.rs`, `tools/attend/src/sensors/mod.rs`, `tools/attend-config/src/schema.rs`, `tools/ways-cli/src/cmd/settings/tui/build.rs`, `tools/ways-agent-core/src/profile.rs`, `tools/agent-settings/src/load.rs`.
- The unmerged module card on `feat/settings-module-card` (`faf55756`) was read with `git show`. The branch was not checked out. Its rows are listed as a fourth surface because they are the newest presentation.
- `ADR-403` was read as it stood on `adr-403-sensor-state` (PR #1204), still proposed. It was accepted and merged later the same day.

## Findings by family

In the tables, "—" means the surface shows nothing for that fact.

### Themes

| Surface | Kind | Origin | Path | Source | Version | State |
|---|---|---|---|---|---|---|
| Store | — | implied: compiled into `agent-theme` (`BUNDLED`), or a file in `$XDG_CONFIG_HOME/agent-ways/themes/<name>.theme` or `.toml`; a user file with a bundled name replaces that theme (`Source::Override`); no project layer | the user file | — | — | `theme.active` in `config.yaml` names one |
| Theme tab | — | row `source`: `bundled (read-only)`, `user  <path>` or `override  <path>` | inside the `source` row; `saves to` names the directory | — | — | row `active`: `yes` or `no (Enter)` |
| `ways settings theme list` | — | text `bundled`, `user` or `override`; JSON `source` with the same values | JSON `file`, `null` for a bundled theme | — | — | text suffix ` active`; JSON `active: true\|false` |
| Module card (branch) | none | `builtin` or `user`; an override reads `user` with `overrides the bundled <name>` | in `source` | `bundled (read-only)` or the path | none | `active` or `inactive (Enter uses it)` |

A user file that fails to load is kept in `ThemeSet.rejected` and is absent from `ways settings theme list`.

### Claude Code mods

| Surface | Kind | Origin | Path | Source | Version | State |
|---|---|---|---|---|---|---|
| Store | — | implied: every mod is shipped, a folder under `plugins/` in the app dir listed in `attend_mod::MODS`; `mods.<name>.install` in the user, target or project config holds the wanted state (ADR-512) | the marketplace folder | the plugin id `<name>@agent-ways` (`Mod.id`) | the plugin manifest's version; Claude Code's installed version per target | derived per target |
| Mod tab (main) | — | — | — | — | — | row `<name>  <state> [a]`; the detail is the key `mod.<name>` and its doc |
| `ways manage mod list` | — | — | — | — | header `── <name>  <shipped>  <state> ──`; `<target>: installed <v>` | `converged`, `drift`, `pending`, `disabled`, `removed`, `off`; no `--json` |
| `ways status` | — | — | — | text `mod: <id>` | JSON `shipped`, `installed`, `runs` | JSON `install.targets[].mods.<name>.state`, with `wanted`, `enabled`, `pending`, `removed`, `foreign`, `error`, `degraded` |
| Module card (branch) | `plugin` | `builtin` | in `source` | `<id> · <folder>` | `shipped <v> · installed <v>` | `installed · enabled`, `drift · repair converges it`, `off · not wanted` and others, which differ from the CLI's `converged`, `drift` and `off` |

### Attend sensors

| Surface | Kind | Origin | Path | Source | Version | State |
|---|---|---|---|---|---|---|
| Store | implied: a name in `BUILTINS` (`attend-config/src/schema.rs`) is compiled; one with a `script` key is a script | implied: compiled, or the layer of the file holding `attend.sensors.<name>.script`, either `$XDG_CONFIG_HOME/attend/config.yaml` or `.claude/attend.yaml` | the config file holding the key | the crate, or the script path | the crate's `CARGO_PKG_VERSION` | `attend.sensors.<name>.enabled` |
| Sensors tab (main) | — | doc line `The built-in <name> sensor`; script sensors are not listed | — | — | — | the `enabled` row |
| `attend sensor list` | column `Kind`: `builtin` or `script` | carried by `Kind` | — | column `Source`: `<crate>@<version>` or the script path | inside `Source` | column `State`: `active`, `off`, `not compiled`, `missing`; no `--json` on main |
| `attend sensor list --json` (branch) | `kind`: `builtin` or `script` | — | — | `source` | inside `source` | `state` |
| `ways settings get\|list --json` | — | — | per key `file` | — | — | per key `value`, `layer` |
| Module card (branch) | `crate` or `script` | `builtin` or `user`; a script sensor in `.claude/attend.yaml` also reads `user` | after the origin word | the crate, or the script path | the crate version | the `enabled` value, or `not compiled` |

ADR-403 moves every sensor to one YAML file in a shipped, user or project directory, with kinds `crate`, `command`, `mcp` and `ways`, and calls the lowest layer `shipped` and `bundled` in its text.

### Ways

| Surface | Kind | Origin | Path | Source | Version | State |
|---|---|---|---|---|---|---|
| Store | — | implied by root (ADR-143): `$XDG_DATA_HOME/agent-ways/hooks/ways`, `$XDG_CONFIG_HOME/agent-ways/ways`, `<project>/.claude/ways`; a same-named way in a higher root replaces the lower one | the way file | — | — | toggles in `.claude/ways.yaml` and `disabled_domains` |
| Ways tab | — | section names `this project`, `your ways`, `shipped`, `another project`; way box row `from this project` | way box rows `root` and `file` | — | — | the toggle's value |
| `ways status` | — | text `Core ways:`, `User ways:`, `Projects:`; JSON `ways.global_total`, `global_semantic`, `user_total`, `user_semantic`, `projects[].ways_count`, `manifest.global_hash` | `projects[].path` | — | — | — |
| `ways author graph` | — | — | — | — | — | — |

ADR-143 names the roots `core`, `user` and `project`. No command lists ways one by one with their origin.

### Judge profiles

| Surface | Kind | Origin | Path | Source | Version | State |
|---|---|---|---|---|---|---|
| Store | `provider` field | implied: a name in `shipped()` (`profiles.yaml` compiled into `ways-agent-core`), or one under `gate.profiles` in `agent.yaml`; a `gate.profiles` entry with a shipped name patches it; a shipped profile has a `display_name` and one of your own has none (`profile.rs`, `Profile`) | `agent.yaml` | `provider` and `model` | — | `gate.engine` names one |
| Gate tab | row `provider` | engine picker: a shipped profile shows its display name, one of your own shows nothing | — | rows `provider`, `model` | — | `● current` in the picker |
| `ways settings help gate.engine` | — | prose: `A shipped profile or one of your own` | — | — | — | — |
| `ways settings list gate.profiles --effective --json` | per key | `layer: default` for a shipped profile | per key `file` | per key | — | — |

### Families with no origin to show

- **Targets** are places, not items. The one implicit default reads `(none recorded)  ~/.claude (default)` on the install tab, and `ways manage target list --json` prints `explicit`, then `path`, `dir`, `enabled`, `observe`, `config`, `config_present` and `state` for each target.
- **Skills, agents and commands** are projected roots. The settings store holds none of them.
- **The MCP server** is one shipped server. `ways status` prints it as `mcp:`.
- **Guard hooks** ship deactivated (ADR-181) and have no settings keys.

### The settings layer word

`ways settings get|list --json` prints a key's `layer` as one of `default`, `user`, `target`, `project` and `file` (`agent-settings/src/load.rs`, `Layer.name`). ADR-403 adds a `shipped` layer for sensor keys.

## Inconsistencies

1. **The built-in origin has five names.** `builtin` is used in the `attend sensor list` Kind column and the module card. `bundled` is used in the theme list, the theme tab and ADR-403's flow text. `shipped` is used in the ways tab, ADR-403's layer, ADR-512 and the `gate.engine` help. `core` is used in ADR-143 and the `ways status` text. `global` is used in the `ways status` JSON and `manifest.global_hash`.
2. **The added origin has six names and no split.** The surfaces use `user`, `your ways`, `your own`, `override`, `this project` and `another project`. The operator has said "new" and "external". The theme list and the branch card cannot show a project origin, and the ways tab can.
3. **Kind and origin are one column in attend.** `attend sensor list` prints `builtin` as a kind. The branch card prints kind `crate` and origin `builtin` for the same sensor.
4. **An override is a third origin in themes.** `source: override` folds a built-in name and a user file into one word. The branch card turns the same theme into `user`.
5. **A project's sensor reads `user`.** On the branch card a script sensor in `.claude/attend.yaml` reads `user`, and `attend sensor list` names no layer or config file.
6. **The path has four field names.** It is `file` in the theme JSON and the settings JSON; `path`, `dir` and `config` in the target JSON; `source` for a script sensor; and `at` or `source` on the card.
7. **The version sits inside another field.** `attend sensor list` prints `sensor-peers@0.6.0` in `Source`. Mods print `shipped`, `installed` and `runs`, and the card prints `version`.
8. **Listing commands without `--json`.** `ways manage mod list` and `attend sensor list` on main have none, which falls short of ADR-185 §2. The branch's `attend sensor list --json` has no origin or path.
9. **State has a different word on each surface.** For themes: JSON `active: true`, the tab's `yes`, and the card's `active`. For mods: the CLI's `converged` and the card's `installed · enabled`. The sensor state `not compiled` has a space in it.
10. **Three tabs show no origin on main.** The mod tab and the sensors tab show none. The ways tab shows it only as a section heading and a `from` row, and in a setting's detail `from` means the settings layer instead. The engine picker implies it by whether a display name is present.
11. **No command lists ways with their origin.** `ways status` gives counts by root, and `ways author graph` gives no origin.
12. **An invalid item is hidden.** A theme file that fails to load is missing from `ways settings theme list` instead of being listed as invalid.
