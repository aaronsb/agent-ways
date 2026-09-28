---
status: Accepted
date: 2026-07-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-118
  - ADR-124
  - ADR-136
  - ADR-170
  - ADR-172
---

# ADR-173: Chat-idiom convergence for the attend command surfaces

## Context

Attend has two command surfaces that grew separate vocabularies for the
same objects. The agent CLI speaks an attention metaphor (`focus on
deploy`, `focus off`, `focus clear`, `send --focus`); the operator TUI
speaks chat idiom (`/join #deploy`, `/leave`, `/channels`, `#name`
syntax). ADR-170 already mixes the registers in one sentence — "humans
join focus groups" — and every doc that touches messaging has to teach
the mapping.

The divergence has a measurable asymmetry in who pays for it. An
operator learns the TUI once. An agent session learns its surface
**every session**, through disclosure: the skill primer, the runtime
reheat, and the just-in-time way exist substantially because the
bespoke verbs are not guessable. Meanwhile the parts of the surface
that align with the chat corpus every model is trained on — `send`,
`reply`, `@name`, `#open`, `inbox`, `/purge` — need no teaching at
all; agents use them correctly on first contact. Bespoke vocabulary is
a recurring disclosure tax; corpus-aligned vocabulary is free.

Two concrete defects sharpened the question. `clear` is a cross-surface
homonym: `attend focus clear` leaves all groups while `/clear` wipes
the transcript — a user of one surface guesses wrong on the other. And
the surfaces' *capabilities* have drifted independently of their
*vocabulary*: operator discovery (`/peers`, `/whois`) is registered but
unimplemented, while the agent has had `peers`/`whoami` from the start.

## Decision

**Converge both surfaces on the chat idiom. Where an agent-facing
surface has a saturated training-corpus convention, prefer it over
bespoke vocabulary — the model's prior is free disclosure; a bespoke
term is a tax paid every session.**

Concretely:

1. **The shared noun is "channel"** — the thing `#name` syntax already
   implies. "Focus group" disappears from user-facing vocabulary; the
   `@name/` directory layout and `_groups.yaml` wire format are
   unchanged (storage is not surface).

2. **Agents adopt the chat verbs as primary.** The mapping:

   | Concept | Operator (attend-chat) | Agent today | Agent after |
   |---|---|---|---|
   | Enter a channel | `/join <#g>` | `focus on <g>` | `join <g>` |
   | Leave a channel | `/leave <#g>` | `focus off <g>` | `leave <g>` |
   | List channels | `/channels` | `focus list` / `focus all` | `channels` |
   | Leave all | — | `focus clear` | `scene private` |
   | Destroy a channel | `/dissolve` | `focus dissolve` | `dissolve <g>` |
   | Scope a send | `#g` prefix | `send --focus <g>` | `send --channel <g>` |
   | Directed send | `@name` | `send --to <path>` | unchanged (`--to`) |
   | New message / reply | type / — | `send` / `reply` | unchanged |

3. **`focus` survives as a deprecated alias** (CLI-is-contract,
   ADR-124): every `focus` invocation keeps working, maps to the new
   verb, and prints a one-line deprecation note to stderr. No script
   breaks; no flag-day.

4. **The `clear` homonym is resolved by removal, not renaming.** The
   agent-side leave-all folds into `scene private` (its existing
   duplicate); `/clear` keeps the universal clear-the-screen meaning —
   which is also the chat prior.

5. **Vocabulary converges; capabilities do not.** The asymmetries are
   design, and this ADR re-affirms them: `/purge` remains
   operator-only (the ADR-136 durability override is a human power
   tool); `run`, `inbox --drain`, `tune`, `sensors`, `config`,
   `permissions` remain agent/session infrastructure. Discovery parity
   is closed in the operator's favor: `/peers` and `/whois` graduate
   from Planned to implemented.

6. **The idiom's borrowed expectations are met or explicitly ended.**
   Adopting chat vocabulary imports chat priors: a DM exists (`send
   --to`), presence exists (`peers`), threading exists (`reply`).
   Where the analogy stops — no read receipts, no typing indicators,
   no message editing, durability-by-liveness instead of retention
   policy — the three synchronized messaging docs say so in one
   sentence rather than leaving the prior to guess.

## Consequences

### Positive

- Zero-shot guessability: an out-of-the-box agent's first attempt
  (`attend join deploy`) is correct, before any disclosure fires.
- The disclosure budget spent teaching `focus on/off` every session is
  reclaimed for guidance that actually needs it.
- One register across both surfaces ends the "humans join focus
  groups" split — docs, ways, and reheat text all speak channel.
- The `clear` homonym — the sharpest cross-surface hazard — is gone.
- Operator discovery reaches parity (`/peers`, `/whois`).

### Negative

- Alias maintenance: `focus` must keep working indefinitely under
  CLI-is-contract, so the CLI carries two spellings of every channel
  verb (one deprecated) plus tests for both.
- The three synchronized messaging docs, the attend way, the skill,
  and the reheat disclosure all need coordinated rewording in one PR —
  the ADR-136 lockstep rule applies in full.
- Borrowed expectations are a standing documentation duty: every new
  chat-prior feature request must be either met or explicitly ended.

### Neutral

- Storage and wire formats are untouched — `@name/` dirs,
  `_groups.yaml`, signal files, seen-set keys all keep their shapes;
  this is a surface decision, not a data migration.
- The adjacent presentation-consistency backlog rides the same
  convention but ships separately: plain render for injected text
  (#388), the cell datetime line and cross-surface timestamps (#389),
  path-based attachments (#390).
- `scene` survives unchanged as the agent's attention-preset layer —
  it has no chat-idiom collision and its semantics (reconfigure many
  channels at once) have no single-channel analog.

## Alternatives Considered

- **Keep both registers and document the mapping.** Rejected: the
  mapping table is a permanent artifact someone must maintain, and the
  agent side keeps paying the disclosure tax the table exists to
  paper over. Documentation is the symptom, not the fix.
- **Converge on "focus" everywhere.** Rejected: it fights the
  training prior on both surfaces — operators know IRC/Slack idiom
  too, and ADR-170 chose `/join` for them deliberately. The attention
  nuance that motivated "focus" (subscription tied to sensor routing)
  never behaviorally diverged from room membership enough to earn its
  vocabulary.
- **Full chat-platform parity (read receipts, edits, presence
  indicators).** Rejected as scope: the idiom is adopted for its
  verb vocabulary, not its feature checklist. Decision 6 handles the
  expectation gap with documentation rather than implementation.
- **Hard rename without aliases.** Rejected: CLI-is-contract
  (ADR-124) — scripts, hooks, and muscle memory built on `focus`
  must not break on a vocabulary decision.
