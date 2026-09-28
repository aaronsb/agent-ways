---
status: Deprecated
date: 2026-02-26
deciders:
  - aaronsb
  - claude
related:
  - ADR-101
---

# ADR-102: IRC-based local agent communication

## Context

ADR-101 proposed a manifest-based relay protocol over magic-wormhole for cross-instance agent communication. Experimental testing (2026-02-26) revealed fundamental limitations: wormhole codes are single-use and role-asymmetric (one sender, one receiver). When both sides race to the relay server, codes are permanently consumed by collisions. The protocol achieved 6/10 successful turns with 3 collisions requiring out-of-band resync — essentially UDP with destructive packet loss.

The core need remains: two Claude Code instances on the same machine need a way to exchange messages in real time. The transport must be persistent, bidirectional, ordered, and simple enough that Claude can operate it with basic file and shell tools.

## Decision

Use IRC for agent-to-agent communication, with `miniircd` (a single-file Python 3 IRC server) for the server and `ii` (suckless filesystem-based IRC client) for the client. The human relays connection details (host, port, channel) between instances. All communication flows over IRC.

### Architecture

```
Claude-5d00b2  →  ii (FIFO/file)  →  miniircd (localhost:PORT)  ←  ii (FIFO/file)  ←  Claude-144dc4
   (~/.claude)                           #relay                                          (~/temp)
```

### Identity: Hash-Based Nicks

Each instance derives a deterministic nick from its working directory using a sha256 hash:

```
/home/aaron/.claude → claude-5d00b2
/home/aaron/temp    → claude-144dc4
```

This replaces hardcoded `claude-a`/`claude-b` naming. The hash is stable across restarts, unique per directory, and fits IRC nick limits (16 chars). The ii base directory uses the same slug: `/tmp/irc-chat-<hash>/`.

Implementation: `~/.claude/hooks/irc-lib.sh` provides `irc_nick_from_dir()` and `irc_slug_from_dir()`.

### Why This Works

- **ii is filesystem-based**: messages are regular files (`out`) and FIFOs (`in`). Claude reads and writes them with standard tools — no IRC library needed.
- **miniircd is zero-config**: a single Python 3 file, starts with one command, no config files, no accounts.
- **IRC handles the hard parts**: message ordering, buffering, fan-out, presence detection — all the problems the wormhole manifest protocol tried to solve manually.

### Bootstrapping

1. Host starts miniircd on a random high port, connects with ii (hash-derived nick), joins `#relay`
2. Human relays host, port, and channel to the other instance (3 values, no wormhole needed)
3. Joiner connects with ii (its own hash-derived nick), joins `#relay`
4. Both sides chat via wrapper scripts (`irc-send.sh`, `irc-read.sh`)

Wormhole is not used for local bootstrapping — the human relay is simpler and more reliable for 3 values on the same machine. Wormhole remains available for remote bootstrapping (delivering connection details to a real IRC server across machines).

### Ambient Monitoring

A `UserPromptSubmit` hook (`~/.claude/hooks/irc-check.sh`) provides tick-based message delivery:

- **High-water mark**: tracks last-read line in `.hwm` file, only surfaces new messages
- **Notification tiers**: direct mentions show the message, ≤3 new messages inline, >3 as a badge count
- **Self-filtering**: own messages excluded to avoid echo
- **Join/part detection**: connection events surfaced as one-liners

Time is tick-based: each Claude Code API round-trip is an epoch. The hook fires on each user prompt, not on wall-clock time.

### Summary Nudge

A tick counter in `irc-check.sh` nudges the agent to post a recap every ~15 ticks using a triple-nudge pattern: ticks 15, 16, 17 each deliver an escalating prompt, then the counter resets for another 15-tick quiet period. Each nudge steers toward brevity — a sentence or two, not a dissertation. Only fires when other users are present in the channel — inactive IRC never triggers nudges.

### Compaction-Safe Session State

Context compaction loses IRC message history. To bridge this gap:

- `irc-check.sh` writes `.irc-has-history` when messages from others are received
- `irc-session.sh` (SessionStart:compact/resume hook) checks this flag and emits a breadcrumb: "N messages received before compaction — run `irc-read.sh` to catch up"
- `irc-read.sh` clears the flag on read

### Hook Wiring

The following hooks are wired in `settings.json`:

- **UserPromptSubmit + Stop**: `irc-check.sh` — tick-based message delivery on every turn
- **SessionStart (compact/resume)**: `irc-session.sh` — compaction breadcrumb
- **Bash allow**: `irc-send.sh *` and `irc-read.sh` — permission-free execution

### Wrapper Scripts

All IRC I/O goes through allowlisted wrapper scripts to avoid permission prompts and path issues (`#` in channel names triggers Claude Code's shell parser):

- `~/.claude/hooks/irc-send.sh <message>` — send to `#relay`
- `~/.claude/hooks/irc-read.sh [N]` — read last N messages

Scripts auto-detect the active `/tmp/irc-chat-*` connection via `irc-lib.sh`.

### Scope

Localhost only for now. Both Claude instances must be on the same machine. Cross-network communication is a future extension (Tailscale, public IRC, Matrix).

## Consequences

### Positive

- Persistent bidirectional channel — no code burning, no timing races
- Claude operates IRC through filesystem I/O — no special libraries or raw socket handling
- Zero infrastructure beyond a single Python process
- Trivially extensible — more agents join the same channel, add more channels for topics
- Ambient monitoring via hooks — messages appear in context automatically without manual polling
- Hash-based identity — deterministic, stable across restarts, no coordination needed

### Negative

- Requires `ii` installed (`pacman -S ii` on Arch, build from source elsewhere)
- Bundles miniircd as a vendored file in the skill directory
- Localhost-only limits cross-machine use cases
- No encryption (acceptable for local-only; would need TLS for network use)

### Neutral

- miniircd is GPL-2.0 licensed — vendoring the single file is fine for personal tooling
- The `/wormhole` skill remains available for one-shot file transfers and potential remote IRC bootstrapping
- IRC skills are split into three commands: `/irc-host` (start server), `/irc-chat` (join existing), `/irc-teardown` (cleanup)
- ADR-101's manifest protocol is deprecated but preserved as documentation of what was tried and why it failed
- Wrapper scripts and permission allowlisting are implementation details that may evolve as Claude Code's permission model changes

## Future: Channel Topology at Scale

With 2 agents, a single `#relay` channel is sufficient. At 3+ agents, noise becomes a design question.

**Options considered:**

- **Single channel (current)**: Everyone sees everything. Noise is bounded by tick rate, not message rate — each agent only processes messages on their turn. May work longer than expected.
- **Topic channels**: `#frontend`, `#backend`, `#deployment`. Focused but reintroduces the human-as-router problem IRC was built to eliminate — someone has to decide who goes where.
- **Hub and spoke**: `#relay` stays as the commons (everyone subscribes, always). Topic channels are additive, not replacements. Cross-cutting context flows through the hub; focused discussion flows through spokes. Nobody leaves the common channel, so no agent gets excluded.

Hub and spoke is the likely answer when the time comes. The third-wheel problem — an agent stranded on a dead topic channel while the real conversation happens elsewhere — is solved by the invariant that `#relay` is never abandoned. miniircd already supports multiple channels; no infrastructure change needed.

**Not building this yet.** The tick-based temporal model and summary nudge already compress signal. The real noise threshold is probably higher than it feels because agents read batched messages per-tick, not a real-time stream. Build when `#relay` actually gets noisy, not before.

## Alternatives Considered

- **Wormhole manifest protocol (ADR-101)**: Tested and deprecated. One-shot codes with role-asymmetric handshakes are structurally unsuited for conversations. See ADR-101 experimental results.
- **Matrix (via matrix-commander)**: E2E encrypted, persistent, bidirectional. Requires accounts and a homeserver — too much infrastructure for localhost same-machine chat.
- **Named pipes (FIFOs) directly**: Simplest possible approach but no message ordering, no buffering, no presence detection. Would need to reimplement what IRC provides for free.
- **Tailscale + socat**: Great for cross-network but requires Tailscale auth on both machines. Overkill for localhost.
- **UnrealIRCd / ngircd**: Production IRC servers with mandatory configuration. miniircd's zero-config single-file design is better for ephemeral use.
