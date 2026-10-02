---
contract: adr/v1
kind: decision
verb: change
capability: [cli, install]
amends: ADR-503#Decision
basis:
  - evidence: 'ways --help at main e9a5ba8a lists 37 top-level commands; #730 added hook and removed response-topics-path, so the count is unchanged from ADR-503''s figure'
  - evidence: 'outside tests and prose, 22 top-level commands have callers at e9a5ba8a (hooks, the settings.json hook table, scripts, skills, commands, Makefile, CI and other binaries); 9 of them run on a hook path. The issue''s figure of 25 predates #730'
  - evidence: 'the updater that runs is the installed release: tools/ways-cli/src/cmd/update.rs:315,319,442,446 spawn the newly installed bin/ways with `corpus --quiet` and `reconcile`'
  - evidence: 'long-running callers outside the ways binary: tools/attend/src/sensors/context.rs:42 and disclosure.rs:86 and tools/sensor-keepwarm/src/lib.rs:240 spawn `ways context --json`; tools/attend/src/sensors/context.rs:182 and tools/sensor-processes/src/lib.rs:93-101 tell the agent to run `ways show attend <signal>`'
  - evidence: 'settings.json:154,158,209 run `ways init` and `ways corpus --if-stale --quiet` on SessionStart; a running Claude Code session keeps the hook table it loaded until restart (update.rs:197-198 tells the operator so)'
  - evidence: 'in symlink mode the projected hooks are the pulled checkout, live before the binary refresh; a failed refresh keeps the previous binary (update.rs:125-130, 200-202)'
  - precedent: ADR-503
  - precedent: ADR-506
  - precedent: ADR-504
  - evidence: ADR-505
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "1. as long as this doesnt interfere with installer projection - the term I use because I don't want to actually affect the claude code installation, and in order to support multiple claude code installations, we would project into multiple directories (for instance, if default is in ~/.claude/ and another claude code install is in ~/Projects/clientname/.claude/... 2. retire. 3. yes. 4. yes to all 5. retire. 6. keep hidden 7. yes to both"
    via: "chat, 2026-10-02, answering the seven open questions on PR #731"
observable:
  - 'see: ways --help lists 14 commands, one line each, none wider than 80 columns, and no banner'
  - 'see: ways hook, show, scan, manifest, project-slug, sessions-root and events-log-path run but are absent from ways --help and from shell completion'
  - 'run: a test parses every `ways …` call site in hooks/, settings.json, scripts/, skills/, commands/, the Makefiles, .github/ and the Rust spawns against the CLI, and fails on one the CLI rejects'
  - 'run: after the change, grep finds no caller, skill, way, doc or user-facing string in the repository naming an old command name from the mapping table'
  - 'run: hook latency for ways hook prompt, command and file stays within the budget of ADR-504 §11 against the commit before the change'
status: accepted
date: 2026-10-02
deciders:
  - aaronsb
related: [ADR-503, ADR-504, ADR-505, ADR-506, ADR-185, ADR-184, ADR-131, ADR-144, ADR-150]
---

# ADR-507: The ways commands regroup into operator commands and six groups; names another process calls stay fixed

## Summary

- **Decided:** `ways --help` shows 14 commands a person types. Settings, targets, the agent, sessions, authoring and tuning are groups (`settings`, `target`, `agent`, `session`, `author`, `tune`). Hook and script plumbing is hidden. A command keeps its current name when something that does not reload with the binary calls it: a running session's hook table, a running `attend`, the previous release's updater, or a projected script. Every other caller moves in the same change, with no aliases (option (a) below).
- **Trades away:** old names stop working on the release that ships this. Operator habits (`ways disable <id>`, `ways lint`, `ways tune`) break, and the release notes are the bridge. Four commands stay at the top level for stability rather than tidiness: `context`, `init`, `corpus` and `reconcile`.
- **One-way?** Expensive, not one-way. Names can move again, but every move churns callers, skills, ways and agent habits. The fixed names become a contract.
- **Probes:** *Confident:* the operator wants `ways --help` to show what a person types, with the hook plumbing out of sight. *Not confident:* whether retiring `ways disable <id>` and `ways enable <id>` in favour of `ways settings set ways.project.<id> false` and `unset` is acceptable for a command typed this often.
- **Inversion:** one end is today's flat list of 37, where nothing is renamed and nothing is findable. The other end puts every command under a group, including the hook plumbing, which renames calls made by processes that cannot be updated at the same moment and so needs aliases. This sits between them: groups are sorted by audience, and names that cross a reload boundary stay.

## Context

ADR-503 §14 left the rest of the command surface to its own record, with old names as "hidden aliases for one release". ADR-506 then ended the round with no legacy compatibility. Under ADR-506 §2, a transition alias may exist only while the round is open, and the PR that adds it names the issue that removes it, which is #717. An alias kept "for one release" would outlive the round, so this record replaces that part of ADR-503 §14.

### The commands at e9a5ba8a

`ways --help` lists 37 top-level commands. #730 added `hook` and removed `response-topics-path`, so ADR-503's count is unchanged. None is hidden.

| Command | Who types or calls it | Executable call sites outside tests |
|---|---|---|
| `hook <event>` | Hook adapters only | `hooks/ways/require-ways.sh:27`, sourced by 11 adapters |
| `init` | Operator; SessionStart hook | `settings.json:154,209` |
| `corpus` | Operator, author; SessionStart hook; updater; build | `settings.json:158`; `update.rs:315,442`; `Makefile:125`; `tools/way-embed/Makefile:91`; `scripts/fix-way-embed-signature.sh:71`; skills ways-update, ways-tests |
| `reconcile` | Operator (repair); updater; installer | `update.rs:319,446`; `scripts/install.sh:313,375`; repair text in `check-setup.sh:29,83`; skill ways-update |
| `context` | Agent through skills; attend; keepwarm | `meta/start/macro.sh:8`; `attend/src/sensors/context.rs:42`, `disclosure.rs:86`; `sensor-keepwarm/src/lib.rs:240`; skills context-status, start, wrap |
| `show way\|check\|core\|attend` | Agent, told by attend; author | Text in `attend/src/sensors/context.rs:182`, `sensor-processes/src/lib.rs:93-101`; skill ways-tests (`show check`) |
| `match` | Author; SessionStart probe; build check | `check-setup.sh:70,72`; `Makefile:264`; skills ways-tests; commands/ways.md |
| `suggest` | Author; a macro | `meta/knowledge/optimization/macro.sh:34`; skill ways-tests |
| `reflow` | Author; a postcheck | `documentation/markdown/reflow/postcheck.sh:62` |
| `project-slug` | A macro | `meta/memory/macro.sh:15` |
| `sessions-root` | A script | `softwaredev/delivery/issues/gh-tasks:163` |
| `events-log-path` | Scripts outside the binary (none at present) | none |
| `scan` | Tests only since #730 | `tools/ways-cli/tests/session_sim.rs`, `project_ways.rs` |
| `manifest` | Debugging `reconcile` | tests only |
| `status` | Operator; installer | `scripts/install.sh:242`; skills ways-tests, ways-update |
| `update`, `uninstall` | Operator | `Makefile:169` (`update`) |
| `settings`, `projects`, `agent` | Operator; landed this round | none outside their own crates; `ways-agent-core` names `agent use` and `agent config` in the header it writes to `agent.yaml` (`profile.rs:216-217`) and in a setting's help (`settings.rs:52`) |
| `config show\|path\|init\|targets\|target` | Operator | skill ways-localize (`config path`); the settings-tui spike, which #697 deletes |
| `disable`, `enable` | Operator | none |
| `lint` | Author; CI | `.github/workflows/build-ways.yml:53`; `Makefile:263`; skills ways-tests; commands/ways.md, project-audit.md |
| `graph`, `language` | Author; build check | `Makefile:265`, `Makefile:312`; skill ways-localize (`language`) |
| `template`, `tree`, `siblings`, `permissions` | Author | skills ways-tests (`tree`, `siblings`) |
| `tune`, `tune-precision`, `stats` | Maintainer | skills ways-localize, ways-tests (`tune`) |
| `list`, `introspect`, `reset` | Operator, agent | skill ways-tests (`list`, `introspect dump`) |
| `rethink` | Operator; #699 retires it | none |

Outside tests and the ways corpus, 22 top-level commands have callers: hooks, the `settings.json` hook table, scripts, skills, commands, Makefiles, CI and other binaries. Nine run on a hook path: `hook`, `init`, `corpus`, `context`, `match`, `suggest`, `reflow`, `project-slug` and `sessions-root`. The ways corpus also names commands in prose that agents act on: the authoring ways, `meta/deployment/deployment.md`, the optimization and tuning ways, `meta/start` and `meta/wrap`, and the markdown and reflow ways.

### When callers and the binary can differ

Installed hooks, skills and ways are projected from the same checkout as the binary, so after a successful `ways update` they agree. They differ in five cases:

1. **A running Claude Code session** keeps the hook table it loaded. On `/clear` it runs `ways init` against the new binary until the session restarts. `ways corpus --if-stale --quiet` runs only at startup, when the session loads the new hook table, and compaction runs no `ways` subcommand.
2. **The previous release's updater** runs the update. After installing the new binary, it spawns `corpus --quiet` and `reconcile` on it. A failed `reconcile` stops the update.
3. **A running `attend` process** and the keepwarm sensor spawn `ways context --json` from code built with the previous release. Attend sensors also tell the agent to run `ways show attend <signal>`. A failed attend refresh keeps the old attend.
4. **In symlink mode**, the projected hooks are the pulled checkout. They are live from the pull until the binary refresh finishes, which can take minutes on a source build.
5. **A failed binary refresh** keeps the previous binary under the pulled hooks until the next successful update. The updater says so.

In cases 1 to 3 an old caller reaches a new binary. An alias in the new binary would cover them. In cases 4 and 5 a new caller reaches an old binary, and no alias covers that, because the alias would have to be in the binary that is not yet installed. Only an unchanged name covers every case.

A second target adds no case. Every target ADR-184 records links to the same checkout and calls the same binary, and `reconcile` converges every enabled target in one run, so the five cases hold for each target at the same moment. A target's hook table names only `init` and `corpus`; everything else it runs goes through the projected hook scripts.

## Decision

1. **Top level.** `ways --help` lists these commands, in this order:

   | Command | What it is |
   |---|---|
   | `status` | Engine health: binary, model, corpus, project |
   | `settings` | Read and change settings; the TUI on a terminal (ADR-503) |
   | `target` | The Claude Code config directories agent-ways is active in (ADR-184) |
   | `agent` | The ways agent: keys, models, the daemon (ADR-502) |
   | `projects` | Claude Code's projects and their session history |
   | `session` | This session and past ones: fired ways, replay, reset |
   | `context` | Context-window usage for a session |
   | `author` | Write and check ways |
   | `tune` | Measure matching against telemetry and locales |
   | `init` | Set up `.claude/ways/` in a project |
   | `corpus` | Rebuild the matching corpus |
   | `update` | Update agent-ways |
   | `reconcile` | Repair the projection into `~/.claude` |
   | `uninstall` | Remove agent-ways |

2. **Fixed names.** A command keeps its name and place when something that does not reload with the binary calls it. These are the cases in Context: a running session's hook table, the previous release's updater, a running `attend`, and a projected script that fails hard on an unknown command. That fixes `hook`, `init`, `corpus`, `reconcile`, `context`, `show`, `project-slug` and `sessions-root`. `events-log-path` stays with them, since it exists for scripts. A projected script that degrades quietly on an unknown command does not fix the name it calls: `match`, `suggest` and `reflow` move, and item 6 says how each of their callers fails. A fixed name is a contract. Changing one needs its own decision and a release in which nothing calls it.

3. **Hidden commands.** `hook`, `show`, `scan`, `manifest`, `project-slug`, `sessions-root` and `events-log-path` are hidden. They are absent from `ways --help` and from shell completion, and they still run and still answer `--help`. They keep their top-level names, and there is no `internal` group: moving a hook-facing command under a group renames it, and case 4 then stops every way from firing between the pull and the binary refresh. `ways hook <event>` stays the one interface the hook adapters call (ADR-504 §11 and its note of 2026-10-02). Its event names are part of the contract.

4. **The mapping.** Every other caller moves in the same change: hooks, scripts, skills, commands, ways, docs, the Makefiles, CI, the Rust spawns and the tests.

   | Old | New |
   |---|---|
   | `status` | `status` |
   | `settings …` | `settings …` |
   | `config show [--json] [--effective]` | `settings list [--json] [--effective]` |
   | `config path` | `settings list --json`, which names each key's file |
   | `config init` | Removed. `settings set` creates the file on first write; `settings emit` prints the canonical file |
   | `config targets [--json]` | `target list [--json]` |
   | `config target plan\|add\|enable\|disable\|remove <dir>` | `target plan\|add\|enable\|disable\|remove <dir>` |
   | `disable <id>` | `settings set ways.project.<id> false` |
   | `disable --list [--names-only]` | `settings list ways.project` |
   | `enable <id>` | `settings unset ways.project.<id>` |
   | `agent key\|models\|status\|load\|unload` | `agent key\|models\|status\|load\|unload` |
   | `agent use <profile> [--model <id>]` | `settings set gate.engine <profile>`, and `settings set gate.profiles.<profile>.model <id>` |
   | `agent mode <mode>` | `settings set gate.mode <mode>` |
   | `agent config` | `settings list gate --effective` |
   | `agent serve` | `agent serve`, hidden: the client spawns `ways-agent serve` directly |
   | `projects …` | `projects …` |
   | `list` | `session ways` |
   | `introspect list` | `session list` |
   | `introspect replay` | `session replay` |
   | `introspect live` | `session live` |
   | `introspect dump` | `session dump` |
   | `introspect fires` | `session fires` |
   | `reset` | `session reset` |
   | `rethink` | Removed by #699; use `session replay`, `session list` and `session dump` |
   | `context` | `context` (fixed) |
   | `lint` | `author lint` |
   | `template` | `author template` |
   | `match` | `author match` |
   | `tree` | `author tree` |
   | `siblings` | `author siblings` |
   | `suggest` | `author suggest` |
   | `graph` | `author graph` |
   | `reflow` | `author reflow` |
   | `permissions audit` | `author permissions` |
   | `tune` | `tune locale` |
   | `tune-precision` | `tune precision` |
   | `stats` | `tune stats` |
   | `language` | `tune language` |
   | `init` | `init` (fixed) |
   | `corpus` | `corpus` (fixed) |
   | `update` | `update` |
   | `reconcile` | `reconcile` (fixed) |
   | `uninstall` | `uninstall` |
   | `hook <event>` | `hook <event>` (fixed, hidden) |
   | `show way\|check\|core\|attend` | `show …` (fixed, hidden) |
   | `scan …` | `scan …` (hidden) |
   | `manifest` | `manifest` (hidden) |
   | `project-slug`, `sessions-root`, `events-log-path` | unchanged (fixed, hidden) |

   `ways-agent`'s `use`, `mode` and `config` are removed with the old output ADR-506 §1 already retires, so `ways agent` keeps only the actions of ADR-503 §11.

   The same change rewrites the header `ways-agent-core` writes to a new `agent.yaml` and the setting help that names `agent use`. An `agent.yaml` already written keeps its old header. The header is a comment that nothing reads, so it is left in place rather than rewritten on the user's machine.

5. **No aliases (option (a)).** No old name is kept, hidden or otherwise. The change migrates every caller in the repository, and the release notes list each old name beside its new one. Option (b), aliases removed by #717, would cover cases 1 to 3 for the part of the round between this change and #717. Item 2 already covers those cases, because every command they call keeps its name. Aliases would then serve only operators typing old names, which is the habit the release notes address. They would also add one more compatibility path for #717 to find and remove.

6. **What still breaks, and how it fails.** Cases 4 and 5 still reach the moved commands that projected scripts call: `author match` in `check-setup.sh`, `author suggest` in the optimization macro, and `author reflow` in the reflow postcheck. The reflow postcheck already reads exit 2 as no finding. The optimization macro shows zero counts in its table. `check-setup.sh` treats an unknown-command exit (2) from its probe as no answer, not as a broken engine, because the updater has already reported a stale binary. A test parses every call site against the CLI, so a missed caller fails `make test`.

7. **Help output.** ADR-503 §10 holds: `ways --help` prints one line per command. Each line fits 80 columns. ADR numbers and detail go in `long_about` and appear in `ways <command> --help`. The banner prints only for a bare `ways` on a terminal. `ways --help`, `ways help` and a bare `ways` in a pipe print help without it. A group run with no verb prints the group's help and exits 2, the usage code of ADR-503 §9. `projects` keeps its default, `list`, and `settings` keeps ADR-503's TUI-or-list default.

8. **Order with the round.** This lands after #699, which removes `rethink`, and after #697, which deletes the settings-tui spike that calls `ways config target plan`. #717's check then searches for the old names in this table as well as the ones ADR-506 §1 lists.

This amends ADR-503's Decision at §14: the groups are the ones above, and old names are not kept as aliases.

## Consequences

### Positive

- `ways --help` drops from 37 commands to 14, and a person reading it sees only what they would type.
- Each group's help shows its verbs together, so `ways author --help` is the authoring reference and `ways session --help` is the introspection one.
- The names that hooks, the updater and attend depend on are written down as a contract, not left implicit.
- No alias code exists for #717 to remove.

### Negative

- Operators and agents lose names they know. `ways lint`, `ways tune`, `ways disable <id>` and `ways list` fail with a usage error until they learn the new ones.
- `ways tune` changes meaning: it was the locale audit and becomes a group.
- `ways disable <id>` becomes a longer command, and a way id inside a dotted key reads less plainly.
- Four commands stay at the top level for stability, so the top level is not purely what an operator types: `corpus` is mostly the hook and the updater's.
- Between a pull and the binary refresh, or after a failed refresh, the moved commands that projected scripts call fail quietly: the reflow postcheck reports nothing, and the optimization table shows zero counts that are wrong.
- An `agent.yaml` written before the change keeps a comment naming `ways agent config` and `ways agent use`, which no longer exist.
- One PR migrates the skills, commands, Makefiles, CI and scripts that name a moved command, the ways corpus prose and the ways-cli tests, which makes it a large review.

### Neutral

- `ways-agent`'s own command line keeps `key`, `models`, `status`, `load`, `unload` and a hidden `serve`.
- The `Bash(ways:*)` permission in `settings.json` covers every new name.
- The hidden commands are listed in the CLI reference doc, since `--help` no longer shows them.
- A later decision can remove `scan` and `manifest`, which only tests and debugging use.

## Alternatives Considered

- **(b) Hidden aliases for the old names, removed by #717.** Not chosen. The callers an alias would protect already reach fixed names under item 2. An alias does not help when the binary is older than the scripts. It would add a compatibility path that #717 has to find and remove.
- **Aliases for one release, as ADR-503 §14 first said.** Rejected by ADR-506 §2: an alias that outlives the round is the compatibility the round removes.
- **A hidden `internal` group for the plumbing.** Rejected. It renames `hook`, `project-slug` and `sessions-root`, whose projected callers fail hard on an unknown command, and in symlink mode every way stops firing from the pull until the binary refresh finishes.
- **Every operator command in a group, `context` under `session` and `init`, `corpus` and `reconcile` under an `install` group.** Rejected. Each is called by a running session's hook table, the previous updater or a running `attend`, so moving it needs an alias or breaks those callers.
- **Keep `disable` and `enable` as permanent top-level verbs over the settings writer.** This was a real option: they would be designed verbs, not compatibility. Not chosen, because ADR-503 §11 makes a one-file change a setting and gives settings one front end. It is the second probe.
- **Leave the surface flat and only shorten the help lines.** Rejected. It fixes the width and leaves 37 entries mixing hook plumbing with operator commands, which is the problem ADR-503's basis names.

## Note (2026-10-02): the agent.yaml write path is gone

Decision item 4 said the change rewrites the header `ways-agent-core` writes to a new `agent.yaml`. The implementation removed that header and the `UserLayer` write path instead, since `ways agent use` and `mode` were its only callers. `ways settings set gate.…` now writes `agent.yaml` through the settings writer, and a new file has no header. An existing file keeps its old comment, as the Negative consequences say.

## Note (2026-10-02): the top-level help may end with a judge footer

Issue #751 adds a footer to the help that item 7 governs. The top-level help (a bare `ways`, `ways --help`, `ways help`) may end with a footer of at most two lines, each within 80 columns, saying the relevance judge cannot gate and how to fix it. It prints only when the judge cannot gate, read from stored state with no network call. Per-command help and hooks never print it.

## Note (2026-10-02): every screen has a command an agent can run

A screen is a view, for a person. An agent authors, refactors and tunes ways through commands, and reads their `--json` form to decide its next edit. So everything a screen shows comes from a command that runs without a terminal and has a `--json` form, scoped as the screen is: the session screen's tabs read what `ways session replay --json`, `ways session fires --json`, `ways agent cost --json`, `ways tune stats --json` and `ways tune precision --json` print, scoped as the tab is (#738). A screen may add navigation and nothing else. Its data stays with the command. A group that opens a screen when run bare on a terminal (#748) keeps every verb, and keeps item 7's help and exit 2 in a pipe.
