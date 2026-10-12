---
contract: adr/v1
kind: decision
verb: change
capability: [attend, cli]
amends: ADR-173#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "ok so we'll fix the multipart message, (remove it basically) as well as make sure that the attend commands get the same simplifcation treatment that ways did. same capabilities but from the cli they're not presented flatly.. from now on the mcp server is the principle way to return path messages"
    via: an attend message to the head session, 2026-10-12 11:01, #open
  - evidence: 'attend --help at main 0f7cace1 lists 22 top-level commands, all flat (tools/attend/src/cli.rs Commands)'
  - evidence: 'the attend mod runs every tool and poll as attend CLI argv: send, reply --re, join, listen on --host mod, listen off, inbox --drain --json, unread --json, peers and keepwarm check (plugins/attend/hooks/lib.ts:15-47)'
  - evidence: 'the Stop hook script execs `attend inbox --drain --format hook` and passes its exit code through (hooks/ways/attend-drain-stop.sh:11); clap exits 2 on a usage error, and Claude Code reads a Stop hook''s exit 2 as a block'
  - evidence: 'attend-chat writes CLI commands into messages that agents receive: `join with: attend join <g>` and `rejoin with: attend join <g>` (tools/attend-chat/src/app/keys.rs:531, 567)'
  - evidence: 'the attend way''s macro calls `attend channels --joined`, `attend peers` and `attend status`, and finds a running loop by matching `run` as the argument after `attend` in ps output (hooks/ways/softwaredev/environment/attend/macro.sh:27, 48-61)'
  - evidence: 'statusline.sh:41-45 calls `attend whoami --display` and falls back to `attend whoami` for builds without the flag'
  - evidence: 'a session runs the mod from the app''s plugins/attend/ folder and keeps it loaded until /reload-plugins or a restart, while an update replaces the attend binary on PATH underneath it (docs/attend-and-monitor/mod.md:76, 96-110)'
  - evidence: 'the removed `attend focus` alias and `send --focus` flag are refused, with a test that keeps them refused (tools/attend/tests/legacy_removed.rs)'
  - precedent: ADR-509
  - precedent: ADR-507
  - precedent: ADR-506
  - precedent: ADR-406
  - precedent: ADR-173
  - precedent: ADR-501
agent:
  name: Claude
  model: claude-opus-5-5
observable:
  - '`attend --help` lists nine commands: run, listen, message, channel, peer, keepwarm, chat, sensor, config'
  - 'a bare `attend` prints the banner, those nine, and one line naming the attend_* tools as the principal path in a mod session'
  - 'in the alias release, `attend send hi` prints ``note: `attend send` is now `attend message send` `` on stderr, sends, and exits as `attend message send hi` does; `attend send --json hi` prints the same JSON on stdout as the new form'
  - '`grep -rnE ''\battend (send|reply|inbox|unread|join|leave|channels|dissolve|scenes?|peers|whoami|status|sensors|permissions|tune|cleanup)\b''` over plugins/, hooks/, skills/, statusline.sh, tools/attend-chat/src and docs/ (accepted records and changelogs excepted) finds nothing'
  - 'the three messaging guidance sources open their messaging section with attend_send and attend_reply, and show the CLI under a heading for hosts without the mod'
  - 'in the release after the alias release, `attend send hi` is a usage error, and the Stop hook script still exits 0 and ends the turn normally'
status: proposed
date: 2026-10-12
deciders:
  - aaronsb
related: [ADR-509, ADR-507, ADR-506, ADR-406, ADR-411, ADR-173, ADR-172, ADR-182, ADR-501, ADR-136]
---

# ADR-413: Attend's CLI groups its verbs by noun; the mod's tools are the principal reply path

## Summary

- **Decided:** attend's 22 flat commands regroup under nine: `run`, `listen`, `message`, `channel`, `peer`, `keepwarm`, `chat`, `sensor` and `config`. Every capability stays, and each verb keeps its flags and output. The old flat verbs remain for one release as hidden aliases that print the new form, and are then removed. In a session with the attend mod, the `attend_*` tools become the principal way an agent sends, replies, joins and lists peers; the three messaging guidance sources lead with them and present the CLI as the path for hosts without the mod.
- **Trades away:** `attend send "..."` and `attend join deploy`, the two-word chat verbs ADR-173 made primary, become three words. For one release attend carries alias code, the bounded exception ADR-509 already made to ADR-506. A session that keeps an old mod loaded across two updates gets usage errors from its tools until it reloads.
- **One-way?** No. The groups can be renamed or flattened in one session. After the aliases are removed, reversing is another rename with its own alias window.
- **Probes:** *Not confident (scope):* by "not presented flatly" you meant every verb moves under a group, including `send`, `reply` and `join`, which agents have been taught as two-word commands, rather than keeping those few chat verbs flat and grouping the rest. *Confident (fallback):* with the mod's tools as the principal return path, an agent in a mod session never needs the attend CLI to talk; the CLI stays for hosts without the mod, for you in a shell, and for hooks and scripts.
- **Inversion:** one end keeps 22 flat verbs and teaches agents the CLI first, so nothing breaks and the help stays long. The other end moves every name at once with no aliases, so a running session's loaded mod and every agent habit fail on the next update. This decision groups everything a person or agent reads, moves every caller in the repository in the same release, and keeps the old names for one release.

## Context

ADR-509 gave the ways CLI's lifecycle commands one group, `ways manage`, with the old names kept as hidden aliases for one release. The operator asked for the same treatment of attend: the same capabilities, not presented flatly. The same message made the attend mod (ADR-406) the principal way messages come back, and removed multipart messages. The one-message-one-signal change (branch `feat/attend-one-message-one-signal`) does the multipart removal, and this record leaves it there.

`attend --help` lists 22 commands in one column. Messaging, channels, presence, sensors and maintenance sit side by side, so an agent reading the help to send one message reads all 22.

### Callers

Each verb, what it does, and who runs it at 0f7cace1. A caller that runs argv is a compatibility contract. Text that tells an agent or a person what to type is a habit.

| Verb | What it does | Runs it as argv | Teaches it |
|---|---|---|---|
| `run` | The sensor loop under Monitor | Monitor in `/attend` sessions; its own re-exec (`cmd/run/tick.rs:174`, which keeps argv); the macro's ps match on `run` | Skill, way, disclosure, docs |
| `send` | A new message | The mod (`lib.ts:15`) | Skill, way, disclosure, drain output, attend-chat help text, docs |
| `reply` | A reply, auto-threaded | The mod (`lib.ts:26`) | As `send`; the plain drain names it |
| `inbox` | List or read messages; `--drain` delivers and marks read (ADR-172) | The Stop hook script (`--drain --format hook`); the mod (`--drain --json`) | Skill, way, disclosure, attend-chat help text |
| `unread` | Count unread messages by room | The mod (`--json`) | Skill, way, disclosure, `listen` output |
| `listen` | Listening on, off, or its state | The mod (`on --host mod`, `on`, `off`) | Skill, way, disclosure |
| `join`, `leave` | Enter or leave a channel | The mod (`join`) | Skill, way, disclosure; attend-chat invite and removal messages sent to agents |
| `channels` | List channels; `create`, `describe`, `pin`, `unpin` | The macro (`--joined`) | Skill, way, disclosure, docs |
| `dissolve` | Remove a channel for every member | None | Skill, docs |
| `scene`, `scenes` | Apply or list a scene | None | Skill, way, disclosure |
| `peers` | List sessions and their channels | The mod; the macro | Skill, way, disclosure, attend-chat comments |
| `whoami` | This session's identity | `statusline.sh` (`--display`, falling back to plain) | Way, statusline docs |
| `status` | Every running loop, signals, channel state | The macro; the live fixture (`tests/fixtures/docker/run-tier1.sh`) | Skill, way |
| `keepwarm` | The cache keepwarm window; `check` for hosts without `run` (ADR-182) | The mod (`check`) | Skill, way, docs |
| `chat` | Exec `attend-chat` | None | Docs |
| `sensors` | List sensors | None | Docs |
| `permissions` | Audit sensor `requires:` against settings.json | None | Docs |
| `tune` | Derive engagement settings from session history | None | Docs |
| `cleanup` | Reap signal files whose project is gone | None | Docs |
| `config` | Show, init, path, lint the settings | None | Docs |

The riskiest callers are the ones that run on every turn or that another release supplies:

1. **The mod in a running session.** Every tool call and every poll is attend argv. The session keeps the mod it loaded while an update replaces the binary on PATH. This is the one caller from another release, as the previous updater was for ADR-509.
2. **The Stop hook.** It runs at the end of every turn of every session and passes attend's exit code through. A usage error from clap is exit 2, which Claude Code reads as a block on stopping, with stderr fed back to the model.
3. **attend-chat's invite and removal messages.** They put `attend join <g>` in a message to an agent, which the agent may run as written.
4. **The statusline and the macro.** They run often and print what they find, so a failure shows as a missing name or a wrong status, not an error.
5. **Operator permission rules.** The shipped baseline allows `Bash(attend:*)`, which every grouped path matches. A rule an operator wrote against a flat prefix, such as `Bash(attend send:*)`, stops matching.

## Decision

1. **Nine top-level commands.** `attend --help` lists these, and the old verbs map to them:

   | Was | Now |
   |---|---|
   | `attend run` | `attend run`, unchanged and a fixed name |
   | `attend listen [on\|off\|status]` | unchanged |
   | `attend keepwarm [on\|off\|status\|check]` | unchanged |
   | `attend chat` | unchanged |
   | `attend config [init\|show\|path\|lint]` | unchanged |
   | `attend tune` | `attend config tune` |
   | `attend send` | `attend message send` |
   | `attend reply` | `attend message reply` |
   | `attend inbox [ID] [--drain]` | `attend message inbox [ID] [--drain]` |
   | `attend unread` | `attend message unread` |
   | `attend cleanup` | `attend message cleanup` |
   | `attend join <g>` | `attend channel join <g>` |
   | `attend leave <g>` | `attend channel leave <g>` |
   | `attend channels [list] [--joined]` | `attend channel [list] [--joined]` |
   | `attend channels create\|describe\|pin\|unpin` | `attend channel create\|describe\|pin\|unpin` |
   | `attend dissolve <g>` | `attend channel dissolve <g>` |
   | `attend scene <name>` | `attend channel scene <name>` |
   | `attend scenes` | `attend channel scenes` |
   | `attend peers` | `attend peer [list]` |
   | `attend whoami` | `attend peer whoami` |
   | `attend status` | `attend peer status` |
   | `attend sensors` | `attend sensor [list]` |
   | `attend permissions [audit]` | `attend sensor permissions` |

   Group names are singular nouns followed by a verb, the shape of `gh pr list` and `git remote add`. Each verb keeps its flags, arguments, output and exit codes. `--json` stays on every verb that has it, and `describe.rs` keeps one description per message verb, keyed by its new path (ADR-501 item 4).

2. **`run` stays a fixed top-level name.** Monitor commands in running sessions, the loop's own re-exec and the macro's process match all use `attend run`, and it is the one command a host without the mod is told to start.

3. **A bare group runs its read-only default.** This is attend's existing convention (`listen`, `keepwarm`, `channels`, `config` and `permissions` default to a view today), kept over ADR-507 §7's help-and-exit-2. The defaults are `message` → `inbox`, `channel` → `list`, `peer` → `list`, `sensor` → `list`, `config` → `show`, `listen` → `status`, `keepwarm` → `status`. None of them marks a message read or changes state. The default verb's flags work on the bare group, as `attend channels --json` does today.

4. **Bare `attend`.** It prints the banner, the nine commands with one line each, and one closing line: in a session with the attend mod, the `attend_*` tools send, reply, join and list peers, and this CLI is for hosts without the mod and for scripts.

5. **One-release aliases.** Every old flat verb in the table stays as a hidden subcommand for the attend release that ships the groups. Each one parses the arguments it parses today, prints one line to stderr naming the new form (``note: `attend send` is now `attend message send` ``), then runs the new verb with the same stdout and exit code. Aliases are absent from `attend --help`, from `docs/cli/attend.md` and from shell completion. The next attend release removes them, and each removed name joins the refusal test in `tools/attend/tests/legacy_removed.rs`. As ADR-506 §2 and ADR-509 item 5 require, the PR that adds the aliases names the issue that removes them, in its body and in a code comment.

6. **Order.** The binary change, the caller moves and the guidance land in the same attend release, in this order:
   1. The grouped tree and the aliases in the binary, with a test that each alias forwards its arguments, stdout and exit code and prints its notice.
   2. Every caller that runs argv moves to the new paths: the mod's argv (`plugins/attend/hooks/lib.ts`) and its tests, the Stop hook script, the macro, `statusline.sh` and its test, the live fixture, attend's own integration tests, and `cli.rs`'s describe test. The Stop hook script also stops passing a usage error through: any exit other than 0 from attend ends the turn normally, so a binary that lacks the name it calls cannot block a stop.
   3. Text that tells someone what to type: attend's own hints and drain output, attend-chat's invite and removal messages and help text, the guidance sources (item 7), and the docs. attend-chat's invite names `attend_join` and the CLI path.
   4. A call-site check, run in CI, that fails on a flat verb in the paths of the fourth observable line, apart from the alias tests and `legacy_removed.rs`.

   The plugin and the binary ship from one release tag (`docs/attend-and-monitor/mod.md:80`), so a fresh install or an update gets a matching pair. A session that loaded the mod before an update reaches the aliases until it reloads. A dev checkout that runs a branch's mod against an older prebuilt attend rebuilds attend from source (`make attend-rebuild`), as testing a branch's mod already requires.

7. **Guidance leads with the tools.** The three synced sources (`hooks/ways/softwaredev/environment/attend/attend.md`, `skills/attend/SKILL.md`, `tools/attend/src/sensors/disclosures/messaging.md`) change together:
   - The messaging section opens with the tools: `attend_reply` to answer, `attend_send` for a new topic, `attend_join` and `attend_peers`, called after `attend_listen`.
   - The CLI follows under a heading for hosts without the mod, with the grouped paths: `attend message send`, `attend message reply`, `attend channel join`, `attend peer list`.
   - Starting stays as it is: `attend_listen` in a mod host, and `attend run` under Monitor where the tool is absent.
   - The autonomy, silence, keepwarm, name and "starting active" paragraphs keep their meaning and use the tool names first. The name paragraph points to `attend peer whoami --display`.
   - The "CLI is the contract" note becomes "attend's surfaces are the contract": the tools, then the CLI. It still forbids reading attend-owned state.
   - The length paragraph follows the one-message-one-signal change.

   This amends ADR-173's Decision: item 2's agent verbs keep their chat names (`join`, `leave`, `channels`, `scene private`, `dissolve`, `send --channel`) under the `channel` and `message` groups, and the agent's primary surface in a mod session is the tools, whose names carry the same verbs.

## Consequences

### Positive

- `attend --help` drops from 22 commands to nine, and each group's help lists only its own verbs.
- An agent in a mod session is taught one path, typed tools with no quoting, and the CLI it falls back to is grouped the same way.
- The Stop hook can no longer block a turn because of a usage error.
- Every caller in the repository moves in one release, and a CI check keeps the flat names out.

### Negative

- The common CLI verbs become three words: `attend message send`, `attend channel join`.
- For one release attend carries alias code for 17 old names.
- A session that keeps a mod loaded across two attend updates without reloading gets usage errors from its tools; `/reload-plugins` or a restart fixes it.
- An operator permission rule written against a flat prefix stops matching the grouped path. The shipped `Bash(attend:*)` is unaffected.
- External text, release notes and agents' habits name the flat verbs. After the alias release they get a usage error.

### Neutral

- The tools keep their names (`attend_send`, `attend_reply`, `attend_join`, `attend_peers`, `attend_listen`); only the argv they run changes.
- `listen`, `keepwarm`, `chat` and `config` keep their subcommands.
- `docs/cli/attend.md` is regenerated from the clap tree (`make docs`).

### Call sites to change (inventory at 0f7cace1)

Found with `grep -rnE '\battend (run|peers|inbox|status|whoami|sensors|keepwarm|send|reply|listen|unread|chat|join|leave|channels|dissolve|scene|scenes|tune|permissions|cleanup|config)\b'`, then checked by hand. Counts are lines.

**Argv callers**

- `plugins/attend/hooks/lib.ts:15-47` and `register.tsx` (7 each), `plugins/attend/tests/attend-output.ts`, `plugins/attend/types/index.d.ts`.
- `hooks/ways/attend-drain-stop.sh:11`.
- `hooks/ways/softwaredev/environment/attend/macro.sh:48, 55, 61` (the `run` match at :27 stays).
- `statusline.sh:41, 45`; `tests/statusline-test.sh`.
- `tests/fixtures/docker/run-tier1.sh` (`attend status`) and its README.
- `tools/attend/tests/*.rs`, `tools/attend/src/cli.rs` tests, `tools/agent-tui/src/app/paneshell.rs` tests.

**Messages in code**

- attend: `cmd/inbox.rs` (26), `cmd/send.rs` (15), `verbs/status.rs` (10), `lib.rs` (6), `verbs/send.rs` (6), `cmd/listen.rs`, `util.rs`, `verbs/listen.rs`, `cmd/scene.rs`, `cmd/peers.rs`, `cmd/keepwarm.rs`, `cmd/tune.rs`, `cmd/status.rs`, `cmd/sensors.rs`, `cmd/run/mod.rs`, `verbs/unread.rs`, `verbs/peers.rs`, `sensors/mod.rs`, `cmd/permissions.rs`, `cmd/config_cmd.rs`, `cmd/cleanup.rs`.
- Crates attend uses: `sensor-peers` (13), `attend-presence/src/enrollment.rs` (13), `sensor-keepwarm` (9), `attend-state/src/cold_start.rs` (7), `attend-config` (12), `agent-identity` (6), `sensor-trait`, `attend-groups`, `attend-instances`, `agent-fmt`.
- attend-chat: `src/app/keys.rs` (8, including the invite and removal bodies at :531 and :567), `src/signal.rs`, `src/app/mod.rs`, `src/main.rs` and comments in five more files.

**Guidance**

- `hooks/ways/softwaredev/environment/attend/attend.md` (17), `skills/attend/SKILL.md` (23), `tools/attend/src/sensors/disclosures/messaging.md` (7).

**Docs**

- `docs/cli/attend.md` (generated), `docs/attend-and-monitor/` (delivery, configuration, channels, signals, keepwarm, mod, engagement, tui, README, authoring-sensors, sensors, loop, first-sensor), `docs/explanation/attend-messaging/` (00, 03, 05, 06, 07), `docs/reference/statusline.md`, `docs/hooks-and-ways.md`, `docs/hooks-and-ways/observed-behavior.md`, `docs/architecture.md`, `docs/cognitive-loop.md`, `README.md`, `docs/explainers/animations/a-turn-with-ways/index.html`.

**Left as they are**

- Accepted records under `docs/architecture/` and the changelogs, which are append-only.
- `tests/fixtures/adr/` and `tests/fixtures/docker/scenarios/adr-migrate/`, frozen copies of records used as test input.

## Alternatives Considered

- **Keep the 22 flat verbs.** Not chosen. The operator asked for the ways treatment, and with the tools as the principal path the CLI's flat list serves mostly people and scripts, who read its help.
- **Keep the chat verbs flat and group the rest.** `send`, `reply`, `join` and `leave` would stay two words, which ADR-173 chose because chat vocabulary is the model's prior. This was a real option. Not chosen, because the operator asked that the CLI not be presented flatly, and in a mod session the chat verbs reach the agent as tools, whose names keep the prior. It is the first probe.
- **Plural groups (`messages`, `channels`, `peers`).** These would leave `attend channels` and `attend peers` working unchanged. Not chosen: `attend messages send` reads worse than `attend message send`, and a mix of plural and singular groups is harder to guess than one rule.
- **A `session` group for `whoami`, `listen` and `keepwarm`.** Not chosen. `listen` and `keepwarm` have their own subcommands and would sit three levels deep, and they are the verbs a host without the mod runs most.
- **Keep the mod's argv as hidden fixed names, as ADR-509 kept `reconcile`.** Not chosen. ADR-509's caller was an updater that cannot be changed and fails silently at its last step. A loaded mod is replaced by a reload or a restart, its failure is a visible usage error, and keeping six hidden names forever is the compatibility code ADR-506 removes.
- **No aliases.** Every caller in the repository moves in the same release, so this was possible. Not chosen, because agents' habits and loaded mods name the flat verbs, and ADR-509 set one release as the window for names people and agents type.
- **Have callers try the new path and fall back to the old one on exit 2.** Not chosen. It is the fallback path ADR-506 removes, and the order in item 6 makes it unnecessary for a matching install.
