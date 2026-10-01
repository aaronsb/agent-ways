---
contract: adr/v1
kind: decision
verb: change
capability: [config, cli]
amends: ADR-185#Decision
basis:
  - operator: aaronsb
    level: directed
    said: I'd like to look over the ways cli and see if we can consolidate some of the commands. for example, settings could be more of a tree, and honestly, a univseral module or crate for interacting in a tui (as an option to setting properties etc)
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: I'd also like to think about how we can reduce the complexity of the non interactive cli - less explainatory text that is by default, and a --help command that replays the same help from the tui.
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: cli for agents and integration and humans who know what they're changing
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: the tui is for setting too. and since both ways of changing config set the config, it's interchangable. I could use the tui and configure stuff, then the file is there. I could just copy the file around and inspect them with the cli too.
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: directed
    said: attend could also use the same system - so let's see if we can re-use the same config model and ux conventions across both (they have quite a bit of shared code already)
    via: chat, session 02e97f86, 2026-10-01
  - operator: aaronsb
    level: authored
    said: one app
    via: chat, session 02e97f86, 2026-10-01, asked whether ways and attend share one settings app or each get their own on the same engine
  - evidence: ways settings live in four files with four writers; only the ways-core targets writer locks, writes atomically and replaces just its own block (tools/ways-core/src/config.rs)
  - evidence: attend's config keys are defined three times (hand parser in tools/attend/src/config.rs, arrays in config_lint.rs, config show); attend tune --apply rewrites the engagement block without a lock and drops comments (tools/attend/src/cmd/tune.rs)
  - evidence: ways --help lists 37 top-level commands mixing operator commands with authoring, tuning and hook plumbing
  - evidence: the dry-run spike tools/spikes/settings-tui (branch spike/settings-tui) loaded the real ways files into one typed tree with queued actions, masked key entry, per-tab review and apply, guided flows and a theme tab, under 107 ratatui TestBackend tests
  - precedent: ADR-185
  - precedent: ADR-131
  - operator: aaronsb
    level: guided
    said: and attend will likely get thinner as we expand the ways mcp
    via: chat, session 02e97f86, 2026-10-01
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "we should have a typed structure template for settings, so we can emit a settings object (or parts of it) as well as lint it. This means that a hand edited- settings can fall back if a section of settings is illegal or a missing part needs to be reconstructed from cannonical"
    via: chat, session 02e97f86, 2026-10-01, answering the ADR probes
    covers: [files]
  - operator: aaronsb
    said: "I think an exit code would make sense for a --property set mode. an object mode could be verbose by accepting a config setting object, return an object with lint result and acceptance, and emit an object fragment that can fit back into the type config."
    via: chat, session 02e97f86, 2026-10-01, answering the ADR probes
    covers: [quiet]
observable:
  - 'see: ways settings set matching.semantic_fire_probability 0.4 on a commented config.yaml changes that one line and keeps every comment'
  - 'see: a config.yaml copied from another machine shows the same values in ways settings list --file <copy> and in the TUI'
  - 'see: ways settings help <key> prints the same text as the TUI''s detail pane for that key'
  - 'see: ways settings shows attend''s tabs beside the ways tabs, and attend tune --apply leaves hand edits in place'
  - 'see: a hand-edited config.yaml with one illegal section loads every other section as written, and the diagnostic names the file, line and section'
  - 'run: ways settings emit <section> prints a fragment that ways settings apply --file accepts unchanged'
  - 'run: ways settings set <key> <value outside its range> exits 3 and writes nothing'
status: proposed
date: 2026-10-01
deciders:
  - aaronsb
related: [ADR-504, ADR-185, ADR-131, ADR-184, ADR-111, ADR-501]
---

# ADR-503: Settings are files described by one typed registry; the CLI and the TUI are two ways in

## Summary

- **Decided:** The settings of `ways` and `attend` are one typed structure, a schema per application, held in a shared `agent-settings` crate that also loads the layers, lints and writes the files. The schema's defaults are the canonical settings, and any part of the canonical or effective settings can be emitted as a fragment shaped like the file. The files stay the source of truth. Loading degrades per section: a section that fails the schema falls back to canonical with a diagnostic, and a missing part is filled from canonical. `ways settings` has a quiet property mode that reports through exit codes and a verbose object mode that takes a settings object and returns lint findings, acceptance and the fragment written. The settings TUI (ADR-504) reads and writes through the same schema and writer, and the CLI's long help is the schema text the TUI shows.
- **Trades away:** Each tool's freedom to define a setting where it reads it: a key not in the schema cannot be read or written. The strict parse of `agent.yaml`, in which an unknown key rejects the file. The prose output of `ways config show`, `ways agent config` and `attend config show`, which scripts may parse.
- **One-way?** No for the files: their paths and formats stay, loading never rewrites them, and the writer keeps everything it does not set. In part for the CLI: the exit codes and the object-mode shape become contracts with scripts and agents, and a script that parses today's prose breaks. Old command names stay as hidden aliases for one release; old output shapes do not.
- **Probes:** *Confident (files):* you expect to edit a settings file by hand or copy it to another machine, and have both the CLI and the TUI pick it up with your comments intact. *Not confident (quiet):* you are content for `ways settings set` to print nothing when it succeeds, at a terminal as well as in a script.
- **Inversion:** Between settings defined by whatever code happens to read them, and settings held in a store that files are generated from. The decision keeps hand-editable files as the truth and puts one schema over them that can lint any part of a file and rebuild it from canonical.

## Context

`ways` settings live in four files, each with its own writer: `config.yaml` (hand edits; `ways config target`), `.claude/ways.yaml` (`ways disable` and `ways enable`, ADR-131), `agent.yaml` (`ways agent use` and `mode`; hand edits) and `keys/<provider>` (`ways agent key`). `ways config show` lists the first and `ways agent config` the third; no command shows all of them, and most values can only be set by editing YAML. `agent.yaml` is parsed with `deny_unknown_fields` (`tools/ways-agent-core/src/profile.rs`), so one unknown key rejects the whole file. Only the targets writer in `ways-core` locks, writes atomically and replaces just its own block (`tools/ways-core/src/config.rs`, `TargetsLock` and `write_targets_to`).

`attend` reads `~/.config/attend/config.yaml` and `.claude/attend.yaml` with a hand-written indent parser (`tools/attend/src/config.rs`, `apply_config`). Its keys are defined three times: in that parser, in the arrays of `config_lint.rs`, and in `config show`. `attend tune --apply` rewrites the whole `engagement:` block without a lock, drops comments and hardcodes two values (`tools/attend/src/cmd/tune.rs`, `apply_engagement_tune`); `attend config init` overwrites the file.

`ways --help` lists 37 top-level commands, mixing the ones an operator runs with authoring, tuning and hook plumbing.

The operator set the terms: the TUI is for setting too; both ways of changing config set the files, so they are interchangeable, and the files can be copied and inspected with the CLI. The CLI serves agents, integrations and people who know what they are changing, with less explanatory text by default and a `--help` that replays the TUI's help. Settings are a typed structure that can emit a settings object or part of one and lint it, so a hand-edited file falls back per section and a missing part is rebuilt from canonical. Setting one property reports through an exit code; an object mode accepts a settings object and returns lint results, acceptance and a fragment that fits back into the structure. `attend` uses the same config model and conventions, in one settings app with `ways`.

A dry-run spike (`tools/spikes/settings-tui` on `spike/settings-tui`) loaded the real `ways` files into one tree of four roots, with typed values, queued actions and masked key entry, and printed the change set a real writer would make.

## Decision

1. **The typed settings structure.** A shared crate, `agent-settings`, holds one schema per application, `ways` and `attend`, registered at build time. The schema names every section and key. Each key has a dotted name, a type (bool, bounded int or float, choice, text, path, read-only, secret), a range, a default, a one-line and a long doc, a scope (user, project or both), the layers it reads, and the file and key path it writes. The defaults together are the canonical settings. Parsing, validation, lint, show, help and shell completion all derive from the schema; no tool keeps a second list of its keys.

2. **Objects and fragments.** From the schema the crate emits the canonical settings object, the effective object (resolved through the layers), or a fragment of either selected by section or key prefix. A fragment has the shape of the file it belongs to, so it can be pasted into that file or passed to object mode (§8). Lint checks a file against the schema and reports each finding with its file, line, section and key: a value of the wrong type or out of range, an unknown key, a missing section.

3. **Layered loading with provenance.** A value resolves through its layers (default, user file, project file, and environment where a key declares one), and keeps the layer it came from.

4. **Loading degrades per section.** A section that fails the schema loads as its canonical value, and the other sections of the file load as written. An unknown key is a lint finding and makes its section fall back. A missing section or key takes its canonical value. Each fallback produces a diagnostic naming the file, line and section, on stderr of the command that loaded it and in `ways status`, with the repair: `ways settings lint` to list the findings, and `ways settings fix <section>` to write the canonical fragment for that section through the writer. Loading never rewrites a file. This replaces the `deny_unknown_fields` parse of `agent.yaml`.

5. **One writer.** Every write to a settings file goes through one writer, generalised from the `ways-core` targets writer: a lock file beside the target, a temporary file renamed into place, and a change to only the keys being set. Other keys, comments and order stay as they were. `ways disable` and `enable`, `ways agent use` and `mode`, the targets verbs, `settings fix`, `attend tune --apply` and `attend config init` all use it. `config init` creates a file only when none exists.

6. **The files are the source of truth.** There is no settings store besides the files. A file written by the TUI, the CLI, a hand edit or a copy reads the same way to both front ends. `--file <path>` points `get`, `list`, `lint` and the TUI at a file other than the live one. A TUI that sees a file change on disk reloads it, and a tab holding pending edits to that file shows the conflict in its review.

7. **Where `set` writes.** A key with one scope writes to it. A key with both scopes writes the user layer unless `--project <dir>` is given.

8. **The CLI.** `ways settings` has two modes over the same schema and writer.
   - *Property mode*, for one key: `get <key>` prints the value, `list [prefix]` prints `key=value` lines, and `set <key> <value>` and `unset <key>` print nothing. It reports through exit codes:

     | Code | Meaning |
     |---|---|
     | 0 | done |
     | 2 | usage error or unknown key |
     | 3 | value rejected by the schema |
     | 4 | written, but a higher layer overrides it |
     | 5 | write failed (lock, permission, disk) |

     On a non-zero code, stderr carries one line naming the problem and the next command, such as `ways settings help <key>` or the overriding file.
   - *Object mode*, for more than one key: `apply` takes a settings object, JSON or YAML, on stdin or from `--file <path>`. It writes the keys the schema accepts and returns a JSON object holding the lint finding for each key, the keys accepted and the keys rejected, and the fragment written, shaped to fit back into the settings. `--dry-run` writes nothing and returns the fragment that would be written. Its exit code follows the table: 3 when any key is rejected, 4 when any accepted key is overridden. Object mode is verbose by design and is the mode agents and integrations use.
   - `emit [section|prefix]` prints the canonical fragment, or with `--effective` the effective one. `lint` exits 3 when it has findings.
   - `--json` on `get` and `list` returns a fragment of the same shape, in the stored and `--effective` views of ADR-185 §3, with each key's layer, default and file beside it. For the settings verbs this replaces ADR-185 §1's table form.
   - Colour only on a terminal without `NO_COLOR`; the same content in a pipe.
   - Shell completion for keys and values comes from the schema.
   - Bare `ways settings` opens the TUI on a terminal and prints `list` in a pipe.

   `ways disable`, `ways enable`, `ways agent use`, `ways agent mode` and `ways config show|path|init` become aliases over `ways settings`.

9. **One help text.** `ways --help` lists one line per command. `ways <command> --help`, `ways settings help <key>` and `ways settings help <tab>` print the schema's long text, which is the text of the TUI's detail pane and help overlay. Ways that teach agents about a setting point at `ways settings help <key>`.

10. **Settings and actions.** A setting is a value `set` writes to one file. An action is a command whose effects reach past one file: target add, enable, disable and remove (they reconcile), key add, rotate, remove and check, `reconcile` and `update`. Actions stay commands. The schema attaches them to keys so the TUI can queue them, and the TUI runs the same command line the CLI does.

11. **Secrets.** A key's value never passes through argv, the screen, the schema or an emitted object. A provider key appears as present or absent. Entry goes on stdin to `ways agent key`.

12. **attend.** `attend`'s keys move into the structure as its own schema, with their files and paths unchanged. The hand parser, the separate lint arrays and the `tune` writer are retired. `attend`'s settings are edited through `ways settings`, on the command line and on `attend`'s tabs in the settings app (ADR-504); `attend` has no settings UI of its own, since it is heading toward an MCP server mode (#538). Channels, scenes in effect, keepwarm, groups and instances under `~/.cache/attend` are runtime state and stay outside the structure. A setting that moves from `attend` to the ways MCP server (ADR-501) keeps its key.

13. **The rest of the command surface is a separate decision.** This decision adds `settings` and folds the settings verbs into it. Regrouping the remaining commands (`target`, `session`, `author`, `tune`, a hidden `internal`, with old names as hidden aliases for one release) is a follow-up record. It depends on nothing here, and it moves the 25 subcommands that hooks, scripts and skills call today, which needs its own migration plan.

Open, and left to that record or a later one: whether domain switches (user scope) and way toggles (project scope, ADR-131) become one switch per scope at every level of the tree.

## Consequences

### Positive

- One place defines a setting, and the parser, lint, help, completion and TUI cannot drift from it.
- A hand-edited file with a mistake in one section still configures everything else, and says where the mistake is.
- A hand-edited or copied file survives a CLI or TUI edit with its comments.
- `attend tune --apply` and `config init` stop destroying hand edits.
- Scripts branch on exit codes; agents set many keys in one call and get back what was accepted, in a shape they can reuse.

### Negative

- A new setting needs a schema entry before any code can read it.
- A section that falls back runs on canonical values until someone reads the diagnostic; a hook's stderr is easy to miss, so `ways status` carries it too.
- The exit codes and the object-mode shape are contracts; changing either breaks callers.
- Scripts that parse today's prose output must move to `--json` or object mode.
- A YAML writer that keeps comments and order has to be written and maintained; `serde_yaml` round-trips lose both.
- `ways` and `attend` share a crate and release its changes together.

### Neutral

- `ways-agent` reads `agent.yaml` through the schema and drops its own loader.
- ADR-131's file and schema are unchanged; its verbs become aliases.

## Alternatives Considered

- **Keep each tool's config code and add a TUI over each.** `attend`'s keys would gain a fourth definition, and the unsafe writers would stay.
- **A settings store with files generated from it.** It breaks hand editing and copying, which the operator named as the point.
- **A layering crate such as `figment` or `config`.** They merge layers for reading and do not write. The writer, provenance, fallback and help are the parts missing.
- **Reject a file that fails the schema.** One typo disables everything the file configures, as `agent.yaml` does today.
- **Repair the file on load.** It rewrites hand edits without the operator asking; `fix` does it on request instead.
- **One combined file for `ways` and `attend`.** It moves paths that installs, docs and copies depend on, for no gain the schema does not already give.
- **Folding the command regrouping into this record.** Rejected for the reason in §13.
