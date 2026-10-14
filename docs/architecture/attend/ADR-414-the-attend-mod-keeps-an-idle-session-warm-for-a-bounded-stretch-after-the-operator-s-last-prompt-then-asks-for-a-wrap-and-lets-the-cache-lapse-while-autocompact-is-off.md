---
contract: adr/v1
kind: decision
verb: add
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "we need, per mod instance (claude code instance) to restart a timer after it's inactive. Just before the timer reaches the one hour mark, we need to send a keepalive message of some kind to the agent, to prevent cache expiry. We need to do this according to some sort of bounding state so eventually, we can invoke the wrap skill. If we were to leave autocompact on for claude code, autocompact will do this before the timer expires too. So perhaps, we need to detect if autocompact  is enabled, and only drive keepalive and wrap if autocompact is disabled. We probably need a configurable state rules of some kind - 'keepalive between these time windows, otherwise run the wrap skill' the wrap skill output, the time windows, the keepalive iteration count all should be configurable with sane defaults."
    via: "the operator in the head session, 2026-10-14, answering what the attend mod's keepalive should do"
  - operator: aaronsb
    level: directed
    said: "wrap output could even be sent to another agent in attend"
    via: "the operator in the head session, 2026-10-14, relayed verbatim by the coordinator while this record was drafted; item 6's `send` destination and `wrap_to`"
  - evidence: "tools/attend/src/verbs/send.rs: `attend message send --to` resolves a session name, a person's member id or a project path, and refuses a session or a project with no live peer, listing the live ones; a person's tray is always written; ADR-412 sends a message of any length as one signal"
  - precedent: ADR-182
  - precedent: ADR-406
  - precedent: ADR-501
  - evidence: "tools/sensor-keepwarm/src/lib.rs: the wake fires at a fixed 50 idle minutes (PING_AFTER_SECS) whatever the cache tier; only the wake gate is tier-aware (TTL_5M_SECS, less GATE_MARGIN_SECS of 120 s); the context floor is 50k tokens (BIG_TOKENS); a wake with no turn inside ten minutes, or a cold verdict, disarms; the price table gives Opus 5 a cache read of $0.50 and a 1-hour cache write of $10 per million tokens"
  - evidence: "`attend keepwarm status` in the session that commissioned this record, 2026-10-14: 606,620 tokens of context, a cold re-write priced at $6.07 against a warm turn at $0.30, break-even at 20 wakes, one cold write of $5.86 already paid in the session, keepwarm armed with 2h48m left"
  - evidence: "plugins/attend/hooks/register.tsx at commit c1dc40b7: the mod runs `attend keepwarm check` at most once a minute on its poll while no main-loop turn runs, and submits the `keepwarm:` line without asUser; it counts main-loop turns only (turn.start and turn.complete with agentId undefined); OPERATOR_ORIGINS (composer, bridge, sdk, slack-ping) names the prompts that are the operator's own; a dormant session runs no poll and no check"
  - evidence: "skills/wrap/SKILL.md: `/wrap stay` skips the land step; the skill squares the TaskList, writes a copy-paste continuation prompt, and ends with a `/compact <focus>` line for the user to run; AskUserQuestion is among its allowed tools"
  - evidence: "the operator's ~/.claude/settings.json sets `autoCompactEnabled: false` (read 2026-10-14); ~/.claude.json carries no such key"
  - upstream: "Claude Code's early-access function-hook API, as declared in plugins/attend/.claude-plugin/types/claude-code/index.d.ts: `$.session.usage({ breakdown })` returns `context.breakdown.isAutoCompactEnabled` and `autoCompactThreshold`, and a `summary` breakdown estimates locally and sends no request; `$.settings.read()` returns the merged settings; `$.session.compact({ instructions })` compacts between turns as `/compact` does; `turn.complete` resolves to the turn's final `text`; `$.fs.write` writes a file"
agent:
  name: Claude
  model: claude-opus-5-5
amends:
  - ADR-182#Decision
  - ADR-406#Decision
  - ADR-501#Decision
considered:
  - operator: aaronsb
    said: "Yes, ~8 h then wrap"
    via: the operator's pick in the head session, 2026-10-14, to the bound probe
    covers: [bound]
  - operator: aaronsb
    said: "any session where our mod is active should get the keepalive"
    via: the operator's written answer in the head session, 2026-10-14, to the dormant probe; read as every interactive session with the mod loaded, listening or not
    covers: [dormant]
status: accepted
date: 2026-10-14
deciders:
  - aaronsb
related:
  - ADR-115
  - ADR-170
  - ADR-172
  - ADR-411
  - ADR-503
observable:
  - "With autocompact off, an idle interactive session of 100k tokens gets a `keepwarm:` turn 50 minutes after its last turn, and the row shows `⟳ 1/8`"
  - "`attend keepwarm status` prints a keepalive line (count used of the maximum, the operator's last prompt, next due, the estimated wrap time, the windows) and a cost line (spent so far, the most still to spend before the wrap, a cold re-write now)"
  - "Left alone, the session gets eight keepalives, then one `wrap:` turn in which the agent runs the wrap skill with `stay` and asks nothing; no turn from the mod follows, and `attend keepwarm status` shows COLD an hour later"
  - "A prompt typed in the session resets the row to `⟳ 0/8`; an attend delivery moves the next due time and leaves the count as it was"
  - "With `attend_mod.keepalive.windows` set to `[\"Mon-Fri 08:00-19:00\"]`, a session that goes idle at 18:45 gets its wrap at the first due point after 19:00 and no keepalive"
  - "With autocompact turned on in /config, no keepalive or wrap follows; the status card says the policy is off because autocompact is on, and `attend keepwarm on 2h` in a listening session still wakes it at 50 idle minutes"
  - "With `attend_mod.keepalive.wrap: file`, the wrap's final reply is in the file `wrap_file` names; with `send` and `wrap_to` empty, it arrives as a direct message to the operator in attend-chat"
  - "With `wrap: send` and `wrap_to` naming a live session, that session receives one message opening `handoff:` and holding the whole continuation prompt; with `wrap_to` naming a session that has exited, the reply is written to `wrap_file` instead and the status card names the refusal"
  - "A `claude -p` run, a session on the five-minute cache tier, a session under 50k tokens and a session with `attend_mod.keepwarm: off` get no keepalive and no wrap"
---

# ADR-414: The attend mod keeps an idle session warm for a bounded stretch after the operator's last prompt, then asks for a wrap and lets the cache lapse, while autocompact is off

## Summary

- **Decided:** in an interactive Claude Code session that runs the attend mod and has autocompact off, an idle session is kept warm from every turn's end. One `keepwarm:` wake goes out ten minutes before the hour-long prompt cache would lapse. The keepalives are counted from the operator's last prompt, and each is allowed only inside configured time windows. At the first due point past the count (default 8) or outside the windows, the mod submits one turn asking the agent to run the wrap skill, sends nothing more, and lets the cache lapse. The wrap's reply stays in the transcript by default; it can also go to a file, or over attend to the operator or to another agent as a handoff. The operator's next prompt starts the count again. With defaults, a session left idle is warm for about eight and a half hours: the wrap runs 7 h 30 min after its last turn and the cache lapses an hour after that. Where autocompact is on, the policy does not run and ADR-182's armed windows apply as today. The judgment stays in attend's keepwarm, extended with the policy. The mod supplies what only it can see (the operator's prompts and the autocompact state) and submits the turns. The policy runs in dormant sessions too, so ADR-406's dormant session is no longer free of turns the mod starts.
- **Trades away:** money on sessions the operator never returns to. At 490k tokens on Opus 5, eight wakes cost about $2 and the wrap turn about $1.50 to $2.50, against a single cold re-write of about $4.90. Each wake also adds a turn to the transcript that the operator did not type. The wrap runs with nobody present, so it cannot land work or ask questions, and its handoff can be thinner than one the operator watches. The autocompact gate reads a field of an early-access API.
- **One-way?** No. Reversible: the policy is configuration and mod code, and its ledger fields are additive. `attend_mod.keepalive.enabled: false` returns a project to ADR-182 behaviour in one line.
- **Probes:** *Confident (bound):* if you step away and nothing else happens, the session stays warm for about eight hours from your last prompt, with a one-word turn every 50 minutes. The agent then runs wrap once and the session goes cold, so your next prompt there pays one full reload unless you start fresh from the continuation prompt. *Not confident (dormant):* keepalive and wrap run in every interactive session with the mod, including sessions that are not listening to attend, so a plain session also gets a one-word turn every 50 idle minutes.
- **Inversion:** at one end, every idle session goes cold an hour after its last turn, which is what happens today wherever nobody arms keepwarm. At the other end, every session is held warm for as long as it stays open. This decision keeps a session warm for a stretch the operator bounds, then closes it with a handoff and a deliberate lapse.

## Context

The main conversation's prompt cache lives for an hour after the last request. A request after that re-writes the whole prefix at the cache-write rate. At 490k tokens on Opus 5 a warm turn reads the prefix for about $0.25 and a cold one re-writes it for about $4.90, so twenty wakes cost about as much as one re-write. ADR-182 added a wake at 50 idle minutes, but only inside a window the operator arms by hand or that a paid cold write arms for three hours. ADR-406 moved that wake into the attend mod for listening sessions. Nothing arms it by default, so most sessions go cold an hour after their last turn.

ADR-501 decided a bounded form of the same idea: a keepalive armed by the operator, measured from the operator's last typed prompt, that asks the agent to wrap when its eight hours close. It was to be delivered over Claude Code channels from `ways-mcp`, which needs a development launch flag on every session, and it has not been built.

The operator has now asked for a keepalive in every Claude Code instance with the mod. It should restart at each period of inactivity, be bounded by state the operator configures (time windows and a keepalive count), and end in the wrap skill. It should run only when autocompact is off, since with autocompact on Claude Code manages the context itself. The operator's machine runs with autocompact off.

Two facts from the source shape the design. The keepwarm sensor wakes at a fixed 50 minutes on every cache tier, and only its wake gate knows the five-minute tier. The mod already knows which prompts are the operator's own (`OPERATOR_ORIGINS`) and counts only the main loop's turns. Claude Code's mod API reports whether autocompact is enabled, can compact the conversation, and hands a hook the text of a turn's final answer.

## Decision

1. **Where the policy runs.** The keepalive policy runs in a session when all of these hold:
   - the session is interactive (`isInteractive`). A `-p` run or an SDK session never gets it, whatever `attend_mod.headless_poll` says, because such a run has no operator whose prompt resets the bound;
   - the mod is installed and `attend_mod.keepwarm` is `on`;
   - `attend_mod.keepalive.enabled` is true (the default);
   - autocompact is off (item 2).

   It runs whether the session is listening or dormant. This amends ADR-406 items 2 and 13 for this policy alone: a dormant session still polls nothing, delivers nothing and joins nothing, and the mod sets one timer for it and runs attend's check at each due point (item 7). Where an `attend run` holds the session, its sensor keeps ADR-182's rules and the check stands aside, as it does today.

   Where the policy does not run, ADR-182 as amended by ADR-406 item 13 applies unchanged: in a listening session the mod runs the plain check, which wakes only inside a window that `attend keepwarm on` or a paid cold write armed. It sends no wrap and keeps no count.

2. **The autocompact gate.**
   - **Detection.** The mod reads `isAutoCompactEnabled` from `$.session.usage({ breakdown: 'summary' })`, which estimates locally and sends no request. It reads it at session start, for the row, and again just before it submits a keepalive or a wrap, because the operator can turn autocompact on or off in `/config` mid-session.
   - **Fallback.** When the breakdown is absent or the call fails, the mod reads `autoCompactEnabled` from `$.settings.read()` and treats only an explicit `false` as off. When neither answers, autocompact counts as on.
   - **Failing closed.** An unreadable gate falls to the side that starts no unbidden turn, as a bad value in ADR-406's switches falls to its quiet side.

   With autocompact on, the mod does what item 1's last paragraph says. Claude Code's own compaction handles a full window, and an operator who wants a warm session can still arm a window.

3. **The clock.** The clock is ADR-182's: the newest request of the main conversation in the session's transcript, read through `ways context --session`, with that request's cache tier. A keepalive is due at that request's time plus its TTL, less `margin_minutes` (default 10, which keeps ADR-182's 50-minute point on the hour tier).
   - **What restarts it.** Every main-loop request restarts the clock, whatever started the turn: an operator prompt, an attend delivery, a keepalive, a background agent's completion notice, a scheduled wakeup.
   - **What does not.** A subagent's or a background agent's own requests do not refresh the main prefix, so they do not restart the clock. Their turns do not hold it either, because the mod counts main-loop turns only (commit c1dc40b7).
   - **No due point during a turn.** Nothing is due while a main-loop turn runs.
   - **No keepalive and no wrap** on the five-minute tier, where a wake every few minutes would cost more than it saves and would interrupt the operator. None either while the context is under ADR-182's 50k-token floor, where a cold re-write is cheap.
   - **Never on a lapsed cache.** At a due point whose cache has lapsed by ADR-406's gate (its TTL less two minutes), nothing is sent and the policy stops until the operator's next prompt. That covers a host that slept through its due point. Sending a wrap there would pay the re-write to produce a handoff nobody asked for.

4. **The bound.** attend keeps one count per session: the keepalives sent since the operator's last prompt.
   - **What resets it.** An operator prompt is one whose origin the mod already treats as the operator's own (`OPERATOR_ORIGINS`: composer, bridge, sdk, slack-ping). The mod reports each one to attend with `attend keepwarm reset`, which sets the count to zero, clears a sent wrap and clears a stop. A `/clear` moves the session to a new id with a fresh ledger, which has the same effect.
   - **What does not reset it.** An attend delivery restarts the clock and leaves the count, a sent wrap and a stop as they were. Only a prompt from the operator's own terminal shows that a person is present. attend cannot tell a message the operator typed in attend-chat from a peer's (ADR-406 item 4), and a busy channel would otherwise hold its sessions warm without end. A delivery that arrives while the cache is warm still wakes the session under ADR-406 item 2, even after a wrap.
   - **At each due point**, in order:
     1. if the count is under `max_keepalives` (default 8) and the moment is inside a window (item 5), the mod submits the keepalive and attend adds one to the count;
     2. otherwise, if `wrap` is not `off`, the mod submits the wrap turn (item 6) and attend records it;
     3. otherwise the policy stops without a turn.
   - **After a wrap or a stop**, the mod sends nothing more until the operator's next prompt, and the cache lapses about an hour after the last request.
   - **The keepalive's verdict** stays ADR-182's. When a wake produces no turn within the margin, or its turn reads a cold cache, the policy stops until the operator's next prompt.
   - **Defaults in time.** With the defaults, the keepalives land 50, 100 and on to 400 minutes after the operator's last turn ends, the wrap at 450 minutes (7 h 30 min), and the cache lapses an hour later. That is ADR-501's eight-hour bound, counted in wakes.

5. **Windows.** `windows` is a list of entries, each `<days> <HH:MM>-<HH:MM>`.
   - **Days** are `daily`, one day (`Sat`), or a range (`Mon-Fri`; a range may wrap the week, as `Fri-Mon` does).
   - **Times.** `24:00` closes a day. An end before its start crosses midnight and belongs to the start's day.
   - **Timezone.** Entries are read in `timezone`, an IANA name, or the host's local zone when it is empty.
   - **The default** is `["daily 00:00-24:00"]`, which sets no time limit and leaves the count as the only bound. An empty list is never inside, so every due point wraps: the session wraps after its first 50 idle minutes.
   - **Bad entries.** An entry that does not parse is dropped, and the status card names it. A list in which every entry is dropped is empty.
   - **What the windows gate.** They gate keepalives only, and the wrap fires outside them, as the operator put it: "keepalive between these time windows, otherwise run the wrap skill".
   - **Arming by hand.** `attend keepwarm on [WINDOW]` stays the operator's word. While its window is open it overrides the count and the windows, so no wrap is sent. When it ends, the next due point is judged by the count and the windows. `attend keepwarm off` stops the policy for the session until the operator's next prompt. The automatic three-hour arm after a paid cold write (ADR-182 item 4) changes nothing where the policy runs, since the policy keeps every stretch warm on its own; it still applies where the policy does not run.

6. **The turns.**
   - **The keepalive** is ADR-182's line, unchanged, so the agent contract in the three synchronized guidance files holds. It names no tool and asks for the least reply a turn can give: `keepwarm: 50m idle, the prompt cache lapses in 10m on 490k tokens. Reply with one word and no tools.`
   - **The wrap turn** is one prompt. Its first line gives the reason, as one of:
     - `wrap: 7h30m since the operator's last prompt and 8 keepalives, the most this session allows.`
     - `wrap: 1h05m since the operator's last prompt, and the keepalive hours (Mon-Fri 08:00-19:00) have ended.`

     Its body is: `The operator is away. Run the wrap skill with the argument stay: square the task list and write the continuation prompt. Ask no questions, land nothing and start no new work. Nothing keeps the prompt cache warm after this turn.` When the output goes elsewhere (below), one more sentence says so: `The mod saves your final reply to <path>.` or `The mod sends your final reply to <target> over attend, where another session may take up the work.`
   - **How they are submitted.** Both turns go through `$.prompt.submit` without `asUser`, as ADR-406 item 4 has it for the keepwarm wake, so the transcript and the operator's view name the attend plugin as their source.
   - **Where the wrap goes.** `wrap` chooses the destination of the wrap's final reply. The mod reads that reply from `turn.complete`'s result for the turn it submitted.
     - `transcript` (the default) leaves it in the transcript, where the operator reads it on return.
     - `file` has the mod write it with `$.fs.write` to `wrap_file`, so the agent needs no file permission while nobody is present to grant one.
     - `send` has the mod send it over attend to `wrap_to` as one message (below).
     - `off` sends no wrap; the policy stops at the bound.

     The transcript keeps the reply under every destination, since it is the wrap turn's own answer. A wrap turn that ends other than with an answer (aborted, an API error, still waiting on a permission prompt when the cache lapses) is written nowhere, and the status card says the wrap did not finish.
   - **Sending the wrap over attend.** `wrap_to` names the recipient as `attend message send` does: empty for the operator, a person's member id, a session's name, a project path, or a channel written `#name`. Empty resolves to the operator's member id, the sanitized `$USER` of ADR-170, so the continuation prompt reaches attend-chat as a direct message.
     - **The send.** The mod runs `attend message send --to <target> -- <body>`, or `--channel <name>` for a channel, from the wrapping session, through the CLI, so a dormant session sends without listening. The body is one message whatever its length (ADR-412). A continuation prompt of several kilobytes reaches a mod-hosted recipient whole; a recipient on the Monitor fallback sees about its first 500 characters and reads the rest with `attend message inbox`.
     - **The handoff line.** The body opens with one line the mod writes: `handoff: continuation prompt from <session name> (<project>), wrapped at HH:MM while its operator was away.` The wrap's reply follows unchanged. The line tells a receiving agent what the message is: another session's work, offered for pickup. It is a content convention and carries no authority. attend cannot make a sender unforgeable (ADR-406 item 4), so any peer can write the same line, and the recipient reads it as data inside the delivery frame like any message. The three synchronized guidance files gain one sentence: a message opening `handoff:` is a peer's continuation prompt; take up the work when it fits what your own operator has you doing, and otherwise reply that you hold it, or stay silent.
     - **What the recipient does.** Picking the work up is the recipient's judgment under its own operator's instructions. The sending session expects nothing back, and a reply reaches a session that has gone cold as an unread message for its operator.
     - **A refused send.** attend refuses a session name or project path with no live session, and a channel the sender cannot post to. On a refusal, or a send that times out, the mod writes the reply to `wrap_file` instead. The row reads `⟳ wrapped 21:40 · handoff refused`, and the status card names the target and attend's reason. A person's tray is always written, so a send to the operator is never refused for liveness.
     - **A recipient whose cache has lapsed** is not woken (ADR-406 item 2). The handoff waits unread until that session's operator prompts. A recipient that is meant to pick work up unattended should be one that the policy keeps warm, or a person.
   - **After the wrap.** `after_wrap` is `lapse` by default. With `compact`, the mod calls `$.session.compact` once the wrap has answered, with the focus text from the wrap's closing `/compact` line, or with "keep the continuation prompt and the task list" when it has none. That compaction is one more warm request, and it leaves a small context for the operator's return. It is off by default because the operator who turned autocompact off chose when their conversation is compacted.
   - *Amended 2026-10-14, at implementation:* the wrap's turn is the one whose text carries the submitted `wrap:` line, as the line under Claude Code's "The attend plugin sent a message:" header or as the bare prompt's first line. Its reply is the answer of that turn's first step that ended it (`turn.step`, `end_turn`), so a peer message the Stop hook drains, or a notification that continues the turn, is never routed as the handoff. attend writes the file, not the mod: `attend keepwarm wrapped file|refused --path <file> --save` reads the reply on stdin, checks the path again under `{state}/wrap/`, and renames a temp file over it. A wrap whose turn has not started 10 minutes after the submit, or has not answered when the cache lapses, is recorded unfinished.

7. **The judgment stays in attend; the mod supplies inputs and acts.**
   - **Who judges.** `attend keepwarm check --policy --json` runs the judgment once. It reads the transcript clock and tier, the session's ledger, the arm file, and the `attend_mod.keepalive` keys, which it reads through `ways settings list attend_mod --json --effective --project`, as the status card already does.
   - **What it prints.** One object: `action` (`none`, `keepalive` or `wrap`), the turn's `text`, `next_at`, `count`, `max`, `wraps_at` (the estimate if nothing else happens), and `reason`.
   - **The ledger** gains the count, the operator's last prompt time, the time a wrap was sent, and the reason for a stop.
   - **What the mod does.** It calls the check only when item 1 and the autocompact gate hold, schedules its next call at `next_at`, and submits what the check returns. In a listening session that rides the existing tick (at most once a minute). In a dormant session it is one timer, set again at each main-loop turn's end.
   - **Unchanged.** The plain `attend keepwarm check`, the Monitor-hosted sensor and the guidance files stay as they are.
   - *Amended 2026-10-14, at implementation:* the policy keeps one timer per session, listening or dormant. It is set at each main-loop turn's end to the turn's end plus the lesser of 60 minutes less `margin_minutes` and 50 minutes, and then to the `next_at` attend returns; a listening session does not ride the poll's tick for it. The plain check, where the policy does not apply, still rides the tick at most once a minute. A check submits nothing when an operator prompt or a main-loop turn started while attend judged, or when the gate, read again just before the submit, no longer reads off. The wrap's turn is the one its own text started. A dormant session with autocompact on runs no attend at all: the mod holds the operator's prompt and sends it with `attend keepwarm reset --at` when the gate first reads off.

8. **Configuration.** The keys sit under `attend_mod.keepalive` in the settings registry (ADR-503). They resolve through the user, target and project layers with the project winning (ADR-115), and are written per project with `ways settings set <key> <value> --project <dir>`. attend-chat's ☰ Settings offers them as it offers `listening` and `keepwarm`, with the same choice of where to write. Scalars use its pickers; numbers, text and `windows` use its entry dialog, with entries separated by commas as `ways settings set` reads a list.

   | Key | Values | Default |
   |---|---|---|
   | `enabled` | true, false | true |
   | `max_keepalives` | 0 to 48 | 8 |
   | `margin_minutes` | 3 to 30 | 10 |
   | `windows` | list of `<days> <HH:MM>-<HH:MM>` | `["daily 00:00-24:00"]` |
   | `timezone` | IANA name, or empty for the host's zone | empty |
   | `wrap` | `transcript`, `file`, `send`, `off` | `transcript` |
   | `wrap_file` | path; `{state}`, `{project}`, `{session}`, `{date}` expand | `{state}/wrap/{session}.md` under the ways state root |
   | `wrap_to` | for `send`: a member id, a session name, a project path or `#channel`; empty for the operator | empty |
   | `after_wrap` | `lapse`, `compact` | `lapse` |

   `attend_mod.keepwarm: off` still turns off every cache wake the mod drives, the policy included. A value of the wrong type keeps its default. A bad `windows` entry is dropped (item 5).

   *Amended 2026-10-14, at implementation:* `enabled` fails closed, as `keepwarm` and `install` do. Only an explicit `true` runs the policy: settings ways cannot answer, a value of the wrong type, and a ways that does not know the key all leave it off, in the registry's closed reading, in the mod and in attend's check, and the status card says why. `wrap_file` must name `{session}` and resolve under `{state}/wrap/` through no symlink; anything else falls back to `{state}/wrap/<session>.md`, and the card names the refusal. `wrap_to` `#open` is refused, since it reaches every listening session, and the reply goes to the operator.

9. **What the operator sees.**
   - **The row.** In a listening session the row gains one segment while the policy runs: `⟳ 2/8 · next 41m · wrap ~21:40`. After a wrap it reads `⟳ wrapped 21:40`, and after a stop `⟳ stopped`. A dormant session draws the same segment alone, in the same slot, from its first keepalive until the operator's next prompt, so an operator who returns can see what ran. Otherwise a dormant session draws nothing, as ADR-406 item 3 has it.
   - **The status card.** `attend keepwarm status` adds a keepalive line, such as `keepalive   2 of 8 since your prompt at 14:02 · next in 41m · wraps about 21:40 · windows daily 00:00-24:00`, or `keepalive   off: autocompact is on (armed windows only)`. It also adds a cost line, such as `cost        2 wakes $0.50 so far · at most $1.50 more in wakes before the wrap, and the wrap reads the context once per tool round · a cold re-write now $4.90`.
   - **The transcript** shows each keepalive and the wrap as the plugin's prompts.

10. **Interactions.**
    - **An attend delivery** is a main-loop turn: it restarts the clock and leaves the bound as it was (item 4).
    - **`/clear`** starts a new session id with an empty ledger. The small new context sits under the 50k floor until it grows.
    - **`/compact`** writes a new, smaller prefix, which the clock sees as a new request.
    - **A resume** (ADR-411) keeps the ledger by session id. The cache is usually cold by then, so nothing is due until the next request, and an operator prompt resets the bound.
    - **A lapsed cache** gets nothing (item 3).
    - **Headless runs** get nothing (item 1).
    - **Several sessions in one project** each keep their own count and clock under their own session id, and share the project's configuration.
    - **Two processes on one session id** share the ledger. ADR-182's one-wake-per-stretch marker keeps them from doubling a keepalive.

## Consequences

### Positive

- An operator with autocompact off comes back after lunch or a meeting to a warm session at the cost of a few cache reads, with nothing to arm.
- The bound is the operator's: a count and time windows they set per project, measured from their own last prompt. No session is held warm indefinitely.
- A session that runs out its bound ends with an honest task list and a continuation prompt. The operator can start a fresh, small session from it rather than paying to re-write the old context.
- Default Claude Code installs, where autocompact is on, see no new turns. The policy reaches only operators who chose to manage their context themselves.
- One judgment in attend serves the mod, the status card and later hosts. The cache clock, the verdict and the prices stay in one tested place.

### Negative

- Money is spent on sessions the operator abandons. At 490k tokens on Opus 5 the worst case per stretch is about $2 in wakes plus a wrap turn of $1.50 to $2.50, roughly one cold re-write. If the operator then continues the same session instead of starting from the handoff, the re-write is paid as well. The cost line shows the arithmetic, and `wrap: off` or a smaller `max_keepalives` lowers it.
- A dormant session is no longer free of turns the mod starts. That departs from ADR-406's "plain Claude Code" for dormant projects.
- A handoff to another agent rests on a content convention. Any peer can write a `handoff:` line, so a receiving agent weighs it as it weighs any peer message, and work handed to a session that has gone cold waits for that session's operator.
- The wrap runs unattended. A tool call that needs permission stalls the turn until the cache lapses, and the skill cannot offer to land work. `stay` and the "ask no questions" line reduce this but cannot rule it out.
- On a subscription the dollars stand in for an undocumented weight against the usage limits, as ADR-182 noted.
- The gate depends on `isAutoCompactEnabled` in an early-access API. The fallback to settings and the closed default limit the damage if it moves.
- Each wake adds about a hundred tokens to the transcript. Eight wakes add under a thousand.

### Neutral

- ADR-182's sensor, its arm file, its verdict and the `keepwarm:` contract are kept. The policy adds a mode to the check, a tier-aware due time (`margin_minutes`) and ledger fields. The Monitor fallback is unchanged.
- ADR-501's keepalive module is put on hold, as its attend module was by ADR-406: this policy delivers its intent (a bound counted from the operator's last prompt and a wrap at the end) without the channel flag. It resumes only for a host that cannot run the mod.
- The `ways-mcp` keepalive tool names that ADR-501 reserved (`keepalive_status`) stay unused.
- Implementation adds `attend keepwarm reset` and `check --policy --json` to attend, the `handoff:` sentence to the three synchronized guidance files, the keys to the settings registry and attend-chat's settings list, and the gate, timer, routing and row segment to the mod, with plugin tests on the mock clock and attend unit tests over fixed transcript readings.

## Alternatives Considered

- **Arm ADR-182's window automatically at every operator prompt.** This would rest on the arm file alone, with a window of about eight hours. Rejected: the window can only stop, so it has no wrap and no time windows. attend cannot see which prompts are the operator's, so it could not restart the window from the right event.
- **The whole policy in the mod, in TypeScript.** Rejected: the mod would need the transcript clock, the tier, the warm-or-cold verdict, the cold-write ledger and the price table, all of which attend already owns and tests, and two judges would drift apart. ADR-406 item 10 already puts the keepwarm judgment in attend. The mod keeps what only it can see.
- **ADR-501's keepalive module over channels.** Rejected for Claude Code: it needs a development launch flag on every session and remains unbuilt. The mod reaches every session with no flag.
- **Let the session go cold with no wrap.** Kept as `wrap: off`. It is not the default, because the operator asked for the wrap, and a continuation prompt lets them resume without paying for the old context.
- **Compact after the wrap by default.** Rejected as the default and kept as `after_wrap: compact`. It would leave a small, cheap context for the return, but it rewrites the operator's conversation while they are away, and they turned autocompact off.
- **Reset the count on any delivered message.** Rejected: a busy channel would hold its sessions warm without end, and a delivery cannot be trusted to come from the operator.
- **A wall-clock bound (hours since the operator's last prompt) in place of a count.** Rejected: the operator asked for an iteration count, and a wake is the unit that costs money. The row shows the wall-clock estimate.
- **Listening sessions only.** Rejected: the operator asked for the policy per Claude Code instance, and the cache cost does not depend on messaging. Held open as the `dormant` probe.
- **Nothing at all where autocompact is on, ADR-182's armed windows included.** Rejected: arming a window is an explicit act by the operator, and it stays honoured.
- **A dedicated handoff frame for a wrap sent to another agent,** drawn by the receiving mod as an instruction to pick the work up. Rejected for now: attend cannot make a sender unforgeable (ADR-406 item 4), so a frame that grants authority to a handoff would grant it to any peer that writes one. The `handoff:` line names the message and leaves the decision to the recipient. A frame can follow once attend has an origin a peer cannot produce.
- **A separate destination value for each kind of recipient** (`dm` for the operator, `agent` for a session, `channel`). Rejected: `attend message send` already resolves every kind of target from one name, so one `send` value and one `wrap_to` key cover them all.
- **Working hours as the default window.** Rejected for now: a window that does not match the operator's schedule or timezone would wrap sessions without warning, and the count already bounds a stretch to about eight hours.
