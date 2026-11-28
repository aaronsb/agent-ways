---
contract: adr/v1
kind: decision
verb: add
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "I'm considering a scenario where I let a top level Claude code session work autonomously in handoffs, and I start another Claude code session that their main job is to basically check in every now and then, say on a monitor that wakes up every 30 minute, to just get a status report."
    via: "the operator in chat with the coordinating session, 2026-11-28, relayed verbatim by the coordinator; the two roles and the 30-minute check"
  - operator: aaronsb
    level: directed
    said: "A peer message of 'do you need any assistance?' Is not a command and likely enough to get an agent going after a handoff."
    via: "the operator in chat with the coordinating session, 2026-11-28, relayed verbatim by the coordinator; the neutral nudge"
  - operator: aaronsb
    level: guided
    said: "That supervisor agent would have some boundaries. It could even be a skill."
    via: "the operator in chat with the coordinating session, 2026-11-28, relayed verbatim by the coordinator; the boundaries in item 5 were worked out by the agent within this direction, and the four cases in item 3 were agent-written in the same chat"
  - evidence: "`ways settings get mods.attend.continuity.max_relays` on the operator's host, 2026-11-28, prints 10; `mods.attend.continuity.relay` prints on and `margin_minutes` prints 10"
  - evidence: "tools/sensor-keepwarm/src/policy.rs on main, `route()`: `wrap_to` passes through as an opaque string, with empty and `#open` replaced by the operator's member id; plugins/attend/hooks/lib.ts sends it as `attend message send --to <wrap_to>`, or `--channel` for `#name`; tools/attend/src/verbs/send.rs `resolve_named` takes a session id first, then a session's full name, then a nickname, then a person's member id, and `resolve_recipient` refuses a finished session with `RecipientFinished`"
  - evidence: "tools/attend/src/verbs/peers.rs on main: a session's name is its persona nickname with an instance suffix, and the first live session in a directory holds the bare name (ADR-129, amendment of 2026-11-24), so a name can move between sessions as they start and exit; a session id cannot"
  - evidence: "tools/sensor-keepwarm/src/policy.rs on main, `hand()`: a relay stop (stop empty, decision or doubt, `max_relays` reached, or a stale handoff) sends the wrap's reply to the route's recipient once, opening with a `handoff:` line that names why the relay stopped; plugins/attend/hooks/register.tsx `sendRelayHandoff` sends it and writes it to `wrap_file` when the send is refused"
  - evidence: "origin/build/adr-418-wave4, plugins/attend/hooks/boundary.ts `settle()`: the boundary tool sends the handoff (`isHandoffSent`) only for a doubt; a judged stop arms the exit and sends nothing"
  - evidence: "origin/build/adr-418-wave4, tools/sensor-keepwarm/src/policy/armed.rs: an armed exit ticks at the last request plus the cache lifetime less `margin_minutes` (default 10, range 3 to 30), so 50 idle minutes on the one-hour tier; with nothing pending and a sufficient handoff the first tick sends no ping and records `finished`; a thread the session sent that awaits an answer counts as pending (tools/attend/src/verbs/awaiting.rs)"
  - evidence: "tools/sensor-keepwarm/src/policy.rs on main: after a relay stop the policy records `stopped`, sends no keepalive until the operator's next prompt, and arms no exit, so the cache lapses at its lifetime and no finished mark is set"
  - evidence: "plugins/attend/hooks/register.tsx `poll` and `prompt.submit`: messages drained on a lapsed cache wait in `isPendingCold` for the operator's next prompt and start no turn; a peer message is submitted with `asUser: true`, whose origin stays `plugin` (Claude Code 2.1.332 bundled types), and only the origins in `OPERATOR_ORIGINS` (composer, bridge, sdk, slack-ping) reset the relay count, disarm an exit or clear the finished mark"
  - evidence: "origin/build/adr-418-wave4, tools/attend/src/cmd/keepwarm.rs: `attend keepwarm status --session <id>` takes a session id, not a name, and exits 3 when no session record names it; with `--json` it prints only `exit` and `boundary` from the ledger and reads no transcript, so the cache state, idle time and relay count appear only on the plain card"
  - precedent: ADR-418
  - precedent: ADR-417
  - precedent: ADR-414
  - precedent: ADR-182
observable:
  - "run: `ways settings get mods.attend.continuity.wrap_to --project <worker repo>` prints the supervisor's session id"
  - "see: a worker whose relay stops on `relay: stop decision` sends the supervisor one message opening `handoff:` at once; the supervisor wakes on it, and the operator receives one direct message in attend-chat with a summary and the handoff's DECISIONS lines quoted"
  - "see: a worker idle after a judged stop, with open tasks and a warm cache, receives one message from the supervisor that asks an open question and holds no instruction, within 30 minutes of the stop and before its first keepwarm tick; its next turn starts from it"
  - "run: after the nudge, `attend keepwarm status --session <worker id>` shows the same relay count as before it, and the worker's exit is not reset"
  - "see: a worker whose relay stopped at 10 of 10 gets no nudge; the operator gets one page, and the supervisor's loop ends"
  - "see: the supervisor's log holds one line per check, and reads the same after the supervisor compacts"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-11-28
deciders:
  - aaronsb
related:
  - ADR-129
  - ADR-171
  - ADR-406
---

# ADR-419: A supervisor session watches a relay-mode worker over attend and nudges it with neutral questions, and forwards what only the operator can decide

## Summary

- **Decided:** While the operator is away, a second Claude Code session, the supervisor, watches a worker session that runs in relay mode. The worker's `wrap_to` names the supervisor by session id, so a relay that stops hands its handoff to the supervisor at once. The supervisor runs a skill, `/supervise <worker>`, that wakes every 30 minutes, reads the worker's state from attend, and sorts it into one of four cases. A worker that is working is left alone. A worker idle with open tasks while its cache is still warm gets one neutral question, such as "do you need any assistance?". A worker stopped on a decision or on doubt is forwarded to the operator with a summary. A worker that has finished, gone cold or used all its relays is reported to the operator once, and the loop ends. The supervisor has attention and no authority. It never instructs, never answers a decision, never writes to the worker's repository, never resets the worker's relay count or finished mark, and nudges at most once per idle stretch.
- **Trades away:** a second session that costs a cache read of its own context at each check, about 48 checks a day. The nudge can be ignored, since a peer message is data and silence is a valid reply, and nothing then moves until the operator returns. A worker whose relay count is exhausted is not nudged on, so an unattended run still ends at `max_relays` even when work remains. The skill depends on two things not on main: `attend keepwarm status --session` (ADR-418 wave 4, on its build branch) and three fields its `--json` output does not carry yet.
- **One-way?** No. Reversible: the supervisor is a skill and a session, and the worker's link to it is one setting, `wrap_to`. Clearing it sends the handoff to the operator again.
- **Probes:** *Confident (nudge-only):* the supervisor only asks open questions like "do you need any assistance?" and passes your decisions on to you; it never answers them, never touches the worker's code and never resets its limits. Lean: yes. *Not confident (cap):* when the worker stops because it used all 10 relays, the supervisor tells you and stops, rather than nudging it into another stretch, so 10 relays still ends an unattended run. Lean: yes; the alternative is one nudge past the cap, which buys another context window of work each time and leaves the run with no fixed end.
- **Inversion:** at one end the operator is the only link the worker waits on, and a worker that stalls after a handoff idles until its cache lapses. At the other end a second agent holds the operator's authority: it answers decisions, clears marks and resets the count, and the run has no human bound. This record sits between them: the supervisor carries attention, and the operator keeps every call that changes what the worker may do. Giving the supervisor that authority is named as an alternative and left open.

## Context

Relay mode lets a session carry its work across context windows on its own: it wraps, compacts on the handoff, and resumes, up to `max_relays` times after the operator's last prompt. The boundary tool adds a clean judge that can stop the session at a work boundary, after which keepwarm holds the cache only while it pays.

What relay mode does not cover is a worker that stops, or idles, while work remains and nobody is there. A relay that stops on a decision sends its handoff to `wrap_to`, which is the operator by default, and the operator may be asleep. A judged stop arms an exit and sends nothing. An agent that ends its turn after a resume, asking a question the handoff told it not to ask, waits for a prompt that does not come. In each case the worker sits idle until its cache lapses, and from then on nothing wakes it.

The operator's proposal is a second session whose job is to check in every 30 minutes and get a status report, with a peer message as light as "do you need any assistance?" to get a stalled worker going. attend already carries what such a session needs: the peer list shows each session's state and context fill, keepwarm's ledger shows the worker's last boundary and its exit, and a direct message wakes an idle session whose cache is warm.

## Decision

### 1. The two sessions

- **The worker** runs in relay mode and listens on attend, on the one-hour cache tier. Its `mods.attend.continuity.wrap_to` names the supervisor by session id. `wrap_to` accepts a session's name too, since attend resolves it as `attend message send --to` does, but a name can move to another session as sessions in a directory start and exit, and an id cannot.
- **The supervisor** is a separate Claude Code session that listens on attend. It runs from its own directory, not the worker's repository, with `mods.attend.continuity.relay: off` in that directory's settings, so its own fill never triggers a relay and the worker's project settings do not apply to it. It runs `/supervise <worker>`, where `<worker>` is the worker's name or session id.
- **The operator** sets both up and leaves. A prompt from the operator in either session ends the arrangement for that stretch (item 5).

### 2. The check

Every 30 minutes the skill wakes the supervisor and runs one check. The wake keeps the supervisor's own cache warm, since 30 minutes is inside its cache lifetime, so a handoff the worker sends in between wakes the supervisor at once.

| Read | What it gives |
|---|---|
| `attend peer list --json` | the worker's session id, its `status` (`working` when its transcript changed in the last 30 seconds, `waiting`, or `finished` with an age), `finished` and `context_pct` |
| `attend keepwarm status --session <worker id> --json` | the worker's exit (`armed`, `finished`, or the move a boundary carried out), pings sent and left, and its last boundary: the judge's verdict, its answer on the handoff, the agent's own call and the reason |
| the worker's `.claude/handoff.wrap.md`, read only | the open TASKS and the DECISIONS with what each blocks |
| messages the worker sent to the supervisor | a `handoff:` message, whose first line says why the relay stopped |

The JSON output of `attend keepwarm status` does not yet carry three things the check needs: whether the cache is warm and when the next tick falls, the relay count as n of `max_relays`, and the time of the operator's last prompt. The plain card shows them. The build adds them to the JSON object, so the skill reads no text written for people.

### 3. The four cases

The check sorts the worker into the first case that holds, in this order.

| Case | Read as | The supervisor |
|---|---|---|
| Stopped on a decision or doubt | a `handoff:` message whose relay stopped on `decision` or `doubt`, or a boundary whose move was `doubt` | sends the operator one direct message over attend: a summary of what landed since the last report, and the handoff's DECISIONS lines quoted as written. It does not nudge the worker. |
| Finished, cold or at the cap | exit `finished`; status `finished`; a cache that has lapsed; a relay stopped on `empty`; or a relay stopped at n of `max_relays` | sends the operator one report, then ends its loop. |
| Idle with open tasks, inside the warm window | status `waiting` at two checks in a row, or an exit `armed`; open TASKS in the handoff; a warm cache before the next tick; relays below the cap | sends the worker one neutral question, such as "Do you need any assistance?", as a direct message. |
| Working | anything else | stays quiet and logs the check. |

A stop at the cap is reported rather than nudged. After a relay stops at `max_relays` the mod compacts with the handoff and submits no resume, so a nudge would start another context window of work. One nudge per idle stretch would then buy one window per stop, and the run would have no end. `max_relays` counts from the operator's last prompt so that an unattended run is bounded, and the supervisor keeps that bound.

### 4. Timing

The figure below follows one worker across a run: its relays, a judged stop, the supervisor's checks, the nudge inside the warm window, a later stop on a decision, and the bound `max_relays` sets from the operator's last prompt.

![Sequence over a time axis in three lanes: worker, supervisor and operator. The operator's last prompt starts the relay count; a bracket over the whole run says max_relays, 10 on this host, counts relays from that prompt and a nudge resets nothing. The worker relays for hours, then makes a judged stop at a boundary, which sends nothing, and idles with its exit armed. The supervisor's checks fall every 30 minutes; the one at about 28 minutes after the stop finds the worker idle with open tasks and a warm cache and sends one neutral nudge, labelled need any help?, and the worker resumes. A dashed line at about 50 idle minutes marks the first keepwarm tick, where without the nudge the cache would lapse and attend would mark the worker finished, refusing sends. Later the worker's relay stops on a decision: an arrow labelled handoff goes to the supervisor at once, by wrap_to, and an arrow labelled decision forwarded goes on to the operator, who is paged with a summary.](figures/ADR-419-supervisor-timeline.svg)

- **The warm window after a judged stop.** An armed exit ticks at the last request plus the cache lifetime, less `margin_minutes`: 50 idle minutes on the one-hour tier with the default margin of 10. With nothing pending and a sufficient handoff, that first tick sends no ping, the cache lapses, and attend marks the worker finished. From then on attend refuses a message to it, so a nudge has to land before the tick.
- **A 30-minute check always lands inside it.** The window lasts at least 30 minutes for any margin attend accepts (3 to 30), and checks 30 minutes apart always put one inside a window that long. At a margin of 30 the check can fall at the window's very end, so the skill skips the nudge when the next tick is under two minutes away and reports instead.
- **After a relay stop.** A relay that stops arms no exit. The policy sends no keepalive until the operator's next prompt, the cache lapses at its lifetime, and no finished mark is set. The handoff reaches the supervisor at once, so the supervisor does not wait for a check.
- **Pending keeps an armed worker warm.** A direct message the worker sent that no recipient has answered counts as pending, and an armed exit with something pending pings up to the price of a fresh start. A handoff the boundary tool sends on a doubt goes to the supervisor, so it stays pending until the supervisor answers it. The supervisor does not answer a forwarded handoff, so the worker stays warm for the operator's answer within that bound.
- **Cold.** A message that reaches a worker whose cache has lapsed starts no turn. It waits for the operator's next prompt. A cold worker is reported, never nudged.
- **The cache tier.** On the five-minute tier the window is a few minutes and the loop misses it. The worker runs on the one-hour tier.

### 5. The supervisor's boundaries

- **Neutral, open questions only.** The nudge asks; it does not tell. It carries no instruction, plan, code, or text from the handoff. "Do you need any assistance?" and "Is anything blocking you?" fit. "Continue with the migration" does not.
- **Decisions go to the operator.** The supervisor never answers a DECISIONS item, never says which way it leans, and never tells the worker a decision is settled. It forwards each one as written.
- **Read-only on the worker's repository.** It reads the handoff and may read files and `git log`. It writes, commits and pushes nothing there, and runs nothing that does.
- **At most one nudge per idle stretch.** An idle stretch ends when the worker's status reads `working` again or a new boundary is recorded. A worker that does not answer the nudge is left alone and reported at the next case change.
- **The worker's bounds stay the operator's.** The supervisor never runs `attend keepwarm reset`, `attend keepwarm on` or `ways settings set` for the worker, and never sends anything meant to stand in for an operator prompt. attend backs this up: a peer message reaches the worker as a plugin-origin prompt, and only the operator's own prompt resets the relay count, disarms an exit or clears the finished mark.
- **When it stops.** The loop ends when the worker finishes, goes cold, or reaches the cap; when the operator prompts the worker or the supervisor; and at the supervisor's own context boundary, where it runs `/wrap handoff`, which wraps without relaying whatever the setting says.

### 6. The supervisor's memory

The supervisor keeps a running log in its own directory, in a file named for the worker's session id. Each check appends one line: the time, the worker's status and fill, the relay count, the exit, the case, and what the supervisor did. The worker compacts at every relay and forgets its earlier windows, and the log keeps the thread across them. The supervisor's reports to the operator are drawn from it, and a supervisor that wraps and starts again reads it first.

### 7. Where it lives

The supervisor is a skill, `skills/supervise/SKILL.md`, with the cases, the boundaries and the nudge text in it. It needs no change to the attend mod. It needs `attend keepwarm status --session`, which the ADR-418 build adds, and the three JSON fields in item 2.

## Consequences

### Positive

- A worker that stalls after a handoff, or stops at a judged boundary with work left, gets one prompt to go on while it can still be woken, instead of idling until its cache lapses.
- A decision the worker cannot make reaches the operator once, as a summary with the questions quoted, rather than as a raw handoff.
- The operator returns to a log of the run and a report of how it ended.
- The arrangement adds no authority anywhere: every limit on the worker stays where the operator set it, and attend enforces the one that matters, that a peer message is no operator prompt.

### Negative

- The supervisor is a second session with its own cost: one cache read of its context every 30 minutes, about 48 a day, plus the reports. At Claude Opus 5.5 list prices and a 100k-token context, a check costs about $0.02, about $1 a day before its own compactions.
- A nudge the worker ignores changes nothing, and the supervisor does not nudge again in that stretch.
- A worker nudged after a judged stop goes back to work with its exit still armed. If it idles again without reaching a new boundary, with nothing pending, its next tick finishes it, because the handoff's first line still carries the old wrap id and the exit is still in force.
- Two sessions now take part in the run's continuity, and a misread case (a worker read as idle while a long tool call runs) sends a nudge to a working session. The two-checks rule for `waiting` makes this rare, and the nudge is harmless when it happens.
- The worker's peer status reads `working` only when its transcript changed in the last 30 seconds, so the check relies on the ledger and the cache state as much as on the status.

### Neutral

- Implementation adds the `supervise` skill, and three fields to `attend keepwarm status --json`: the cache state with the next tick, the relay count, and the operator's last prompt.
- The worker's setup is two settings: relay on, and `wrap_to` naming the supervisor's session id. The supervisor's is relay off in its own directory.
- The setup works with the boundary tool and without it. Without it the worker reaches the supervisor only through relay stops and idle stretches.

## Alternatives Considered

- **No supervisor.** The worker relays, and its stops go to the operator. Rejected: the operator asked not to be the link the worker waits on, and a worker that stalls with work left idles until its cache lapses, after which nothing wakes it.
- **A supervisor that nudges only.** Chosen. It covers the stall case with the lightest touch the operator named, and adds no authority.
- **A supervisor with operator authority.** The supervisor could clear the finished mark, reset the relay count, and answer decisions. Not chosen now, and left open. It would let one model lift the bounds the operator set on another, so an unattended run would have no human limit, and it would make a peer message act as an instruction, which attend's contract says it is not. It is the step to take only if nudges prove too weak (Open questions).
- **A keepwarm ping as the nudge.** The worker's own keepwarm wake already starts a turn. Rejected: that wake asks for one word and no tools so that it costs one cache read, and turning it into a resume would make every keepalive a turn of work, which the armed exit is designed to avoid.
- **A shell script on a timer.** A script can read the same JSON. Rejected: it cannot tell a stall from a decision by reading a handoff, or write the operator a summary.
- **`wrap_to` as a channel.** A channel post never counts as awaiting a reply, so a doubt's handoff would not keep the worker warm for the operator's answer. A direct message to the supervisor's session id does.

## Open questions

1. **Operator authority for the supervisor.** If nudges prove too weak, which of the operator's powers, if any, should a supervisor hold, and with what bound of its own? This is the named alternative, deferred.
2. **The wake mechanism.** The operator named a monitor that wakes every 30 minutes. Whether the skill uses Claude Code's Monitor tool or `/loop` for that, and how either behaves across the supervisor's own compactions, is not verified.
3. **Whether the nudge works.** That a neutral question is enough to get a stalled agent going is the operator's expectation, not a measurement. The log will show how often a nudge is followed by `working`.
4. **The supervisor's model.** A smaller model may be enough to sort four cases and write a summary, at a fraction of the cost. Not decided here.

## References

- ADR-418 — the boundary tool, the judge, the armed exit and its tick, and the finished mark that refuses sends; the supervisor reads its ledger and times the nudge to its tick.
- ADR-417 — relay mode, `max_relays` counted from the operator's last prompt, and the stop that sends the handoff to `wrap_to`; `/wrap handoff` forces handoff mode.
- ADR-414 — `wrap_to` and the `handoff:` message, and the operator prompt that resets the counts.
- ADR-182 — keepwarm's 50-minute wake on the one-hour cache.
- ADR-129 — session names and instance suffixes, which is why the worker names the supervisor by session id.
- ADR-171 — stable session identity.
- ADR-406 — the attend mod, through which peer messages reach a session as data.
