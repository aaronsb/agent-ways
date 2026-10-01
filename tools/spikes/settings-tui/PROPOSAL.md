# Spike: one settings tree for `ways`, and a shared TUI for it

Status: spike on branch `spike/settings-tui`. Nothing here ships, and the crate sits outside the `tools/` workspace.

## Try it

```
cargo run --manifest-path tools/spikes/settings-tui/Cargo.toml
cargo run --manifest-path tools/spikes/settings-tui/Cargo.toml -- --project ~/some/repo
cargo run --manifest-path tools/spikes/settings-tui/Cargo.toml -- --print gate
```

It reads your real user config, the project's `.claude/ways.yaml`, `agent.yaml` and the shipped engine profiles. Edits stay in memory. Actions are queued, not run. On exit it prints the change set, file by file, that a real `ways settings` would write, then the queued commands in order, with secrets shown as `<stdin>`. Key files are checked for existence and never opened. `?` lists the keys.

Try it on a provider key: open `gate.keys` and press Enter on a provider to type a key (it shows only as dots), or `a` for set/rotate, remove and check. Press `a` on `install.targets` to add or plan a target, on a target to enable, disable or remove it, and on `install` to reconcile. `c` shows the pending changes and queued actions together; `x` drops the last queued action.

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
gate.keys.anthropic                       present / absent; actions set, rotate, remove, check
install.targets./home/me/.claude          read-only value; actions enable, disable, remove
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

- `tree.rs`: typed settings (bool, bounded float and int, choice, text, read-only, secret), a node that may be both a setting and a group (a way with child ways), actions and their queue, filtering, and the change set.
- `ui.rs` and `ui/render.rs`: browse, filter, edit with validation, reset to default, revert, the action menu, confirm and masked entry, the pending pane. It knows nothing about ways.
- `ways.rs`: the adapter that loads the four files into the tree.

As a crate (`ways-tui`, or `agent-tui` beside `agent-fmt`), the first two serve `ways settings`, `ways-agent` (a separate binary that today only prints its config) and any later picker. The tree becomes the registry's view, and the adapter becomes the registry.

## Actions and keys

A node holds a value, actions, or both. A value is something `ways settings set` writes to a file. An action is a named `ways` command line that the tree queues and does not run: a target add, a key rotation, a reconcile. The generic half (`tree.rs`, `ui.rs`) knows an action as a label, a command, an argument kind and a confirm flag; `ways.rs` supplies the real commands.

Actions are queued, in order, beside the value changes. The pending pane (`c`) lists both, `x` unqueues the last action, and the exit summary prints the value changes by file and then the commands. This is the order a real `ways settings` would apply them.

An API key is a `Kind::Secret` node. It shows present or absent, taken from whether the key file exists. Enter opens a masked entry; committing queues `ways agent key add|rotate --provider <p> < <stdin>` and does not change the value. The real command would pipe the typed key to the existing stdin path of `ways agent key`, so the key never appears in argv, in the process list or on the screen. The typed text lives in a buffer whose `Debug` is redacted, which is overwritten when it drops, and which the queue never receives; the queued line carries only `<stdin>`.

An action is confirmed (y/n) when it is destructive or reconciles: key remove, target add, disable and remove, and `ways reconcile`. Key set, rotate and check, target plan and enable queue at once. A declined confirm queues nothing.

Per-target state stays a read-only value because enabling or disabling reconciles. Way toggles under `project.ways` stay values; the real command for them is `ways disable <id>` and `ways enable <id>`.

## Decided

- **ratatui for every TUI.** The repo has a hand-rolled crossterm TUI (`rethink`, about 2,900 lines) and an iocraft one (`attend-chat`). The shared crate is built on ratatui, the maintained Rust TUI library, whose `TestBackend` renders frames in tests. Moving `rethink` and `attend-chat` onto it are separate increments.

## Testing TUIs: a way, two skills and the crate's test half

roguemap develops its TUI through a loop written in its CLAUDE.md, Makefile and `docs/testing.md`, with no Claude skills of its own: render a frame headless, look at it as a PNG, iterate the look without tests, then pin what settled as golden frames in `cargo test`. In its two recorded sessions the agent read rendered PNGs 94 times. agent-ways adopts the loop in three parts.

- **The crate's test half.** A headless `render(state, w, h) -> Buffer` entry point and a `--snap W H OUT key=value…` flag; one shot list (name and state) that drives both the snapshot command and the golden test; a frame format of glyph, foreground, background and modifiers; a comparator that reports identical cells, colour distance and the first differing cells, exact by default for text UIs, with record, strict and dump switches; a PNG renderer for review. The comparator is tested itself: identical frames, one changed cell, a shift.
- **A way, `softwaredev/code/testing/tui`.** Guidance only: render headless from the start; judge the look from an image; iterate the look before pinning it; golden frames are a reviewed baseline, and a re-record names its frames and the reason in the commit; a failure shows what drifted; cover the minimum size (80x25) and a reference size; drive interaction through the real key handler; add a seeded random session for state-dependent faults; name the property under test; builders gate on check and goldens in a worktree, reviewers stay read-only.
- **Skills, shipped to every project.** `tui-snap` renders one or more shots to PNGs in the scratchpad, reads each, and reports what changed, with variants side by side. `tui-golden` runs the golden check; on a diff it renders expected and actual, reviews each frame, and on an intended change re-records and drafts the commit text naming the frames and why. Both find the project's commands through `make help` or the crate's flags. A `tui-scaffold` skill that wires the crate into a new ratatui project is optional.

The settings spike is the first user: its one render test today only checks that a key name appears in the frame.

## Decisions this spike leaves open

1. **Where `set` writes when a project overrides.** The spike writes the user file and shows the source layer. A project override then hides the edit. The options are `--project` on `set`, or writing to whichever layer currently wins.
2. **Domain switches by project.** `disabled_domains` is user scope and `ways:` is project scope (ADR-131), so the tree has `domains.*` and `project.ways.*` side by side. One switch per scope at every level of the tree would be simpler to read, at the cost of changing ADR-131.
3. **An ADR.** The command regrouping and the settings schema are decisions other tools and docs depend on; both go through `docs/scripts/adr` before implementation.

## What the spike does not do

It writes no files and runs no commands. It does not preserve YAML comments; the real writer would reuse the key-block replacement in `ways-core::config`. Validation is per field only; there are no cross-field checks such as `max_candidates` against `timeout_ms`. It reads the user ways root only through the projected corpus in `~/.claude/hooks/ways`.
