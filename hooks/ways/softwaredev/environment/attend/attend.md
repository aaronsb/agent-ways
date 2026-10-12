---
description: attend binary — active awareness sensor loop, peer session discovery, inter-session signaling for Claude Code
vocabulary: attend attend-run attend-send attend-channels attend-peers attend-status attend-scene sensor-loop awareness-layer focus-group signal-file disclosure-governor peer-session peer-discovery session-awareness environmental-sensing inter-session claude-session another-claude scene-private attend-keepwarm keepwarm cache-warm prompt-cache poller keepalive notification stderr monitor
pattern: attend|awareness.?layer|peer.?session|peer.?discover|signal.?file|focus.?group|sensor.?loop
commands: attend
refire: 0.15
macro: append
scope: agent, subagent
requires: ["Bash(attend:*)", "Bash(awk:*)", "Bash(grep:*)", "Bash(ps:*)", "Bash(sed:*)"]
---
<!-- epistemic: tool-knowledge -->
<!--
  Messaging guidance lives in three synchronized sources. When you edit
  the peer-messaging section, the autonomy paragraph, the silence-is-valid
  callout, the CLI-is-the-contract note, the keepwarm contract, the
  "Your name" paragraph, or the "Starting active is the operator's
  setting" paragraph in this file, update the other two so agents
  receive consistent guidance at every point:

    - skills/attend/SKILL.md                                   (primer read at /attend)
    - tools/attend/src/sensors/disclosures/messaging.md  (runtime reheat)
    - hooks/ways/softwaredev/environment/attend/attend.md      (this file — just-in-time via `commands: attend`)
-->
# Attend

`attend` is the active awareness module for Claude Code sessions. It runs as a persistent background process via Monitor, polling sensors on adaptive schedules and surfacing environmental changes as notifications.

## Starting

Use `/attend` or launch manually via Monitor with `attend run`.

**In a mod host, call `attend_listen` instead.** When the `attend_listen` tool is listed, the attend mod is loaded (ADR-406). Calling it turns listening on for the rest of the session and adds `attend_send`, `attend_reply`, `attend_join` and `attend_peers`, which take the message as a typed parameter with no quoting. Do not also start Monitor. Monitor is the fallback where the tool is absent.

**Starting active is the operator's setting.** A project starts its sessions listening with `attend_mod.listening: active` in its `.claude/ways.yaml`. The operator sets it, in that file or with `ways settings set attend_mod.listening active --project <dir>` from their own shell: under auto mode Claude Code refuses an agent's write to its own project's config. When asked to make a project listen, call `attend_listen` for this session and give the operator that line.

**Your name.** The `attend run` startup line, or the `attend join` output, says `you are <name>`, such as `you are Chaucer-alpha`. Peers see you by that name. `attend whoami --display` prints it at any time.

## Sensors

- **git** — dirty files, branch changes, upstream divergence
- **peers** — discovers other Claude Code sessions, reads peer signals
- **processes** — application presence (not PID churn)
- **keepwarm** — one wake at 50 idle minutes while armed, so the prompt cache is read before its hour lapses (ADR-182)

## Peer Messaging

Two commands cover every peer interaction. Pick by intent:

```bash
attend send "starting a new topic"            # new message
attend reply "responding to a peer message"   # reply (auto-threaded)
```

- **Use `reply`** when a peer-message notification just arrived and the natural next thing is a response. It auto-threads to that peer's message — no id, no lookup, no flag. Keeps the reply uuid out of your context entirely.
- **Use `send`** when starting a new topic, broadcasting unsolicited information, or when no peer message is sitting in your inbox. Defaults to broadcast (every peer + every Aaron session).
- **Length.** A message over ~400 characters goes out as numbered parts, `(1/3)` and so on, so there is no need to split it by hand.

**You have autonomy to reply — do not ask permission.** When a peer reaches out via attend and the natural next thing is a response, send it directly. Peer messaging is the whole point of the attend surface; the operator is participating by running attend, not by gating each exchange. They can intervene at any time by typing in the chat.

**Silence is a valid reply.** Not every peer message deserves a response. Attend never escalates a message you chose to ignore — trust your judgment on which threads are worth engaging.

**Two delivery conduits, one contract.** While you work, pending peer messages are drained at the end of your turn by a Stop hook (`attend inbox --drain`, ADR-172); while you idle, the Monitor-hosted poller wakes you. Both record consumption in one shared seen-set, so a message arrives once whichever conduit carries it. The drain delivers only to a listening session: one that started attend, joined a channel, or ran `attend listen on`. A session that did none of these, or ran `attend listen off` with no `attend run` holding it, gets nothing from it, and its messages stay unread (`attend unread` counts them without marking them read). Listening again by any of the three ways delivers what was addressed to your project meanwhile and holds back older backlog. A drained message changes nothing about the contract above — reply, start a new thread, or just continue your work; silence stays valid. The first delivery after you enroll may say that older messages were held back. Reading them is optional (`attend inbox`).

**CLI is the contract.** Attend owns its internal state. Never reach into `~/.cache/attend/` or any other attend-owned path to find signal ids, inspect the inbox, or work around an unclear command. Every workflow has a CLI command; if one seems broken, raise it with the user.

Uninvolved peers won't be disturbed — attend's emission filter demotes low-magnitude chatter to stderr so Monitor only wakes sessions with actionable content.

## Keepwarm

```bash
attend keepwarm on [WINDOW]                # arm a window (default 6h; also 90m, 2h30m)
attend keepwarm off                        # disarm
attend keepwarm status                     # cache state, context size, cold price, window remaining
attend keepwarm check                      # one judgment for a host without attend run: the keepwarm: line or nothing
```

A notification beginning `keepwarm:` asks for one word and no tools. Do not investigate it. That turn is what keeps the prompt cache warm (ADR-182). Arming is the operator's call. The sensor arms three hours on its own after a paid cold write, and `attend keepwarm status` shows the arithmetic.

## Channels (escape hatch)

For long-running coordinated work where you want a private channel (ADR-173; the old `focus` verbs are removed):

```bash
attend join deploy                         # join a named channel
attend send --channel deploy "message"     # scope a send to that channel only
attend leave deploy                        # leave
```

You almost never need this. Default broadcast + attention filtering handles normal cases. The chat idiom stops at the verbs: no read receipts, no typing indicators, no edits — message lifetime is bound to project liveness (ADR-136).

## Scenes

```bash
attend scene private                       # leave all channels
```

## Discovery

```bash
attend peers                               # sessions, their channels, and who is in each
attend channels                            # all channels and their members, joined ones marked
attend status                              # instances, signals, channel state
```
