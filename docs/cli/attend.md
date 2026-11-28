# Command-Line Help for `attend`

This document contains the help content for the `attend` command-line program.

**Command Overview:**

* [`attend`↴](#attend)
* [`attend listen`↴](#attend-listen)
* [`attend listen on`↴](#attend-listen-on)
* [`attend listen off`↴](#attend-listen-off)
* [`attend listen status`↴](#attend-listen-status)
* [`attend listen beat`↴](#attend-listen-beat)
* [`attend listen finish`↴](#attend-listen-finish)
* [`attend message`↴](#attend-message)
* [`attend message send`↴](#attend-message-send)
* [`attend message reply`↴](#attend-message-reply)
* [`attend message inbox`↴](#attend-message-inbox)
* [`attend message unread`↴](#attend-message-unread)
* [`attend message awaiting`↴](#attend-message-awaiting)
* [`attend message lapse`↴](#attend-message-lapse)
* [`attend message cleanup`↴](#attend-message-cleanup)
* [`attend message delete`↴](#attend-message-delete)
* [`attend message tombstone`↴](#attend-message-tombstone)
* [`attend channel`↴](#attend-channel)
* [`attend channel list`↴](#attend-channel-list)
* [`attend channel join`↴](#attend-channel-join)
* [`attend channel add`↴](#attend-channel-add)
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
* [`attend keepwarm reset`↴](#attend-keepwarm-reset)
* [`attend keepwarm wrapped`↴](#attend-keepwarm-wrapped)
* [`attend keepwarm compacted`↴](#attend-keepwarm-compacted)
* [`attend keepwarm resumed`↴](#attend-keepwarm-resumed)
* [`attend keepwarm judged`↴](#attend-keepwarm-judged)
* [`attend keepwarm drained`↴](#attend-keepwarm-drained)
* [`attend keepwarm goal`↴](#attend-keepwarm-goal)
* [`attend keepwarm stopped`↴](#attend-keepwarm-stopped)
* [`attend keepwarm wrap-id`↴](#attend-keepwarm-wrap-id)
* [`attend chat`↴](#attend-chat)
* [`attend sensor`↴](#attend-sensor)
* [`attend sensor list`↴](#attend-sensor-list)
* [`attend sensor poll`↴](#attend-sensor-poll)
* [`attend sensor fire`↴](#attend-sensor-fire)
* [`attend sensor probe`↴](#attend-sensor-probe)
* [`attend sensor permissions`↴](#attend-sensor-permissions)
* [`attend sensor host`↴](#attend-sensor-host)
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

* `listen` — Turn listening on or off for this session without joining a channel, mark it finished, or show it (default: status)
* `message` — Send, reply to, read, count and delete messages, and tombstone a session (default: inbox)
* `channel` — Join, leave, list and manage channels and scenes (default: list)
* `peer` — List sessions, show this session's identity and the running loops (default: list)
* `keepwarm` — Keep the prompt cache warm across idle stretches (ADR-182; default: status)
* `chat` — Launch the interactive chat TUI (ADR-120)
* `sensor` — List, poll, probe and test-fire sensors, and audit their permissions (default: list)
* `config` — Manage configuration (default: show)



## `attend listen`

Turn listening on or off for this session without joining a channel, mark it finished, or show it (default: status)

**Usage:** `attend listen [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `on` — Listen: the drain delivers to this session. Messages addressed to its project while it was not listening are delivered by the next drain; older #open and channel backlog is held back. The Stop hook delivers only with `--host mod`
* `off` — Stop listening: the drain delivers nothing and marks nothing read. Channel memberships are kept; a later join or `listen on` listens again
* `status` — Print `listening` or `not listening` (default)
* `beat` — Mark a listening session alive: touch its heartbeat, deliver nothing. After a /clear it first takes over the enrollment the session's earlier id made, as the drain does. The attend mod runs it on a timer, through turns too, so the session stays within the liveness grace while it lives. A session that does not listen is written nothing
* `finish` — Mark this session finished: peers see it finished, and attend refuses new messages to it. Its queued messages stay queued, and its Claude Code process runs on. `listen on` clears the mark; a beat leaves it

###### **Options:**

* `--json` — Print the state as one JSON document



## `attend listen on`

Listen: the drain delivers to this session. Messages addressed to its project while it was not listening are delivered by the next drain; older #open and channel backlog is held back. The Stop hook delivers only with `--host mod`

**Usage:** `attend listen on [OPTIONS]`

###### **Options:**

* `--host <HOST>` — What hosts delivery, when not a shell: `mod`, the attend mod, whose agent answers with attend_reply and attend_send. The Stop-hook drain then delivers to the session, in the mod's format and naming those tools

  Possible values:
  - `mod`:
    The attend mod (ADR-406)




## `attend listen off`

Stop listening: the drain delivers nothing and marks nothing read. Channel memberships are kept; a later join or `listen on` listens again

**Usage:** `attend listen off`



## `attend listen status`

Print `listening` or `not listening` (default)

**Usage:** `attend listen status`



## `attend listen beat`

Mark a listening session alive: touch its heartbeat, deliver nothing. After a /clear it first takes over the enrollment the session's earlier id made, as the drain does. The attend mod runs it on a timer, through turns too, so the session stays within the liveness grace while it lives. A session that does not listen is written nothing

**Usage:** `attend listen beat [OPTIONS]`

###### **Options:**

* `--mod <VERSION>` — The attend mod's own version. The mod sends it with every beat; attend records it for the session, and the Stop-hook drain compares it with the installed mod to tell a session that loaded an older one. A mod that sends none is compared by its start time



## `attend listen finish`

Mark this session finished: peers see it finished, and attend refuses new messages to it. Its queued messages stay queued, and its Claude Code process runs on. `listen on` clears the mark; a beat leaves it

**Usage:** `attend listen finish`



## `attend message`

Send, reply to, read, count and delete messages, and tombstone a session (default: inbox)

**Usage:** `attend message [OPTIONS]
       message <COMMAND>`

###### **Subcommands:**

* `send` — Send a message: a direct message with --to, a channel post with --channel, else to #open
* `reply` — Reply to the most recent peer message: a direct message to its sender, or in its named channel
* `inbox` — Read pending messages from peers
* `unread` — Count this session's unread messages by room, marking none of them read
* `awaiting` — List the direct messages this session sent that no recipient has answered yet, marking nothing read
* `lapse` — Mark this session's pending event-lane notices read at a session boundary, delivering none of them
* `cleanup` — Reap signal files whose owning project is gone, and prune empty project dirs. Lifetime is bound to project liveness (ADR-136); the one removal by age is of a message `attend message delete` marked, 7 days after the mark, or for a message in a project tray once that project has started 3 Claude Code sessions since, each /clear counting as one (ADR-404 D3e).
* `delete` — Mark for deletion the messages no live session still has to read
* `tombstone` — Tombstone a session that has exited, so every reader skips its messages

###### **Options:**

* `--limit <LIMIT>` — Max messages per page, newest first

  Default value: `25`
* `--page <PAGE>` — Page number; 1 = newest. Higher numbers walk back into history

  Default value: `1`
* `--before <TS>` — Cursor: only show messages older than this unix timestamp
* `--drain` — Atomically deliver pending messages and record their consumption. The Stop-hook fast path: a silent no-op under an unresolved identity or for a session not listening (#720), and in hook format for a session the attend mod did not enroll (`attend listen on --host mod`). On a cold start (no seen-set) it delivers addressed mail and fresh messages and counts the older backlog it does not show. A system signal past its expiry is recorded read and never delivered
* `--wake` — With --drain: deliver and mark nothing unless a pending message would wake this session, one addressed to it, sent to its project, a reply to a message it sent, or any post in one of its named channels; then deliver all that is pending. Other `#open` posts and attend's own event-lane notices alone wait for the next turn. Once the session's prompt cache has lapsed (its tier's TTL less two minutes after its last turn, or a transcript not readable in five seconds), nothing wakes it: the drain delivers nothing and reports `cache_lapsed`. A session with no turn yet has no cache to lose and is woken. The attend mod polls with it while idle
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
* `--drain` — Atomically deliver pending messages and record their consumption. The Stop-hook fast path: a silent no-op under an unresolved identity or for a session not listening (#720), and in hook format for a session the attend mod did not enroll (`attend listen on --host mod`). On a cold start (no seen-set) it delivers addressed mail and fresh messages and counts the older backlog it does not show. A system signal past its expiry is recorded read and never delivered
* `--wake` — With --drain: deliver and mark nothing unless a pending message would wake this session, one addressed to it, sent to its project, a reply to a message it sent, or any post in one of its named channels; then deliver all that is pending. Other `#open` posts and attend's own event-lane notices alone wait for the next turn. Once the session's prompt cache has lapsed (its tier's TTL less two minutes after its last turn, or a transcript not readable in five seconds), nothing wakes it: the drain delivers nothing and reports `cache_lapsed`. A session with no turn yet has no cache to lose and is woken. The attend mod polls with it while idle
* `--format <FMT>` — Output format for --drain: `plain` (human/agent readable), `hook` (Claude Code Stop-hook JSON; reads the hook's stdin payload for `stop_hook_active`) or `json` (the same as --json)

  Default value: `plain`
* `--json` — Print the listing, the message, or with --drain the drained messages, held-back count, note and name, as one JSON document



## `attend message unread`

Count this session's unread messages by room, marking none of them read

**Usage:** `attend message unread [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend message awaiting`

List the direct messages this session sent that no recipient has answered yet, marking nothing read

A message waits when it went to a peer session or to a person's member id and no later message from a recipient threads under it or is addressed to this session. Posts to #open and to named channels never wait, and nor does a message to a session that has exited or finished. Each line gives the handle, the recipient, the age and the first words, and marks a message to a person as the operator's. --json prints {count, operator, items}.

**Usage:** `attend message awaiting [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend message lapse`

Mark this session's pending event-lane notices read at a session boundary, delivering none of them

**Usage:** `attend message lapse --events`

###### **Options:**

* `--events` — Lapse the pending event-lane notices: git, processes, presence, context, the update check and any other sensor's. A peer's message stays queued. The attend mod runs this at a compaction or a /clear



## `attend message cleanup`

Reap signal files whose owning project is gone, and prune empty project dirs. Lifetime is bound to project liveness (ADR-136); the one removal by age is of a message `attend message delete` marked, 7 days after the mark, or for a message in a project tray once that project has started 3 Claude Code sessions since, each /clear counting as one (ADR-404 D3e).

**Usage:** `attend message cleanup [OPTIONS]`

###### **Options:**

* `-n`, `--dry-run` — List what would be removed without deleting
* `--all` — Remove every signal file regardless of project liveness



## `attend message delete`

Mark for deletion the messages no live session still has to read

A message is marked when it is in the scope, older than --older-than, older than 90 seconds, and no session that may still read the bus would be delivered it: a session keeps what `attend message unread` counts for it, its cold start included, and one this host cannot judge keeps its mail while its last heartbeat is under 7 days old. A person in attend-chat holds nothing. Every reader skips a marked message, and `attend message cleanup` removes it 7 days after the mark, or for a message in a project tray once that project has started 3 Claude Code sessions since, each /clear counting as one (ADR-404 D3e). --undo takes a delete back, and its messages are delivered again, while cleanup has not yet removed them.

**Usage:** `attend message delete [OPTIONS]`

###### **Options:**

* `--open` — Delete in #open
* `--channel <NAME>` — Delete in this channel
* `--dm <SESSION-ID>` — Delete in the direct-message thread between you and this session: what it sent you and what you sent it. You are the session this runs in, under its current id and the ids it had before /clear, or outside a session the person at this terminal
* `--everywhere` — Delete in every room: #open, each channel, each project tray and each person's tray
* `--from <SESSION-ID>` — Only messages this session sent
* `--older-than <WHEN>` — How old a message must be: `current` (before the earliest start of any live session), `today` (before local midnight), `all`, a duration such as 90m, 2h30m, 3d or 1w, or a local date YYYY-MM-DD

  Default value: `all`
* `-n`, `--dry-run` — Report what would be marked and mark nothing
* `--undo <BATCH>` — Remove the marks of the newest delete, or of the batch named
* `--json` — Print the result as one JSON document



## `attend message tombstone`

Tombstone a session that has exited, so every reader skips its messages

The drain, the mod, `attend message unread` and `attend message inbox` all skip what a tombstoned session sent, and the drain records it read, up to when it was tombstoned; what the session sends later, back under --resume, shows. Only an agent's session that this host saw exit is tombstoned: a person's id, a session that may still read the bus, an id such a session had before /clear and this session's own are refused. Its messages stay on disk; `attend message delete --everywhere --from <id>` marks them for deletion. --ended tombstones, as one batch, every agent with a message still shown on the bus that the same rule allows, and --undo with no session takes the newest batch back.

**Usage:** `attend message tombstone [OPTIONS] [SESSION-ID]`

###### **Arguments:**

* `<SESSION-ID>` — The session to tombstone

###### **Options:**

* `--ended` — Tombstone, as one batch, every agent session with a message still shown on the bus that the single tombstone's rule allows
* `-n`, `--dry-run` — With --ended, print what would be tombstoned and change nothing
* `--undo <SESSION-ID|BATCH>` — Remove the tombstone of a session, named here or as the session id. With no session, remove the tombstones the newest --ended batch wrote, or those of the batch named here; a tombstone written after the batch stays. What a drain recorded read while a tombstone stood stays read
* `--list` — List the tombstoned sessions
* `--json` — Print the result as one JSON document



## `attend channel`

Join, leave, list and manage channels and scenes (default: list)

**Usage:** `attend channel [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `list` — List every channel, marking the ones this session is in
* `join` — Join a channel (creates it if absent)
* `add` — Add a session to a channel; it is told, and handed the recent history
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



## `attend channel add`

Add a session to a channel; it is told, and handed the recent history

The session is enrolled with no answer from it (ADR-404 D1), told by a direct message to its project, and handed the newest 50 of the channel's messages to read, as a join answering an invitation is. Any invitation it had there is spent.

**Usage:** `attend channel add [OPTIONS] <MEMBER> [CHANNEL]`

###### **Arguments:**

* `<MEMBER>` — The session to add: its name or id
* `<CHANNEL>` — Channel name (with or without the # prefix); with none, the one channel this session is in

###### **Options:**

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
* `status` — Show every running sensor host with the session it serves, signals, and channel state

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

Show every running sensor host with the session it serves, signals, and channel state

**Usage:** `attend peer status [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend keepwarm`

Keep the prompt cache warm across idle stretches (ADR-182; default: status)

**Usage:** `attend keepwarm [COMMAND]`

###### **Subcommands:**

* `on` — Arm a window: one wake at 50 idle minutes keeps the cache read, not re-written
* `off` — Disarm and forget the window
* `status` — Cache state, context size, cold price, window left, and this session's cold writes. With --json, the stop exit alone, read from the ledger with no transcript read: `exit` is its state (armed or finished), wrap id, pings, rent and purchase, or null with none
* `check` — Run the keepwarm judgment once: print the `keepwarm:` wake line when one is due, else nothing (exit 0 either way). The attend mod polls it
* `reset` — The operator prompted: the keepalive and relay counts start again, and a sent wrap and a stop are cleared. The attend mod runs it for each prompt typed in the session
* `wrapped` — Record where the keepalive policy's wrap reply went, for the status card. The attend mod runs it once the wrap turn ends
* `compacted` — Record the compaction after the wrap, for the status card and so nothing more follows. The attend mod runs it once the compaction ends
* `resumed` — Record that a relay's resume turn went in: the relay count goes up by one and the keepalive count starts again. The attend mod runs it once the compaction after a relay wrap has run
* `judged` — Record the judge's verdict for a boundary wrap, its answer on the handoff and the agent's own call. With --verdict stop it arms the exit: each idle tick then pings while something is pending or the handoff holds detail, up to the price of what replaces the session, and otherwise lets the cache lapse and marks the session finished. Refused, changing nothing, when .claude/handoff.wrap.md is missing or its first line does not carry wrap=<ID>
* `drained` — Record that relay's drain turn went in, which starts this generation's drain: no further drain turn is offered until a compaction, a resume or a fill under the drain point ends it. The attend mod runs it once the drain turn has entered
* `goal` — Record a `/goal` the attend mod saw typed: active, or none for `/goal clear`. The relay stops once a goal that was active has ended
* `stopped` — Record that a resumed turn ended with a `relay: stop <reason>` line, which stops the relay until the operator's next prompt
* `wrap-id` — Print the id of this session's last recorded wrap, or nothing and exit 1 when none is recorded. `ways hook pre-compact` reads it to tell whether .claude/handoff.wrap.md is fresh



## `attend keepwarm on`

Arm a window: one wake at 50 idle minutes keeps the cache read, not re-written

**Usage:** `attend keepwarm on [WINDOW]`

###### **Arguments:**

* `<WINDOW>` — Window such as 6h, 90m, or 2h30m (default 6h)



## `attend keepwarm off`

Disarm and forget the window

**Usage:** `attend keepwarm off`



## `attend keepwarm status`

Cache state, context size, cold price, window left, and this session's cold writes. With --json, the stop exit alone, read from the ledger with no transcript read: `exit` is its state (armed or finished), wrap id, pings, rent and purchase, or null with none

**Usage:** `attend keepwarm status [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend keepwarm check`

Run the keepwarm judgment once: print the `keepwarm:` wake line when one is due, else nothing (exit 0 either way). The attend mod polls it

**Usage:** `attend keepwarm check [OPTIONS]`

###### **Options:**

* `--policy` — Apply the continuity policy: from each turn's end, one wake the margin before the cache lapses, counted from the operator's last prompt and bounded by mods.attend.continuity, then one wrap turn; in relay mode, a compaction and a resume after it
* `--json` — Print the decision as one JSON object: action (none, keepalive, drain, wrap, compact or finish), text, next_at, count, max, wraps_at, reason, and exit for an armed or finished exit
* `--autocompact <AUTOCOMPACT>` — The autocompact gate as the host read it: on, off or unknown. The policy runs only with off; unknown counts as on

  Default value: `unknown`

  Possible values: `on`, `off`, `unknown`

* `--skills <SKILLS>` — The skills the session lists, comma-separated, as the host read them. The wrap and the compaction after it run only when mods.attend.continuity.wrap_skill names one of them. Absent, as from a host that predates the flag, the wrap is not held to a listing
* `--skills-unread` — The host could not read the session's skills: no wrap or compaction
* `--skills-partial` — The --skills listing was cut short by the host's budget: a skill missing from it is not held against the wrap
* `--fill <FILL>` — The context's tokens at a main-loop turn's end, for relay's fill path: with relay on, a fill past mods.attend.continuity.relay_at_pct of --window starts the drain with one drain turn, a fill past mods.attend.continuity.hard_at_pct sends a wrap, and a fill under the drain point ends the drain
* `--window <WINDOW>` — The model's whole context window, in tokens
* `--native-at <NATIVE_AT>` — Claude Code's own autocompact threshold, in tokens; with autocompact on, the hard line sits five points of the window under it where that is lower
* `--pending <COUNTS>` — What still runs, as the host counted it: agents=N,shells=N,watchers=N. While an exit is armed, these and the threads awaiting a reply keep the cache warm up to the price of what replaces the session. Shells and watchers left out are the ones the last Stop hook listed



## `attend keepwarm reset`

The operator prompted: the keepalive and relay counts start again, and a sent wrap and a stop are cleared. The attend mod runs it for each prompt typed in the session

**Usage:** `attend keepwarm reset [OPTIONS]`

###### **Options:**

* `--at <AT>` — When the operator prompted, in epoch seconds (default now). The mod passes a prompt it held while autocompact was on



## `attend keepwarm wrapped`

Record where the keepalive policy's wrap reply went, for the status card. The attend mod runs it once the wrap turn ends

**Usage:** `attend keepwarm wrapped [OPTIONS] <OUTCOME>`

###### **Arguments:**

* `<OUTCOME>` — transcript, file, sent, refused or unfinished

###### **Options:**

* `--detail <DETAIL>` — The file written, the recipient, or attend's reason for a refusal

  Default value: ``
* `--path <PATH>` — With --save: the wrap file to write the reply to, checked again under the ways state root's wrap/ directory
* `--save` — Write the reply read on stdin to --path (outcome file or refused), through a temp file renamed over it
* `--focus <FOCUS>` — The focus of the wrap's closing `/compact` line, kept for the compaction `after_wrap: compact` asks for after the wrap's keepalives
* `--marker <MARKER>` — The relay marker the reply ended with. With no relay wrap waiting, it records a `/wrap` run in relay mode



## `attend keepwarm compacted`

Record the compaction after the wrap, for the status card and so nothing more follows. The attend mod runs it once the compaction ends

**Usage:** `attend keepwarm compacted [OPTIONS]`

###### **Options:**

* `--failed <FAILED>` — Why the compaction failed; absent when it ran
* `--trigger <TRIGGER>` — boundary: the boundary tool started the compaction, and its resume counts toward mods.attend.continuity.max_relays. Refused with exit 1, recording nothing, while a relay wrap waits to be judged

  Possible values: `boundary`




## `attend keepwarm resumed`

Record that a relay's resume turn went in: the relay count goes up by one and the keepalive count starts again. The attend mod runs it once the compaction after a relay wrap has run

**Usage:** `attend keepwarm resumed [OPTIONS]`

###### **Options:**

* `--failed <FAILED>` — Why the resume did not go in, which stops the relay
* `--trigger <TRIGGER>` — boundary: the resume after a compaction the boundary tool started, which counts one relay like any other

  Possible values: `boundary`




## `attend keepwarm judged`

Record the judge's verdict for a boundary wrap, its answer on the handoff and the agent's own call. With --verdict stop it arms the exit: each idle tick then pings while something is pending or the handoff holds detail, up to the price of what replaces the session, and otherwise lets the cache lapse and marks the session finished. Refused, changing nothing, when .claude/handoff.wrap.md is missing or its first line does not carry wrap=<ID>

**Usage:** `attend keepwarm judged [OPTIONS] --wrap <ID> --verdict <VERDICT> --handoff <HANDOFF> --own <OWN>`

###### **Options:**

* `--wrap <ID>` — The boundary wrap's id, as the handoff's first line carries it
* `--verdict <VERDICT>` — The judge's verdict

  Possible values: `stop`, `compact`, `continue`, `doubt`

* `--handoff <HANDOFF>` — Whether a fresh start from the handoff replaces the session (sufficient) or getting it back takes a cold reload (holds-detail)

  Possible values: `sufficient`, `holds-detail`

* `--own <OWN>` — The working agent's own call, recorded beside the judge's

  Possible values: `stop`, `compact`, `continue`

* `--reason <REASON>` — The judge's one line of reason



## `attend keepwarm drained`

Record that relay's drain turn went in, which starts this generation's drain: no further drain turn is offered until a compaction, a resume or a fill under the drain point ends it. The attend mod runs it once the drain turn has entered

**Usage:** `attend keepwarm drained`



## `attend keepwarm goal`

Record a `/goal` the attend mod saw typed: active, or none for `/goal clear`. The relay stops once a goal that was active has ended

**Usage:** `attend keepwarm goal <STATE>`

###### **Arguments:**

* `<STATE>`

  Possible values: `active`, `none`




## `attend keepwarm stopped`

Record that a resumed turn ended with a `relay: stop <reason>` line, which stops the relay until the operator's next prompt

**Usage:** `attend keepwarm stopped [OPTIONS] <REASON>`

###### **Arguments:**

* `<REASON>`

  Possible values: `empty`, `decision`, `doubt`


###### **Options:**

* `--detail <DETAIL>` — The rest of the line: the agent's reason

  Default value: ``



## `attend keepwarm wrap-id`

Print the id of this session's last recorded wrap, or nothing and exit 1 when none is recorded. `ways hook pre-compact` reads it to tell whether .claude/handoff.wrap.md is fresh

**Usage:** `attend keepwarm wrap-id [OPTIONS]`

###### **Options:**

* `--record <ID>` — Record ID as this session's last wrap instead: the wrap skill names each wrap's id this way, so a handoff-mode wrap, which ends with no relay marker, is recorded too



## `attend chat`

Launch the interactive chat TUI (ADR-120)

**Usage:** `attend chat [PASSTHROUGH]...`

###### **Arguments:**

* `<PASSTHROUGH>` — Arguments passed through to the `attend-chat` binary



## `attend sensor`

List, poll, probe and test-fire sensors, and audit their permissions (default: list)

**Usage:** `attend sensor [COMMAND]`

###### **Subcommands:**

* `list` — List every sensor the files declare, with its kind, origin, file, source and state
* `poll` — Run one real poll of a sensor and deliver what its threshold lets through to the session
* `fire` — Send a synthetic result through a sensor's threshold and lane to the session, marked [test]
* `probe` — Call an mcp sensor's tool twice and record whether reading it changed it
* `permissions` — Compare each sensor's `requires:` against settings.json
* `host` — Run one session's sensors and write what they report to it as system signals



## `attend sensor list`

List every sensor the files declare, with its kind, origin, file, source and state

**Usage:** `attend sensor list [OPTIONS]`

###### **Options:**

* `--json` — Print the result as one JSON document



## `attend sensor poll`

Run one real poll of a sensor and deliver what its threshold lets through to the session

**Usage:** `attend sensor poll [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The sensor's name

###### **Options:**

* `--session <ID>` — The session whose store the poll reads and to which it delivers; unset, the session this shell runs in
* `--dry-run` — Stop before delivery
* `--json` — Print the result as one JSON document



## `attend sensor fire`

Send a synthetic result through a sensor's threshold and lane to the session, marked [test]

**Usage:** `attend sensor fire [OPTIONS] <TARGET>`

###### **Arguments:**

* `<TARGET>` — The sensor, or `<sensor>/<trigger>` for one trigger

###### **Options:**

* `--input <FILE|->` — The synthetic result: a file, or `-` for stdin. A command sensor with `output: json` takes one JSON poll result, which its state and triggers evaluate. Any other sensor takes a JSON list of `{magnitude, description}` or `magnitude|description` lines; with none, one observation at the sensor's threshold
* `--session <ID>` — The session whose store the fire reads and to which it delivers; unset, the session this shell runs in
* `--dry-run` — Stop before delivery
* `--commit` — Keep the trigger memory and declared state the fire leaves; a test fire writes neither without it
* `--json` — Print the result as one JSON document



## `attend sensor probe`

Call an mcp sensor's tool twice and record whether reading it changed it

The probe calls the tool twice in a row and checks that the second call returned every item of the first, by each trigger's key, or for state triggers the same state. It cannot see read state the service keeps, such as a message marked read, so it asks you at the terminal to confirm in another client that the items are untouched; with no terminal the record says they were not confirmed. From Claude Code, run it as `! attend sensor probe <name>`. It writes the record as `probe` in the sensor's user file, created with that field alone when the sensor has none. An mcp sensor runs only once its record holds: taken with its server's definition, tool and args, with the calls matching and confirmed.

**Usage:** `attend sensor probe [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The mcp sensor's name

###### **Options:**

* `--json` — Print the record as one JSON document



## `attend sensor permissions`

Compare each sensor's `requires:` against settings.json

**Usage:** `attend sensor permissions`



## `attend sensor host`

Run one session's sensors and write what they report to it as system signals

The attend mod starts one host for each listening session and stops it when the session stops listening. The host polls the sensors the files declare for --cwd; the peers sensor reports sessions starting and leaving but no messages. Each report the governor lets through becomes a signal from `system:<sensor>` addressed to --session, on the sensor's lane, with the expiry its trigger sets; the mod delivers it as it delivers a message. Each signal written prints one line on stdout, `signal <id>`, and nothing else does; logs go to stderr. A changed binary or sensor file re-executes the host in place.

**Usage:** `attend sensor host [OPTIONS] --session <SESSION> --cwd <CWD>`

###### **Options:**

* `--session <SESSION>` — The session the signals are addressed to
* `--cwd <CWD>` — The session's directory: the project the sensors watch, whose tray the signals go to
* `--moved-from <OLD_ID>` — The id the session had before /clear: its sensor stores and the memory of triggers kept for the session move to --session
* `--parent <PID>` — The process that started the host, Claude Code: the host exits once it is gone, even when it went before the host could look at its own parent



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

* `--apply` — Write the derived absolute_refractory and decay_per_minute to the user config, changing nothing else



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
