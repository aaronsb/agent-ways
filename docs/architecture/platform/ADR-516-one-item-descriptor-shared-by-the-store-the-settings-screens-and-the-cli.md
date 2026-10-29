---
contract: adr/v1
kind: decision
verb: constrain
capability: [config, cli, attend]
amends: [ADR-143#Decision, ADR-403#Decision]
basis:
  - operator: aaronsb
    level: directed
    said: "let's make sure that in the configuration store, ui and cli config paths, we're consistent with how we keep and present configurations (such as the type of item it is, builtin, or external (and a path) etc)"
    via: session 2026-10-29
  - operator: aaronsb
    level: directed
    said: "the 'module' pattern likely needs to be shown and be re used consistently with more detail in the sensors, mods, and themes. this way we can see at a glance in the right side info panel if it's builtin or new. this also means that attend sensors that are just using the matcher/executor engine (the external sensor) can be listed there"
    via: session 2026-10-29, earlier in the same session
  - evidence: ADR-515
  - evidence: "the module card on feat/settings-module-card (faf55756, unmerged): one Card type in tools/agent-tui/src/tree.rs with title, summary, origin (Builtin | User), at, kind, source, version, state and rows, drawn by tools/agent-tui/src/app/card.rs for mods, sensors and themes"
  - precedent: ADR-503
  - precedent: ADR-185
  - precedent: ADR-143
  - precedent: ADR-403
  - precedent: ADR-512
  - precedent: ADR-506
  - upstream: "systemd keeps unit files in vendor, admin and runtime directories and never in a field of the file; systemd-delta(1) reports a unit changed in a higher directory as overridden (a whole file replaces it) or extended (a drop-in adds to it), next to the vendor file it changes"
agent:
  name: Claude
  model: claude-opus-5-5
observable:
  - 'run: ways settings items --json lists every theme, mod, sensor, way and judge profile, and selecting origin != "builtin" leaves exactly the ones a person or a project added, each with its path'
  - 'see: a command sensor in a project''s .claude/attend/sensors/ reads origin project on its card, in attend sensor list and in ways settings items'
  - 'see: a user file named nord.theme reads origin builtin with override user replaces and its path on the theme tab, and the same overrides entry in ways settings theme list --json'
  - 'run: for one item of each family, the card''s origin, override, kind, source, version and state rows equal the fields of its ways settings items --json object'
status: proposed
date: 2026-10-29
deciders:
  - aaronsb
related: [ADR-515, ADR-503, ADR-504, ADR-185, ADR-143, ADR-403, ADR-512, ADR-514, ADR-199, ADR-506, ADR-184]
---

# ADR-516: One item descriptor of kind, origin, path, source, version and state for themes, mods, sensors, ways and judge profiles, shared by the store, the settings screens and the CLI

## Summary

- **Decided:** Every theme, mod, sensor, way and judge profile is described by one descriptor: `family`, `name`, `kind`, `origin`, `path`, `overrides`, `source`, `version`, `state` and `detail`. `origin` is `builtin`, `user` or `project`. It is taken from where the definition is found and is never written into a file. A user or project item that shares a built-in's name is that built-in with an override, which either `extends` it or `replaces` it. One serializable type in `agent-settings` holds the descriptor. Each family builds its items in one function, and both the CLI and the screens call it. Every listing command's `--json` prints the descriptor under these field names, and a new `ways settings items` lists every family at once. The settings screens head each item's detail with one card drawn from it. The words `bundled`, `shipped`, `core`, `global`, `your ways` and `override` leave every surface.
- **Trades away:** The current JSON of `ways settings theme list` (`source`, `file`, `active`), the `ways` block of `ways status --json`, and the `Kind` column of `attend sensor list` all change, with no aliases (ADR-506). The ways tab's section names and the module card's own state wording, such as `installed · enabled`, give way to the shared words. A user's own `nord.theme` reads `builtin` with an override line instead of `user`. Each family loses the freedom to name these facts its own way.
- **One-way?** No. Reversible: the change covers words, JSON field names in five commands, and one Rust type. No file a person writes changes, since origin is never stored. Reverting gets more expensive as scripts and agents come to read the new field names, which ADR-185 §2 makes part of each verb's contract.
- **Probes:** *Not confident (words):* anything agent-ways did not ship reads `user` when it sits in your own config folder and `project` when it sits in a project's `.claude/` folder. You have called these "new" and "external", and this record keeps "external" for the kind, a sensor run by a command or an MCP server, which can sit in either place. *Confident (tweaked):* a built-in you have changed stays `builtin` at a glance, with a line under it naming your file and whether it adds to or replaces the built-in. That covers the git sensor with your own interval, or your own `nord.theme` in place of the bundled Nord.
- **Inversion:** At one end each surface describes items in its own words, tuned to its family and drifting apart. At the other end one generic registry holds every item as the same record, with nothing particular to its family. This record takes one shape for the facts every family shares and puts each family's own fields after them.

## Context

The settings store, the `ways settings` screens and the CLI each describe the items a person can add beside the ones agent-ways ships: themes, Claude Code mods, attend sensors, ways and judge profiles. ADR-515 audits all three surfaces. They describe the same six facts in different words. The built-in origin has five names (`builtin`, `bundled`, `shipped`, `core`, `global`), and the added origin has six (`user`, `your ways`, `your own`, `override`, `this project`, `another project`). attend folds origin into its kind column. A theme override is a third origin. A project's script sensor reads `user`, and the path sits under four field names. Two listing commands have no `--json`, which falls short of ADR-185 §2. No family stores an origin field: each one implies it by directory, by a compiled-in list or by the presence of a key.

The operator asked for the store, the screens and the CLI to keep and present an item the same way, including its type, whether it is built in, and its path. The detail pane should show at a glance whether a sensor, mod or theme is built in or new, and should list sensors that run on attend's generic engine beside the compiled ones. An unmerged branch adds one module card to the detail pane for mods, sensors and themes, with two origin values. ADR-403 moves sensors to one file per sensor in shipped, user and project directories, and ADR-503 §7 already holds that the CLI and the screens read one model.

## Decision

### 1. The descriptor

| Field | Type | Meaning |
|---|---|---|
| `family` | `theme`, `mod`, `sensor`, `way`, `profile` | the kind of item |
| `name` | text | unique within the family; a way's name is its id, such as `softwaredev/delivery/commits` |
| `kind` | text or null | what implements the item, from the family's list in §2; null where the family has one implementation |
| `origin` | `builtin`, `user`, `project` | the lowest layer that defines an item of this name |
| `path` | absolute path or null | the origin layer's definition; null when it exists only inside a binary |
| `overrides` | list | each higher layer that changes the item, lowest first: `{origin, how, path}`, where `how` is `extends` (it sets fields over the definition beneath) or `replaces` (a whole definition of the same name takes its place) |
| `source` | text or null | what runs or supplies the item when that is something other than its file: a crate, a command, an MCP server and tool, a plugin, a model |
| `version` | text or null | the source's version as the source reports it |
| `state` | one word | from the family's list in §2 |
| `detail` | text or null | one line that qualifies the state |

The origins:
- **`builtin`**: agent-ways ships the item, either compiled into a binary or as a file in the app dir, `$XDG_DATA_HOME/agent-ways`. A built-in can be copied to a new name and cannot be renamed or deleted (ADR-403 §5, and the theme tab's flow).
- **`user`**: the item is defined in the person's config root under `$XDG_CONFIG_HOME` and applies in every project.
- **`project`**: the item is defined under a project's `.claude/` and travels with the repository. A project's command and MCP sensors load only for a project on the allow-list (ADR-403 §2).

A target's config file sets keys and defines no items, so `target` is not an origin.

An item is identified by its family and name. A user or project definition that shares a built-in's name is an override of the built-in, and the item's origin stays `builtin`. A whole same-named file is `replaces`: a theme file, or a way under ADR-143's whole-file shadow. Field-level layering is `extends`: ADR-403's sensor files, a `gate.profiles` patch of a shipped profile, or an `attend.sensors.<name>.*` key in a config file today. The same rule applies between `user` and `project` for an item a person added. Whether an item is built in then depends only on its name, and the override entries show what changed it.

### 2. Each family's words

| Family | `kind` | `source` | `version` | `state` |
|---|---|---|---|---|
| theme | null | null | null | `active`, `inactive`, `invalid` |
| mod | `plugin` | the plugin id, such as `attend@agent-ways` | the shipped plugin version | `converged`, `drift`, `pending`, `disabled`, `removed`, `off` (ADR-512) |
| sensor | `crate`, `script`; under ADR-403, `crate`, `command`, `mcp`, and `ways` for ADR-199's sensors | the crate; the command's executable; `<server> <tool>` | the crate's version; null for the others | `active`, `off`, `not-compiled`, `missing`, `refused`, `failing`, `invalid` |
| way | null | null | null | `enabled`, `disabled`, `invalid` |
| profile | the provider: `anthropic`, `openrouter`, `claude-code` | the model | null | `active`, `inactive`, `invalid` |

A state that takes more than one word is written as a hyphenated token. A definition that fails its schema is listed with state `invalid`, and its `detail` names the first finding. It is never left out of a list. Fields that belong to one family follow the descriptor: a mod's `wanted` and its per-target installed versions, a sensor's `description`, `interval` and `min_interval`, a theme's `label`.

### 3. The store encodes origin by where a definition is found

No item file carries an origin. A file takes its origin from the directory it is found in, so a file copied between layers takes the origin of the place it was copied to.

| Family | `builtin` | `user` | `project` |
|---|---|---|---|
| theme | compiled into `agent-theme` | `$XDG_CONFIG_HOME/agent-ways/themes/<name>.theme` or `.toml` | none |
| mod | `plugins/<name>` in the app dir | none | none |
| sensor, under ADR-403 | shipped files, projected to `$XDG_DATA_HOME/agent-ways/attend/sensors/` | `$XDG_CONFIG_HOME/attend/sensors/<name>.yaml` | `.claude/attend/sensors/<name>.yaml` |
| sensor, until then | a name in `BUILTINS` | `attend.sensors.<name>.script` in `$XDG_CONFIG_HOME/attend/config.yaml` | the same key in `.claude/attend.yaml` |
| way | `$XDG_DATA_HOME/agent-ways/hooks/ways` | `$XDG_CONFIG_HOME/agent-ways/ways` | `<project>/.claude/ways` |
| profile | `profiles.yaml`, compiled into `ways-agent-core` | `gate.profiles.<name>` in `agent.yaml` | none |

A mod's `mods.<name>.*` keys are settings that apply to the mod, and the mod is defined by its plugin folder.

The settings `layer` field (ADR-503 §9) uses the same words. A key set by a shipped file reports `layer: builtin`, which is ADR-403's shipped layer. `default` remains the schema's canonical value, and `user`, `target`, `project` and `file` are unchanged.

### 4. One type, one producer per family

The descriptor is one type, `Item`, in `agent-settings`. That crate depends on no other workspace crate, and `ways-cli`, `attend` and `agent-tui` can all reach it. It serializes to the JSON of §5. Each family's owner builds its items in one function, which the family's list verb and the screens both call. The screens read sensors from `attend sensor list --json` until the sensor spec of ADR-403 §7 lets `ways` read the files itself. The branch's `Card` is built from an `Item` plus the family's rows, and its `Origin` gains `Project`.

### 5. The CLI

Every command that lists or shows items prints one object per item under `--json`. The descriptor's fields come first, under the names of §1, and are always present, null where they do not apply. The family's own fields follow. Text output uses the same words.

- `ways settings items [<family>] [--json]` is new. It lists every item of every family, or of one, and is the CLI form of the screens' cards. Its text prints one line per item: family, name, origin, kind, state and path.
- `ways settings theme list` drops `source`, `file` and `active` and prints `origin`, `path`, `overrides` and `state`, with `label` after them. Its text prints the origin and the state, and lists invalid files.
- `ways manage mod list` gains `--json`: the descriptor, `wanted`, and a `targets` list carrying the fields `ways status` prints for each target.
- `attend sensor list` gains `--json`: the descriptor, then `description`, `interval` and `min_interval`. `kind` carries the kind, and `<crate>@<version>` splits into `source` and `version`. The text table gains an `Origin` column, and its `Kind` column prints the kind.
- In `ways status`, `ways.global_total`, `global_semantic`, `user_total` and `user_semantic` become `ways.builtin` and `ways.user`, each `{total, semantic}`. The text line `Core ways:` becomes `Builtin ways:`, and the project counts stay under `projects`. This amends ADR-143 §3, which named that line `core`. ADR-143's roots keep their locations and precedence.

### 6. The card

Every item node on the mod, sensors, theme, ways and gate tabs heads its detail with the card for its `Item`. The figure shows three cards: a built-in sensor a user file extends, a project's command sensor that the allow-list refuses, and a bundled theme that a user file replaces.

![Three cards side by side. The git sensor: origin builtin, override user extends with its sensor file, kind crate, source attend, version 0.33.0, state active. The slack-unread sensor: origin project with its file under .claude/attend/sensors, kind command, source slack-mcp unread, state refused because the project is not on attend.project_sensors. The Nord theme: origin builtin, override user replaces with the user's nord.theme, state active.](figures/ADR-516-item-card.svg)

- **Rows, in order:** the title and a dim summary line; a blank line; `origin` (the word, then ` · ` and the path when there is one); one `override` row per entry, as `<origin> <how> · <path>`; `kind`; `source`; `version`; `state` (the word, then ` · ` and the detail); then the family's rows. A null field draws no row.
- **Colour:** `builtin` takes the info role. `user` and `project` take the off-default accent, in bold. Each family has one table that maps its state words to a tone (ok, plain, warn, err), and the card and the CLI's coloured text both read it.
- **Ways tab:** its scope sections are named `project`, `user` and `builtin`. `other projects`, `prefixes` and `not found` stay, since they are views and placeholders. The way box's `from`, `root` and `file` rows become the card's origin and path rows.
- **Sensors tab:** it lists every sensor that `attend sensor list --json` returns, of every kind.
- **Engine picker:** it shows each profile's origin word beside its name.

## Consequences

### Positive

- The detail pane shows whether an item is built in, added by its user or added by its project, in the same row and word on every tab.
- An agent or a script can find what a person added with one command and one field, across every family.
- A project's own sensors, ways and their files can be told apart from the user's, which is where the allow-list of ADR-403 §2 applies.
- A file that fails to load shows up as `invalid` instead of being missing from the list.
- The card, the text output and the JSON come from one function per family, so they cannot drift apart again.

### Negative

- Scripts that read `ways settings theme list --json`, the `ways` block of `ways status --json` or the `Kind` column of `attend sensor list` break, and nothing keeps the old names.
- The screens depend on `attend sensor list --json` for sensor descriptors, so an absent or slow `attend` leaves the sensor cards with only what the settings layers say.
- Every family has to define a list of state words and their tones, and keep the list closed.
- The branch in flight changes before it merges: `Origin` gains `Project`, a theme override reads `builtin`, and the card's state words become the CLI's.

### Neutral

- This record amends ADR-403 §2 and §5, which call the lowest sensor layer `shipped` and its files `bundled`. That layer is the origin `builtin` and reports `layer: builtin`. The dated note goes on ADR-403 when this record is accepted.
- Targets, skills, agents, commands, the MCP server and guard hooks have no origin to show and stay outside the descriptor (ADR-515).
- Internal names such as `manifest.global_hash` and `WayScope` can keep their names. Only what a surface prints changes.

## Alternatives Considered

- **Two origins, `builtin` and `user`, as the branch has them.** A project's items would read as the user's, and the screens could not show which items came with a repository and run code its author chose.
- **`builtin` and `external`, with the layer in a second field.** This needs two fields for one fact. "External" is also the operator's word for a sensor run by a command or an MCP server, which is a kind and can be a user's or a project's.
- **An `origin` key in each item file.** A copied file would carry a stale origin, and the directory already says where the file is, which ADR-143 and ADR-403 §2 rely on.
- **Origin as the layer of the definition in effect, so a user's `nord.theme` reads `user`.** Whether a name is shipped would no longer be a fact of the item, and the copy, rename and delete rules of ADR-403 §5 key on exactly that.
- **A shared vocabulary with JSON shaped per family.** The screens and scripts would still parse each family separately, and `ways settings items` could not exist.
- **`layer: default` for keys set by shipped sensor files.** `default` would then mean both the schema's canonical value and a file agent-ways ships.
