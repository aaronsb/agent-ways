---
id: 01.017.E
domain: ways
mode: explanation
related:
  - "[[ADR-123]]"
  - "[[ADR-126]]"
  - "[[ADR-134]]"
  - "[[ADR-196]]"
  - "[[01.018.E]]"
  - "[[01.019.E]]"
aliases: []
---

# How ways works — the model

When ways is doing its job, you don't notice it. A premise lands in Claude's
context at the moment it's relevant, Claude reasons with it, and the work moves
on. Nothing announces itself. That invisibility is the design working — but it
makes the system hard to *believe in*, because the help leaves no mark on the
conversation you can point to.

It does leave a mark somewhere else. **Every time a way fires, the cheap
substrate writes a line to `$XDG_STATE/agent-ways/events.jsonl`.** That append-only
record is the observable shadow of the cognitive loop: a turn-by-turn account of
which premises surfaced, why, and when. This cluster is about reading that
shadow — what it records, what it reveals about how ways helps, and how to pull
it out for a session of your own ([[01.019.E]]) or walk a real long one
([[01.018.E]]).

For the *design* — substrate separation, progressive disclosure, the ledger, the
awareness layer — read [the cognitive loop](../../cognitive-loop.md). This page
sits one level lower: not how the system is built, but how you *watch it run*.

## What the record captures

Each line is one event with a timestamp, a session id, a project, and a trigger.
These event types are the ones that make a kind of help visible:

| Event | What it means | What it tells you |
|-------|---------------|-------------------|
| `way_fired` | A premise matched and was injected | The system decided this guidance was relevant *here* |
| `way_redisclosed` | An already-seen way surfaced again after its cooldown | The premise had faded from attention and was refreshed |
| `check_fired` | A depth-on-demand sub-way pulled in under a fired way | Claude got *more* detail because the situation warranted it |
| `way_nearmiss` | A way scored close to the fire bar but did **not** fire | The boundary: what the system *almost* surfaced, and held back |
| `way_judged` | The relevance gate asked a judge model whether a matched way is relevant | Whether a match reached Claude, or was blocked as irrelevant |
| `session_start` | A session began | The anchor every other event hangs off |

The log also records ways held back by their cooldown or the context budget
(`way_suppressed`), keyword hits vetoed by the semantic floor
(`way_keyword_gated`), ways withheld from subagents (`injection_suppressed`), and
the judge's provider calls. [The event log](../../reference/events.md) lists every
event and field. The first five rows above map onto five behaviours worth
understanding separately.

## Five behaviours, made observable

```mermaid
flowchart TB
    subgraph Loop["Claude's turn — the expensive substrate"]
        direction TB
        Prompt["user prompt<br/>+ tool calls<br/>+ prior topics"]
    end

    subgraph Match["the matcher — cheap, runs before Claude sees anything"]
        direction TB
        Cand["<b>match</b><br/>clears the bar"]
        Near["<b>near-miss</b><br/>close but under<br/>→ way_nearmiss"]
        Gate{"<b>relevance gate</b><br/>prompt lane<br/>→ way_judged"}
        Fire["<b>first-fire</b><br/>→ way_fired"]
        Re["<b>re-disclosure</b><br/>seen before, cooled down<br/>→ way_redisclosed"]
        Chk["<b>check</b><br/>depth pulled on demand<br/>→ check_fired"]
        Block["<b>blocked</b><br/>judged irrelevant"]
    end

    Record[("events.jsonl<br/>the observable shadow")]

    Prompt --> Cand
    Prompt --> Near
    Cand --> Gate
    Gate -->|pass| Fire
    Gate -->|pass| Re
    Gate -->|block| Block
    Fire --> Chk
    Fire --> Record
    Near --> Record
    Re --> Record
    Chk --> Record
    Gate --> Record

    classDef expensive fill:#7c3aed,color:#ffffff,stroke:#4a5568
    classDef cheap fill:#2d7d9a,color:#ffffff,stroke:#4a5568
    classDef miss fill:#f6821f,color:#1a1a1a,stroke:#4a5568
    classDef durable fill:#2d8e5e,color:#ffffff,stroke:#4a5568
    class Prompt expensive
    class Cand,Gate,Fire,Re,Chk cheap
    class Near,Block miss
    class Record durable

    style Loop stroke:#8b5cf6,fill:#7c3aed1a,color:#cbd5e1
    style Match stroke:#2d7d9a,fill:#2d7d9a1a,color:#cbd5e1
```

The gate runs only on the prompt lane and skips `pattern_strict` ways. Command,
file and state matches go straight from match to fire.

**First-fire — precision matching.** A way fires the first time its trigger
matches: a keyword in the prompt (floor-gated), a semantic match whose
calibrated relevance probability `g(s)` clears the global fire threshold `τ_s`, a
file being edited, a bash command about to run, a context-threshold crossed. (The
fire rule is global, not per-way — see [the engine
reference](../../hooks-and-ways/engine-reference.md).)
The trigger type is recorded verbatim (`keyword`, `semantic:late-interaction:en`,
`semantic:embedding:en`, `semantic:bash:en`, `state`, `bash`, `file`, `check-pull`
and others; see [trigger values](../../reference/events.md#trigger-values)). The mix of
trigger types across a session is the clearest single signal of *how* ways is
reaching Claude — a session dominated by `semantic` fires is being steered by
meaning; one dominated by `bash` and `file` is being steered by what Claude is
physically doing.

**Re-disclosure — habituation.** Once a way has fired, it is marked disclosed
and won't fire again until its cooldown — measured in *tokens of context
consumed*, not turns or wall-clock — expires ([[ADR-123]], [[ADR-126]]). When the trigger
recurs after the cooldown, the way re-surfaces fresh as a `way_redisclosed`
event. This is the mechanism that keeps a long session from either drowning in
repeated guidance or silently losing premises it surfaced eighty turns ago. The
ratio of re-disclosures to first-fires is the signature of session *length*: a
short session is almost all first-fires; a multi-day session re-discloses its
core premises many times over. The cadence of that re-disclosure is itself
tunable from this data ([[ADR-123]]).

**Near-miss — the threshold boundary.** When a way scores within a small margin
of its effective semantic threshold but doesn't clear it, the matcher records the
would-be fire — its English and multilingual relevance probabilities, the
semantic threshold `τ_s`, and the margin by which it missed ([[ADR-134]]).
Near-misses are the only window onto
*false silence*: the guidance that almost helped and was held back. They are
invisible in the conversation and invisible in the TUI replay; the JSON dump
([[01.019.E]]) is the only way to see them. A way that near-misses constantly is
a vocabulary-tuning opportunity; a near-miss right before a mistake is guidance
that should have surfaced — the remedy is to strengthen the way's vocabulary or
pattern until the match clears `τ_s`, since firing is global and there is no
per-way threshold to lower.

**Relevance gate — a second opinion.** When a judge is configured and the gate is
in `enforce` mode ([[ADR-196]]), the prompt-lane matches go to a small model in one
batched call, which scores how likely each way is to be relevant to the prompt. A
way under the profile threshold is withheld, and its `way_judged` line carries
`verdict: block`; no `way_fired` follows. In `shadow` mode the verdict is
`would_block` and the way fires anyway, which is how a gate is evaluated before it
is trusted. A matched way that never reached Claude shows up only here. If the
judge cannot answer in time, the gate fails open and logs `gate_fallback`.

**Check — depth on demand.** Some ways are trees: a parent premise fires, and
under it sit *checks* — finer-grained sub-ways that pull in only when their own
trigger matches in the window the parent opened. A `check_fired` event means
Claude didn't just get "think about code quality," it got the specific
sub-premise about, say, performance or supply-chain, because that's what the
moment called for. Checks are how progressive disclosure goes *deep* without the
parent way having to carry every detail at all times.

## One turn, in order

The flowchart above shows *which* behaviours exist; it can't show the one thing
that makes them matter — that the matcher runs to completion *before* Claude sees
anything, and that a near-miss reaches the record but never reaches Claude. That
asymmetry is temporal, so it wants a sequence:

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant M as Matcher
    participant R as events.jsonl
    participant C as Claude

    Note over M: cheap substrate — runs before Claude sees anything
    User->>M: prompt + tool calls + prior topics
    Note over M: scores every way against this frame

    M->>R: way_judged — prompt-lane matches, pass or block
    Note right of M: a blocked match stops here

    M->>R: way_fired — matched and passed
    M->>C: inject premise into context

    M->>R: check_fired — depth pulled under a fired way
    M->>C: inject sub-premise on demand

    M->>R: way_redisclosed — seen before, cooled down
    M->>C: refresh the faded premise

    M-->>R: way_nearmiss — close, but under threshold
    Note right of M: recorded, never injected — the boundary of false silence

    Note over C: expensive substrate — reasons with whatever surfaced
    C->>User: work moves on
```

Read top to bottom, the ordering is the argument. The matcher does all its
scoring and judging and writes every decision to `events.jsonl` *first*; only the
premises that cleared their bar and passed the gate are injected into Claude's
context. The near-miss is the dashed line: it lands in the record and stops there,
and so does a blocked match. Claude never reasons over
it, which is precisely why the conversation can't reveal it and the JSON dump
([[01.019.E]]) can.

## Why the record is trustworthy

The events are written by the same code path that does the matching, at the
moment the decision is made — not reconstructed after the fact, not inferred from
the transcript. The `fire_score` on a semantic fire is the exact score that cleared
the bar; the near-miss probabilities are the exact values that didn't; the judge's
`p_yes` is the exact probability it returned. This is persistence of a decision already made, not new computation
([[ADR-134]]). What you read back is what actually happened.

The one caveat worth holding: re-disclosure cooldowns shown in a *replay*
reflect each way's `refire:` as it stands *today*, because the cadence lives in
the way's frontmatter, not in the event line. If you've retuned a way's `refire:`
since the session ran, the replay shows the new value. Everything else — what fired, when,
at what score, against what threshold — is frozen at the moment it happened.

## Where this sits

- **The design behind all of this:** [the cognitive loop](../../cognitive-loop.md)
  and the ADRs it cites.
- **The same model in a real long session:** [[01.018.E]] walks a 97-hour,
  78-way session and shows the behaviours in its actual numbers.
- **Pulling the record yourself:** [[01.019.E]] covers `ways session ways`, `ways tune stats`,
  and `ways session replay --json` — what each shows and what the data means.
