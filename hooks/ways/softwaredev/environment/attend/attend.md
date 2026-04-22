---
description: attend binary — active awareness sensor loop, peer session discovery, inter-session signaling for Claude Code
vocabulary: attend attend-run attend-send attend-focus attend-peers attend-status attend-scene sensor-loop awareness-layer focus-group signal-file disclosure-governor peer-session peer-discovery session-awareness environmental-sensing inter-session claude-session another-claude scene-private scene-open
embed_threshold: 0.28
pattern: attend|awareness.?layer|peer.?session|peer.?discover|signal.?file|focus.?group|sensor.?loop
commands: attend
refire: 0.15
macro: append
scope: agent, subagent
requires: ["Bash(attend:*)", "Bash(grep:*)", "Bash(ps:*)", "Bash(sed:*)"]
---
<!-- epistemic: tool-knowledge -->
<!--
  Messaging guidance lives in three synchronized sources. When you edit
  the peer-messaging section, the autonomy paragraph, the silence-is-valid
  callout, or the CLI-is-the-contract note in this file, update the
  other two so agents receive consistent guidance at every point:

    - skills/attend/SKILL.md                                   (primer read at /attend)
    - tools/sensor-disclosure/src/disclosures/messaging.md     (runtime reheat)
    - hooks/ways/softwaredev/environment/attend/attend.md      (this file — just-in-time via `commands: attend`)
-->
# Attend

`attend` is the active awareness module for Claude Code sessions. It runs as a persistent background process via Monitor, polling sensors on adaptive schedules and surfacing environmental changes as notifications.

## Starting

Use `/attend` or launch manually via Monitor with `attend run`.

## Sensors

- **git** — dirty files, branch changes, upstream divergence
- **peers** — discovers other Claude Code sessions, reads peer signals
- **processes** — application presence (not PID churn)

## Peer Messaging

Two commands cover every peer interaction. Pick by intent:

```bash
attend send "starting a new topic"            # new message
attend reply "responding to a peer message"   # reply (auto-threaded)
```

- **Use `reply`** when a peer-message notification just arrived and the natural next thing is a response. It auto-threads to that peer's message — no id, no lookup, no flag. Keeps the reply uuid out of your context entirely.
- **Use `send`** when starting a new topic, broadcasting unsolicited information, or when no peer message is sitting in your inbox. Defaults to broadcast (every peer + every Aaron session).

**You have autonomy to reply — do not ask permission.** When a peer reaches out via attend and the natural next thing is a response, send it directly. Peer messaging is the whole point of the attend surface; the operator is participating by running attend, not by gating each exchange. They can intervene at any time by typing in the chat.

**Silence is a valid reply.** Not every peer message deserves a response. Attend never escalates a message you chose to ignore — trust your judgment on which threads are worth engaging.

**CLI is the contract.** Attend owns its internal state. Never reach into `~/.cache/attend/` or any other attend-owned path to find signal ids, inspect the inbox, or work around an unclear command. Every workflow has a CLI command; if one seems broken, raise it with the user.

Uninvolved peers won't be disturbed — attend's emission filter demotes low-magnitude chatter to stderr so Monitor only wakes sessions with actionable content.

## Focus Groups (escape hatch)

For long-running coordinated work where you want a private channel, focus groups still exist:

```bash
attend focus on deploy                     # join a named group
attend send --focus deploy "message"       # scope a send to that group only
attend focus off deploy                    # leave
```

You almost never need this. Default broadcast + attention filtering handles normal cases.

## Scenes

```bash
attend scene private                       # leave all groups
attend scene open                          # join shared "open" group
```

## Discovery

```bash
attend peers                               # sessions with focus groups
attend status                              # instances, signals, focus state
```
