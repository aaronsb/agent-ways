---
status: Deprecated
date: 2026-02-21
deciders:
  - aaronsb
  - claude
related: []
---

# ADR-101: Wormhole relay protocol for cross-instance agent communication

## Status: Deprecated

**Deprecated 2026-02-26** after experimental validation. The manifest protocol works mechanically but wormhole's one-shot, role-asymmetric design makes it fundamentally unsuited for conversations. See [Experimental Results](#experimental-results-2026-02-26) below. For agent-to-agent chat, use a persistent bidirectional transport (IRC, Matrix, or similar). Wormhole remains the right tool for one-shot file transfers via the `/wormhole` skill.

## Context

Claude Code teams currently operate within a single machine — a lead agent spawns teammates, they share a task list, and coordinate via SendMessage. There is no mechanism for two independent Claude instances on different machines (different users, different accounts, different networks) to exchange files or data.

magic-wormhole provides secure, NAT-transparent, credential-free file transfer using short codes. We already have a `/wormhole` skill for interactive and automated transfers. The natural next step is a protocol layer that allows two Claude teams to sustain a multi-turn conversation over wormhole, without requiring a human to relay codes after the initial handshake.

The core challenge: wormhole codes are single-use. Once a transfer completes, the code is consumed. A stateless transport needs a convention to become stateful.

## Decision

Define a **manifest-based relay protocol** where each wormhole transfer includes a manifest file alongside the payload. The manifest carries the state needed to continue the conversation.

### Manifest Structure

```json
{
  "protocol": "claude-wormhole-relay",
  "version": "0.1",
  "turn": 1,
  "direction": "a-to-b",
  "codes": ["99-beta-canyon", "99-gamma-delta", "99-echo-falcon"],
  "continuation": "99-zulu-renew",
  "payload": ["report.md"],
  "message": "Here's the analysis you requested."
}
```

| Field | Purpose |
|-------|---------|
| `protocol` | Identifies this as a relay manifest |
| `version` | Protocol version for forward compatibility |
| `turn` | Monotonic turn counter |
| `direction` | Which side is sending this turn |
| `codes` | Pre-agreed codes for upcoming turns (consumed in order) |
| `continuation` | Special code that signals "this turn's payload is a fresh batch of codes" |
| `payload` | List of filenames included in this transfer |
| `message` | Free-text message between agents (context, instructions, requests) |

### Turn Lifecycle

1. **Initiation**: Human A tells Claude A to send a file to another machine. Claude A generates a manifest with N pre-generated codes and sends it via the first code (which the human relays to side B).
2. **Steady state**: Each side consumes the next code from the manifest to send their turn. Both sides know the full code sequence, so no human relay is needed.
3. **Renewal**: When the `continuation` code is reached, that turn's payload is a new manifest with fresh codes — extending the conversation.
4. **Termination**: A manifest with empty `codes` and no `continuation` signals the conversation is complete.

### Code Generation

Codes follow the pattern `<session-number>-<word>-<word>` (or longer). Claude generates the word pairs. Both sides receive the same code list via the manifest, so no shared seed or deterministic generation is required — the manifest IS the shared state.

### Team Integration

A dedicated **comms agent** teammate handles the relay:
- Watches for incoming turns (runs `wormhole receive` with the next expected code)
- Delivers received payloads and messages to the team lead
- Sends outgoing payloads when the lead requests a transfer
- Manages manifest state (current turn, remaining codes, renewal)

### Error Recovery

- **Failed transfer**: Retry the same code. Wormhole codes remain valid until successfully used.
- **Missed turn**: The receiving side can keep listening on the expected code indefinitely (with a configurable timeout).
- **Code exhaustion without renewal**: The comms agent notifies the lead that the channel is closing. A human must broker a new initial code to restart.

## Consequences

### Positive

- Two Claude instances on separate machines can exchange files and messages without shared credentials, SSH keys, or network access
- NAT/firewall transparent — both sides only need outbound internet
- No persistent infrastructure — no server to maintain, no accounts to manage
- Each side retains full autonomy — separate teams, separate humans, separate contexts
- The manifest pattern is extensible (add fields for encryption, compression, routing)

### Negative

- Latency per turn — each exchange requires a full wormhole handshake through the relay server
- Not suitable for high-frequency or real-time communication
- Single point of failure in the wormhole relay server (transit.magic-wormhole.io)
- Code list is transmitted in plaintext within the manifest — if intercepted, future turns are compromised
- Requires magic-wormhole installed on both machines

### Neutral

- The protocol is transport-agnostic in principle — the manifest pattern could work over other transports (email, shared storage, etc.) but this ADR focuses on wormhole
- Human supervision remains at the edges — each side's human can inspect manifests and payloads
- This does not define authentication between agents — both sides trust whoever holds the code

## Alternatives Considered

- **SSH/SCP between machines**: Requires pre-shared credentials, network access, and firewall configuration. Higher throughput but much higher setup cost. Better for recurring transfers between known hosts.
- **Cloud storage (S3, GCS) as intermediary**: Requires accounts on a shared service, credentials management, and cleanup. More infrastructure than the problem warrants.
- **Email-based exchange**: Async but requires email configuration, attachment size limits, and parsing complexity.
- **Custom relay server**: Maximum control but requires hosting, maintenance, and security hardening. Overkill for ad-hoc agent communication.
- **Pre-shared deterministic code generation (shared seed)**: Both sides generate codes from a seed without manifests. Simpler but fragile — if turns desync, the protocol breaks with no recovery path. The manifest approach is more resilient because state is explicit.

## Related Patterns: Ralph Loops

A ralph loop is a self-referential agent feedback cycle where an agent's output becomes its own input across iterations. Combining a ralph loop with this relay protocol creates an interesting (and potentially hazardous) topology: two independent Claude instances, each running their own ralph loop, exchanging intermediate state via wormhole manifests.

This could enable:
- **Collaborative iteration**: Agent A refines a document, sends it to Agent B for a different perspective, receives it back, continues refining — each side's ralph loop incorporating the other's feedback
- **Distributed analysis**: Each side processes a portion of a problem, exchanges findings, and iterates toward convergence

The risks are obvious:
- **Runaway amplification**: Two feedback loops feeding each other can diverge rapidly — each side responding to the other's responses with increasing elaboration and no natural stopping point
- **Resource exhaustion**: Without explicit turn limits or convergence criteria, the loop runs until context windows fill, API budgets drain, or a human intervenes
- **Semantic drift**: Successive rounds of interpretation and reinterpretation can drift far from the original intent

Any implementation combining ralph loops with the relay protocol MUST include hard bounds: maximum turn count, convergence detection (output similarity between rounds), and mandatory human checkpoints. The continuation mechanism in the manifest provides a natural rate limiter — code exhaustion forces a pause unless explicitly renewed.

## Experimental Results (2026-02-26)

Two Claude Code instances tested the manifest protocol over 10 pre-agreed codes with a UUID-tagged naming scheme (`N-439351ef-word-word`).

### What Worked

- **Initial handshake**: Human-relayed random codes work reliably for bootstrapping
- **Manifest delivery**: JSON manifest transferred cleanly, both sides parsed it
- **Sequenced turns**: When the human explicitly said "side B receive, now side A send," transfers succeeded every time
- **6 of 10 turns completed** successfully, including 2 out-of-band resyncs

### What Failed

- **Role collisions**: 3 of 10 turns failed with `ServerError: crowded` — both sides attempted the same role (both receiving or both sending) on the same code simultaneously
- **Burned codes**: Collisions permanently consume the code. Unlike TCP, there is no retry — the channel number is gone
- **Human sequencing required**: Despite pre-agreed sender/receiver assignments per turn, the human had to manually sequence each exchange. The "autonomous after initial handshake" goal was not achieved
- **No automatic recovery**: When codes burned, the only recovery path was out-of-band fallback codes (side B generated random codes and the human relayed them)

### Root Cause

Wormhole's PAKE handshake is **role-asymmetric**: one side must be the sender and the other the receiver. If both connect as the same role, the relay server returns `crowded` and the code is consumed. This is by design — wormhole assumes a human is coordinating both ends in real-time for a single transfer.

The manifest protocol tried to pre-assign roles, but both Claude instances execute asynchronously. Without a synchronization mechanism to guarantee receiver-before-sender ordering, collisions are inevitable. The protocol is essentially **UDP with no retransmit and destructive packet loss**.

### Conclusion

Wormhole is the wrong transport for conversations. It was designed for one-shot file drops, and it excels at that. Forcing statefulness onto a stateless, single-use protocol creates fragility that no amount of manifest engineering can fix.

For cross-instance agent chat, use a transport designed for persistent bidirectional messaging:

| Transport | E2E Encrypted | Bidirectional | Persistent | No Accounts | Setup Cost |
|-----------|:---:|:---:|:---:|:---:|---|
| **IRC** (self-hosted ircd) | TLS | Yes | Yes | Yes | Minimal — one process |
| **Matrix** (via matrix-commander) | Yes (Olm) | Yes | Yes | No | Moderate |
| **Tailscale + socat** | Yes (WireGuard) | Yes | Yes | No | Low if already using Tailscale |

IRC with a local ircd is the simplest option: zero accounts, zero credentials, one package install, both sides join a channel and talk. The channel handles ordering, buffering, and fan-out — all the problems the manifest protocol tried to solve.

**Wormhole's role going forward**: one-shot file transfers between machines via the `/wormhole` skill. No conversation protocol.
