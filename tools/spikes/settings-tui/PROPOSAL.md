# Spike: one settings tree for `ways`, and a shared TUI for it

Status: spike on branch `spike/settings-tui`. Nothing here ships, and the crate sits outside the `tools/` workspace.

## Try it

```
cargo run --manifest-path tools/spikes/settings-tui/Cargo.toml
cargo run --manifest-path tools/spikes/settings-tui/Cargo.toml -- --project ~/some/repo
cargo run --manifest-path tools/spikes/settings-tui/Cargo.toml -- --print gate
```

It reads your real user config, the project's `.claude/ways.yaml`, `agent.yaml` and the shipped engine profiles. Edits stay in memory. On exit it prints the change set, file by file, that a real `ways settings` would write. Key files are checked for existence and never opened. `?` lists the keys.

## The problem

`ways --help` lists 37 top-level commands. Two kinds are mixed together:

- **Commands an operator runs:** status, update, uninstall, reconcile, config,
  agent, disable, enable.
- **Authoring, tuning and plumbing:** lint, reflow, corpus, match, embed,
  siblings, graph, tree, suggest, template, language, tune, tune-precision,
  permissions, manifest, scan, init, response-topics-path, sessions-root,
  events-log-path. `rethink` duplicates `introspect replay`.

Settings live in four files, each with its own writer:

| What | File | Changed by |
|---|---|---|
| Matching, disclosure, domains, targets | `config.yaml` | hand edits; `ways config target …` |
| A way on or off in one project | `.claude/ways.yaml` | `ways disable` / `ways enable` |
| Gate engine, mode, profile tuning | `agent.yaml` | `ways agent use` / `mode`; hand edits |
| Provider keys | `keys/<provider>` | `ways agent key` |

`ways config show` lists the first file and `ways agent config` the third. No command shows all of them, and most values can only be set by editing YAML.

## Proposal

### 1. `ways settings`: one tree, one schema

Every setting gets a dotted key in one tree:

```
matching.semantic_fire_probability        config.yaml
disclosure.refire_presets.normal          config.yaml
domains.itops                             config.yaml   disabled_domains
project.enabled                           .claude/ways.yaml
project.ways.softwaredev.code.quality     .claude/ways.yaml   ways:
gate.mode                                 agent.yaml
gate.profiles.anthropic.max_candidates    agent.yaml
gate.keys.anthropic                       read-only: present / absent
install.targets./home/me/.claude          read-only: an action, not a value
install.secret_path_deny                  config.yaml
```

A schema registry holds each key's type, range, default, doc line, the layers it reads and the file and key it writes. Every front end reads that registry:

```
ways settings                      TUI on a terminal, the tree as text in a pipe
ways settings get <key>            value and the layer it came from
ways settings set <key> <value>    validated, written to the owning file
ways settings unset <key>          back to the layer below
ways settings list [prefix] [--json]
```

`ways disable`, `ways enable`, `ways agent use`, `ways agent mode` and `ways config show|path|init` become thin aliases over `settings`. Actions stay commands: target add/remove (they reconcile), key add/rotate (secret input), reconcile, update.

### 2. Regroup the command surface

```
ways status | update | uninstall | reconcile | settings      operator
ways target  plan|add|enable|disable|remove                  from `config target`
ways agent   key|models|serve|status|load|unload             unchanged, minus use/mode
ways session list|context|stats|reset|introspect …           from top level
ways author  lint|reflow|template|suggest|show|match|embed|siblings|tree|graph
ways tune    alias|precision|language|permissions
ways internal scan|corpus|manifest|init|paths …              hidden from --help
```

Hooks, scripts and skills call 25 distinct `ways` subcommands today (`lint` 22 times, `tune` 20, `reconcile` 19, `corpus` 15, `match` 13). The old names stay as hidden aliases, so no hook changes in the release that regroups, and a later release retires them.

### 3. A shared TUI crate

The spike splits into a generic half and an adapter:

- `tree.rs`: typed settings (bool, bounded float and int, choice, text, read-only), a node that may be both a setting and a group (a way with child ways), filtering, and the change set.
- `ui.rs`: browse, filter, edit with validation, reset to default, revert, the change pane. It knows nothing about ways.
- `ways.rs`: the adapter that loads the four files into the tree.

As a crate (`ways-tui`, or `agent-tui` beside `agent-fmt`), the first two serve `ways settings`, `ways-agent` (a separate binary that today only prints its config) and any later picker. The tree becomes the registry's view, and the adapter becomes the registry.

## Decisions this spike leaves open

1. **The TUI library.** The repo already has two TUI stacks: `rethink` is about 2,900 lines of hand-rolled crossterm, and `attend-chat` uses iocraft. The spike uses ratatui (MIT, immediate mode, a `TestBackend` that makes the UI unit-testable; one test here drives keys and renders). The shared crate should settle on one; moving `rethink` onto it is a separate increment.
2. **Where `set` writes when a project overrides.** The spike writes the user file and shows the source layer. A project override then hides the edit. The options are `--project` on `set`, or writing to whichever layer currently wins.
3. **Domain switches by project.** `disabled_domains` is user scope and `ways:` is project scope (ADR-131), so the tree has `domains.*` and `project.ways.*` side by side. One switch per scope at every level of the tree would be simpler to read, at the cost of changing ADR-131.
4. **An ADR.** The command regrouping and the settings schema are decisions other tools and docs depend on; both go through `docs/scripts/adr` before implementation.

## What the spike does not do

It writes no files. It does not preserve YAML comments; the real writer would reuse the key-block replacement in `ways-core::config`. Validation is per field only; there are no cross-field checks such as `max_candidates` against `timeout_ms`. It reads the user ways root only through the projected corpus in `~/.claude/hooks/ways`.
