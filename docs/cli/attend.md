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
* [`attend send`↴](#attend-send)
* [`attend reply`↴](#attend-reply)
* [`attend chat`↴](#attend-chat)
* [`attend join`↴](#attend-join)
* [`attend leave`↴](#attend-leave)
* [`attend channels`↴](#attend-channels)
* [`attend dissolve`↴](#attend-dissolve)
* [`attend focus`↴](#attend-focus)
* [`attend focus on`↴](#attend-focus-on)
* [`attend focus off`↴](#attend-focus-off)
* [`attend focus clear`↴](#attend-focus-clear)
* [`attend focus pin`↴](#attend-focus-pin)
* [`attend focus unpin`↴](#attend-focus-unpin)
* [`attend focus dissolve`↴](#attend-focus-dissolve)
* [`attend focus all`↴](#attend-focus-all)
* [`attend focus list`↴](#attend-focus-list)
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
* `send` — Send a signal to peer sessions (defaults to #open base channel)
* `reply` — Reply to the most recent peer message (auto-threaded)
* `chat` — Launch the interactive chat TUI (ADR-120)
* `join` — Join a channel (creates it if absent)
* `leave` — Leave a channel
* `channels` — List channels — all available, with joined ones marked
* `dissolve` — Dissolve a channel (removes it for every member)
* `focus` — Deprecated alias for the channel verbs (join/leave/channels/dissolve; ADR-173)
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



## `attend sensors`

List all sensors — built-in and config-defined script sensors

**Usage:** `attend sensors`



## `attend send`

Send a signal to peer sessions (defaults to #open base channel)

**Usage:** `attend send [OPTIONS] [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>` — Message body (must follow all flags)

###### **Options:**

* `--broadcast` — Force broadcast (every peer + every Aaron session)
* `--to <PATH>` — Scope send to a specific project path
* `--channel <NAME>` — Scope send to a named channel (accepts `--focus` as a deprecated alias)



## `attend reply`

Reply to the most recent peer message (auto-threaded)

**Usage:** `attend reply [OPTIONS] [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>` — Message body (must follow all flags)

###### **Options:**

* `--broadcast` — Force broadcast
* `--to <PATH>` — Scope send to a specific project path
* `--channel <NAME>` — Scope send to a named channel (accepts `--focus` as a deprecated alias)



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

List channels — all available, with joined ones marked

**Usage:** `attend channels`



## `attend dissolve`

Dissolve a channel (removes it for every member)

**Usage:** `attend dissolve <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend focus`

Deprecated alias for the channel verbs (join/leave/channels/dissolve; ADR-173)

**Usage:** `attend focus [COMMAND]`

###### **Subcommands:**

* `on` — Join a focus group
* `off` — Leave a focus group
* `clear` — Leave every joined group
* `pin` — Pin a group so it persists across scene changes
* `unpin` — Unpin a group
* `dissolve` — Dissolve a group (remove for every peer)
* `all` — Show every available group
* `list` — List joined groups (default action)



## `attend focus on`

Join a focus group

**Usage:** `attend focus on [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Group name

###### **Options:**

* `--pin` — Pin so it persists across scene changes



## `attend focus off`

Leave a focus group

**Usage:** `attend focus off <NAME>`

###### **Arguments:**

* `<NAME>` — Group name



## `attend focus clear`

Leave every joined group

**Usage:** `attend focus clear`



## `attend focus pin`

Pin a group so it persists across scene changes

**Usage:** `attend focus pin <NAME>`

###### **Arguments:**

* `<NAME>`



## `attend focus unpin`

Unpin a group

**Usage:** `attend focus unpin <NAME>`

###### **Arguments:**

* `<NAME>`



## `attend focus dissolve`

Dissolve a group (remove for every peer)

**Usage:** `attend focus dissolve <NAME>`

###### **Arguments:**

* `<NAME>`



## `attend focus all`

Show every available group

**Usage:** `attend focus all`



## `attend focus list`

List joined groups (default action)

**Usage:** `attend focus list`



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
