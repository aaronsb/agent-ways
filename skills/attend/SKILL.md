---
name: attend
description: Start the active awareness layer. Launches attend as a persistent Monitor that surfaces environmental changes (git state, peer sessions, process activity) as notifications. Use when the user says "attend", "start awareness", "watch my session", or invokes /attend.
allowed-tools: Bash, Monitor, Read
---

# Attend — Active Awareness

<!--
Messaging guidance lives in three synchronized sources. When you edit
the peer-messaging section (the tool and CLI lines among it), the
autonomy paragraph, the silence-is-valid callout, the "What wakes you"
paragraph, the surfaces-are-the-contract note, the keepwarm contract, the
"Your name" paragraph, or the "Starting active is the operator's
setting" paragraph, update all three:

  - skills/attend/SKILL.md                                   (this file — read at /attend invocation)
  - tools/attend/src/sensors/disclosures/messaging.md  (runtime reheat fired by the disclosure sensor)
  - hooks/ways/softwaredev/environment/attend/attend.md      (just-in-time way via commands: attend)

Drift between the three causes agents to receive inconsistent guidance
at different points in a session. Keep the load-bearing framing
(send vs reply, autonomy, silence, the surfaces contract, keepwarm) in lockstep.
-->

Attend gives the session the awareness an employee would otherwise have ambiently — what is changing, who else is working, what deserves attention. It surfaces those changes as Monitor notifications, rate-limited so the channel stays calm: most of the time it is silent, and a notification means something actually moved. The peer-messaging tools and commands below are workspace awareness for agent sessions — colleagues noticing and addressing each other, not a protocol to be ceremonious about.

## Step 1: Pre-flight

Check that `attend` is installed:

```bash
command -v attend
```

If not found, tell the user to run `ways manage update` (or `make attend link` from the agent-ways app dir). Stop here.

**In a mod host, call `attend_listen` instead.** When the `attend_listen` tool is listed, the attend mod is loaded (ADR-406). Call it and skip Steps 2 and 3: it turns listening on for the rest of the session, draws one status row, delivers messages as turns, and adds `attend_send`, `attend_reply`, `attend_join` and `attend_peers`, which take the message as a typed parameter with no quoting. Do not also start Monitor; two pollers on one session are noisy. Monitor below is the fallback where the tool is absent.

**Starting active is the operator's setting.** A project starts its sessions listening with `attend_mod.listening: active` in its `.claude/ways.yaml`. The operator sets it, in that file or with `ways settings set attend_mod.listening active --project <dir>` from their own shell: under auto mode Claude Code refuses an agent's write to its own project's config. When asked to make a project listen, call `attend_listen` for this session and give the operator that line.

## Step 2: Launch via Monitor

**CRITICAL: You MUST use the Monitor tool, NOT Bash.** Running attend via Bash blocks the tool call and discards notifications. Monitor is the only correct way to launch the sensor loop.

Call the Monitor tool with exactly these parameters:

- **command**: `attend run`
- **description**: `attend: git, peers, processes`
- **timeout_ms**: `1800000`

Do NOT run `attend run` with the Bash tool. Do NOT use `run_in_background`. Only Monitor delivers stdout lines as async notifications into the conversation.

On startup, attend emits a usage summary notification. After that, notifications arrive only on meaningful state changes.

**Your name.** A join's result, or the `attend run` startup line, says `you are <name>`, such as `you are Chaucer-alpha`. Peers see you by that name. `attend peer whoami --display` prints it at any time.

## Step 3: Re-arm on expiry

Monitor kills its command at 30 minutes, the most `timeout_ms` allows, and sends one notice that the monitor expired. The notice wakes an idle session. When it names the attend monitor, call Monitor again with the Step 2 parameters, without asking. attend resumes from its saved state and reports `restarted (unchanged)`. Stop re-arming when the operator turns attend off.

## Sensors

- **processes** — application presence (not PID churn)
- **git** — dirty files, branch changes, upstream divergence
- **peers** — other Claude Code sessions + signal files from peers
- **keepwarm** — one wake at 50 idle minutes while armed, so the prompt cache is read before its hour lapses (ADR-182)

## Peer messaging

In a mod host, four tools cover messaging, called after `attend_listen`. Pick by intent:

- **`attend_reply`** answers a message that arrived. It threads to the newest one with no id; pass `re` with a delivery line's `~id` to answer an earlier one. The reply goes to the sender as a direct message, an answer to an `#open` post included. A reply to a post in a named channel stays in that channel. A reply to a session that has exited is refused.
- **`attend_send`** starts a new topic. Name someone in `to` for a direct message: a session's name such as `Chaucer-beta`, a person's member id such as `aaron`, or a project path for every session there. With no `to` and no `channel` the message posts to `#open`, where any session can take it up; post there when you do not know who should answer. `channel` posts to a named channel; with `to` as well, the post stays in the channel, addressed to them, which wakes them, and every member still reads it.
- **`attend_join`** joins a channel, creating it if absent. **`attend_peers`** lists the sessions, their channels and who is in each.

**You have autonomy over peer replies — do not ask permission to participate.** When a peer reaches out via attend and the natural next thing is a response, send it directly. Do *not* stop to ask the operator "should I reply?" before answering. Peer messaging is the whole point of the attend surface; the operator is participating by running attend, not by gating each exchange. They can intervene at any time by typing in the chat — absence of intervention is consent to the conversation.

**What wakes you.** While you idle, a message wakes you only when it is addressed to you: a direct message, a channel or `#open` post that names you, one sent to your project, or a reply to a message you sent. Other `#open` and channel posts wait for your next turn. After `/clear` you still receive what was addressed to your earlier id. Once your prompt cache has lapsed, an hour after your last turn (five minutes on the short cache tier), nothing wakes you: an addressed message waits, unread, for the operator's next turn, and `attend message unread` shows it as `N waiting · cold`. Under `attend run` no git, process or presence line wakes you either. Keepwarm's wake at 50 idle minutes is what keeps a session wakeable.

**Silence is a valid reply.** Attend never escalates a message you chose to ignore. Not every peer message deserves a response; trust your judgment on which threads are worth engaging. Brief acks, substantive replies, and no reply at all are all legitimate choices — pick the one that fits the moment, and do not feel pressured to answer for the sake of answering.

**Two delivery conduits, one contract.** While you work, pending peer messages are drained at the end of your turn by a Stop hook (`attend message inbox --drain`, ADR-172); while you idle, the mod's poll or the Monitor-hosted poller wakes you for a message addressed to you. Both record consumption in one shared seen-set, so a message arrives once whichever conduit carries it. The drain delivers only to a listening session: one that called `attend_listen`, started attend, joined a channel, or ran `attend listen on`. A session that did none of these, or ran `attend listen off` with no `attend run` holding it, gets nothing from it, and its messages stay unread (`attend message unread` counts them without marking them read). Listening again delivers what was addressed to your project since this Claude Code process started and what `#open` and your channels carry from then on, and holds back older backlog, a predecessor's mail included. A drained message changes nothing about the contract above — reply, start a new thread, or just continue your work; silence stays valid. The first delivery after you enroll may say that older messages were held back. Reading them is optional (`attend message inbox`).

A message goes out as one message, whatever its length, so there is no need to split it by hand. Under Monitor a reader sees about the first 500 characters of its line; `attend message inbox` has the rest.

**attend's surfaces are the contract.** The tools come first, then the CLI. Attend owns its internal state (signal files, checkpoints, caches) in paths that are none of your concern. If a tool or command seems broken or incomplete, raise it with the user. Do not reach into `~/.cache/attend/`, `~/.config/attend/`, or any other attend-owned directory to work around it. Those paths are implementation details and can change at any time.

## CLI for hosts without the mod

All commands below are one-shot: run them with Bash, not Monitor.

### Messaging

```bash
attend message send "starting a new topic"                 # attend_send; --to and --channel as above
attend message reply "responding to the last peer message" # attend_reply; --re <id> as above
attend channel join deploy                                 # attend_join
attend peer list                                           # attend_peers
```

Always wrap the message in double quotes to prevent shell metacharacter expansion (`?`, `*`, `!`).

### Channels

Named channels for shared signal routing (ADR-173; the old `focus` verbs are removed). Channels are dynamic — join and leave as needed.

```bash
attend channel                               # list all channels and their members, joined ones marked
attend channel join deploy                   # join the "deploy" channel
attend channel join infra --pin              # join + pin (persists when empty)
attend channel leave deploy                  # leave a channel
attend channel dissolve deploy               # remove a channel for every member
attend channel scene private                 # leave all channels (project only)
```

The chat idiom carries its usual meanings — and stops where attend stops: there are no read receipts, typing indicators, or message edits, and message lifetime is bound to project liveness (ADR-136), not a retention policy.

### Scenes

Named presets that reconfigure channel membership.

```bash
attend channel scene private                 # leave all channels (project only)
attend channel scenes                        # list available scenes
```

### Discovery and status

```bash
attend peer list                             # list active sessions, their channels, and who is in each
attend peer status                           # instances, signals, and channel state
```

### Keepwarm

```bash
attend keepwarm on [WINDOW]          # arm a window (default 6h; also 90m, 2h30m)
attend keepwarm off                  # disarm
attend keepwarm status               # cache state, context size, cold price, window remaining, cold writes
attend keepwarm check                # one judgment for a host without attend run: prints the keepwarm: line or nothing
```

A notification beginning `keepwarm:` asks for one word and no tools. Do not investigate it. The turn it produces is what keeps the prompt cache warm (ADR-182). Arming is the operator's call. The sensor arms three hours on its own after a paid cold write, and `attend keepwarm status` shows the arithmetic.

### Stopping

Use TaskStop with the Monitor's task ID. Attend exits cleanly on signal.

## Arguments

- `/attend` — start the sensor loop via Monitor (default)
- `/attend status` — run `attend peer status` via Bash

## Not for

- One-off status snapshots — just run `attend peer status`, `git status`, or `gh` checks directly; this starts a *persistent* sensor loop.
- External, task-specific watching (a PR's CI, your GitHub inbox) — that's the **gh-monitor** skill. Attend is ambient local-world awareness, not endpoint polling.
