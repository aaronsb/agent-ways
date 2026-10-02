# Command-Line Help for `attend`

This document contains the help content for the `attend` command-line program.

**Command Overview:**

* [`attend`↴](#attend)
* [`attend run`↴](#attend-run)
* [`attend peers`↴](#attend-peers)
* [`attend inbox`↴](#attend-inbox)
* [`attend status`↴](#attend-status)
* [`attend whoami`↴](#attend-whoami)
* [`attend sensors`↴](#attend-sensors)
* [`attend keepwarm`↴](#attend-keepwarm)
* [`attend keepwarm on`↴](#attend-keepwarm-on)
* [`attend keepwarm off`↴](#attend-keepwarm-off)
* [`attend keepwarm status`↴](#attend-keepwarm-status)
* [`attend send`↴](#attend-send)
* [`attend reply`↴](#attend-reply)
* [`attend chat`↴](#attend-chat)
* [`attend join`↴](#attend-join)
* [`attend leave`↴](#attend-leave)
* [`attend channels`↴](#attend-channels)
* [`attend channels list`↴](#attend-channels-list)
* [`attend channels pin`↴](#attend-channels-pin)
* [`attend channels unpin`↴](#attend-channels-unpin)
* [`attend channels create`↴](#attend-channels-create)
* [`attend channels describe`↴](#attend-channels-describe)
* [`attend dissolve`↴](#attend-dissolve)
* [`attend scene`↴](#attend-scene)
* [`attend scenes`↴](#attend-scenes)
* [`attend tune`↴](#attend-tune)
* [`attend permissions`↴](#attend-permissions)
* [`attend permissions audit`↴](#attend-permissions-audit)
* [`attend cleanup`↴](#attend-cleanup)
* [`attend config`↴](#attend-config)
* [`attend config init`↴](#attend-config-init)
* [`attend config show`↴](#attend-config-show)
* [`attend config path`↴](#attend-config-path)
* [`attend config lint`↴](#attend-config-lint)

## `attend`

Active awareness for Claude Code sessions

**Usage:** `attend [COMMAND]`

###### **Subcommands:**

* `run` — Start the sensor loop (use with Monitor for async delivery)
* `peers` — List active Claude Code sessions and their channels
* `inbox` — Read pending messages from peers
* `status` — Show running instances, signals, and channel state
* `whoami` — Print this session's canonical bus identity (issue #378)
* `sensors` — List all sensors — built-in and config-defined script sensors
* `keepwarm` — Keep the prompt cache warm across idle stretches (ADR-182; default: status)
* `send` — Send a signal to peer sessions (defaults to #open base channel)
* `reply` — Reply to the most recent peer message (auto-threaded)
* `chat` — Launch the interactive chat TUI (ADR-120)
* `join` — Join a channel (creates it if absent)
* `leave` — Leave a channel
* `channels` — Channel lifecycle (default: list all, joined ones marked)
* `dissolve` — Dissolve a channel (removes it for every member)
* `scene` — Activate a named scene (reconfigure channel membership; `scene private` leaves all channels)
* `scenes` — List available scenes
* `tune` — Survey session history and derive engagement config
* `permissions` — Audit sensor permissions against settings.json (default: audit)
* `cleanup` — Reap signal files whose owning project is gone, and prune empty project dirs. Messages are never removed by age — lifetime is bound to project liveness (ADR-136)
* `config` — Manage configuration (default: show)



## `attend run`

Start the sensor loop (use with Monitor for async delivery)

**Usage:** `attend run [OPTIONS]`

###### **Options:**

* `--catchup` — Replay backlog signals on startup before normal cadence



## `attend peers`

List active Claude Code sessions and their channels

**Usage:** `attend peers`



## `attend inbox`

Read pending messages from peers

**Usage:** `attend inbox [OPTIONS] [MSG_ID]`

###### **Arguments:**

* `<MSG_ID>` — Specific message id to read in detail (omit to list inbox)

###### **Options:**

* `--limit <LIMIT>` — Max messages per page, newest first

  Default value: `25`
* `--page <PAGE>` — Page number; 1 = newest. Higher numbers walk back into history

  Default value: `1`
* `--before <TS>` — Cursor: only show messages older than this unix timestamp
* `--drain` — Atomically deliver pending messages and record their consumption (ADR-172). The Stop-hook fast path: no-op under an unresolved identity, and on a cold start (no seen-set) baselines the backlog without delivering
* `--format <FMT>` — Output format for --drain: `plain` (human/agent readable) or `hook` (Claude Code Stop-hook JSON; reads the hook's stdin payload for `stop_hook_active`)

  Default value: `plain`



## `attend status`

Show running instances, signals, and channel state

**Usage:** `attend status`



## `attend whoami`

Print this session's canonical bus identity (issue #378)

**Usage:** `attend whoami [OPTIONS]`

###### **Options:**

* `--machine` — Emit the stable key as `key=value` lines for scripts/hooks (session_id, origin_path, resolved) instead of the human-readable table. Downstream state must key on these fields — the display name is presentation, never a key
* `--display` — Print only the display name, for status lines and prompts. Presentation only: it can change between sessions, so never key state on it. An unresolved identity prints the name of the process cwd, with no notice



## `attend sensors`

List all sensors — built-in and config-defined script sensors

**Usage:** `attend sensors`



## `attend keepwarm`

Keep the prompt cache warm across idle stretches (ADR-182; default: status)

**Usage:** `attend keepwarm [COMMAND]`

###### **Subcommands:**

* `on` — Arm a window: one wake at 50 idle minutes keeps the cache read, not re-written
* `off` — Disarm and forget the window
* `status` — Cache state, context size, cold price, window left, and this session's cold writes



## `attend keepwarm on`

Arm a window: one wake at 50 idle minutes keeps the cache read, not re-written

**Usage:** `attend keepwarm on [WINDOW]`

###### **Arguments:**

* `<WINDOW>` — Window such as 6h, 90m, or 2h30m (default 6h)



## `attend keepwarm off`

Disarm and forget the window

**Usage:** `attend keepwarm off`



## `attend keepwarm status`

Cache state, context size, cold price, window left, and this session's cold writes

**Usage:** `attend keepwarm status`



## `attend send`

Send a signal to peer sessions (defaults to #open base channel)

**Usage:** `attend send [OPTIONS] [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>` — Message body (must follow all flags)

###### **Options:**

* `--to <PATH>` — Scope send to a specific project path
* `--channel <NAME>` — Scope send to a named channel



## `attend reply`

Reply to the most recent peer message (auto-threaded)

**Usage:** `attend reply [OPTIONS] [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>` — Message body (must follow all flags)

###### **Options:**

* `--to <PATH>` — Scope send to a specific project path
* `--channel <NAME>` — Scope send to a named channel



## `attend chat`

Launch the interactive chat TUI (ADR-120)

**Usage:** `attend chat [PASSTHROUGH]...`

###### **Arguments:**

* `<PASSTHROUGH>` — Arguments passed through to the `attend-chat` binary



## `attend join`

Join a channel (creates it if absent)

**Usage:** `attend join [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)

###### **Options:**

* `--pin` — Pin so the channel persists across scene changes



## `attend leave`

Leave a channel

**Usage:** `attend leave <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend channels`

Channel lifecycle (default: list all, joined ones marked)

**Usage:** `attend channels [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `list` — List all channels with joined marks (default)
* `pin` — Pin a channel so it persists when empty
* `unpin` — Unpin a channel; it is removed if empty
* `create` — Create a channel without joining it (pinned so it persists empty)
* `describe` — Set or replace a channel's single-line description

###### **Options:**

* `--joined` — List only the channels this session has joined (read-only: no cleanup)



## `attend channels list`

List all channels with joined marks (default)

**Usage:** `attend channels list`



## `attend channels pin`

Pin a channel so it persists when empty

**Usage:** `attend channels pin <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend channels unpin`

Unpin a channel; it is removed if empty

**Usage:** `attend channels unpin <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend channels create`

Create a channel without joining it (pinned so it persists empty)

**Usage:** `attend channels create <NAME> [DESCRIPTION]...`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)
* `<DESCRIPTION>` — Optional single-line description (all trailing words)



## `attend channels describe`

Set or replace a channel's single-line description

**Usage:** `attend channels describe <NAME> [DESCRIPTION]...`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)
* `<DESCRIPTION>` — The description text (all trailing words; empty clears)



## `attend dissolve`

Dissolve a channel (removes it for every member)

**Usage:** `attend dissolve <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend scene`

Activate a named scene (reconfigure channel membership; `scene private` leaves all channels)

**Usage:** `attend scene <NAME>`

###### **Arguments:**

* `<NAME>` — Scene name (try `attend scenes` to list)



## `attend scenes`

List available scenes

**Usage:** `attend scenes`



## `attend tune`

Survey session history and derive engagement config

**Usage:** `attend tune [OPTIONS]`

###### **Options:**

* `--apply` — Write derived values to the user config



## `attend permissions`

Audit sensor permissions against settings.json (default: audit)

**Usage:** `attend permissions [COMMAND]`

###### **Subcommands:**

* `audit` — Compare each sensor's `requires:` against settings.json



## `attend permissions audit`

Compare each sensor's `requires:` against settings.json

**Usage:** `attend permissions audit`



## `attend cleanup`

Reap signal files whose owning project is gone, and prune empty project dirs. Messages are never removed by age — lifetime is bound to project liveness (ADR-136)

**Usage:** `attend cleanup [OPTIONS]`

###### **Options:**

* `-n`, `--dry-run` — List what would be removed without deleting
* `--all` — Remove every signal file regardless of project liveness



## `attend config`

Manage configuration (default: show)

**Usage:** `attend config [COMMAND]`

###### **Subcommands:**

* `init` — Write a default config file to the user scope
* `show` — Display the current effective configuration (default)
* `path` — Print the user/project config file paths
* `lint` — Validate the config file



## `attend config init`

Write a default config file to the user scope

**Usage:** `attend config init`



## `attend config show`

Display the current effective configuration (default)

**Usage:** `attend config show`



## `attend config path`

Print the user/project config file paths

**Usage:** `attend config path`



## `attend config lint`

Validate the config file

**Usage:** `attend config lint [OPTIONS]`

###### **Options:**

* `--fix` — Auto-fix what can be fixed
* `--check` — Exit non-zero on errors (for CI)



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
