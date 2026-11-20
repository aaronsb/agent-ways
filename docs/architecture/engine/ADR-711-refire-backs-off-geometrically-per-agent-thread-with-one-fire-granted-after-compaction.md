---
contract: adr/v1
kind: decision
verb: change
capability: disclosure
amends:
  - ADR-126#preset-configuration
  - ADR-126#resolution-semantics
basis:
  - operator: aaronsb
    level: directed
    said: "I think there might need to be a better quenching of refire on a total session. we haven't visited that in a while. it's likely that if something is firing several times throughout the session won't be as necessary"
    via: "the operator in the head session, 2026-11-20"
  - operator: aaronsb
    level: directed
    said: "let's see if we can do this. honestly, thats what I thought the refire supression was supposed to do."
    via: "the operator in the head session, 2026-11-20, after the events-log measurement"
  - operator: aaronsb
    level: directed
    said: "remember this would have to track per agent/subagent thread, not holistically for the whole project ( a given session could spawn tens or subagents each of which need their own refire holdback metrics)"
    via: "the operator in the head session, 2026-11-20"
  - operator: aaronsb
    level: guided
    said: "I picked the vtium project because it has an unusual number of compaction cycles and presents an extreme example"
    via: "the operator in #ways-decay-control, 2026-11-20, on the session the peer model measured"
  - operator: aaronsb
    level: directed
    said: "the subagents really need to maintain their steering - without the steering they can deviate significantly more"
    via: "the operator in #ways-decay-control, 2026-11-20, rejecting a stopgap that would switch off the semantic Bash lane in subagents until ADR-188 ships; relayed verbatim by the coordinator"
  - evidence: "the events log, 2026-10-21 to 2026-11-20 (~/.local/state/agent-ways/events.jsonl and its dated archives, read by the script refire_stats.py): 194,017 fires over 126,916 (session, agent, way) groups. 34.6% of fires repeat a way already fired in the same group: 71.8% in main threads and 27.5% in subagent threads. 17.6% of main-thread groups fire 5 or more times. The gap before a refire has median 1.15 H (p10 1.02, p90 1.86), where H is the way's half-life, so a way refires almost as soon as it is eligible. Nothing in the log marks compaction; the measurement inferred it from drops in token position"
  - evidence: "the same log, simulated: the share of fires removed by a back-off of g = 2 is 14.8% in main threads if the count resets at compaction and 32.1% if it is kept, and 4.2% and 4.4% in subagent threads; at g = 3, 19.4% and 34.7% in main threads. The top repeaters are softwaredev/delivery/branching (2,380 repeats, at most 56 in one group), softwaredev/code/testing/evaluation/screens (2,238), softwaredev/delivery/commits (2,173), softwaredev/visualization/diagrams (2,095) and writing/voice (1,996). One main thread fired softwaredev/delivery about 35 times over 3 days"
  - evidence: "a peer session's model of the vtium session 6dda20bd (7 compactions, so 8 windows, claude-opus-5-5, tokens estimated as characters / 4, a clock error band of 3-8%). In the current window the Bash lane injects 206k of 222k tokens; g = 2 removes 84% of it with the count kept across compaction and no grant, 57% under this record's rule (g = 2 and g = 3 remove the same), and 21% with the count reset at compaction. Over the whole session subagent threads take 86% of injection (8.93M of 10.4M tokens), and 78% of theirs is first fires, which no back-off reaches; back-off saves subagent threads 1-4%. Of 56 semantic-lane fires that survive back-off, about 3 applied to the call, consistent with ADR-707's about 3% on topic"
  - evidence: "the same peer model, all 180 subagent threads of 6dda20bd, PreToolUse on Bash: semantic-lane hits are 10,541 fires and about 7.72M tokens (91% of the subagents' Bash-lane injection), commands: regex hits 518 fires and 0.54M (6%), and macro output 0.24M (3%). Switching the semantic Bash lane off in subagents would cut about 7.7M tokens, about 74% of all injection in the session, and at about 3% on topic would also drop about 230k tokens of guidance that applied"
  - evidence: "ADR-126 lines 126-128 describe the presets in fires per session: once is \"effectively once per session (never re-fires before end)\", rare \"1–2 fires per session\", normal \"~3 fires per session\". Lines 178-179 make Curve::Exponential the only shape the engine sees for a refire-bearing way"
  - evidence: "ADR-123 lines 118-135 define ProgressiveStaircase and ActionPotential as available curve shapes for ways; line 262 calls progressive disclosure \"a named curve variant that any way can opt into\". No way ever used either shape, and ADR-159 removed the curve: field. ADR-121 line 94, superseded, says curves with both a refractory and a decay side are possible but unused"
  - evidence: "tools/sensor-trait/src/engagement.rs:160-170, prune_decayed: for every curve but ActionPotential the history is pruned to the most recent fire, so an Exponential way's stored state cannot say how many times it fired"
  - evidence: "tools/ways-cli/src/session/engagement.rs:223 is the one comparison against REFIRE_FLOOR (0.5, line 32), so a way refires once its salience has decayed by one half-life; :254-260, load_engagement_for_tick, discards the stored state when its last fire is past the current tick"
  - evidence: "tools/ways-cli/src/cmd/hook/mod.rs:157-160: SessionStart, compaction included, calls reset::clear_session, which removes the session's state directory (tools/ways-cli/src/cmd/reset.rs:23-28). That directory sits under the runtime sessions root (tools/ways-cli/src/session.rs:46), so compaction erases every fire clock in the session"
  - evidence: "tools/ways-cli/src/cmd/show/pull.rs:142-184, stamp: a ways_read pull records a fire through record_way_fire even when the gate's outcome is Suppressed. tools/ways-cli/src/cmd/show/mod.rs:521-559: a way injected at SubagentStart is recorded as fired for the subagent by record_injected"
  - evidence: "tools/ways-cli/src/cmd/introspect/frames.rs:70-76 caches one refire threshold per way id, computed from one context window, for every agent in the session; tools/ways-cli/src/cmd/list.rs computes thresholds the same way"
  - precedent: ADR-126
  - precedent: ADR-123
  - precedent: ADR-159
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "that diagram looks reasonable"
    via: "the operator in #ways-decay-control, 2026-11-20, after seeing the ADR-711 figure of four refire schedules with the chosen rule (count kept, one fire granted per window) highlighted; confirmed in the head session the same day: \"the diagram was helpful\". The question on what once means was asked alongside and is unanswered"
    covers: [compaction-grant]
  - operator: aaronsb
    said: "I think once per context window don't you? a new window would mean it's lost"
    via: the operator in the head session, 2026-11-20, answering whether the once preset means once per context window or once for the whole session
    covers: [once]
status: proposed
date: 2026-11-20
deciders:
  - aaronsb
related:
  - ADR-121
  - ADR-123
  - ADR-126
  - ADR-159
  - ADR-188
  - ADR-606
  - ADR-707
observable:
  - "run: in one thread, a way that matches on every call fires at its first match, again after H, then after 2H, then after 4H (g = 2), and its four way_fired events carry n = 1, 2, 3, 4"
  - "run: two subagents in one session that match the same way each start with no fires and each get their own first fire; neither moves the other's n or main's"
  - "run: after a compaction, a way with n = 3 fires at its first match in the new window (n = 4) and then waits 8H; the fire row carries the next compaction epoch"
  - "run: a ways_read pull of a way leaves its n unchanged; a way injected at SubagentStart is the subagent's first fire, n = 1 after it, and its first refire waits H"
  - "run: with the back-off factor set to 1, fires match today's schedule"
  - "run: ways list and introspect, in a session with a subagent, show that subagent's threshold for a way from its own window and n, different from main's"
---

# ADR-711: Refire backs off geometrically per agent thread, with one fire granted after compaction

## Summary

- **Decided:** Each agent thread keeps, for each way, a count of how many times the way has fired in it. Main and every subagent count apart. After n fires in a thread, the next refire waits H × g^(n-1), where H is the half-life ADR-126 already gives (the way's `refire:` fraction times the context window) and g is a back-off factor in config, default 2. The count survives compaction. Compaction still clears the fire clock, so a way fires once at its first match in a new window, because compaction removed its text. Its later refires in that window wait the backed-off time. Back-off reaches only refires. A subagent's first fires are not reduced; the Bash-lane work in ADR-188 owns those. `once` comes to mean once per context window.
- **Trades away:** A way that a long thread needs again and again comes back less often, so drift it would have corrected can run longer. The count is new state outside the session directory, so it needs pruning. Thresholds become per thread, so `ways list` and introspect compute more. On the measured month, the saving is about a third of main-thread fires at most and a few percent of subagent fires; the larger subagent cost stays until ADR-188 ships. The operator rejected a stopgap that would have cut about three quarters of one heavy session's injection, to keep subagents steered.
- **One-way?** No. Setting g to 1 restores today's schedule, and the count is session-scoped state that a release can drop.
- **Probes:** *Confident (compaction-grant):* After a compaction, a way that has already fired several times in this thread comes back once in the new window, because its text is gone, and then waits longer than before. Is that the behaviour you want, rather than staying quiet until its long wait runs out? *Not confident (once):* Under this record a `once` way comes back once after each compaction. Is "once per context window" what you mean by `once`, or should it stay once for the whole session?
- **Inversion:** At one end every refire waits the same half-life, as today, and a way that matches all session keeps coming back. At the other a way fires a fixed number of times and goes dark. This record sits between: waits grow with each fire in the thread, and compaction grants one fire because the model can no longer see the text.

## Context

ADR-126 sets how soon a way may refire: its `refire:` value times the context window gives a half-life H, and the way refires once its salience has decayed by half, that is, H tokens after its last fire. The record describes its presets in fires per session ("~3 fires per session" for `normal`), but nothing bounds the fires in a thread. Every refire waits the same H, so a way that matches on every call refires every H tokens for as long as the thread runs.

The events log for 2026-10-21 to 2026-11-20 shows this. Over 194,017 fires, 34.6% repeat a way already fired in the same thread, and in main threads 71.8% do. The gap before a refire has median 1.15 H, so ways refire almost as soon as they may. One main thread fired the same delivery way about 35 times over three days. The operator's view is that a way that keeps firing through a session is less needed each time, and that refire suppression was meant to do this already.

The existing engine cannot count. For an Exponential curve, the only shape a refire-bearing way has, `prune_decayed` keeps only the last fire, so the stored state cannot say how many times the way fired. The state also does not survive compaction: SessionStart on compaction removes the session's state directory, every fire clock with it. After compaction every way that matches fires again, which is the right outcome for the first fire, because compaction removed the way's text from the context, but it also means nothing carries forward.

The cost is per thread, not per session. A session can spawn tens of subagents, and each one starts with no fires of its own, so each needs its own count. The log shows subagent threads repeat far less (27.5% of their fires) than main threads; their cost is first fires. In the most compaction-heavy session measured, subagent threads take 86% of injection, 78% of it first fires, and about 91% of their Bash-lane injection comes from the semantic lane, where ADR-707 measured about 3% of fires on topic. Back-off cannot reach a first fire. That problem belongs to ADR-188's parse-and-judge pipeline.

## Decision

### 1. A fire count per agent thread

Each (agent thread, way) pair keeps a count n of the fires the gate has recorded for that way in that thread. Threads are keyed as session state is today: main at the session root and each subagent by its agent key, resolved through `current_agent()` from `CLAUDE_AGENT_ID`. Main and every subagent count separately, and nothing sums across them.

The count lives outside the session directory, under the state root (`$XDG_STATE_HOME/agent-ways`), keyed by session id and agent key, so the compaction reset does not erase it. `/clear` starts a new session id, so it starts with no counts. The state is pruned by age, as the subagent switches are.

### 2. Geometric back-off

After n fires of a way in a thread, its next refire waits H × g^(n-1) tokens after the last fire. After the first fire it waits H, as today; at the default, 2H after the second and 4H after the third. Every fire the gate records for the way in that thread counts toward n, including the fire compaction grants (section 3).

g is a number in the ways config beside `refire_presets`, read at fire time like the presets, default 2, and exposed through `ways settings` with the other matching keys. A value of 1 turns back-off off. The engine curve does not change: the CLI resolves H × g^(n-1) into the half-life of the `Curve::Exponential` it hands the engine, as ADR-126 resolves presets today.

Back-off applies to subagent threads in the same way as to main. It is computed from the thread's own n and its own context window.

### 3. One fire granted after compaction

Compaction clears the fire clock but not the count. A way's first match after compaction fires once, because the compaction removed its text from the context. The count carries over, so the way's later refires in that window wait H × g^(n-1) with n counted across every window of the session for that thread.

The figure shows one way that matches on every call in one thread over four windows, under today's schedule, the two alternatives the record did not choose, and this rule. Today's schedule fires 24 times. Resetting the count at compaction fires 12. Keeping the count with no grant fires 5, and leaves the way dark for most of windows 2 to 4. This rule fires 6, once at the start of each later window.

![Fires of one way that matches on every call, in one agent thread, over four context windows, with H at 0.15 of the window, g = 2, and compaction at 0.8 of the window. Today's schedule fires every H, 24 fires. Resetting the count at compaction fires at 0, H and 3H in each window, 12 fires. Keeping the count with no grant fires at 0, 0.15, 0.45, 1.05 and 2.25 windows, 5 fires, and the way's text is out of context from each compaction to its next fire, most of windows 3 and 4. The chosen rule fires at 0, 0.15 and 0.45 in window 1 and once at the start of each later window, 6 fires, with no gap after compaction.](figures/ADR-711-refire-schedules.svg)

### 4. Fires the gate does not decide

- A `ways_read` pull records a fire even when the gate would have suppressed it. A pull does not count toward n: the model asked for the way, which says nothing about whether matching should bring it back.
- A way injected at SubagentStart is that subagent's first fire. It sets the thread's clock as it does today and counts toward n.
- A switched-off way's discarded fires keep their own state under `discarded-fires/` and are not changed.

### 5. What the events log records

`way_fired` and `way_suppressed` events carry the tick, the backed-off threshold in tokens, and n. Fire rows carry a compaction epoch, a number that grows by one at each compaction in the session, so a later measurement can split a thread's fires by window without inferring compaction from token positions.

### 6. Thresholds shown per thread

`ways list` and introspect compute each threshold per (agent, way), from that agent's context window and its n. Today they compute one threshold per way from main's window and show it for every agent.

### 7. `once` means once per context window

ADR-126 says `once` is "effectively once per session (never re-fires before end)". That has not held since compaction began clearing state: a `once` way fires again after each compaction. Under this record `once` means once per context window: it gets the compaction grant and never refires within a window. `rare` and `normal` likewise give no fixed number of fires per session; the record's counts describe a single window before back-off.

### Out of scope

First fires. The semantic Bash lane fires off-topic ways at their first match in every thread, and in subagent-heavy sessions those first fires are most of the cost. Back-off does not reach them and must not: a subagent's first fires are its steering. ADR-188's parse-and-judge pipeline for the Bash lane owns that work.

## Consequences

### Positive

- A way that matches all through a thread comes back less often each time. On the measured month, g = 2 with this rule removes between about 15% and 32% of main-thread fires, the two bounds being the reset and kept-count simulations; in the compaction-heavy session it removes 57% of the current window's Bash-lane injection.
- A way never stays dark after compaction removed its text, so refire keeps its purpose of correcting drift in a long session.
- Each subagent's back-off depends only on its own fires, so a session that spawns tens of subagents does not let one thread's history silence a way in another.
- The log can split fires by window, so the effect of this record, and of ADR-188 later, can be measured without inference.

### Negative

- A way a thread really does need repeatedly within one window comes back later than today. A thread that stays in one window for a long time and keeps doing the same kind of work feels this most.
- Subagent cost barely moves: back-off saves subagent threads 1-4% because their cost is first fires. Until ADR-188 ships, the semantic Bash lane keeps injecting in subagents at about 3% on topic. In the compaction-heavy session that is about 7.7M tokens, about 74% of the session's injection, which the rejected stopgap would have cut. That session is an extreme case chosen for its compaction count, so its ratios carry over to other sessions better than its absolute numbers.
- The count is new persistent state, keyed by session and agent, outside the runtime directory that compaction clears. It needs its own pruning, and a pruning bug leaks files.
- Introspect and `ways list` compute a threshold per (agent, way) instead of per way.
- The figures behind g = 2 are a simulation over logged fires; the realized saving depends on how the model behaves once ways come back less often, which the log cannot predict.

### Neutral

- `once` changes meaning on paper only: it already refired after each compaction.
- The engine in `sensor-trait` is not changed. The count lives in `ways-cli`, so ActionPotential's burst history stays the only history the engine keeps.
- A later per-way ceiling (below) can read the same count.

## Alternatives Considered

### Reset the count at compaction

Back-off would apply only within a window, and every window would start from n = 0. It is the simplest form: the count could live in the session directory. It saves too little: 14.8% of main-thread fires on the month against 32.1% with the count kept, and 21% of the compaction-heavy session's current-window Bash-lane injection against 57% under this rule. In sessions that compact often, it removes almost nothing.

### Keep the count with no grant after compaction

The fire clock would survive compaction along with the count, so a way that had fired many times would wait its long backed-off time even after compaction removed its text. It saves the most: 32.1% of main-thread fires (34.7% at g = 3), and 84% of the compaction-heavy session's current-window Bash-lane injection. It was not chosen because it keeps a way dark after the context lost it, which defeats refire's purpose of correcting drift; in the figure the way is out of context for most of windows 3 and 4.

### A per-way `max_fires:` ceiling with a global default

A way would stop firing in a thread after a fixed number of fires. Deferred, not rejected. Under the compaction grant, g already makes late refires within a window rare, and a ceiling adds a per-way authoring field. Reopen it if the log after rollout, split by compaction epoch, shows long single-window threads still repeating a way many times.

### Re-enable ADR-123's ActionPotential or ProgressiveStaircase curves through frontmatter

ADR-123 designed both shapes as available to ways, and neither was wired. ADR-159 removed the `curve:` field. Both would make back-off a per-way authoring choice, a burden on every author, while the problem is session-wide: every way that keeps matching repeats. One factor in config fixes it for every way.

### Switch the semantic Bash lane off in subagents until ADR-188 ships

A stopgap, rejected by the operator. It would cut the largest cost: in the compaction-heavy session, about 7.7M tokens, about 74% of all injection, all of it first fires that back-off cannot reach. It would also drop the guidance that applied, about 3% of those fires and about 230k tokens. The operator rejected it because subagents without that steering deviate more. The cost of the rejection is that subagent injection stays near its current level until ADR-188's parse-and-judge pipeline replaces the lane.

## References

- ADR-126: the half-life H and the presets this record backs off from, and the `once` claim it corrects.
- ADR-123: the curve shapes this record does not re-enable.
- ADR-159: the removal of the `curve:` field.
- ADR-188: the Bash-lane rebuild that owns first fires.
- ADR-707: the about 3% on-topic rate of the semantic Bash lane.
- ADR-606: the deferred delivery that ADR-188 builds on.
