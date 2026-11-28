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
  - evidence: "plugins/attend/hooks/boundary.ts on main (attend 0.39.0) `settle()`: the boundary tool sends the handoff (`isHandoffSent`) only for a doubt; a judged stop arms the exit and sends nothing"
  - evidence: "tools/sensor-keepwarm/src/policy/armed.rs on main (attend 0.39.0): an armed exit ticks at the last request plus the cache lifetime less `margin_minutes` (default 10, range 3 to 30), so 50 idle minutes on the one-hour tier; with nothing pending and a sufficient handoff the first tick sends no ping and records `finished`; a thread the session sent that awaits an answer counts as pending (tools/attend/src/verbs/awaiting.rs)"
  - evidence: "tools/sensor-keepwarm/src/policy.rs on main: after a relay stop the policy records `stopped`, sends no keepalive until the operator's next prompt, and arms no exit, so the cache lapses at its lifetime and no finished mark is set"
  - evidence: "plugins/attend/hooks/register.tsx `poll` and `prompt.submit`: messages drained on a lapsed cache wait in `isPendingCold` for the operator's next prompt and start no turn; a peer message is submitted with `asUser: true`, whose origin stays `plugin` (Claude Code 2.1.332 bundled types), and only the origins in `OPERATOR_ORIGINS` (composer, bridge, sdk, slack-ping) reset the relay count, disarm an exit or clear the finished mark"
  - evidence: "tools/attend/src/cmd/keepwarm.rs on main (attend 0.39.0): `attend keepwarm status --session <id>` takes a session id, not a name, and exits 3 when no session record names it; with `--json` it prints only `exit` and `boundary` from the ledger and reads no transcript, so the cache state, idle time and relay count appear only on the plain card"
  - evidence: "tools/sensor-keepwarm/src/policy.rs and policy/armed.rs on main (attend 0.39.0), read 2026-11-28 and stepped in a throwaway policy test that was not committed: once a relay wrap is judged at `max_relays`, `judge_relay` records the stop (`10 of 10` on this host), asks for one compaction with the handoff and no resume, and from then until the operator's next prompt returns `stopped` with no action at every check, ahead of the fill path and the armed exit; so a turn a peer starts gets no drain, no wrap at the hard line and no further compaction, and `compact_refusal` refuses a boundary `compact` with `the relay stopped: 10 of 10`, carried out as `stop`, whose exit then runs no tick. Where the cap is reached through boundary compactions with no relay wrap, `compact_refusal` says `max_relays reached` and the stop arms an exit that ticks as usual"
  - evidence: "~/.claude/settings.json on the operator's host, 2026-11-28: `autoCompactEnabled` is false, so Claude Code's own compaction does not run; the wrap way says an agent cannot run `/compact` itself"
  - precedent: ADR-418
  - precedent: ADR-417
  - precedent: ADR-414
  - precedent: ADR-182
observable:
  - "run: `ways settings get mods.attend.continuity.wrap_to --project <worker repo>` prints the supervisor's session id"
  - "see: a worker whose relay stops on `relay: stop decision` sends the supervisor one message opening `handoff:` at once; the supervisor wakes on it, and the operator receives one direct message in attend-chat with a summary and the handoff's DECISIONS lines quoted"
  - "see: a worker idle after a judged stop, with open tasks and a warm cache, receives one message from the supervisor that asks an open question and holds no instruction, within 30 minutes of the stop and before its first keepwarm tick; its next turn starts from it"
  - "run: after the nudge, `attend keepwarm status --session <worker id>` shows the same relay count as before it, and the worker's exit is not reset"
  - "see: a worker whose relay stopped at 10 of 10, with open tasks and a warm cache, gets one neutral nudge and works on; `attend keepwarm status --session <worker id>` still shows 10 of 10 after it, and the supervisor's log records that the run went past the cap"
  - "see: the supervisor's log holds one line per check, and reads the same after the supervisor compacts"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "3. It's guidance not a hard rule [the supervisor's boundaries, carried by the skill]. Supervisors supervise not do the work."
    via: the operator in chat with the coordinating session, 2026-11-28, answering this record's two probes; relayed verbatim by the coordinator
    covers: [nudge-only]
  - operator: aaronsb
    said: "4. No [the cap does not end an unattended run, and the supervisor may nudge a worker past max_relays]"
    via: the operator in chat with the coordinating session, 2026-11-28, answering this record's two probes; relayed verbatim by the coordinator
    covers: [cap]
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

- **Decided:** While the operator is away, a second Claude Code session, the supervisor, watches a worker session that runs in relay mode. The worker's `wrap_to` names the supervisor by session id, so a relay that stops hands its handoff to the supervisor at once. The supervisor runs a skill, `/supervise <worker>`, that wakes every 30 minutes, reads the worker's state from attend, and sorts it into one of four cases. A worker that is working is left alone. A worker idle with open tasks while its cache is still warm gets one neutral question, such as "do you need any assistance?", and that holds for a worker that has used all its relays too. A worker stopped on a decision or on doubt is forwarded to the operator with a summary. A worker that has finished or gone cold is reported to the operator once, and the loop ends. The supervisor supervises and does not do the work. The skill gives it that principle and its limits as guidance, not as rules a tool enforces: it nudges with neutral questions, passes decisions on to the operator as written, stays out of the worker's repository, leaves the worker's relay count and finished mark to the operator, and nudges once per idle stretch. The operator can direct it to do more.
- **Trades away:** a second session that costs a cache read of its own context at each check, about 48 checks a day. The nudge can be ignored, since a peer message is data and silence is a valid reply, and nothing then moves until the operator returns. `max_relays` no longer ends an unattended run. A nudge past the cap starts more work, so the run has no fixed end in relays, and what bounds it is the operator's return, the supervisor's own context and stop conditions, and the cost. In the code today a nudge past the cap buys at most the rest of one context window, because attend compacts nothing after the cap's stop and Claude Code's own compaction is off on the operator's host. The limits hold as far as the supervisor follows its skill, and attend enforces only one of them: a peer message is no operator prompt. The skill needs three fields that `attend keepwarm status --json` does not carry yet.
- **One-way?** No. Reversible: the supervisor is a skill and a session, and the worker's link to it is one setting, `wrap_to`. Clearing it sends the handoff to the operator again.
- **Probes:** *Confident (nudge-only):* the supervisor only asks open questions like "do you need any assistance?" and passes your decisions on to you; it never answers them, never touches the worker's code and never resets its limits. Lean: yes. *Not confident (cap):* when the worker stops because it used all 10 relays, the supervisor tells you and stops, rather than nudging it into another stretch, so 10 relays still ends an unattended run. Lean: yes; the alternative is one nudge past the cap, which buys another context window of work each time and leaves the run with no fixed end. *Answered 2026-11-28:* on nudge-only the operator said "It's guidance not a hard rule. Supervisors supervise not do the work.", so the limits are guidance the skill carries, under that principle (item 5). On cap the operator said "No": the supervisor nudges a worker past `max_relays`, and the cap no longer ends the run (item 3).
- **Inversion:** at one end the operator is the only link the worker waits on, and a worker that stalls after a handoff idles until its cache lapses. At the other end a second agent stands in for the operator: it does the work, answers decisions, clears marks and resets the count. This record sits between them: the supervisor carries attention and keeps the worker going, and the operator keeps the decisions, the worker's limits and the right to direct the supervisor further. With the cap no longer ending the run, the operator's return is the bound that is left. Giving the supervisor the operator's authority is named as an alternative and left open.

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
| Finished or cold | exit `finished`; status `finished`; a cache that has lapsed; or a relay stopped on `empty` | sends the operator one report, then ends its loop. |
| Idle with open tasks, inside the warm window | status `waiting` at two checks in a row, an exit `armed`, or a relay stopped at n of `max_relays`; open TASKS in the handoff; a warm cache, before the next tick where an exit is armed | sends the worker one neutral question, such as "Do you need any assistance?", as a direct message. At the cap it also logs that the run went past `max_relays`. |
| Working | anything else | stays quiet and logs the check. |

A stop at the cap is nudged like any other idle stretch with work left. `max_relays` counts relays from the operator's last prompt, and a nudge is no operator prompt, so the count stays at the cap and the worker's limits stay where the operator set them. The run then has no fixed end in relays. What bounds it is the operator's return, the supervisor's own context and its stop conditions (item 5), and the cost of the work the worker does.

> Edited 2026-11-28: the operator answered the cap probe "No". The paragraph read: "A stop at the cap is reported rather than nudged. After a relay stops at `max_relays` the mod compacts with the handoff and submits no resume, so a nudge would start another context window of work. One nudge per idle stretch would then buy one window per stop, and the run would have no end. `max_relays` counts from the operator's last prompt so that an unattended run is bounded, and the supervisor keeps that bound." The cap was a case of its own beside finished and cold, and the idle case required relays below the cap.

#### What a nudge past the cap does

The cap probe priced a nudge past the cap at one more context window each time. The code on main (attend 0.39.0) gives less than that.

- **At the cap the relay stops for good.** When a relay wrap is judged at `max_relays`, attend records the stop (`10 of 10` on the operator's host), the mod compacts once with the handoff, and it submits no resume. Until the operator's next prompt every check returns at that stop: attend sends no keepalive, offers no drain, sends no wrap at the hard line, runs no armed-exit tick and asks for no further compaction.
- **A nudge starts work in the window that is left.** A nudge that reaches the worker while its cache is warm starts a turn, and the worker works on from the compacted handoff. The nudge resets nothing.
- **The boundary tool compacts no more.** At a later boundary a `compact` is refused with "the relay stopped: 10 of 10" and carried out as `stop`. That stop records an armed exit, but no tick runs while the relay is stopped, so attend never finishes the worker: its cache lapses at its lifetime and no finished mark is set. A `doubt` still sends its handoff, since the mod sends that one itself.
- **The other way to reach the cap.** Where the count reaches `max_relays` through compactions the boundary tool started, with no relay wrap, the tool refuses `compact` with "max_relays reached" and the stop arms an exit that ticks as after any judged stop. If the fill path later sends a relay wrap, that wrap stops the relay at the cap as above, with its one compaction. Either way, attend runs at most one compaction after the cap is reached, and no resume after it.
- **The window fills.** The worker cannot run `/compact` itself, and Claude Code's own compaction is off on the operator's host (`autoCompactEnabled: false`), so nothing compacts the worker once that window fills. What Claude Code does when a window fills with its compaction off is not verified here; the expected result is that the worker's requests fail until the operator compacts or clears the session. Where Claude Code's compaction is on, it would compact the window with its own summary and the worker could go on, window after window; that is not verified either.

So on the operator's host each nudge past the cap buys the rest of the one window the cap's compaction left, not a new window per nudge. The trade the cap probe named, one window per nudge and no fixed end, is the upper bound, and it holds where something other than attend compacts the worker. These findings come from reading the policy and from a throwaway test of it, not from a live run.

### 4. Timing

The figure below follows one worker across a run: its relays, a judged stop, the supervisor's checks, the nudge inside the warm window, a later stop on a decision, and the relay count, which runs from the operator's last prompt and has no closing bracket, since the supervisor may nudge past `max_relays`.

![Sequence over a time axis in three lanes: worker, supervisor and operator. The operator's last prompt starts the relay count; an open-ended bracket runs from that prompt past the right edge and says the relay count runs from it, a nudge resets nothing, and the supervisor may nudge past max_relays, 10 on this host, so the run has no fixed end. The worker relays for hours, then makes a judged stop at a boundary, which sends nothing, and idles with its exit armed. The supervisor's checks fall every 30 minutes; the one at about 28 minutes after the stop finds the worker idle with open tasks and a warm cache and sends one neutral nudge, labelled need any help?, and the worker resumes. A dashed line at about 50 idle minutes marks the first keepwarm tick, where without the nudge the cache would lapse and attend would mark the worker finished, refusing sends. Later the worker's relay stops on a decision: an arrow labelled handoff goes to the supervisor at once, by wrap_to, and an arrow labelled decision forwarded goes on to the operator, who is paged with a summary.](figures/ADR-419-supervisor-timeline.svg)

- **The warm window after a judged stop.** An armed exit ticks at the last request plus the cache lifetime, less `margin_minutes`: 50 idle minutes on the one-hour tier with the default margin of 10. With nothing pending and a sufficient handoff, that first tick sends no ping, the cache lapses, and attend marks the worker finished. From then on attend refuses a message to it, so a nudge has to land before the tick.
- **A 30-minute check always lands inside it.** The window lasts at least 30 minutes for any margin attend accepts (3 to 30), and checks 30 minutes apart always put one inside a window that long. At a margin of 30 the check can fall at the window's very end, so the skill skips the nudge when the next tick is under two minutes away and reports instead.
- **After a relay stop.** A relay that stops arms no exit. The policy sends no keepalive until the operator's next prompt, the cache lapses at its lifetime, and no finished mark is set. The handoff reaches the supervisor at once, so the supervisor does not wait for a check. After a stop at the cap the mod has just compacted, so the worker's cache is warm for its lifetime, and the supervisor's nudge lands inside it.
- **Pending keeps an armed worker warm.** A direct message the worker sent that no recipient has answered counts as pending, and an armed exit with something pending pings up to the price of a fresh start. A handoff the boundary tool sends on a doubt goes to the supervisor, so it stays pending until the supervisor answers it. The supervisor does not answer a forwarded handoff, so the worker stays warm for the operator's answer within that bound.
- **Cold.** A message that reaches a worker whose cache has lapsed starts no turn. It waits for the operator's next prompt. A cold worker is reported, never nudged.
- **The cache tier.** On the five-minute tier the window is a few minutes and the loop misses it. The worker runs on the one-hour tier.

### 5. The supervisor's limits, as guidance

The supervisor supervises and does not do the work. The skill states the limits below as guidance for the supervisor's judgment under that principle. No tool enforces them, apart from the one attend enforces on its own: a peer message reaches the worker as a plugin-origin prompt, and only the operator's own prompt resets the relay count, disarms an exit or clears the finished mark. The operator can direct the supervisor to do more than this guidance says, in the `/supervise` call or in the supervisor's session, and the supervisor follows the operator's direction over the skill's defaults.

- **Neutral, open questions.** The nudge asks rather than tells. It carries no instruction, plan, code, or text from the handoff. "Do you need any assistance?" and "Is anything blocking you?" fit. "Continue with the migration" does not.
- **Decisions go to the operator.** The supervisor forwards each DECISIONS item as written. It does not answer one, say which way it leans, or tell the worker that a decision is settled.
- **Out of the worker's repository.** The supervisor reads the handoff and may read files and `git log`. It does not write, commit or push there, or run anything that does.
- **One nudge per idle stretch.** An idle stretch ends when the worker's status reads `working` again or a new boundary is recorded. A worker that does not answer the nudge is left alone and reported at the next case change.
- **The worker's limits stay the operator's.** The supervisor does not run `attend keepwarm reset`, `attend keepwarm on` or `ways settings set` for the worker, and sends nothing meant to stand in for an operator prompt. Past the cap it nudges as it does below the cap, which starts more work and leaves the count where it is.
- **When it stops.** The loop ends when the worker finishes or goes cold; when the operator prompts the worker or the supervisor; and at the supervisor's own context boundary, where it runs `/wrap handoff`, which wraps without relaying whatever the setting says.

> Edited 2026-11-28: the operator answered the nudge-only probe "It's guidance not a hard rule. Supervisors supervise not do the work." The section was titled "The supervisor's boundaries" and stated each limit as a rule: "Neutral, open questions only", "never answers a DECISIONS item", "Read-only on the worker's repository", "At most one nudge per idle stretch", and "The supervisor never runs `attend keepwarm reset` [...]". The loop also ended when the worker reached the cap.

### 6. The supervisor's memory

The supervisor keeps a running log in its own directory, in a file named for the worker's session id. Each check appends one line: the time, the worker's status and fill, the relay count, the exit, the case, and what the supervisor did. The worker compacts at every relay and forgets its earlier windows, and the log keeps the thread across them. The supervisor's reports to the operator are drawn from it, and a supervisor that wraps and starts again reads it first.

### 7. Where it lives

The supervisor is a skill, `skills/supervise/SKILL.md`, with the cases, the guidance in item 5 and the nudge text in it. It needs no change to the attend mod. It reads `attend keepwarm status --session`, which attend 0.39.0 ships with the boundary tool, and it needs the three JSON fields in item 2, which no release carries yet.

## Consequences

### Positive

- A worker that stalls after a handoff, or stops at a judged boundary with work left, gets one prompt to go on while it can still be woken, instead of idling until its cache lapses.
- A decision the worker cannot make reaches the operator once, as a summary with the questions quoted, rather than as a raw handoff.
- The operator returns to a log of the run and a report of how it ended.
- A worker that reaches `max_relays` with work left goes on while its cache is warm, instead of ending the run there.
- The supervisor takes no authority from the operator: the relay count, the exit and the finished mark answer only to the operator's prompt, which attend enforces, and the decisions stay with the operator.

### Negative

- The supervisor is a second session with its own cost: one cache read of its context every 30 minutes, about 48 a day, plus the reports. At Claude Opus 5.5 list prices and a 100k-token context, a check costs about $0.02, about $1 a day before its own compactions.
- A nudge the worker ignores changes nothing, and the supervisor does not nudge again in that stretch.
- A worker nudged after a judged stop goes back to work with its exit still armed. If it idles again without reaching a new boundary, with nothing pending, its next tick finishes it, because the handoff's first line still carries the old wrap id and the exit is still in force.
- Two sessions now take part in the run's continuity, and a misread case (a worker read as idle while a long tool call runs) sends a nudge to a working session. The two-checks rule for `waiting` makes this rare, and the nudge is harmless when it happens.
- The worker's peer status reads `working` only when its transcript changed in the last 30 seconds, so the check relies on the ledger and the cache state as much as on the status.
- `max_relays` no longer bounds an unattended run. Past the cap the run ends when the operator returns, when the supervisor's loop ends, or when the worker's last window fills, and no setting caps what the work past the cap costs.
- Past the cap attend runs no armed exit and sets no finished mark, so a worker that stops there idles until its cache lapses, and peers can still send to it. The supervisor reports it cold at the next check after the lapse.
- The supervisor's limits rest on its model following the skill. A supervisor that strays, for example by editing the worker's files, is caught only by its log and the operator's review.

### Neutral

- Implementation adds the `supervise` skill, and three fields to `attend keepwarm status --json`: the cache state with the next tick, the relay count, and the operator's last prompt.
- The worker's setup is two settings: relay on, and `wrap_to` naming the supervisor's session id. The supervisor's is relay off in its own directory.
- The setup works with the boundary tool and without it. Without it the worker reaches the supervisor only through relay stops and idle stretches.

## Alternatives Considered

- **No supervisor.** The worker relays, and its stops go to the operator. Rejected: the operator asked not to be the link the worker waits on, and a worker that stalls with work left idles until its cache lapses, after which nothing wakes it.
- **A supervisor that supervises and does not do the work.** Chosen. It covers the stall case with the lightest touch the operator named, and takes no authority from the operator.
- **Limits a tool enforces.** The supervisor's limits could be mechanical: a send that refuses any nudge that is not a question, or a supervisor with no write access to the worker's repository. Not chosen: the operator called the limits "guidance not a hard rule", and mechanical limits would also stop the operator from directing the supervisor to do more.
- **The cap as the end of the run.** The supervisor reports a worker at `max_relays` and ends its loop, so the cap bounds every unattended run. Rejected by the operator's "No" to the cap probe. It kept a fixed end at the cost of leaving work undone at the cap while the worker could still be woken.
- **A supervisor with operator authority.** The supervisor could clear the finished mark, reset the relay count, and answer decisions. Not chosen now, and left open. It would let one model lift the limits the operator set on another and settle what only the operator should, and it would make a peer message act as an instruction, which attend's contract says it is not. It is the step to take only if nudges prove too weak (Open questions).
- **A keepwarm ping as the nudge.** The worker's own keepwarm wake already starts a turn. Rejected: that wake asks for one word and no tools so that it costs one cache read, and turning it into a resume would make every keepalive a turn of work, which the armed exit is designed to avoid.
- **A shell script on a timer.** A script can read the same JSON. Rejected: it cannot tell a stall from a decision by reading a handoff, or write the operator a summary.
- **`wrap_to` as a channel.** A channel post never counts as awaiting a reply, so a doubt's handoff would not keep the worker warm for the operator's answer. A direct message to the supervisor's session id does.

## Open questions

1. **Operator authority for the supervisor.** If nudges prove too weak, which of the operator's powers, if any, should a supervisor hold, and with what bound of its own? This is the named alternative, deferred.
2. **The wake mechanism.** The operator named a monitor that wakes every 30 minutes. Whether the skill uses Claude Code's Monitor tool or `/loop` for that, and how either behaves across the supervisor's own compactions, is not verified.
3. **Whether the nudge works.** That a neutral question is enough to get a stalled agent going is the operator's expectation, not a measurement. The log will show how often a nudge is followed by `working`.
4. **The supervisor's model.** A smaller model may be enough to sort four cases and write a summary, at a fraction of the cost. Not decided here.
5. **A full window past the cap.** What Claude Code does when the worker's window fills with its own compaction off is not verified, and neither is how far a worker runs past the cap with that compaction on. A live run past the cap would settle both, and would show whether the run past the cap needs a bound of its own, such as a cost ceiling.

## References

- ADR-418 — the boundary tool, the judge, the armed exit and its tick, and the finished mark that refuses sends; the supervisor reads its ledger and times the nudge to its tick.
- ADR-417 — relay mode, `max_relays` counted from the operator's last prompt, and the stop that sends the handoff to `wrap_to`; `/wrap handoff` forces handoff mode.
- ADR-414 — `wrap_to` and the `handoff:` message, and the operator prompt that resets the counts.
- ADR-182 — keepwarm's 50-minute wake on the one-hour cache.
- ADR-129 — session names and instance suffixes, which is why the worker names the supervisor by session id.
- ADR-171 — stable session identity.
- ADR-406 — the attend mod, through which peer messages reach a session as data.
