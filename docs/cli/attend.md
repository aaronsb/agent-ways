# Command-Line Help for `attend`

This document contains the help content for the `attend` command-line program.

**Command Overview:**

* [`attend`↴](#attend)
* [`attend run`↴](#attend-run)
* [`attend listen`↴](#attend-listen)
* [`attend listen on`↴](#attend-listen-on)
* [`attend listen off`↴](#attend-listen-off)
* [`attend listen status`↴](#attend-listen-status)
* [`attend listen beat`↴](#attend-listen-beat)
* [`attend message`↴](#attend-message)
* [`attend message send`↴](#attend-message-send)
* [`attend message reply`↴](#attend-message-reply)
* [`attend message inbox`↴](#attend-message-inbox)
* [`attend message unread`↴](#attend-message-unread)
* [`attend message cleanup`↴](#attend-message-cleanup)
* [`attend channel`↴](#attend-channel)
* [`attend channel list`↴](#attend-channel-list)
* [`attend channel join`↴](#attend-channel-join)
* [`attend channel leave`↴](#attend-channel-leave)
* [`attend channel create`↴](#attend-channel-create)
* [`attend channel describe`↴](#attend-channel-describe)
* [`attend channel pin`↴](#attend-channel-pin)
* [`attend channel unpin`↴](#attend-channel-unpin)
* [`attend channel dissolve`↴](#attend-channel-dissolve)
* [`attend channel scene`↴](#attend-channel-scene)
* [`attend channel scenes`↴](#attend-channel-scenes)
* [`attend peer`↴](#attend-peer)
* [`attend peer list`↴](#attend-peer-list)
* [`attend peer whoami`↴](#attend-peer-whoami)
* [`attend peer status`↴](#attend-peer-status)
* [`attend keepwarm`↴](#attend-keepwarm)
* [`attend keepwarm on`↴](#attend-keepwarm-on)
* [`attend keepwarm off`↴](#attend-keepwarm-off)
* [`attend keepwarm status`↴](#attend-keepwarm-status)
* [`attend keepwarm check`↴](#attend-keepwarm-check)
* [`attend chat`↴](#attend-chat)
* [`attend sensor`↴](#attend-sensor)
* [`attend sensor list`↴](#attend-sensor-list)
* [`attend sensor permissions`↴](#attend-sensor-permissions)
* [`attend config`↴](#attend-config)
* [`attend config init`↴](#attend-config-init)
* [`attend config show`↴](#attend-config-show)
* [`attend config path`↴](#attend-config-path)
* [`attend config lint`↴](#attend-config-lint)
* [`attend config tune`↴](#attend-config-tune)

## `attend`

Active awareness for Claude Code sessions

**Usage:** `attend [COMMAND]`

In a session with the attend mod, the attend_* tools send, reply, join and list peers. This CLI is for hosts without the mod and for scripts.

###### **Subcommands:**

* `run` — Start the sensor loop (use with Monitor for async delivery)
* `listen` — Turn listening on or off for this session without joining a channel, or show it (default: status)
* `message` — Send, reply to, read and count messages (default: inbox)
* `channel` — Join, leave, list and manage channels and scenes (default: list)
* `peer` — List sessions, show this session's identity and the running loops (default: list)
* `keepwarm` — Keep the prompt cache warm across idle stretches (ADR-182; default: status)
* `chat` — Launch the interactive chat TUI (ADR-120)
* `sensor` — List sensors and audit their permissions (default: list)
* `config` — Manage configuration (default: show)



## `attend run`

Start the sensor loop (use with Monitor for async delivery)

**Usage:** `attend run [OPTIONS]`

###### **Options:**

* `--catchup` — Replay backlog signals on startup before normal cadence



## `attend listen`

Turn listening on or off for this session without joining a channel, or show it (default: status)

**Usage:** `attend listen [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `on` — Listen: the Stop-hook drain delivers to this session. Messages addressed to its project while it was not listening are delivered by the next drain; older #open and channel backlog is held back
* `off` — Stop listening: the drain delivers nothing and marks nothing read. Channel memberships are kept; a later join, `attend run` or `listen on` listens again
* `status` — Print `listening` or `not listening` (default)
* `beat` — Mark a listening session alive: touch its heartbeat, deliver nothing. The attend mod runs it on a timer, through turns too, so the session stays within the liveness grace while it lives. A session that does not listen is written nothing

###### **Options:**

* `--json` — Print the state as one JSON document



## `attend listen on`

Listen: the Stop-hook drain delivers to this session. Messages addressed to its project while it was not listening are delivered by the next drain; older #open and channel backlog is held back

**Usage:** `attend listen on [OPTIONS]`

###### **Options:**

* `--host <HOST>` — What hosts delivery, when not Monitor or a shell: `mod`, the attend mod, whose agent answers with attend_reply and attend_send. The drain then delivers in the mod's format and names those tools

  Possible values:
  - `mod`:
    The attend mod (ADR-406)




## `attend listen off`

Stop listening: the drain delivers nothing and marks nothing read. Channel memberships are kept; a later join, `attend run` or `listen on` listens again

**Usage:** `attend listen off`



## `attend listen status`

Print `listening` or `not listening` (default)

**Usage:** `attend listen status`



## `attend listen beat`

Mark a listening session alive: touch its heartbeat, deliver nothing. The attend mod runs it on a timer, through turns too, so the session stays within the liveness grace while it lives. A session that does not listen is written nothing

**Usage:** `attend listen beat`



## `attend message`

Send, reply to, read and count messages (default: inbox)

**Usage:** `attend message [OPTIONS]
       message <COMMAND>`

###### **Subcommands:**

* `send` — Send a message: a direct message with --to, a channel post with --channel, else to #open
* `reply` — Reply to the most recent peer message: a direct message to its sender, or in its named channel
* `inbox` — Read pending messages from peers
* `unread` — Count this session's unread messages by room, marking none of them read
* `cleanup` — Reap signal files whose owning project is gone, and prune empty project dirs. Messages are never removed by age — lifetime is bound to project liveness (ADR-136)

###### **Options:**

* `--limit <LIMIT>` — Max messages per page, newest first

  Default value: `25`
* `--page <PAGE>` — Page number; 1 = newest. Higher numbers walk back into history

  Default value: `1`
* `--before <TS>` — Cursor: only show messages older than this unix timestamp
* `--drain` — Atomically deliver pending messages and record their consumption (ADR-172). The Stop-hook fast path: a silent no-op under an unresolved identity or for a session not enrolled by attend run or a channel join (#720). On a cold start (no seen-set) it delivers addressed mail and fresh messages and counts the older backlog it does not show
* `--wake` — With --drain: deliver and mark nothing unless a pending message would wake this session, one addressed to it, sent to its project, a reply to a message it sent, or any post in one of its named channels; then deliver all that is pending. Other `#open` posts alone wait for the next turn (ADR-406 item 2). Once the session's prompt cache has lapsed (its tier's TTL less two minutes after its last turn, or no transcript readable in five seconds), nothing wakes it: the drain delivers nothing and reports `cache_lapsed`. The attend mod polls with it while idle
* `--format <FMT>` — Output format for --drain: `plain` (human/agent readable), `hook` (Claude Code Stop-hook JSON; reads the hook's stdin payload for `stop_hook_active`) or `json` (the same as --json)

  Default value: `plain`
* `--json` — Print the listing, the message, or with --drain the drained messages, held-back count, note and name, as one JSON document



## `attend message send`

Send a message: a direct message with --to, a channel post with --channel, else to #open

**Usage:** `attend message send [OPTIONS] [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>` — Message body (must follow all flags)

###### **Options:**

* `--to <WHO>` — Send a direct message: to a session's name or id, a person's member id, or a project path for every live session there
* `--channel <NAME>` — Post to a named channel; `open` or `#open` posts to #open. With --to as well, the post stays in the channel, addressed to them
* `--json` — Print the written signal as one JSON document



## `attend message reply`

Reply to the most recent peer message: a direct message to its sender, or in its named channel

**Usage:** `attend message reply [OPTIONS] [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>` — Message body (must follow all flags)

###### **Options:**

* `--re <ID>` — Reply to this message id instead of the newest one delivered. The reply goes to its sender, or into its named channel, as without --re
* `--to <WHO>` — Send the reply as a direct message to this session, person or project path instead of to the parent's sender
* `--channel <NAME>` — Post the reply to this channel instead; `open` posts to #open
* `--json` — Print the written signal as one JSON document



## `attend message inbox`

Read pending messages from peers

**Usage:** `attend message inbox [OPTIONS] [MSG_ID]`

###### **Arguments:**

* `<MSG_ID>` — Specific message id to read in detail (omit to list inbox)

###### **Options:**

* `--limit <LIMIT>` — Max messages per page, newest first

  Default value: `25`
* `--page <PAGE>` — Page number; 1 = newest. Higher numbers walk back into history

  Default value: `1`
* `--before <TS>` — Cursor: only show messages older than this unix timestamp
* `--drain` — Atomically deliver pending messages and record their consumption (ADR-172). The Stop-hook fast path: a silent no-op under an unresolved identity or for a session not enrolled by attend run or a channel join (#720). On a cold start (no seen-set) it delivers addressed mail and fresh messages and counts the older backlog it does not show
* `--wake` — With --drain: deliver and mark nothing unless a pending message would wake this session, one addressed to it, sent to its project, a reply to a message it sent, or any post in one of its named channels; then deliver all that is pending. Other `#open` posts alone wait for the next turn (ADR-406 item 2). Once the session's prompt cache has lapsed (its tier's TTL less two minutes after its last turn, or no transcript readable in five seconds), nothing wakes it: the drain delivers nothing and reports `cache_lapsed`. The attend mod polls with it while idle
* `--format <FMT>` — Output format for --drain: `plain` (human/agent readable), `hook` (Claude Code Stop-hook JSON; reads the hook's stdin payload for `stop_hook_active`) or `json` (the same as --json)

  Default value: `plain`
* `--json` — Print the listing, the message, or with --drain the drained messages, held-back count, note and name, as one JSON document



## `attend message unread`

Count this session's unread messages by room, marking none of them read

**Usage:** `attend message unread [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend message cleanup`

Reap signal files whose owning project is gone, and prune empty project dirs. Messages are never removed by age — lifetime is bound to project liveness (ADR-136)

**Usage:** `attend message cleanup [OPTIONS]`

###### **Options:**

* `-n`, `--dry-run` — List what would be removed without deleting
* `--all` — Remove every signal file regardless of project liveness



## `attend channel`

Join, leave, list and manage channels and scenes (default: list)

**Usage:** `attend channel [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `list` — List every channel, marking the ones this session is in
* `join` — Join a channel (creates it if absent)
* `leave` — Leave a channel
* `create` — Create a channel without joining it (pinned so it persists empty)
* `describe` — Set or replace a channel's single-line description
* `pin` — Pin a channel so it persists when empty
* `unpin` — Unpin a channel; it is removed if empty
* `dissolve` — Dissolve a channel (removes it for every member)
* `scene` — Activate a named scene (reconfigure channel membership; `scene private` leaves all channels)
* `scenes` — List available scenes

###### **Options:**

* `--joined` — List only the channels this session has joined (read-only: no cleanup)
* `--json` — Print the listing as one JSON document



## `attend channel list`

List every channel, marking the ones this session is in

**Usage:** `attend channel list [OPTIONS]`

###### **Options:**

* `--joined` — List only the channels this session has joined (read-only: no cleanup)
* `--json` — Print the listing as one JSON document



## `attend channel join`

Join a channel (creates it if absent)

**Usage:** `attend channel join [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)

###### **Options:**

* `--pin` — Pin so the channel persists across scene changes
* `--json` — Print the result as one JSON document



## `attend channel leave`

Leave a channel

**Usage:** `attend channel leave [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend channel create`

Create a channel without joining it (pinned so it persists empty)

**Usage:** `attend channel create <NAME> [DESCRIPTION]...`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)
* `<DESCRIPTION>` — Optional single-line description (all trailing words)



## `attend channel describe`

Set or replace a channel's single-line description

**Usage:** `attend channel describe <NAME> [DESCRIPTION]...`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)
* `<DESCRIPTION>` — The description text (all trailing words; empty clears)



## `attend channel pin`

Pin a channel so it persists when empty

**Usage:** `attend channel pin <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend channel unpin`

Unpin a channel; it is removed if empty

**Usage:** `attend channel unpin <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend channel dissolve`

Dissolve a channel (removes it for every member)

**Usage:** `attend channel dissolve <NAME>`

###### **Arguments:**

* `<NAME>` — Channel name (with or without the # prefix)



## `attend channel scene`

Activate a named scene (reconfigure channel membership; `scene private` leaves all channels)

**Usage:** `attend channel scene <NAME>`

###### **Arguments:**

* `<NAME>` — Scene name (try `attend channel scenes` to list)



## `attend channel scenes`

List available scenes

**Usage:** `attend channel scenes`



## `attend peer`

List sessions, show this session's identity and the running loops (default: list)

**Usage:** `attend peer [OPTIONS]
       peer <COMMAND>`

###### **Subcommands:**

* `list` — List active Claude Code sessions and their channels
* `whoami` — Print this session's canonical bus identity (issue #378)
* `status` — Show every running `attend run` with the session it serves, signals, and channel state

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend peer list`

List active Claude Code sessions and their channels

**Usage:** `attend peer list [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend peer whoami`

Print this session's canonical bus identity (issue #378)

**Usage:** `attend peer whoami [OPTIONS]`

###### **Options:**

* `--machine` — Emit the stable key as `key=value` lines for scripts/hooks (session_id, origin_path, resolved) instead of the human-readable table. Downstream state must key on these fields — the display name is presentation, never a key
* `--display` — Print only the display name, for status lines and prompts. Presentation only: it can change between sessions, so never key state on it. An unresolved identity prints the name of the process cwd, with no notice



## `attend peer status`

Show every running `attend run` with the session it serves, signals, and channel state

**Usage:** `attend peer status [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend keepwarm`

Keep the prompt cache warm across idle stretches (ADR-182; default: status)

**Usage:** `attend keepwarm [COMMAND]`

###### **Subcommands:**

* `on` — Arm a window: one wake at 50 idle minutes keeps the cache read, not re-written
* `off` — Disarm and forget the window
* `status` — Cache state, context size, cold price, window left, and this session's cold writes
* `check` — Run the keepwarm judgment once: print the `keepwarm:` wake line when one is due, else nothing (exit 0 either way). For a host that polls without `attend run`; prints nothing while an `attend run` holds the session, since its sensor wakes it



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



## `attend keepwarm check`

Run the keepwarm judgment once: print the `keepwarm:` wake line when one is due, else nothing (exit 0 either way). For a host that polls without `attend run`; prints nothing while an `attend run` holds the session, since its sensor wakes it

**Usage:** `attend keepwarm check`



## `attend chat`

Launch the interactive chat TUI (ADR-120)

**Usage:** `attend chat [PASSTHROUGH]...`

###### **Arguments:**

* `<PASSTHROUGH>` — Arguments passed through to the `attend-chat` binary



## `attend sensor`

List sensors and audit their permissions (default: list)

**Usage:** `attend sensor [COMMAND]`

###### **Subcommands:**

* `list` — List all sensors — built-in and config-defined script sensors
* `permissions` — Compare each sensor's `requires:` against settings.json



## `attend sensor list`

List all sensors — built-in and config-defined script sensors

**Usage:** `attend sensor list`



## `attend sensor permissions`

Compare each sensor's `requires:` against settings.json

**Usage:** `attend sensor permissions`



## `attend config`

Manage configuration (default: show)

**Usage:** `attend config [COMMAND]`

###### **Subcommands:**

* `init` — Write the default config file to the user scope, if none is there
* `show` — Print each setting in effect as key=value (default)
* `path` — Print the user/project config file paths
* `lint` — Check the config files against the schema; exit 3 with findings
* `tune` — Survey session history and derive engagement config



## `attend config init`

Write the default config file to the user scope, if none is there

**Usage:** `attend config init`



## `attend config show`

Print each setting in effect as key=value (default)

**Usage:** `attend config show`



## `attend config path`

Print the user/project config file paths

**Usage:** `attend config path`



## `attend config lint`

Check the config files against the schema; exit 3 with findings

**Usage:** `attend config lint`



## `attend config tune`

Survey session history and derive engagement config

**Usage:** `attend config tune [OPTIONS]`

###### **Options:**

* `--apply` — Write the derived absolute_refractory, decay_per_minute and peer_activity_window to the user config, changing nothing else



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
