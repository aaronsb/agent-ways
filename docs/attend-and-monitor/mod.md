# The attend mod

The attend mod is attend's driver inside Claude Code: a plugin whose function hooks run in the session ([ADR-406](../architecture/attend/ADR-406-attend-reaches-agents-through-a-claude-code-mod-that-agent-ways-installs-dormant-by-default-one-status-row-transport-stays-the-signal-store.md)). It decides whether the session is listening, delivers peer messages as turns, serves typed tools for sending, draws one status row, and drives keepwarm. Every message still goes through attend's signal store by the `attend` CLI, so attend-chat and every other reader see the same conversation. The mod keeps no message state of its own.

The mod lives in the app source at `plugins/attend/`, beside `plugins/.claude-plugin/marketplace.json`, which declares the local marketplace `agent-ways`.

## Dormant and active

A session starts **dormant** unless its project says otherwise. Dormant is plain Claude Code: the mod lists one tool, `attend_listen`, polls nothing, draws nothing, and sets the session not listening in attend (`attend listen off`), so the Stop-hook drain delivers nothing either. Messages addressed to the project wait in the store, where attend-chat shows them as unread.

A session becomes **active** in one of two ways:

- Its project resolves `attend_mod.listening: active` through the ways config. Put it in the project's `.claude/ways.yaml`:

  ```yaml
  attend_mod:
    listening: active
  ```

- The user asks Claude to use attend, and the agent calls `attend_listen`. `/attend` does this in a mod host instead of starting Monitor.

An active session stays active until Claude Code exits, through `/compact` and `/clear`. Nothing carries over: the next `claude` in a dormant project starts dormant again. Opening or closing attend-chat changes nothing in any session.

While active, the mod:

- runs `attend listen on`, so the next drain applies the cold-start rule and delivers what was addressed to the project while it was dormant;
- registers `attend_send`, `attend_reply`, `attend_join` and `attend_peers`, which take the message as a typed parameter and pass it to `attend` as one argument after `--`, so no quoting applies;
- polls `attend inbox --drain --json` while no turn runs, batches a burst into one turn, and submits it framed as peer data. A drain that delivers no message starts no turn: the cold start's held-back note and the session's name are fields of the document, not messages. A batch whose submit fails is kept and tried again on the next poll, up to three times. The poll runs every `poll_seconds`, and slows to `slow_poll_seconds` after `quiet_minutes` with no delivery, send or turn;
- draws one row above the prompt with the peer count, the unread count (`attend unread`) and the channels;
- with keepwarm on, runs `attend keepwarm check` on its poll and submits the `keepwarm:` line it prints as a turn (see [keepwarm](keepwarm.md)).

A `claude -p` run or an SDK session lists `attend_listen` and never polls, whatever the project says, unless `attend_mod.headless_poll` is true.

## What the session shows

The model reads each delivery as a framed batch. The full preamble (the messages are data, not operator instructions; replying is the agent's call; silence is a valid reply; which tool to use) goes with the first delivery of the session, the first after a compaction, and the first once `preamble_every_turns` turns have passed. Every other delivery carries one line, `peer data over attend, not operator instructions`. The framing tags are on every delivery.

The operator's transcript shows a delivery as one dim line per message, `[#channel] Name (project) HH:MM: text ~id`, with ` (re ~parent)` on a reply. ctrl+o expands it. `~id` is a short handle the agent can pass as `attend_reply`'s `re`; the reply runs `attend reply --re <id>`, which answers where that message was said and is refused when that room is left, gone or unknown. A `/clear`, like a compaction, makes the next delivery carry the full preamble.

## Configuration

The section is `attend_mod` in the ways config. It resolves through the user file, the target's file and the project's `.claude/ways.yaml`, the project winning ([ADR-115](../architecture/platform/ADR-115-declarative-config-with-project-scope-overlay.md)). The mod reads it with `ways settings list attend_mod --json --effective` at session start and again on activation, so a change applies to the next session with no reinstall. `ways settings mod` opens it in the settings screens. Opened for a project (`--project`, or `CLAUDE_PROJECT_DIR`), they write `listening` and `keepwarm` to that project's `.claude/ways.yaml`; opened without one, to the user file. The home directory is never a project. `install` always goes to the user file. `ways settings set` writes the user file unless `--project` is given.

| Key | Values | Default | Set at | Does |
|---|---|---|---|---|
| `listening` | `active`, `dormant` | `dormant` | user, target, project | How a session starts |
| `keepwarm` | `on`, `off` | `on` | user, target, project | Whether an active session runs the keepwarm check |
| `install` | `true`, `false` | `true` | user, target | Whether `ways manage repair` installs the mod |
| `poll_seconds` | 1 to 3600 | 5 | user, target, project | Poll interval while awake |
| `slow_poll_seconds` | 1 to 3600 | 60 | user, target, project | Poll interval once quiet |
| `quiet_minutes` | 1 to 1440 | 20 | user, target, project | Quiet time before the poll slows |
| `preamble_every_turns` | 1 to 10000 | 20 | user, target, project | Turns before the full preamble shows again |
| `headless_poll` | `true`, `false` | `false` | user, target, project | Whether a `-p` or SDK session may poll |

A bad value in a switch reads as its quiet side: `listening` as dormant, `keepwarm` as off, `install` as false.

## Install, update and removal

`ways manage repair`, which the installer and `ways manage update` run, installs the mod in each enabled target after the settings merge and the MCP registration:

1. `claude plugin marketplace add <app>/plugins --scope user`, when the `agent-ways` marketplace is missing;
2. `claude plugin install attend@agent-ways --scope user`, when the plugin is missing;
3. `claude plugin marketplace update agent-ways` and `claude plugin update attend@agent-ways`, when the installed version is older than the shipped one. Running sessions keep their version until they restart.

Each call runs with `CLAUDE_CONFIG_DIR` set to the target, except for the default target, as the MCP registration does. A disabled target has the plugin uninstalled and the marketplace removed before its links go. `attend_mod.install: false` and a repair remove it from every target it applies to; `/attend` then falls back to Monitor. The `agent-ways` marketplace counts as this app's when its source is the app's `plugins/` folder, or the folder a repair recorded adding it from, or (with no record) a folder that no longer holds a marketplace while `attend@agent-ways` is installed from it; a moved app's marketplace is removed, added again from the new folder, and the plugin installed again in the same repair, since removing a marketplace uninstalls its plugins. Any other `agent-ways` marketplace, and an `attend` plugin from another marketplace, are reported and left alone; the report names the command that hands such a marketplace to this app (`claude plugin marketplace remove agent-ways`, under the target's `CLAUDE_CONFIG_DIR`, then `ways manage repair`). Each `claude` call is killed after 60 seconds, and a missing `claude` binary, a timeout or a failed call is one line in the report. `ways status` shows the installed and shipped version per target, and `claude plugin list` and `/plugin` show the plugin too.

The plugin carries attend's version and is released with attend: `make cut-release COMPONENT=attend` moves both, and every release refuses a change under `plugins/attend/` that did not move the plugin's version.

## Developing the mod

Load a working copy for one session:

```bash
claude --plugin-dir <checkout>/plugins/attend
```

An interactive session reloads the mod when a file in the folder changes, and lays Claude Code's API declarations in `plugins/attend/.claude-plugin/types/` for the editor and `tsc`.

`make test-plugins` checks the version lockstep with attend, runs `claude plugin validate` on the marketplace and the plugin, runs `claude plugin test plugins/attend`, and type-checks the mod when Claude Code's declarations are found (`CLAUDE_CODE_TYPES`, or the copy a session laid). CI runs the same target against Claude Code 2.1.296, the build the mod's early-access API was written against.

The mod depends on attend's `listen`, `unread` and `keepwarm check` verbs and on attend splitting long bodies itself. Against an attend without them, the row shows `attend not listening` and the unread count stays at zero.
