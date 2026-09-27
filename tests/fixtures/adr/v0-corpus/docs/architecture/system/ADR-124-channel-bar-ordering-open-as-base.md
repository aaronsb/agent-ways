---
status: Accepted
date: 2026-04-16
deciders:
  - aaronsb
  - claude
related:
  - ADR-118
  - ADR-120
---

# ADR-124: TUI Legend Architecture — Base Channel, Liveness, and Ordering

## Context

The attend-chat TUI has two legend strips — a **channel bar** across
the top (focus groups) and an **agent legend** above the status row
(claudes and humans seen on the signal bus). Both are identity
surfaces: they're how the human locates the set of peers and channels
currently available to address.

Three problems have surfaced in use:

**1. `#open` is semantically special but visually equal.** ADR-118's
scenes define `open` as the well-known "everyone who wants shared
coordination" group. It's the canonical fallback that agents opt into
when they're *not* in a specialized focus. A new user looking at the
channel bar has no cue that `#open` is the place to drop a general
message — it's just another group in an alphabetical row.

**2. `_broadcast/` and `@open/` are distinct on disk but convey
overlapping intent.** `_broadcast/` is "reach everyone regardless of
group membership"; `@open/` is "the named group people gather in for
shared coordination". In practice they're used the same way, and the
dual affordance confuses both the TUI's mental model and the human
writing a message ("do I `#open ...` or just plain text?").

**3. No ordering rule beyond alphabetical.** As groups grow, the
channel bar becomes a noisy alphabetical strip with no affordance for
"active" vs. "idle" or "pinned" vs. "transient".

**4. The agent legend never forgets.** Its seed source is
`~/.claude/sessions/*.json` — a disk-persistent record that outlives
any given claude process. When a claude exits, its session file
lingers; the agent's nickname keeps appearing in the legend long
after the agent is gone. Observed concretely: stopping the claude in
`/home/aaron/temp` left `@Urban` in the legend indefinitely. The
legend misrepresents system state.

This ADR resolves all four. It's a single record because they share
one substrate (the two legends) and one framing (what the TUI
surfaces as the current coordination topology), so splitting them
would scatter related consequences across multiple ADRs without
making any of them clearer.

## Decision

### 1. `#open` is the base channel — always leftmost

The channel bar renders with a fixed first position reserved for the
base channel (`#open`). Every other discovered `@group` follows it,
in a defined order (see §3).

The base channel:

- Always appears, even when its signal dir is empty
- Never gets a removal affordance (can't `/dissolve #open`)
- Is the implicit target of plain-text messages (no sigil required)
- Is the implicit target of `attend send "msg"` with no flags

`#open` isn't "just another group" — it's the commons.

### 2. Resolve `_broadcast/` vs. `@open/`: `#open` becomes the canonical base

Rename the display layer: what today lives at `~/.cache/attend/signals/_broadcast/`
is rendered and addressed in the TUI as `#open`. This is a UX
rename, not a wire-format change — the directory on disk stays
`_broadcast/` so existing peers continue to receive messages there.
`@open/` the named group is folded into the same logical channel.

Two options for the folding:

**Option A — `_broadcast/` is canonical; `@open/` is removed.** The
scene named `open` updates to mean "subscribe to the base channel"
(which is automatic for everyone anyway). Any existing `@open/` dir
is migrated: messages move to `_broadcast/`, the dir is cleaned up
on next peer-sensor poll, and the scene preset is simplified.

- Pros: one dir, one concept. No ambiguity.
- Cons: breaks any hand-crafted scenes/flows that rely on `@open/`
  specifically. Migration script needed.

**Option B — both dirs stay, rendered as the same `#open` channel.**
The TUI merges signals from `_broadcast/` and `@open/` into a single
`#open` channel view. Writes from `#open ...` land in `_broadcast/`
(the canonical source of truth). `@open/` becomes deprecated but not
removed — signals there are still read, just not written to by new
senders.

- Pros: no migration. Backwards-compatible.
- Cons: two dirs, one channel — the ambiguity moves from the UX to
  the implementation. Drift risk over time.

**Recommendation: Option A.** Cleaner long-term, and the `@open`
scene preset has been around long enough to carry a one-shot migration
note in release notes. The `_broadcast/` directory name stays (on-disk
convention), but `@open/` goes away.

### 3. Channel ordering rule

Left-to-right order:

1. **`#open` (base)** — always first, never moved, never hidden.
2. **Everything else** — whatever order the underlying scan
   returns (today that's alphabetical by group name).

Originally this section prescribed bands for pinned / recent /
quiet. That ordering rule deferred — pin-order would require a
`_groups.yaml` schema change (pin timestamp), and recency requires
a per-group mtime index the scan doesn't track. The base-channel
leftmost is the one invariant this PR needs; richer ordering can
land in a follow-up once there's pressure for it.

### 4. Visual treatment

- Base channel `#open` renders with its hashed glyph + color same as
  any group, but with a bold weight so the eye registers its
  special role as the commons.
- Pin indicators and unread dots are deferred to a follow-up PR
  alongside the ordering rule — no value in half-implementing them
  without the band structure they're meant to cue.

### 5. Agent legend: three-state presence

The agent legend renders each claude in one of three states:

| State | Condition | Rendering |
|---|---|---|
| **Live** | session file exists AND PID is alive AND PID is a claude process | full-weight, normal identity color |
| **Declared (nobody home)** | session file exists BUT PID is dead | dimmed, same color/glyph, addressable |
| **Absent** | no session file | not shown |

**Declared agents stay addressable** — that's the point of dimming
rather than dropping. The signal bus is filesystem-mediated: a
message sent to a declared-but-dead claude writes to its cwd-encoded
inbox dir and sits there. The next time a claude starts in that cwd,
attend's backlog handling picks it up. Dimming signals "nobody's home
right now, but your message won't be lost."

This mirrors how a physical name card on an empty desk still tells
you where to leave a note.

Humans are **not** gated this way. Their presence in the legend is
keyed on having emitted a signal that's still in the TUI's buffer
(or, in a future PR, being derived from a human-membership key).
Humans are "ephemeral by message" — a human who hasn't typed in a
while fades via the buffer cap, not a liveness check.

#### Detection mechanism

Port the liveness idiom already used by `sensor-peers::pid_is_claude`
into `attend-chat::sessions`. Each render-time `discover()` call
filters session files by liveness before returning them to the
identity registry.

Concrete implementation:

- `/proc/<pid>` existence check on Linux (single `stat`, sub-
  microsecond).
- Fall back to `ps -p <pid> -o comm` on non-Linux or when `/proc` is
  unavailable.
- Match the parent process name against `claude` to exclude PIDs that
  happen to be reused by another program after the session file was
  written.

#### Refresh cadence

Per-render liveness checks over 20+ session files are cheap on Linux
but unnecessary. A 2-second TTL cache is plenty — the human won't
notice a two-second lag between a claude exiting and its chip fading
out, and the sub-second render work stays bounded.

#### Why not rely on the peer sensor

`sensor-peers` already does liveness. But it lives in the attend
process, not attend-chat; and ADR-118's data flow is filesystem-
mediated (everything goes through `~/.cache/attend/signals/`).
There's no pre-computed "live peers" file attend-chat can read. We
replicate the detection inline rather than introducing a new shared
state file — the check is small enough that duplication is cheaper
than a new coordination surface.

### 6. Channel-bar: same three-state rule

The channel bar applies the same presence logic as §5 at the group
level:

| State | Condition | Rendering |
|---|---|---|
| **Active** | at least one live member (per §5) | full-weight |
| **Declared (nobody home)** | membership non-empty but zero live members | dimmed, addressable |
| **Empty pinned** | zero members, `pinned: true` | full-weight (the pin overrides quiet) |

Same reasoning as §5: a declared-but-inactive channel still has its
directory on disk, and a message sent there sits until the next
claude to join picks it up. Dimming, not hiding.

`#open` (the base) is **never** dimmed or hidden — it's the commons
whether or not anyone is listening. A message to `#open` always has
someone eventually.

## Consequences

### Positive

- New users see `#open` as the obvious commons without reading docs.
- Ordering has a principled rule, not just alpha — the bar
  self-organizes around what the human cares about.
- The `_broadcast/@open` dual affordance disappears; there's one
  default channel, not two overlapping ones.

### Negative

- Migration needed for any existing `@open/` dir (Option A). One-
  shot at first run; script is trivial.
- `attend` CLI output (`peers`, `focus all`) needs the same rename
  to match the TUI — otherwise the CLI shows `open` as a group and
  the TUI shows it as the base, which is worse than the current
  state.
- Release note: "`@open` is now the base channel; messages sent
  plain-text reach it; the old `@open` group no longer exists."

### Neutral

- `_broadcast/` on disk stays. We're renaming the display layer, not
  breaking the wire format.

## Implementation notes

The work naturally splits into two PRs (post-slash-commands):

**PR A — Base channel.** §1 through §4.

1. attend-chat: `#open` pinned leftmost in `group_legend_row`; sort
   remaining by (pinned, recent, alpha).
2. attend-chat: `_broadcast/` reads surface as `#open`; plain-text
   writes route to `_broadcast/` (already do); `#open ...` writes
   also route to `_broadcast/` (new — today they'd route to
   `@open/`).
3. attend: `groups.rs` removes special-case handling of "open" as a
   scene name; scenes.yaml docs update; one-shot migration of any
   lingering `@open/` dir on `attend run` startup.
4. attend: `peers`, `focus all` output uses `#open` as the base
   channel label; drop any display of `_broadcast/` as a thing the
   user addresses directly.

**PR B — Liveness.** §5 and §6.

1. attend-chat: new `liveness` module with cached
   `pid_is_claude(pid)` (2-second TTL). Port sensor-peers' detection
   pattern.
2. attend-chat: `sessions::discover` reads PID from each session
   file and tags entries with a `Live | Declared` presence state
   rather than filtering. The registry carries the state through to
   render.
3. `KnownIdentity` gains a `presence` field; renderers consult it to
   pick full-weight vs. dimmed color.
4. attend-chat: `groups::scan` cross-references each group's
   membership list against the live-agent set; groups with zero
   live members render dimmed (unless pinned or `#open`).
5. Tests: synthesize a session file with a dead PID, assert legend
   renders it as declared/dimmed (not dropped); assert routing to
   `@DeclaredName body` still writes to disk successfully.

## Open questions

Settled during PR A:

- `#open` is display-only — `_broadcast/` stays as the on-disk name.
- `open` is reserved in `validate_group_name` so nobody can
  create an `@open/` group that would shadow the base.

Still open (for PR B and beyond):

- Do we need a `/unpin` slash command pair to `/pin`, or does
  `attend focus unpin <name>` suffice? (Slash commands PR will
  decide.)
- Liveness cache TTL — is 2 seconds the right number? Too short and
  we burn syscalls on a busy signal stream; too long and a stopped
  agent lingers visibly. 2s is a guess based on "humans don't
  notice", but we should measure and tune after PR B lands.
- Should `attend peers` CLI output apply the same liveness filter as
  the TUI? If so, the detection logic wants to live in a shared
  crate (or in agent-identity), not duplicated. Leaving undecided
  until the PR B implementation — two duplicates is tolerable,
  three is extract-pressure.
- Richer channel ordering (pinned band, recent-activity band) —
  deferred per §3, revisit when there's concrete pressure.
