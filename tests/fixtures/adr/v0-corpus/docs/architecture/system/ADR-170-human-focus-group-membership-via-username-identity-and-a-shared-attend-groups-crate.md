---
status: Accepted
date: 2026-07-21
deciders:
  - aaronsb
  - claude
related:
  - ADR-118
  - ADR-120
  - ADR-124
  - ADR-129
---

# ADR-170: Human focus-group membership via username identity and a shared attend-groups crate

## Context

attend-chat (ADR-120) is the human's seat on the signal bus, but focus-group
membership (ADR-118) is claude-only: `_groups.yaml` members are Claude Code
session UUIDs, liveness is judged against the per-session heartbeat sidecar
(ADR-129), and the only writer of the yaml is the `attend` binary acting for a
claude session. The chat TUI's slash registry advertises `/join` and `/leave`
as planned, and three deferred pieces all converge on them:

1. **Humans have no membership key.** The chat user has no session UUID and no
   heartbeat, so there is nothing to write into a `members:` list — and a
   heartbeat-less member would immediately count as dead to `live_peer_count`
   and be swept by `cleanup_stale`.
2. **attend-chat has no write path.** Its `groups` module is an explicitly
   read-only byte-mirror of `attend::groups` — two hand-rolled YAML parsers
   kept in sync by golden tests, with the module docs deferring "a shared I/O
   layer" to the `/join` write path.
3. **Membership is invisible on human chips.** The chip renderer looks up
   group glyphs by session UUID, which humans don't have.

Separately, agent-side send validation (`attend send --focus`) counts a member
as live only if it appears in `PeerSensor::live_session_ids` — a claude-process
scan. A human member would never count, so a group whose only live member is a
human would reject agent sends with "no live peers".

Channel hygiene has a matching blind spot. The chat's channel bar renders
every `@name/` directory on disk, but attend's `cleanup_stale` iterates
`_groups.yaml` entries — an **orphan dir** (no yaml entry at all) is invisible
to cleanup and renders in the bar forever. In practice stale test channels
accumulate and the human has no way to remove them from the surface where
they cause the confusion.

## Decision

**A human's membership identity is their sanitized username** (e.g. `aaron`,
via `agent_identity::sanitize_id_component($USER)`). One entry per human,
regardless of terminal or cwd — deliberately matching the chat registry's
existing human-dedupe rule (the same person in two terminals is one identity,
where two claudes in two cwds are two).

**Human liveness rides the existing heartbeat sidecar.** While attend-chat
runs, it touches `heartbeat/<username>` on its existing 5-second refresh tick.
No new liveness mechanism and no yaml format change: `members:` lists now mix
session UUIDs and usernames, and every consumer already judges members by
heartbeat freshness (`live_peer_count`, `cleanup_stale`), so human members age
out after `DEFAULT_GRACE` exactly like abandoned claude sessions. attend-chat
does not clear the heartbeat on exit — a second chat instance for the same
user may still be running, and the 90s grace self-corrects.

**Group I/O is extracted into a shared `attend-groups` workspace crate**,
following the pattern set by `attend-heartbeat` and `attend-instances`. The
crate owns `GroupEntry`, `Groups` (join/leave/pin/dissolve/cleanup and the
yaml read-modify-write), `validate_group_name`, and the parse/serialize pair.
`attend` keeps its attend-specific pieces (the ADR-124 `@open` migration);
attend-chat's mirror parser and the cross-crate golden-drift tests collapse
into the shared crate's own tests.

**agent-side liveness validation gains a heartbeat fallback.** `attend send
--focus` counts a member live if it is a live claude session *or* its
heartbeat is fresh — making a human-only group a valid send target. This
deliberately loosens the claude-side check too: a session whose claude died
but whose attend still heartbeats now counts as live, consistent with the
crate's `member_alive` philosophy (no attend, no mesh participation — and the
converse). The chat-side gate mirrors the agent side's self-exclusion: the
sender's own membership (now heartbeat-backed for humans) never counts toward
"live peers", or a solo human's send would validate against their own
heartbeat and sit unread.

**The TUI wires the commands.** `SlashOutcome` grows effect variants; dispatch
stays IO-free (parse + validate only) and the key handlers execute effects:

- `/join <group>` / `/leave <group>` call the shared `Groups` with the
  username identity; `/clear` empties the message buffer (display-only).
- `/dissolve <group>` removes a channel entirely — yaml entry and `@dir`,
  including orphan dirs the yaml doesn't know about. Chat-side it carries a
  live-member guard the CLI's `attend focus dissolve` does not: a group with
  heartbeat-fresh members refuses to dissolve, since from the TUI this is a
  hygiene action and should not yank a channel out from under active peers.
- `/channels` lists every channel with `live/total` member counts in the
  status row (IRC `/list` shaped) — `0/0 live` is the tell for `/dissolve`
  fodder.
- `/purge [group]` deletes a channel's on-disk signal history (default: the
  base channel's `_broadcast/`), keeping a heartbeat-grace tail so nothing a
  peer's sensor may be mid-scan on is deleted. This is a **deliberate operator
  override of ADR-136's durability default** — that ADR forbids *automatic*
  age-reaping; an explicit human purge of a named channel is a different act,
  and the sensors already tolerate signal deletion (the project-liveness
  cleanup and `attend cleanup --nuke-all` predate this). Membership and the
  channel itself survive a purge, unlike `/dissolve`. When a per-session
  consumption checkpoint exists (the drain-verb work being specced against
  ADR-136), purge should tighten to also refuse signals unconsumed by a live
  session; the grace tail then becomes the fallback for non-live consumers.

Human chips look up group glyphs by username so membership renders the same
as it does for claudes.

**`cleanup_stale` gains an orphan-dir sweep.** After the member pass, `@name/`
dirs with no yaml entry and an mtime older than the heartbeat grace window are
removed — closing the accumulation path so stale channels stop outliving
their groups. Three guards defend concurrent joins, since `create_dir_all` on
a pre-existing orphan dir does not refresh its mtime: `join` saves its yaml
entry *before* touching the dir, the sweep re-reads the yaml immediately
before each removal, and the chat's group resolver falls back to the yaml
entry when the dir is missing (signal writers re-create their target dir), so
even the residual sub-millisecond race self-heals. Reserved names are never
swept — a lingering `@open/` belongs to the ADR-124 migration, which moves
its signals into `_broadcast/` rather than deleting them.

## Consequences

### Positive

- Humans become first-class group members with zero wire-format change —
  every existing consumer works unchanged because liveness was already
  heartbeat-shaped, not UUID-shaped.
- The two hand-rolled YAML parsers and their golden-mirror maintenance burden
  are replaced by one implementation with one test suite.
- Group lifecycle rules (empty-unpinned GC, stale sweeps) apply to humans for
  free; an abandoned chat session cannot pin a group open forever.
- `attend send --focus` stops lying about human-only groups.
- Stale channels become manageable from the surface where they confuse:
  `/channels` shows which are dead, `/dissolve` removes them, and the orphan
  sweep stops the accumulation at the source.

### Negative

- Usernames and session UUIDs share one namespace in `members:`. Collision is
  implausible (UUIDs vs short login names) but the list is no longer
  homogeneous, and tooling that assumed "member = session UUID" must not
  reappear.
- A username heartbeat conflates "some chat instance is running" with "this
  chat instance is running" — two instances for one user share a heartbeat by
  design, so per-instance presence for humans is out of scope.
- One more workspace crate to version and build.

### Neutral

- attend-chat gains its first write responsibilities on the signal base
  (yaml read-modify-write and heartbeat touches), inheriting the same
  last-writer-wins races attend sessions already tolerate. The extraction
  hardened the write itself to keep that risk model honest: per-writer unique
  tmp names mean concurrent savers can no longer publish a torn hybrid file —
  the worst case is genuinely last-writer-wins, not corruption.
- The chat watcher still renders all groups regardless of membership;
  subscribed-group *filtering* remains future ADR-120 work — after this ADR,
  joining changes presence and addressability, not what the human sees.

## Alternatives Considered

- **Prefixed human member ids (`human:aaron`)** — would make the member kind
  explicit, but changes the yaml contract in both parsers, requires
  special-casing in every liveness check, and buys nothing the heartbeat
  doesn't already provide. Rejected for format churn without benefit.
- **Per-instance human identity (`aaron@kitty`)** — mirrors the wire `from`
  field, but splits one person into N members, contradicts the chat
  registry's human-dedupe rule, and makes `/leave` ambiguous about which
  instance leaves. Rejected: membership is about the person, not the seat.
- **attend-chat depends on the attend binary crate as a library** — avoids a
  new crate but drags sensor/CLI machinery into the TUI build and inverts the
  dependency taxonomy the small shared crates established. Rejected.
- **Keep duplicating: hand-roll a second yaml writer in attend-chat** — the
  read-side mirror is already a documented maintenance hazard; a write-side
  mirror doubles the drift surface on the file both binaries mutate.
  Rejected; the golden tests exist precisely because this was fragile.
- **Synthetic session files for humans** — writing fake
  `~/.claude/sessions/*.json` entries so humans traverse the claude discovery
  path. Rejected: pollutes Claude Code-owned state and misrepresents what a
  session is.
