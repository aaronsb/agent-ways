---
contract: adr/v1
kind: decision
verb: change
capability: attend
basis:
  - upstream: "Claude API pricing page (platform.claude.com/docs/en/about-claude/pricing), read 2026-11-27: Claude Opus 5.5 is $4 per million input tokens, $8 for a one-hour cache write (2x input), $0.20 for a cache hit or refresh (0.05x input) and $20 for output; Claude Haiku 5.5 is $0.10 input and $0.50 output per million tokens for prompts up to 100,000 tokens, and five times that above"
  - upstream: "Claude Code 2.1.304's bundled mod types, as read for the relevance judge's Claude Code engine: `$.model.complete` runs one text completion through the session's own API client and credentials, with no tools, no history and no system prompt beyond the CLI's identity block and `request.system`; it takes `model` as an alias such as `haiku`; `$.model.fork` instead runs over the session's own transcript"
  - evidence: "tools/sensor-keepwarm/src/lib.rs: the keepwarm wake fires at a fixed 50 idle minutes (PING_AFTER_SECS); its price table matches a model id by the first family name it contains, so `claude-opus-5-5` matches the `opus-5` row and is priced at a $0.50 cache read and a $10 one-hour write, the Opus 5 prices, a write-to-read ratio of 20 where Opus 5.5's list prices give 40"
  - evidence: "plugins/attend/types/index.d.ts: the attend mod sees `session.end` as an event with a reason; the design discussion of 2026-11-27 found no mod action that ends a session or marks it finished for peers"
  - evidence: "The operator's session on the morning of 2026-11-27: an idle session at a clean stopping point after the ways 1.57.1 release showed one keepwarm wake after another, each a cache read of the whole context, with no one using the session"
  - evidence: "The relay of 2026-11-27: a code reviewer and a CI watcher, dispatched as subagents before the wrap, delivered their reports after the compaction, and the resumed session went on from them"
  - evidence: "The design page this record was drawn from (claude.ai artifact 6gjJNpvV8EF2XWXKuRR5ge, 2026-11-27): its script holds the price model behind the performance curves; this record's figures are recomputed from that model by a throwaway generator with the same constants"
  - precedent: ADR-417
  - precedent: ADR-414
  - precedent: ADR-182
  - precedent: ADR-708
agent:
  name: Claude
  model: claude-opus-5-5
amends:
  - ADR-417#Decision
considered:
  - operator: aaronsb
    said: "allowing an agent a tool use to orchestrate their own wrap-compact-handoff if they're at a boundry of work, (like this one, where we've finished with a release)"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "the goal is to minimize the state of cache miss, while staying intentional in the handoff."
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "in the 'exit scenario' where the agent elects to stop, they always need to write a handoff, since that's the way to pick up later in a new session"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "what about a clean agent, given the wrap output? I'm trying to get a low biased determination for the call"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "the judgement agent needs to know not to pick up the work though"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "I'll be honest the bands for me are a guess. they're a hedge against increasingly expensive context length vs the cost of 'starting clean'. I am trying to avoid placing my bias of day/night routine into it too. it should run on it's own. but the judge needs to consider open questions for the operator, but not just throw hands up at the first 'the operator isn't around to answer a question' either."
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "to me, I get the maximal amount of agent capability all the way up to about the 80 percent mark in most cases"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "so a relay at 80 percent would be ideal, and setting it at 75 percent gives us that 5 percent 'late handoff'"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "maybe the actual metric is keepalive vs current context. it's less expensive to keepalive at 50 percent several times than to keepalive at 70 percent the same count of times."
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "I nearly never reload a cold session with a lot of context like 500k. this is why I'm trying to 'trap' all the exit conditions with a wrap, so it's just a matter of re-reading wrap which is usually sufficient"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "a cold reload is used if there was a failure, or I couldn't return, or it was really that important to get all the details back"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "we need to be able to have wiggle room the other way, favoring the completion of work or task. if we begin thinking about it at 75 percent, but we have to continue things along until we are at 85 percent, an 85 percent wrap and compact with completion of work is better than 75 percent wrap and compact when the work is dangling."
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "this favors the pattern of mostly delgating work to subagents and allowing them to work, and the main agent's wrap-compact-relay flow favors getting context back from the agents and acting as the supervisory role. in certain conditions, we can also favor two claudes that work together through attend, but I am not sure if we want to add that to this consideration right now"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "I'd like these graphics to make their way into an adr … as well as the narrative like you wrote too"
    via: operator in session, 2026-11-27, design discussion after the ways 1.57.1 release
  - operator: aaronsb
    said: "I accept this - we can use the same judge interface we use for disclosing a way too"
    via: operator in session, 2026-11-27, after reviewing ADR-418 and its four figures
    covers: [settings, judge-model, first]
  - operator: aaronsb
    said: "perhaps the judge interface gets different judgement profiles - a ways disclosure and continuance profile. we can specify the models just the same, but they're not tied to each other"
    via: operator in session, 2026-11-27, refining the earlier answer on the judge's profile
    covers: [judge-model]
status: accepted
date: 2026-11-27
deciders:
  - aaronsb
related:
  - ADR-165
  - ADR-196
  - ADR-610
---

# ADR-418: At a work boundary a session wraps and a clean judge chooses to stop, compact or continue; keepwarm holds the cache only while it pays; from 75% the session drains

## Summary

- **Decided:** When a piece of work lands, the working agent calls a tool that the attend mod registers. The call runs the wrap, which writes the handoff, and the mod then sends the handoff and the conditions it measures to the judge: a clean model call through the same judge interface that decides which ways are disclosed, under its own judgement profile, `continuance`, beside the `disclosure` profile. Each profile names its own model, and neither follows the other. The judge returns `stop`, `compact`, `continue` or `doubt`, and says whether the handoff is enough to start fresh from. A stop arms an exit: keepwarm pings only while something is pending or the session holds detail the handoff lacks, and only while the pings cost less than whatever would replace the session. Otherwise the first keepwarm tick lets the cache lapse and marks the session finished. From 75% of the window the session drains: it starts nothing new, finishes the work in flight, and wraps at the next boundary. Only at a hard line, proposed at 85%, does the mod wrap mid-work. This amends the relay mode: `relay_at_pct` becomes the drain point, a new `hard_at_pct` marks the mid-work wrap, the judge is the independent check the relay mode deferred, and the idle path gains the armed exit.
- **Trades away:** a model call at every boundary, which costs little but can be wrong, and a wrap at every boundary even when the session carries on. The drain spends context between 80% and 85%, past where the operator sees the agent's capability hold, to finish work instead of leaving it dangling. A session that stops lets its cache lapse, so an operator who returns after the exit starts fresh from the handoff or pays a cold reload. The bands are the operator's observation, not a measurement, and the price of a fresh start (100k tokens) is an estimate. The design rests on early-access mod calls (`$.model.complete`, tool registration) and on a way to mark a session finished that does not exist yet.
- **One-way?** No. Reversible: the bands are settings, the tool, the judge and the armed exit are mod code, and relay mode stays off by default. A stop deletes nothing: the transcript stays on disk, and `claude --resume` brings the session back.
- **Probes:** *Confident (settings):* `relay_at_pct` keeps its 75 but now means "start draining", and a new setting, `hard_at_pct`, proposed at 85, is where the session wraps mid-work. Lean: yes. *Not confident (judge-model):* the judge runs on Claude Haiku 5.5: a different model from the working agent, about a tenth of a cent for a brief of ten thousand tokens, and answering from a closed set of verdicts. Lean: Haiku 5.5; the alternative is the working agent's own model in a clean call, which reads more carefully but shares more of the working agent's habits. *Not confident (first):* what ships first is two probes: whether the mod can mark a session finished for its peers, and the measured size of a fresh start. Lean: yes; the alternative is to build the tool and the judge first on the 100k estimate. *Answered 2026-11-27:* the operator accepted the record and the leans on settings and first, and settled judge-model differently: the judge uses the same interface as way disclosure, under a `continuance` judgement profile of its own beside the `disclosure` profile, each naming its model independently.
- **Open:** where the hard line can sit under Claude Code's own compaction point, whether the price ratio holds under a subscription, and two Claudes working together through attend, which stays deferred (Open questions).
- **Inversion:** at one end, every session stays warm until the operator returns and pays a cache read of its whole context every 50 minutes. At the other end, every session ends at its first boundary and the next one always starts fresh from the handoff. This record sits between them: what is pending, and the price of the pings against the price of a replacement, decide how long a stopped session stays warm, and a clean judge, not the working agent, makes the call. A third answer outside that framing, two sessions handing work to each other through attend, is deferred.

## Context

A wrap closes a stretch of work: it squares the task list and writes a handoff, `.claude/handoff.wrap.md`, that a later session can pick the work up from. The relay mode follows a wrap with a compaction on that handoff and a turn that resumes the work. It wraps at a fixed context fill, 75% by default, whatever the work is doing at that moment. The keepalive path keeps an idle session's prompt cache warm with a one-word turn every 50 idle minutes, up to a count, and then asks for a wrap.

On the morning of 2026-11-27 an idle session showed keepwarm wake after keepwarm wake. Each wake is a cache read of the whole context, paid to keep warm a session that nobody was using. The work had reached a clean stopping point after a release, so a wrap and an exit would have been enough to pick up and keep going.

The operator rarely reloads a cold session that carries a lot of context. A new session that re-reads the wrap is usually enough. A cold reload is the insurance, used when something failed, when the operator could not get back in time, or when getting every detail back really matters. The aim is therefore to trap every way a session can end with a wrap, so that the next session starts by reading it, and to pay for as few cache misses as possible while every pickup starts from a handoff written on purpose.

A first draft of this design used fixed fill bands. Those bands were a guess at the trade between a growing context and a clean start. This record works the trade out from list prices and from what is pending, with no daily routine built in, and keeps fill bands only where the operator's observation of capability sets them.

## Decision

### 1. The rule

- **Wrap at every boundary.** A boundary is the moment a piece of work lands: a merge, a release, a closed task with nothing in flight behind it. The session wraps there even when it carries on. The cache stays warm while something is pending, or while the session holds detail the handoff lacks. Otherwise the session exits. The handoff always says how to reload the session cold.
- **From 75%, drain.** The session starts nothing new, finishes the work in flight, and wraps at the next boundary. It wraps mid-work only at 85%.

### 2. What the context fill decides

The figure below shows the bands across the window: the agent's capability holds to about 80%, the drain runs from 75% to 85%, and the hard line at 85% is the only place the session wraps mid-work.

![Strip graph of context fill from 0 to 100 percent in three rows. Agent capability: full capability up to about 80 percent, the operator's observation; workable from 80 to 85; past 85 Claude Code compacts on its own. At a work boundary: below 75 percent the session wraps, then a clean judge chooses stop, compact or continue, and what is pending and the prices decide how long to stay warm; from 75 to 85 percent the session drains and wraps at the boundary. Mid-work: below 75 percent the session keeps working, with agents carrying the focal work and the main session supervising; from 75 to 85 it drains, finishing work and collecting reports; at the 85 percent hard line it wraps mid-work.](figures/ADR-418-fill-bands.svg)

- **75% is where the session starts thinking about the wrap, not where the wrap must happen.** During the drain the session starts nothing new, lets the work in flight finish, collects its agents' reports, and wraps at the next boundary. A wrap at 85% with the work complete is better than a wrap at 75% with the work dangling.
- **The hard line.** At 85% the mod wraps mid-work, as the relay mode wraps at its threshold today. The hard line has to sit below the point where Claude Code compacts on its own. The mod reads that point from the session's settings and does not assume it.
- **The settings.** The relay mode's settings change as follows:

  | Key | Today | Proposed |
  |---|---|---|
  | `relay_at_pct` | 50 to 95, default 75: the mod submits the relay's wrap at this fill | same range and default: the drain starts here |
  | `hard_at_pct` | none | new, proposed default 85: the mod wraps mid-work here; it sits above `relay_at_pct` and below Claude Code's own compaction threshold, with room for the wrap |
  | `max_relays` | 0 to 50, default 5 | unchanged; a compaction the agent starts through the tool counts toward it |

### 3. Every exit finds a wrap

Some exits can run a wrap when they happen. Others cannot, because nothing runs a model at that moment. Those rely on the last boundary wrap, which is why a session wraps at every boundary even when it carries on.

| Exit | Can a wrap run there? | Covered by |
|---|---|---|
| idle, at a keepwarm tick | yes | the armed exit; the wrap already ran at the boundary |
| the drain, 75% to 85% | yes | the wrap at the next boundary, or mid-work at 85% |
| Claude Code's own compaction past the hard line | barely | the PreCompact hook, which checks for a fresh handoff |
| terminal closed, `/exit`, `/clear`, a crash | no | the last boundary wrap only |

### 4. What carries the work through a compaction

The handoff on disk is the backstop: it survives a compaction, a restart and a crash, and reading it costs no cold reload of the old context. It is not the only carrier. A compaction keeps the Claude Code process, so whatever lives in the process comes through as well.

| Carrier | Through a compaction | Through a restart |
|---|---|---|
| the handoff on disk | yes | yes |
| the task list | yes | carried forward to a resumed session |
| subagents in flight | yes, and they report back after it | no, they end with the process |
| the transcript | on disk | only as a cold reload |

The relay of 2026-11-27 showed this working: a code reviewer and a CI watcher, dispatched before the wrap, delivered their reports after the compaction, and the work went on from them. The design therefore favors delegation. Subagents hold the focal work in their own contexts. The main session supervises: it briefs, collects reports and merges, and it holds reports, not diffs. Its context grows slowly, its compaction loses little, and the work it handed out comes through the compaction in the agents doing it.

Agents in flight therefore block a stop, because exiting ends them. They do not block a compaction, which they come through.

### 5. The numbers

Claude API list prices for Claude Opus 5.5 are $4 per million input tokens, $0.20 for a cache read (0.05 times input), and $8 for a cache write with the one-hour lifetime (2 times input). Keepwarm's 50-minute tick keeps a one-hour cache alive, so a ping costs one cache read of the whole context plus a few output tokens, and a cold start pays the write price on whatever it writes.

| Context | Per ping | Cold reload of this context | Pings before a fresh start is cheaper |
|---|---|---|---|
| 200k (20%) | $0.04 | $1.60 | 20 (about 17 h) |
| 350k (35%) | $0.07 | $2.80 | 11 (about 9 h) |
| 500k (50%) | $0.10 | $4.00 | 8 (about 6.7 h) |
| 700k (70%) | $0.14 | $5.60 | 6 (about 4.8 h) |

The last column assumes that a fresh start writes about 100k tokens: the system prompt, the tools and ways, the handoff, and what the successor re-reads before its first edit, about $0.80. That figure is an estimate to measure. A write costs 40 times a read, so the rule fits on one line:

```
pings worth paying = 40 × (the price of whatever replaces this session ÷ current context)
```

This is the ski-rental rule, named for the choice between renting skis each day and buying a pair: keep paying rent while the rent paid stays under the purchase price, then buy. Here the rent is a ping and the purchase is whatever replaces the session. The rule needs no forecast of when the operator returns, and it never costs more than twice the best choice made with hindsight. What replaces the session sets the purchase price, and the judge's answer on the handoff chooses it (item 10).

### 6. How long to stay warm

The figure below draws, in keepwarm ticks, how long an armed exit stays warm in each of three cases, for a session at 35% of the window with a 100k fresh start.

![Three lanes of idle time in keepwarm ticks, drawn for a session at 35 percent of a 1M window with a 100k fresh start; each tick is about 50 idle minutes, and the first hour is free for all three. When the handoff is enough and nothing is pending, the session exits at the first tick. When something is pending, such as a reply, a peer or a CI run, it stays warm until that arrives, at most 11 ticks, about 9 hours. When the session holds detail the handoff lacks, it stays warm until the operator returns, up to 40 ticks, about 33 hours.](figures/ADR-418-stay-warm.svg)

- **The handoff is enough and nothing is pending.** The operator will start fresh from the handoff anyway, so pings buy nothing, and the session leaves at the first tick.
- **Something is pending.** A reply, a peer's message or a CI run makes it worth staying, up to the price of a fresh start.
- **The session holds detail the handoff lacks.** Getting it back would cost a full cold reload, so the session stays warm for up to 40 pings' worth of rent.

### 7. Performance curves

The figure above reads one fill. The figure below reads every fill. It is laid out like an engine's dyno sheet, where fuel flow, torque and power share the rpm axis: four panels share the context fill axis, so a reader picks a fill and reads down. Each panel holds one quantity on its own scale, and the drain band, the 80% capability line and the 85% hard line run through all four.

![Four panels over context fill from 0 to 100 percent of a 1M window, with the drain band from 75 to 85 percent, a dashed line at 80 percent and the hard line at 85 percent through all four. Price of one turn or one ping: a straight line rising to 20 cents at a full window. Input cost of working up to the fill: a curve rising to about 20 dollars at a full window. How long staying warm pays: about 33 hours at every fill when the session holds detail the handoff lacks; when something is pending, 33 hours up to 10 percent, then falling to about 9.5 hours at 35 percent and under 4 hours at 85. Turns for a compaction to pay back: from 62 at 10 percent down to about 5 at 85; the curve starts at 10 percent because below it a compaction barely shrinks the context.](figures/ADR-418-performance-curves.svg)

The curves assume Claude Opus 5.5 list prices, one ping every 50 idle minutes, a fresh start of 100k tokens, 5k tokens added per working turn from a 50k start, and a compaction that writes a 10k summary into a 50k context. With nothing pending and a sufficient handoff, staying warm pays nothing past the free first hour, so that case has no curve. The same model gives these values:

| Fill | Per turn or ping | Working up to here | Warm, something pending | Warm, holds detail | Compaction payback |
|---|---|---|---|---|---|
| 10% | $0.020 | $0.15 | 33.3 h | 33.3 h | 62 turns |
| 20% | $0.040 | $0.75 | 16.7 h | 33.3 h | 21 turns |
| 35% | $0.070 | $2.40 | 9.5 h | 33.3 h | 11 turns |
| 50% | $0.100 | $4.95 | 6.7 h | 33.3 h | 8 turns |
| 70% | $0.140 | $9.75 | 4.8 h | 33.3 h | 6 turns |
| 75% | $0.150 | $11.20 | 4.4 h | 33.3 h | 5 turns |
| 85% | $0.170 | $14.40 | 3.9 h | 33.3 h | 5 turns |
| 100% | $0.200 | $19.95 | 3.3 h | 33.3 h | 4 turns |

### 8. Compact or continue

While the agent works, every turn reads the whole context. A stretch of work that grows the context from C₀ to C₁ by g tokens a turn costs about

```
0.05 × input price × (C₁² − C₀²) ÷ 2g
```

so the cost grows with the square of the context, and the last stretch before 80% is the most expensive part of a session. Compacting resets the context to a summary S, and pays back after about `compaction cost ÷ (0.05 × input price × (C − S))` turns. At 60% with a 50k summary, the call, the summary's output and its cold write come to about $0.72 against $0.11 saved each turn: about 7 turns. At 30%, about 13 turns.

On price alone, then, compacting at a boundary pays whenever real work remains. The counterweight is fidelity, which the arithmetic cannot count: a compaction summarizes, and a chain of relays summarizes summaries. That is the judge's call, informed by the relay count.

### 9. The states a session moves through

The figure below shows the states and the moves between them, with the moves this record adds drawn apart from the ones the relay mode has today.

![State diagram. Working moves to Boundary when a piece of work is done, then to Wrap, then judge through the agent's tool call; at 85 percent with no boundary yet, Working moves straight to Wrap, then judge, a mid-work wrap. The judge's verdicts: continue returns to Working and the wrap stands as a checkpoint; compact goes to Compacting, which resumes into Working when tasks remain or goes to Armed exit when none remain; stop goes to Armed exit. From Armed exit, an operator prompt disarms it and returns to Working with the cache still warm; at a keepwarm tick with nothing pending, or once rent would pass the price of a replacement, the cache lapses and the session is Finished, with its transcript kept. From Finished, a new session reads the handoff, or claude --resume reloads this one cold. Boundary, Armed exit and Finished are new states; the edges for done, tool, stop, continue, the tick, the operator prompt and the new session are new; the mid-work wrap, compact, resume and no-resume edges exist in the relay mode today.](figures/ADR-418-states.svg)

Every move at a boundary passes through the wrap, so a session never stops or compacts without a handoff on disk. During the drain the next boundary goes the same way, except that `continue` is off the table. Each keepwarm tick checks that the recorded wrap id still matches the handoff. A stale handoff means the tick pings as it does today.

### 10. The judge

The working agent wrote the handoff, but it does not make the call. A clean model call does, through the same judge interface that decides which ways are disclosed. The interface keeps a judgement profile per purpose: `disclosure` for ways and `continuance` for this call. Each profile names its model the same way, and the two are set independently; the shipped `continuance` profile names a different, smaller model from the working agent. It starts with nothing but its brief, so the working agent's momentum and its sunk cost in a warm thread do not reach it. The judge also acts as the stranger the wrap skill writes the handoff for: if it cannot name the next step from the handoff, the handoff is too thin to stop on.

| The brief | Who supplies it | What it holds |
|---|---|---|
| handoff | the working agent, in the wrap | `.claude/handoff.wrap.md`: tasks, state, DO NEXT, DECISIONS with what each one blocks, and the resume line |
| conditions | the mod, measured | Fill in percent and tokens, and the price per ping. Background agents, shells and CI watchers still running. Questions posted to the operator and peer threads awaiting a reply. Open and in-progress tasks. `git status`. Open PRs and their checks. Minutes since the operator's last prompt. The relay count, n of `max_relays`. Commits, merges and closed tasks since the last wrap's `head`. |
| verdicts | fixed | `stop`, `compact`, `continue` or `doubt`; and `sufficient` or `holds detail`, saying whether the handoff carries what matters. Each comes with one line of reason, the turns of work the judge thinks are left, and the next step it would take from the handoff. |

- **The judge decides and never works.** The handoff is written to be acted on: it opens with "Continue work on" and ends with DO NEXT. So the judge is no agent at all. The mod sends the brief as one judge request through the relevance judge's interface, under the `continuance` profile, with no tools and no session, so the call cannot pick up the work. On the Claude Code engine that request is a `$.model.complete` call; on a key engine the ways agent makes it. The handoff sits in the brief as quoted data, under a line that says the judge is deciding whether this session stops, compacts or continues, not resuming the work. The mod keeps only the verdict line and discards anything else.
- **Sufficiency sets the purchase price.** `sufficient` means a fresh start replaces the session, at the price of the fresh start. `holds detail` means getting the session back takes a cold reload, at the price of the whole context.
- **The working agent's preference stays out of the brief.** The working agent records its own call separately. The two calls are compared afterwards, and both go into the handoff's DECISIONS.
- **The relay count is evidence.** Each continuance compacts an earlier summary, and the operator has been away longer, so a high count leans toward stop. A relay that closed nothing since the last wrap leans toward `doubt`.
- **When the two calls disagree, the more reversible move wins.** An armed stop beats a compaction, because the stop waits for the keepwarm tick and the operator can undo it while the cache is warm. When the working agent says continue and the judge says stop, the handoff is missing what the working agent sees: the working agent fixes the handoff and continues.
- **The mod's refusals come first.** Agents, shells or CI watchers in flight refuse a stop, since exiting would end them; they do not refuse a compaction. During the drain the mod refuses `continue`. The `max_relays` cap ends the chain. None of these reach the judge.

### 11. Questions for the operator

An open question is not by itself a reason to stop. Each line under DECISIONS in the handoff says which tasks it blocks, the lean, and whether acting on the lean can be undone. The judge reads those three things.

- **The question blocks some tasks, not all.** Work continues on the rest, and the question goes to the operator over attend.
- **Its lean is reversible.** Work goes ahead on the lean, and the call is logged for the operator to overrule.
- **Its lean is a one-way step.** That task waits for the operator. A question posted and awaiting an answer counts as pending, so the session stays warm for the answer up to the price of a fresh start.
- **Everything left waits on a one-way call.** Only then is the verdict `stop decision`. The questions stay in the handoff, so an answer that arrives after the session has finished lands in the next one. In relay mode, `doubt` becomes `relay: stop doubt`, and the handoff goes to the operator over attend.

### 12. The tool the agent calls

| Verdict | When | What happens |
|---|---|---|
| `stop` | at a boundary, with no agents in flight | The mod records the wrap id and arms the exit. Keepwarm goes on while something is pending or the session holds detail, up to the purchase price. Otherwise the first tick lets the cache lapse and marks the session finished. |
| `compact` | at a boundary, when the payback and fidelity favor it; agents in flight come through | The mod compacts with the handoff, then resumes when tasks remain or stops when none do. |
| `continue` | at a boundary below 75%, when work remains and compacting does not pay | The agent goes back to work. The wrap stands as the checkpoint for exits that cannot wrap. |
| drain | from 75% to 85%; the mod sets it, and it is not a verdict | Nothing new starts. The next boundary wraps and then compacts or stops. At 85% the mod wraps mid-work, as the relay mode does at its threshold today. |

### 13. Rules

- **Every stop writes a handoff.** The handoff is how a later session picks the work up, so a stop runs the full wrap, and the mod refuses to arm the exit until the file exists.
- **The handoff names its session.** It carries the line that reloads this session cold, `claude --resume <session id>`, and says what only the transcript holds, so the operator can tell whether a reload is worth paying for.
- **The handoff is the exit flag.** Its first line carries the wrap id, and attend records the same id. No separate token exists.
- **An operator prompt disarms the exit.** The handoff stays on disk, and the next wrap overwrites it.
- **No self-compaction while the operator is talking to the agent.** A reply to an operator who is present ends a step of the agent's work, but the operator's conversation goes on, so the agent does not treat it as a boundary to compact on.
- **Self-relays count.** A compaction the agent starts through the tool counts toward `max_relays` (default 5) like any other relay.

### 14. What changes in the relay mode

The relay-mode record is not edited by this one. When this record is accepted, it gains dated amendment notes at the items below.

- **Item 1, the settings table.** `relay_at_pct` keeps its range and default and changes meaning, from "submit the wrap" to "start the drain". `hard_at_pct` joins the table (item 2 above).
- **Item 2(a), the fill trigger.** At `relay_at_pct` the mod no longer submits a wrap. It tells the agent that the drain has started, refuses `continue` from then on, and lets the next boundary wrap. The mod submits the mid-work wrap at `hard_at_pct`. The rule that lowers the trigger under Claude Code's own `autoCompactThreshold` moves from the drain point to the hard line.
- **Item 2(b), the agent's own wrap.** Besides `/wrap`, the agent calls the boundary tool, which wraps and asks the judge. A `/wrap` the operator types stays the operator's own and is not judged.
- **Item 2(c) and the idle path.** After a `stop` verdict the idle path runs the armed exit: the keepalive count no longer decides alone how long the session stays warm. The tick pings while something is pending or the session holds detail, up to the ski-rental bound, and otherwise lets the cache lapse and marks the session finished. Without a fresh handoff whose wrap id matches, the tick pings as today.
- **Item 4, the state machine.** The policy gains the states `armed` and `finished`, and the ledger records the judge's verdict, its sufficiency answer and the working agent's own call for each boundary wrap.
- **Item 5, the bounds.** The stop conditions gain the judge's `stop` and `doubt`. `max_relays` counts self-relays.
- **The deferred independent check.** The relay mode left an independent reader of the handoff before each resume as the first addition to consider. The judge is that reader. It uses `$.model.complete` with a brief, not `$.model.fork`, because a fork runs over the session's own transcript and would carry the working agent's context into the call.

## Consequences

### Positive

- An idle session at a clean stopping point stops paying keepwarm pings that buy nothing, and the pickup starts from a handoff written on purpose.
- Every exit that can run a model finds a fresh wrap, and the exits that cannot find the last boundary wrap.
- A session with work in flight at 75% finishes it before wrapping, so the handoff describes landed work instead of dangling work.
- The stop, compact or continue call is made by a reader with no stake in the warm thread, and the judge's inability to name a next step exposes a thin handoff before the session stops on it.
- How long a stopped session stays warm follows from prices and from what is pending, with no assumption about the operator's daily routine.

### Negative

- Every boundary costs a wrap and a judge call, even when the session carries on.
- A wrong `stop` lets the cache lapse; the operator then starts fresh from the handoff or pays a cold reload. A wrong `continue` spends context the judge thought was worth spending.
- The drain runs the session between 80% and 85%, where the operator sees capability begin to fall.
- The purchase price rests on an estimated 100k fresh start until it is measured, and on API list prices, whose weight under a subscription is unverified.
- Keepwarm's price table matches `claude-opus-5-5` to its Opus 5 row today, a write-to-read ratio of 20 where Opus 5.5's list prices give 40, so the bound it computes is half the one this record derives until the table gains the Opus 5.5 row.
- "Finished" cannot yet be marked for peers: without a mod action for it, peers may send messages that wait for a session that will not wake.

### Neutral

- Implementation adds the boundary tool and its verdict handling to the mod, the brief and its request through the relevance judge's interface, a `continuance` judgement profile beside `disclosure` in the judge settings, the `armed` and `finished` states and the judge's fields to attend's ledger, `hard_at_pct` to the settings registry, the resume line and the DECISIONS columns (blocks, lean, reversible) to the wrap skill's handoff, and the Opus 5.5 row to keepwarm's price table.
- The PreCompact hook stays as the relay mode defines it, and covers the compaction past the hard line.
- The design favors the main session as a supervisor of subagents, which keeps its context small and makes its compactions cheap to lose.

## Alternatives Considered

- **Fixed fill bands for stop, compact and continue.** The first draft of this design used them. Rejected: the bands were a guess, and they built a daily routine into the call. Prices and what is pending decide instead, and fill bands remain only where the operator's observation of capability sets them (the drain and the hard line).
- **The working agent judges its own stop, as the relay mode's marker does today.** Rejected: the working agent carries momentum and a sunk cost in the warm thread, and it is the one reader for whom any handoff looks sufficient.
- **The judge as `$.model.fork` over the session's transcript.** Rejected: a fork carries the working agent's whole context into the call, which is the bias the judge exists to remove, and it bills the whole prefix again once the cache lapses.
- **The judge as a subagent.** Rejected: a subagent has tools and would act on a handoff written to be acted on. A plain completion with no tools cannot pick up the work.
- **Keep the wrap at 75% wherever the work is.** Rejected: a wrap at 75% with the work dangling hands the next session half-done work, where a wrap at 85% with the work complete hands it a clean boundary.
- **Keep keepalives up to a count whatever the session holds.** Rejected: a session whose handoff is enough and that waits on nothing pays for pings that buy nothing, as the morning of 2026-11-27 showed.
- **A cold reload as the normal pickup.** Rejected: the operator rarely does it, and it costs the write price on the whole context. It stays the insurance, named in every handoff.
- **Two Claudes working together through attend, handing work across with `handoff:` messages.** Deferred: it is another way to carry work past a compaction, and it stays out of this design for now.

## Open questions

1. **Can the mod mark a session finished for peers?** The mod API exposes `$.session.end` only as an event the mod receives. Without an action for it, "finished" means no more pings plus a mark in attend, so peers see the session as gone instead of sending it messages that wait cold. This needs a probe.
2. **The size of a fresh start.** The 100k figure is an estimate. Measure it after each real stop: the system prompt and the handoff, plus what the successor reads before its first edit.
3. **Prices under a subscription.** The figures here are API list prices. Under a subscription the cost lands on the usage budget instead. The read-to-write ratio probably weighs the same there, but that is unverified.
4. **Where the hard line can sit.** 85% works only below the point where Claude Code compacts on its own. The mod reads that point from the session's settings and sets the hard line under it with room for the wrap; how much room the wrap needs is not yet measured.
5. **Deferred: two Claudes through attend.** Two sessions working together, handing work across with attend's `handoff:` messages, stays out of this design for now.

## References

- ADR-417 — the relay mode this record amends: its settings table (item 1), its fill trigger (item 2a), the agent's wrap (item 2b), the idle path (item 2c), its state machine (item 4), its bounds (item 5), and the independent check it deferred among its alternatives.
- ADR-414 — the idle keepalive path and its count, which the armed exit replaces after a `stop` verdict.
- ADR-182 — keepwarm: the 50-minute wake that keeps the one-hour cache warm, and the price table the ski-rental bound reads.
- ADR-708 — the Claude Code engine of the relevance judge, which owns the `$.model.complete` path the judge uses.
- ADR-196 — the relevance gate, the first judge in this project that decides by a closed verdict from a model with no tools.
- ADR-165 — the wrap skill and its handoff, which this record asks to carry the resume line and the DECISIONS columns.
- ADR-610 — the record of every wrap and the session-start way that steers the next session to the handoff.
