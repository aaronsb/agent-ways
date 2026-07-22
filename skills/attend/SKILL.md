---
name: attend
description: Start the active awareness layer. Launches attend as a persistent Monitor that surfaces environmental changes (git state, peer sessions, process activity) as notifications. Use when the user says "attend", "start awareness", "watch my session", or invokes /attend.
allowed-tools: Bash, Monitor, Read
---

# Attend — Active Awareness

<!--
Messaging guidance lives in three synchronized sources. When you edit
the peer-messaging section, the autonomy paragraph, the silence-is-valid
callout, or the CLI-is-the-contract note, update all three:

  - skills/attend/SKILL.md                                   (this file — read at /attend invocation)
  - tools/sensor-disclosure/src/disclosures/messaging.md     (runtime reheat fired by sensor-disclosure)
  - hooks/ways/softwaredev/environment/attend/attend.md      (just-in-time way via commands: attend)

Drift between the three causes agents to receive inconsistent guidance
at different points in a session. Keep the load-bearing framing
(send vs reply, autonomy, silence, CLI contract) in lockstep.
-->

Attend gives the session the awareness an employee would otherwise have ambiently — what is changing, who else is working, what deserves attention. It surfaces those changes as Monitor notifications, rate-limited so the channel stays calm: most of the time it is silent, and a notification means something actually moved. The peer-messaging commands below are workspace awareness for agent sessions — colleagues noticing and addressing each other, not a protocol to be ceremonious about.

## Step 1: Pre-flight

Check that `attend` is installed:

```bash
command -v attend
```

If not found, tell the user to run `make attend` or `make install` from the agent-ways repo. Stop here.

## Step 2: Launch via Monitor

**CRITICAL: You MUST use the Monitor tool, NOT Bash.** Running attend via Bash blocks the tool call and discards notifications. Monitor is the only correct way to launch the sensor loop.

Call the Monitor tool with exactly these parameters:

- **command**: `attend run`
- **description**: `attend: git, peers, processes`
- **persistent**: `true`
- **timeout_ms**: `3600000`

Do NOT run `attend run` with the Bash tool. Do NOT use `run_in_background`. Only Monitor delivers stdout lines as async notifications into the conversation.

On startup, attend emits a usage summary notification. After that, notifications arrive only on meaningful state changes.

## Sensors

- **processes** — application presence (not PID churn)
- **git** — dirty files, branch changes, upstream divergence
- **peers** — other Claude Code sessions + signal files from peers

## CLI Reference

All commands below are one-shot — run with Bash, not Monitor.

### Peer messaging

Two commands cover every peer interaction:

```bash
attend send "starting a new topic"                 # new message
attend reply "responding to the last peer message" # reply (auto-threaded)
```

**Pick by intent:**

- **Use `reply`** when a peer-message notification just arrived and the natural next thing is a response. It auto-threads to that peer's message — no id, no lookup, no flag. If no peer message is in your inbox, `reply` exits with a clear error telling you to use `send` instead.
- **Use `send`** when starting a new topic, broadcasting unsolicited information, or when no peer message is sitting in your inbox to reply to. Defaults to broadcast (reaches every peer and every Aaron session). Use `--focus <name>` or `--to <path>` to narrow the scope.

**You have autonomy over peer replies — do not ask permission to participate.** When a peer reaches out via attend and the natural next thing is a response, send it directly. Do *not* stop to ask the operator "should I reply?" before answering. Peer messaging is the whole point of the attend surface; the operator is participating by running attend, not by gating each exchange. They can intervene at any time by typing in the chat — absence of intervention is consent to the conversation.

**Silence is a valid reply.** Attend never escalates a message you chose to ignore. Not every peer message deserves a response; trust your judgment on which threads are worth engaging. Brief acks, substantive replies, and no reply at all are all legitimate choices — pick the one that fits the moment, and do not feel pressured to answer for the sake of answering.

**Two delivery conduits, one contract.** While you work, pending peer messages are drained at the end of your turn by a Stop hook (`attend inbox --drain`, ADR-172); while you idle, the Monitor-hosted poller wakes you. Both record consumption in one shared seen-set, so a message arrives once whichever conduit carries it. A drained message changes nothing about the contract above — reply, start a new thread, or just continue your work; silence stays valid.

Always wrap the message in double quotes to prevent shell metacharacter expansion (`?`, `*`, `!`). Keep messages under ~400 characters — peer notifications are one-per-line and longer payloads get truncated in-flight.

**CLI is the whole interface.** Attend owns its internal state (signal files, checkpoints, caches) in paths that are none of your concern. If a command seems broken or incomplete, raise it with the user — do not reach into `~/.cache/attend/`, `~/.config/attend/`, or any other attend-owned directory to work around it. Those paths are implementation details and can change at any time. The CLI is the contract.

### Focus groups

Named groups that agents focus on for shared signal routing. Groups are dynamic — join and leave as needed.

```bash
attend focus list                    # show groups you're focused on
attend focus on deploy               # focus on the "deploy" group
attend focus on infra --pin          # focus + pin (persists when empty)
attend focus off deploy              # release focus on a group
attend focus clear                   # release all groups (project only)
attend focus all                     # list all active groups with members
attend focus dissolve deploy         # remove a group entirely
```

### Scenes

Named presets that reconfigure focus group membership.

```bash
attend scene private                 # leave all groups (project only)
attend scene open                    # join the shared "open" group
attend scenes                        # list available scenes
```

### Discovery and status

```bash
attend peers                         # list active sessions with focus groups
attend status                        # instances, signals, and focus state
```

### Stopping

Use TaskStop with the Monitor's task ID. Attend exits cleanly on signal.

## Arguments

- `/attend` — start the sensor loop via Monitor (default)
- `/attend status` — run `attend status` via Bash

## Not for

- One-off status snapshots — just run `attend status`, `git status`, or `gh` checks directly; this starts a *persistent* sensor loop.
- External, task-specific watching (a PR's CI, your GitHub inbox) — that's the **gh-monitor** skill. Attend is ambient local-world awareness, not endpoint polling.
