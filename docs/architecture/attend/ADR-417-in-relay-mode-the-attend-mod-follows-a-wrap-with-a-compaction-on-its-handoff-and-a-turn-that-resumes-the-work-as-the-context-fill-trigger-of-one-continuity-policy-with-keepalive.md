---
contract: adr/v1
kind: decision
verb: add
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "I think it's time to restructure wrap in a way that let's us facilitate both modes: if auto compact is off, wrap basically behaves as it is now. this is useful for when I just need to end a session early and I know I won't be back to 'help' (aka, watch for total account usage, or to offer steering and the agent deviates in a direction I wouldn't like it to). then, if autocompact (or a custom method to autocompact) is enabled, then the wrap skill is ran first, the compact command is collected from the wrap skill, compact is invoked with that summary, and after the compact, an initial injection or trigger is sent to cause the agent to read the continuance file and continue. if in the middle of a session turn autocompact on or off globally, then any local project where autocompact+wrap was not defined as enabled follows the global (user) scope. if there's a local ways configuration json in the project dir, any agents invoked there follow that ways configuration, and use the autocompact+wrap, or not, depending on the config. (so if it was enabled in user scope, and local scope said disabled, it would be disabled)"
    via: "the operator in the coordinating session, 2026-11-26, relayed verbatim by the coordinator; the two modes, item 1's resolution order and item 4's resume. Read with the correction below: \"autocompact\" here names the relay setting at user and project scope, not Claude Code's own autocompact switch"
  - operator: aaronsb
    level: guided
    said: "I have, to this point been relatively unconfident about leaving claude unattended for many sessions in a row but I think I've gained that confidence, and now I can't be the link that claude waits for"
    via: "the operator in the coordinating session, 2026-11-26, earlier the same day, relayed verbatim by the coordinator; item 2(c) and item 5's bounds"
  - operator: aaronsb
    level: guided
    said: "we can always run the wrap command then the compact and trigger a continuance after the compact (so the agent wakes up and isn't idle)"
    via: "the operator in the coordinating session, 2026-11-26, earlier the same day, relayed verbatim by the coordinator; item 4"
  - operator: aaronsb
    level: directed
    said: "keepalive is, I think, a similar class as this autocompact+wrap"
    via: "the operator in the coordinating session, 2026-11-26, relayed verbatim by the coordinator while this record was drafted; the one continuity policy, item 1's settings family and item 2(c)"
  - operator: aaronsb
    level: directed
    said: "also remember, we're using claude code's compact, but we're using it in a specific way with the wrap skill so it's not just a flag for generally toggling autocompactg off or on"
    via: "the operator in the coordinating session, 2026-11-26, relayed verbatim by the coordinator as a correction while this record was drafted; item 1's key is its own setting and does not follow Claude Code's autocompact state, and item 7's matrix"
  - operator: aaronsb
    level: directed
    said: "this is like a 'softer slash-goal command' (goal being a claude code feature)"
    via: "the operator in the coordinating session, 2026-11-26, relayed verbatim by the coordinator after the first draft; item 6, relay and /goal as layers"
  - operator: aaronsb
    level: directed
    said: "let's fold it in"
    via: "the operator in the coordinating session, 2026-11-26, relayed verbatim by the coordinator, asking that the /goal layering, probe (e), the deferred independent check, the idle-path gate on the relay setting, and the per-wrap id go into this record"
  - upstream: "Claude Code 2.1.330's mod API types, searched 2026-11-26 for `goal`: no field, event or call reports whether a /goal is active or what its evaluator decided. The event `command.run` fires when a slash command is about to run, typed or from a plugin, with its name and args. Read from the types; not yet exercised by this design"
  - upstream: "Claude Code 2.1.330's mod API types (the bundled plugin-authoring types/claude-code.d.ts, vendored at plugins/attend/.claude-plugin/types/claude-code/index.d.ts): `$.session.compact({ instructions })` is `the event session.compact with trigger plugin, the same call /compact makes, between turns`, `resolves { skip } when a hook vetoed it; rejects while a turn runs`; the event `session.compact` (triggers manual, auto, plugin, precompute) may rewrite `instructions` or `messages`, answer `{ messages }`, or answer `{ skip }`; `$.prompt.submit({ text })` is `a turn of its own, once the session is idle`; `session.measure` fires `after each main-thread turn` and carries the usage figures; the usage breakdown carries `isAutoCompactEnabled` and `autoCompactThreshold`. Read from the types; not yet exercised by this design"
  - upstream: "Claude Code hooks documentation (code.claude.com/docs/en/hooks), read 2026-11-26: the PreCompact hook matches `manual` or `auto` and receives `trigger` and `custom_instructions`; on exit 0 Claude Code uses its stdout as instructions for the compaction summary, capped at 10,000 characters; exit 2 or `decision: block` blocks the compaction, and a block during context-limit recovery fails the request. PostCompact receives `compact_summary` and has no decision control. SessionStart with source `compact` adds context and starts no turn. CLAUDE_AUTOCOMPACT_PCT_OVERRIDE cannot raise the threshold. Read from the docs; not yet exercised by this design"
  - evidence: "This host has no managed settings (/etc/claude-code is absent), so `session.compact` reaches the attend mod here. Under an org-managed install Claude Code passes the mod over (plugins/attend/tests/passed-over.ts), so a classic hook is the interception path that holds on every install"
  - evidence: "plugins/attend/hooks/lib.ts and register.tsx at commit e6bbfbdd: the keepalive policy's state is one of running, wrapped, compacted, stopped, off; `WrapRoute` carries the wrap's destination and `after` (lapse or compact); `compactFocus` takes the focus from the wrap's closing `/compact` line; `compactAfterWrap` calls `$.session.compact({ instructions })` between turns and reports `attend keepwarm compacted`, or `--failed` with the skip reason; the mod reads `isAutoCompactEnabled`, falling back to `autoCompactEnabled` in settings"
  - evidence: "`ways settings list` on this host, 2026-11-26: the policy's keys are `mods.attend.keepalive.{enabled, max_keepalives, margin_minutes, windows, timezone, wrap, wrap_file, wrap_to, after_wrap, after_wrap_keepalives, wrap_skill}`, with `wrap_skill=wrap stay` and `after_wrap=lapse`; `ways settings get <key> --project <dir>` prints the effective value with the project's layer applied"
  - evidence: "Three files say a skill or way cannot start a compaction: skills/wrap/SKILL.md (its introduction, Step 4, Key Principles and Not for), hooks/ways/meta/wrap/wrap.md (The compaction caveat), and hooks/ways/meta/compaction-checkpoint/compaction-checkpoint.md (Step 4). ADR-165 item 2 rests on the same assumption"
  - precedent: ADR-414
  - precedent: ADR-165
  - precedent: ADR-503
  - precedent: ADR-512
  - precedent: ADR-610
agent:
  name: Claude
  model: claude-opus-5-5
amends:
  - ADR-165#Decision
  - ADR-414#Decision
status: proposed
date: 2026-11-26
deciders:
  - aaronsb
related:
  - ADR-115
  - ADR-182
  - ADR-406
  - ADR-416
observable:
  - "In a project with `mods.attend.continuity.relay: on` and Claude Code's autocompact off, a session that reaches 75% of its window gets one `wrap:` turn from the mod; the wrap ends with a `relay: continue ... wrap=<id>` line whose id is on the handoff's first line, the conversation is compacted with the handoff file's text, and a `relay:` turn follows that reads `.claude/handoff.wrap.md` and starts its DO NEXT; the row reads `⟳ relay 1/5`"
  - "With `relay` unset in the project and `on` in the user config, `ways settings get mods.attend.continuity.relay --project <dir>` prints `on`; with the project set to `off`, it prints `off` and the same session ends its wrap with a `/compact` line for the operator"
  - "After five relays with no operator prompt, the sixth wrap compacts and submits no resume; the operator receives the handoff over attend, and the row reads `⟳ relay stopped: 5 of 5`"
  - "A wrap that ends `relay: stop decision` is compacted, no resume is submitted, and the handoff reaches the operator over attend"
  - "With relay off and Claude Code's autocompact on, an automatic compaction gets the latest fresh handoff, or the wrap outline, as its instructions from the PreCompact hook; no turn follows it"
  - "A config file holding `mods.attend.keepalive.max_keepalives: 2` and no `continuity` section still runs two keepalives; `ways settings lint` names the old key and `ways settings fix mods.attend.continuity` moves it"
---

# ADR-417: In relay mode the attend mod follows a wrap with a compaction on its handoff and a turn that resumes the work, as the context-fill trigger of one continuity policy with keepalive

## Summary

- **Decided:** Keepalive (ADR-414) and relay are two triggers of one session-continuity policy in the attend mod. Idle time drives the keepalive-then-wrap path. Context fill drives a new wrap-compact-resume path. Both share the wrap skill, the routing of the wrap's reply, the judgment in `attend keepwarm check --policy`, and the mod's calls to `$.session.compact` and `$.prompt.submit`. A new setting, `mods.attend.continuity.relay` (`on` or `off`, default `off`), picks what follows a wrap. With `off` (handoff mode), wrap behaves as it does today and ends with a `/compact` line for the operator. With `on` (relay mode), the mod compacts the conversation with the wrap's handoff as instructions and then submits a turn that tells the agent to read `.claude/handoff.wrap.md`, check its tasks and do its DO NEXT. The setting resolves project, then user, then the shipped default, and is read when each wrap starts. It is its own setting, not a proxy for Claude Code's autocompact switch. With relay on, the idle path runs whatever Claude Code's autocompact switch says. Relays are capped at five per operator prompt and stop when the wrap reports no work, a decision only the operator can make, or high doubt. Relay is the soft continuity layer under `/goal`: the agent judges its own stop. An active `/goal` stays the hard layer, and only its evaluator says when it is met. A PreCompact hook directs every native compaction that arrives without a fresh wrap, in both modes. The policy's settings move from `mods.attend.keepalive.*` to `mods.attend.continuity.*`, with a dual read of the old names and a fix command.
- **Trades away:** money and oversight. In relay mode the agent judges its own stop. Each relay generation spends a near-full context of tokens while nobody watches, and the transcript gains turns the operator did not type. The relay depends on an early-access mod API whose compaction and submit calls this design has not exercised yet. The rename makes every persisted keepalive key move once.
- **One-way?** No. Reversible: relay defaults off, and one project line turns it off. The rename is expensive to undo once users' files carry the new names, so it should land once, with the dual read.
- **Probes:** *Not confident (idle-relay):* with relay on, a session you leave idle still gets its keepalives first. At the end of them, instead of going quiet, it wraps, compacts and starts its next task on its own, up to five times before it stops and messages you. *Not confident (rename):* your keepalive settings move to a new name, `mods.attend.continuity`, beside the new relay switch. Both names are read for a while, and `ways settings fix mods.attend.continuity` rewrites your files.
- **Inversion:** at one end, every session stops at its wrap and waits for the operator, which is today's behaviour. At the other end, every session keeps relaying itself for as long as it has tasks. This decision lets the operator choose per project, defaults to stopping, and bounds the relay by a count and by the agent's own report.

## Context

A wrap closes a session: it squares the task list, writes a continuation prompt to `.claude/handoff.wrap.md`, and hands the operator a `/compact <focus>` line (ADR-165). ADR-414 added a second way in: when an idle session runs out its keepalives, the attend mod asks for a wrap, and with `after_wrap: compact` it compacts the conversation with the wrap's focus. That path then stops. The session goes quiet until the operator prompts again.

The operator now leaves sessions unattended across several context windows and does not want to be the link each one waits for. They asked for two modes. In handoff mode, wrap behaves as it does today, for a session they mean to end. In relay mode, the wrap runs, the conversation is compacted with the wrap's handoff, and a turn starts that tells the agent to read the handoff and continue. They also said that keepalive and this relay are the same class of thing, and that relay uses Claude Code's compaction in a specific way through the wrap. It is not a general switch for Claude Code's own autocompact.

Three facts from the mod API and the hooks docs make this possible now. `$.session.compact({ instructions })` makes the same call `/compact` makes, between turns. `$.prompt.submit({ text })` starts a turn of its own once the session is idle. A classic PreCompact hook can supply the instructions for a compaction that Claude Code starts on its own. ADR-165 item 2 and the wrap skill assume that compaction cannot be automated. That holds for a skill, and it does not hold for the mod.

Issue #857 proposes answering every compaction with a structured handoff in place of the generic summary. Issue #975 proposes a Memento-style relay in which a fresh instance continues from a wrap. This record is the same-process variant of #975: the session keeps its id and its prompt cache prefix up to the compaction, and the handoff travels as the compaction's instructions.

This record sits in the attend band because the mod and attend's check do the work. Wrap stays the practice bookend of ADR-165, which this record amends.

## Decision

1. **One continuity policy, its settings, and how they resolve.**
   - **One policy, two triggers.** The ADR-414 policy is renamed the continuity policy. Idle time drives its keepalive path: keepalives, then a wrap at the bound. Context fill drives its relay path: a wrap at a threshold, then a compaction and a resume. An explicit `/wrap` enters the same machinery. Every wrap, whatever started it, ends the same way for a given mode (items 2 to 4).
   - **The family moves to `mods.attend.continuity`.** The keys stay one family. `keepalive` named one of two triggers, and half of the family (the wrap's destination, file, recipient, skill and what follows it) belongs to both. Keeping the old name would make `mods.attend.keepalive.relay` read as part of the idle path, which it is not. The new names:

     | New key | Was | Values | Default |
     |---|---|---|---|
     | `keepalive` | `keepalive.enabled` | true, false | true |
     | `max_keepalives`, `margin_minutes`, `windows`, `timezone` | `keepalive.<same>` | as ADR-414 item 8 | as ADR-414 item 8 |
     | `wrap`, `wrap_file`, `wrap_to`, `wrap_skill` | `keepalive.<same>` | as ADR-414 item 8 | as ADR-414 item 8 |
     | `after_wrap`, `after_wrap_keepalives` | `keepalive.<same>` | as ADR-414 item 8; handoff mode only (item 2c) | as ADR-414 item 8 |
     | `relay` | new | `on`, `off` | `off` |
     | `relay_at_pct` | new | 50 to 95 | 75 |
     | `max_relays` | new | 0 to 50 | 5 |

   - **Migration and dual read.** These keys are persisted in users' and projects' files, so the rename crosses a persistence boundary. For a dual-read period, attend's check and the mod read `mods.attend.continuity.<key>` first and fall back to the old `mods.attend.keepalive.<key>` in the same layer. A new key in a lower layer still loses to an old key in a higher one, so a project's old value keeps winning over the user's new one. `ways settings lint` and `ways manage update` name each file that still holds the old section. `ways settings fix mods.attend.continuity` (with `--project <dir>` for a project file) moves each value, renames `enabled` to `keepalive`, keeps the new value where a file holds both, and says what it dropped, as ADR-512's move of `attend_mod` did. ADR-512 ignored the old section at once. This rename reads both, because `keepalive.enabled` fails closed, and ignoring it would silently turn the policy off on every host that set it. The fallback is removed two minor releases of `ways` after the rename ships, tracked in an issue opened with the implementation.
   - **Resolution.** `relay` resolves through the registry's layers with the project winning (ADR-503, ADR-115): the project's `.claude/ways.yaml`, then the user config under `$XDG_CONFIG_HOME/agent-ways/`, then the shipped default `off`. A project that says `off` is off even when the user says `on`, as the operator put it. An unset key does not follow Claude Code's autocompact state. The setting picks how the wrap uses Claude Code's compaction, and Claude Code's own switch picks whether Claude Code compacts on its own (item 7).
   - **Why the default is off.** Relay starts turns and spends money while nobody watches. ADR-414 kept every unbidden turn behind a setting the operator chose and failed closed on any doubt. The same caution applies here, and an operator who wants relay sets one line.
   - **When the value is read.** attend reads the effective value when a wrap starts: when it judges a fill or idle wrap, and the wrap skill reads it at its Step 0 for a `/wrap`. That value governs the wrap through its compaction and resume, so the skill's output and the mod's action agree. A change made mid-session applies from the next wrap. The idle path's gate (item 2) reads the value at each due point, as ADR-414 reads the autocompact state.

2. **What starts a wrap in relay mode.**
   - **(a) Context fill.** The mod listens to `session.measure`, which fires once after each main-loop turn and carries the context's `tokens`. When the context reaches `relay_at_pct` of the model's window, the mod asks attend's check with the fill, and attend returns `action: wrap`. The mod submits the wrap turn between turns, as ADR-414 item 6 does, with the reason line `wrap: context at 76% (760k of 1M), past the relay threshold of 75%.` and the same body naming `wrap_skill`. The default of 75% leaves a quarter of the window for the wrap to read state and write a dense handoff. It sits below the 85% at which the compaction-checkpoint way fires. With Claude Code's autocompact on, attend lowers the trigger to five points of the window under the native threshold when that is lower, so the relay's wrap comes first. The native threshold is read from `$.session.usage({ breakdown: 'summary' })`, whose `autoCompactThreshold` is the token count at which Claude Code compacts, as ADR-414 reads `isAutoCompactEnabled` from the same call. It is not read from `session.measure`, whose `window` stays at the model's full window when a smaller autocompact window is set. One fill wrap runs per generation: none is judged again until the conversation has been compacted or the operator has prompted.
   - **A long turn is the hook's case.** `session.measure` fires only at a turn's end, never between compactions inside one turn. A single long turn that crosses the native threshold mid-turn is therefore compacted by Claude Code before the relay can wrap. The PreCompact hook directs that compaction (item 3), the turn goes on with the summary, and no resume is needed or submitted.

     > Edited 2026-11-26, per the item 10 probe: `session.measure` reports the model's full window under a smaller autocompact window and does not fire between mid-turn compactions. The bullet read the native threshold without naming its source, and the long-turn bullet is new. It read, in part: "With Claude Code's autocompact on, attend lowers the threshold to five points under `autoCompactThreshold` when that is lower, so the relay's wrap comes first."
   - **(b) `/wrap`, typed by the operator or invoked by the agent.** In a relay-mode session the wrap skill ends with the relay marker (item 8), and the mod acts on it at that turn's end as it does for a wrap it submitted. A `/wrap` the operator types is an operator prompt, so it resets the relay count first. `/wrap handoff` forces handoff mode for that one wrap, for an operator who wants to stop a relay-mode session by hand.
   - **(c) The idle wrap follows the mode.** In relay mode, ADR-414's idle wrap is followed by the same compaction and resume, not by ADR-414's lapse. This follows from the one-policy framing: the wrap is the same act whichever trigger started it, and the mode alone decides what follows it. The operator's point is that they are not the link. A session that goes idle with open tasks and no question for the operator should pick its work up again. A session that went idle because it is waiting on the operator says so in its wrap marker (`relay: stop decision`), and item 5 stops it. In relay mode `after_wrap` and `after_wrap_keepalives` do not apply, since their purpose is to keep the whole context for a returning operator. The keepalive count before the idle wrap still applies, so a project that wants faster relays after idling lowers `max_keepalives`.
   - **The idle path's gate follows the relay setting.** ADR-414 item 2 runs the idle path only while Claude Code's autocompact is off, on the operator's 2026-10-14 reasoning that native autocompact manages the context otherwise. With relay `on`, the idle path runs whatever Claude Code's autocompact switch says, because relay is not an autocompact switch, as the operator said. Relay mode drives its own compaction after every wrap, so a native switch that is on does not make the idle wrap redundant. With relay `off`, ADR-414's gate on native autocompact stays as it is.

3. **The safety net: a PreCompact hook in both modes.** Claude Code can still start a compaction on its own, at its threshold, during context-limit recovery, or by an operator's bare `/compact`. The ways hooks gain a classic PreCompact command, `ways hook pre-compact`, matched on `manual` and `auto`, so that no compaction gets a generic summary.
   - When `custom_instructions` is non-empty, the hook prints nothing. That covers an operator's `/compact <focus>` and every compaction the mod starts: Claude Code fires the classic PreCompact hook for the mod's `$.session.compact` call with `trigger: manual` and `custom_instructions` set to the mod's instructions, and it adds the hook's stdout to those instructions rather than replacing them. Without this rule the safety net would append a second copy of the handoff to every relay compaction. The mod's own `session.compact` handler does not hear the mod's own call, so the mod needs no guard of its own there.

     > Edited 2026-11-26, per the item 10 probe: the hook fires for the mod's compactions as `manual` with the mod's text, and its stdout is appended. It read: "When `custom_instructions` is non-empty, the operator gave a focus, and the hook prints nothing."
   - Otherwise it prints the handoff at `.claude/handoff.wrap.md` under the repository root when that handoff is fresh. The hook has no marker to compare, so it asks attend for the wrap id the ledger recorded for this session's last wrap, and the handoff is fresh when its first line carries that id. Where attend has no recorded wrap (the mod is absent or passed over), the hook falls back to a first line that names the current HEAD commit and today's date. It opens with one line, `Summarize this session as the handoff below; keep its TASKS and DO NEXT whole.`, and is cut to 10,000 characters at a section boundary, with a closing line saying the file holds the rest.
   - With no fresh handoff it prints the wrap skill's handoff outline (the Step 3 sections), so the summary is structured even when no wrap ran.
   - It always exits 0. It never blocks, because a block during context-limit recovery fails the request.
   - It is a classic hook, not a `session.compact` handler in the mod, because an org-managed install passes the mod over and the classic hook still runs. The mod registers no `session.compact` handler for this, so there is one path.

4. **The compaction and the resume.**
   - **The compaction.** When a relay-mode wrap ends with a marker that parses and a fresh handoff (its first line carries the marker's wrap id, item 8), attend reads the handoff file and returns `action: compact` with `text` set to the instructions item 3 describes and `resume` set to the resume text. The mod calls `$.session.compact({ instructions: text })` between turns. Relay uses `instructions`, not a `{ messages }` answer: Claude Code still writes the summary, the handoff steers it, and nothing replaces the conversation by hand.
   - **The resume, exactly.** Only after `$.session.compact` resolves without `skip`, the mod submits through `$.prompt.submit`, without `asUser`, this text, with the path absolute and the counts filled in: `relay: the conversation was compacted after a wrap; this is relay 2 of 5 since the operator's last prompt. Read /path/to/repo/.claude/handoff.wrap.md. The task list survived the compaction; work from it and the handoff's TASKS, then start its DO NEXT. The operator is away: ask no questions. If a step needs a decision only the operator can make, or your doubt about the direction is high, do not start it; end your reply with one line, relay: stop decision or relay: stop doubt, followed by the reason, and the mod sends the handoff to the operator over attend.`
   - **Under an active `/goal`** the mod compacts and submits the resume as it does without one. The resume is the message that continues the paused goal, and the goal's evaluator still decides when it ends (item 6).

     > Edited 2026-11-26, per the item 10 probe: the goal starts no turn after a compaction, and the resume continued it without a collision. The task list survived four compactions, so the resume text no longer asks the agent to recreate tasks. This bullet read: "**Under an active `/goal`** the mod compacts and submits no resume, as item 6 says." The resume text read, in part: "Check the task list against its TASKS and recreate any that are missing, then start its DO NEXT."
   - **On skip or failure.** A compaction that resolves `{ skip }`, or rejects, gets no resume. The mod logs the reason with `$.ui.log` and reports it with `attend keepwarm compacted --failed <why>`, and the policy stops with that reason on the row and the status card. A resume that fails to submit is reported with `attend keepwarm resumed --failed <why>` and stops the policy the same way. An operator prompt or any other turn that starts between the wrap's answer and the compaction cancels both, as ADR-414's compaction after the wrap is cancelled. The next turn's end judges again.
   - **The state machine.** The policy's states become `running`, `wrapped`, `compacted`, `resumed`, `stopped` and `off`, and the ledger records the mode and trigger of each wrap, the relay count, and the stop reason:

     | From | Event | To |
     |---|---|---|
     | `running` | idle bound, fill threshold, or a `/wrap` marker | `wrapped`, with mode and trigger recorded |
     | `wrapped` (handoff) | ADR-414's `after_wrap` | `compacted` or `stopped`, as today |
     | `wrapped` (relay) | marker `continue`, under the cap | `compacted`, then `resumed` once the resume is submitted |
     | `wrapped` (relay) | marker `stop`, the cap reached, or a compaction skipped or failed | `stopped`, with the reason; a stop compacts first when the marker parsed and the handoff is fresh (item 5) |
     | `resumed` | the resume turn starts | `running`, relay count plus one, keepalive count zero |
     | any | an operator prompt (`OPERATOR_ORIGINS`) | `running`, both counts zero |

     attend's decision gains `resume`, `relay_count`, `max_relays` and `relay_at_tokens`. The row reads `⟳ relay 2/5` while relaying and `⟳ relay stopped: decision` after a stop.

5. **Bounds on unattended relays.**
   - **A count per operator prompt.** `max_relays` (default 5) caps the resumes submitted since the operator's last prompt. Five is several context windows of unattended work, enough to carry a task list through a long evening, and few enough that a session drifting in the wrong direction stops within a bounded spend. An operator prompt resets the count, as it resets the keepalive count.
   - **Stop conditions.** The relay stops, with no resume, when any of these holds: the wrap's marker says `stop empty` (the task list holds no open work), `stop decision` (the next step needs a decision only the operator can make) or `stop doubt` (the agent's doubt about the direction is high, the doubt rule in core.md); a resumed generation ends a turn with a `relay: stop` line; the count reaches `max_relays`; the marker is missing or malformed, or the handoff is not fresh; or a `/goal` that was active has ended (item 6).
   - **What a stop does.** When the marker parsed and the handoff is fresh, the mod still compacts with the handoff, so the operator returns to a small context with the handoff at its head. It submits no resume. It sends the wrap's reply over attend to `wrap_to` (empty is the operator) with ADR-414's `handoff:` line, and falls back to `wrap_file` when the send is refused. A missing marker or a stale handoff compacts nothing, and a native compaction later meets item 3's hook. A `relay: stop` line in a resumed turn sends that turn's reply and compacts nothing.

6. **Relay and `/goal` are two layers.**
   - **Two layers.** Relay is the soft continuity layer. It keeps a session going across context windows, the agent judges its own stop in the wrap marker, and `max_relays` caps it. Claude Code's `/goal` is the hard condition layer. Its end state is written in advance, a separate evaluator judges it, and the model cannot end it. Both can hold at once: relay carries a session across windows, and a goal says when the work is done.
   - **The wrap under a goal.** Under an active `/goal`, the wrap restates the goal condition at the top of the handoff, as the skill already does. The agent may not end the relay by claiming the goal is met, because only the goal's evaluator judges it. The marker has no `goal-met` reason. The marker reports instead whether a goal is active (`goal=active` or `goal=none`, item 8).
   - **The goal's outcome ends the relay.** The ledger records `goal=active` from the marker. When a later marker reads `goal=none` after one that read `active`, the goal has ended, met or cleared, and the relay stops as item 5 says. The relay never carries the session past the end state the operator wrote.
   - **One driver at a time.** `/goal` drives its work as Stop-hook continuations inside one turn. When it has not converged after its passes, Claude Code pauses the goal, which stays active and waits for a message. The goal survives a compaction, the mod's included, and starts no turn on its own after one. A relay resume submitted after the compaction is the message that continues the paused goal, so the resume and the goal never drive the session at once. The mod therefore resumes under an active goal as it does without one, and the goal's evaluator still decides when the goal is met. `max_relays` counts these relays like any other.

     > Edited 2026-11-26, per the item 10 probe: the probe ran nine goal passes in one turn, saw the goal pause and survive two compactions with no turn of its own, and saw the mod's resume continue it until the evaluator judged it met. The bullet read: "**No second driver.** ... If `/goal`'s loop starts the next turn after a compaction on its own, a relay resume would be a second turn driving the same session. So while a goal is active the mod compacts with the handoff and submits no resume. ..."
   - **Detecting a goal.** No mod API reports an active goal or its evaluator's verdict: the types have no such field or event, and `$.tool.call` has no `GetGoal` tool to call. The `command.run` event is a reliable signal for a goal typed while the mod is loaded: it arrives with `command` `goal` and the condition in `args`. It misses a goal set before the mod loaded. The wrap marker's `goal=` field, the agent's report of a condition it can see in its context, stays in the marker and fills the ledger in that case. The ledger uses it to stop the relay when a goal ends (above).

     > Edited 2026-11-26, per the item 10 probe: `command.run` showed `/goal` with its condition as `args`, and `GetGoal` was rejected as no such tool. The bullet read, in part: "The mod uses the marker as the signal and `command.run` as a cross-check, and probe (e) settles whether either is reliable."

7. **Relay against Claude Code's own autocompact.** The two settings are independent:

   | `relay` | Native autocompact | What happens |
   |---|---|---|
   | on | off | The intended pairing. The policy alone decides when to compact: at `relay_at_pct`, at the idle bound, or at a `/wrap`. The idle path runs. |
   | on | on | The fill trigger fires below the native threshold (item 2a). A native compaction that comes first, as in one long turn that crosses both, meets the PreCompact hook and gets no resume. The idle path runs, since its gate follows the relay setting (item 2). |
   | off | on | Claude Code compacts on its own. The PreCompact hook directs it with the latest fresh handoff or the outline, and nothing resumes. This is what a project set to `off` does when the native switch is on. |
   | off | off | As today: wrap ends with a `/compact` line for the operator, and ADR-414's idle path ends in its lapse or its compaction after the wrap. |

8. **Changes to the wrap skill.**
   - **Step 0** reads the effective mode with `ways settings get mods.attend.continuity.relay --project "$(git rev-parse --show-toplevel)"`, alongside the context gauge. The argument `handoff` forces handoff mode for the run. A mod-submitted wrap's turn already says which mode it is in.
   - **Step 4** branches. In handoff mode it prints the `/compact <focus>` line as it does now. In relay mode it prints no `/compact` line. It ends with the relay marker as its last line, one of:
     - `relay: continue tasks=<open tasks> goal=<active|none> wrap=<wrap id> head=<short HEAD>`
     - `relay: stop <empty|decision|doubt> tasks=<open tasks> goal=<active|none> wrap=<wrap id> head=<short HEAD>`

     The mod reads the marker from the last line of the turn's final text, matching `^relay: (continue|stop (empty|decision|doubt)) tasks=\d+ goal=(active|none) wrap=[0-9A-Za-z-]{8,64} head=[0-9a-f]{7,40}$`. In relay mode the skill runs with `stay` semantics whoever invoked it: it lands nothing and asks nothing, and a decision for the operator goes into DECISIONS and the `stop decision` marker.
   - **A wrap id per wrap.** HEAD alone cannot show freshness: a generation that makes no commits leaves HEAD unchanged, so a wrap that failed to rewrite the handoff would let the previous one pass. Each wrap therefore carries an id unique to it. For a wrap the mod submits, the mod issues the id in the wrap turn's text and records it in the ledger before the turn starts. For a `/wrap`, the skill makes the id from the UTC time and the session id, such as `20261126T2140Z-3f9a1c2e`, and the mod records it from the marker. The skill writes the id on the handoff's first line, after the date and HEAD (`2026-11-26 e6bbfbdd wrap=20261126T2140Z-3f9a1c2e`), and in the marker. The mod treats the handoff as fresh only when its first line carries the id of the wrap just judged, which for a mod-submitted wrap is also the id it issued.
   - **The claim that a skill cannot compact** changes where it is now false. A skill still cannot run `/compact`, and in relay mode the attend mod compacts after the wrap. The implementation edits these files, outside this record's change: `skills/wrap/SKILL.md` (the introduction, Step 4, Key Principles and Not for), `hooks/ways/meta/wrap/wrap.md` (The compaction caveat), and `hooks/ways/meta/compaction-checkpoint/compaction-checkpoint.md` (Step 4, which also learns that in relay mode the relay's wrap at 75% comes before it).

9. **Relationship to other decisions.**
   - **Amends ADR-165 item 2.** Its assumption that compaction cannot be automated holds for a skill, and the attend mod now compacts and resumes after a relay-mode wrap. ADR-165 gains a dated amendment note at item 2 when this record is accepted.
   - **Amends ADR-414.** Item 6's lapse ending and its compaction after the wrap now apply in handoff mode only, and item 8's keys move to `mods.attend.continuity` (item 1). Item 2's autocompact gate for the idle path applies with relay `off` only; with relay `on` the idle path runs whatever native autocompact says (item 2). ADR-414 gains dated amendment notes at items 2, 6 and 8 when this record is accepted.
   - **Issue #857.** Relay passes the handoff as `instructions` to Claude Code's own summarizer. Answering `session.compact` with `{ messages }`, the handoff as the whole compacted conversation, stays a later option for #857, once the `instructions` path is exercised and shown to lose content.
   - **Issue #975.** This record is the same-process variant of the Memento relay: the same session continues after a compaction. #975's fresh instance after `/clear`, and its team mode, stay open there.
   - **ADR-610.** The handoff file is the one ADR-610's session-start way reads. A relay's resume keeps the session id, so it is the writer's own successor at distance zero.

10. **Verification before acceptance.** This record is accepted only after a probe in an interactive session runs a wrap, then `$.session.compact`, then `$.prompt.submit`, and confirms:
   - (a) the resume turn starts cleanly after the compaction, as its own main-loop turn, and reads the handoff;
   - (b) whether a compaction the mod starts runs through the mod's own `session.compact` handlers, and whether it fires the classic PreCompact hook, so the safety net neither doubles nor rewrites the relay's instructions;
   - (c) the PreCompact hook's stdout directs a native automatic compaction, with the summary visibly following the handoff;
   - (d) `session.measure` reports the context fill after each main-loop turn, so the fill trigger can be judged there;
   - (e) what `/goal` does after a compaction the mod started: whether the goal survives it, whether its loop starts a turn on its own, whether a turn the mod submits can get in between the goal's turns, and whether the marker's `goal=` field or `command.run` shows an active goal reliably (item 6).

   A finding that contradicts an item is recorded as a dated note here before acceptance.

   **For the implementation PR.** The probe submitted a fixed resume prompt and did not run the wrap skill, so a resume that reads a handoff the skill wrote was not exercised. The implementation PR's acceptance criteria include one: in an interactive session, the relay-mode wrap skill writes `.claude/handoff.wrap.md` with its wrap id, the mod compacts with it and resumes, and the resumed turn reads that file, works from its TASKS and starts its DO NEXT. This record is accepted when that implementation merges.

   **Probe, 2026-11-26.** The probe ran Claude Code 2.1.332 interactively in tmux on Haiku 5.5, with a throwaway mod and a classic PreCompact hook that printed a heron marker. It cost about $0.19 by `session.measure`'s `cost`. The mod compacted with a kingfisher marker as instructions and then submitted a fixed resume. It did not run the wrap skill, so a resume that reads a real handoff was not exercised.
   - (a) Confirmed. `$.session.compact` resolved without `skip` (42,896 to 7,573 tokens). `$.prompt.submit` then started a main-loop `turn.start` with no `agentId`, framed as "The relayprobe plugin sent a message: …", and the model answered "resumed".
   - (b) The mod's own `session.compact` hook did not hear the mod's own call, though it heard a typed `/compact` (`manual`) and native compactions (`auto`). The classic PreCompact hook fired for the mod's call with `trigger: "manual"`, not `plugin`, and with `custom_instructions` set to the mod's text. The hook's stdout was added to the mod's instructions and did not replace them: the summary named both kingfisher and heron. Item 3's rule that the hook prints nothing when `custom_instructions` is non-empty is what keeps the safety net from doubling the relay's instructions.
   - (c) Confirmed for `manual` and `auto`. With `CLAUDE_CODE_AUTO_COMPACT_WINDOW=100000`, one turn that read 250 KB compacted twice in mid-turn with `trigger: auto`, and each PostCompact `compact_summary` named heron.
   - (d) Confirmed, with two limits. `session.measure` fired once after each main-loop turn and carried `context.tokens`. Its `window` stayed at 1,000,000 under the 100,000 autocompact window, and it did not fire between the two mid-turn compactions. A fill check at a turn's end therefore cannot come before a native compaction inside one long turn (item 7), and `percent` does not measure against the autocompact threshold that item 2a lowers the relay threshold under.
   - (e) `/goal` drives its work as Stop-hook continuations inside one turn: nine "Goal not yet met" passes shared one `turnId`, then the engine paused the goal ("send a message to continue") and the footer still read `◎ /goal active`. So a session under a goal is idle only once the goal has paused or ended. A bare `/compact` and then the mod's compaction both kept the goal, and no turn started on its own after either. The mod's submitted resume counted as the message that continues a paused goal, and the evaluator judged the goal met at the end of that turn, so the resume and the goal did not collide. `command.run` showed `/goal` with the condition as `args`. No mod API showed an active goal: `$.tool.call({ tool: "GetGoal" })` rejected because the session has no tool of that name.
   - The task list survived all four compactions: `TaskList`, called through `$.tool.call`, still returned task 1 as pending.
   - Contradicted: item 6's rule that the mod submits no resume under an active goal, and the matching line in item 4. The goal's loop does not start a turn after the mod's compaction, so by item 6's own condition the resume is submitted under a goal too. Item 2a's threshold under `autoCompactThreshold` cannot be read from `session.measure`, as (d) says.

## Consequences

### Positive

- An operator can leave a session to work through its task list across several context windows, with a handoff at each seam and a cap on how far it goes.
- One policy, one judgment in attend and one settings family cover idle time and context fill. The wrap skill, the reply routing and the compaction plumbing are shared, not copied.
- No compaction gets a generic summary, in either mode and on managed installs too, because the PreCompact hook directs the ones the policy did not start.
- Handoff mode is today's behaviour, and it stays the default.

### Negative

- Unattended generations cost money. At 75% of a 1M window on Opus 5, each generation is a long run of turns over a large prefix. Five generations may cost tens of dollars with nobody watching, and on a subscription they weigh on the usage limits in an undocumented way. `max_relays` and the stop conditions bound it, and nothing else does.
- The transcript gains turns the operator did not type: wraps, resumes and the work between them. The plugin is named as their source, and the operator still has to read what ran.
- In relay the agent judges its own stop. No independent check reads the handoff before a resume unless a `/goal` supplies one.
- A drifting generation is caught only by the agent's own doubt report and the cap. The operator's steering, the reason they gave for handoff mode, is absent by design.
- The relay rests on an early-access mod API (`$.session.compact`, `$.prompt.submit`, `session.measure`) that this design has not exercised. Item 10 exists because the types and docs are not proof.
- The rename touches every persisted keepalive key, the registry, attend-chat's settings list, the mod, attend's check, their tests, and ADR-414's text. The dual read keeps old files working for its period, and one more lint finding appears until each file is fixed.
- A single long turn that crosses the native threshold mid-turn is compacted by Claude Code before the relay's fill check can run, because `session.measure` fires only at a turn's end. The PreCompact hook directs that compaction, and the turn continues with no resume and no wrap of its own, so the summary rests on the last fresh handoff or the outline.

### Neutral

- Implementation adds `relay`, `relay_at_pct` and `max_relays` and the family rename to the settings registry, `attend keepwarm resumed` and the decision's relay fields to attend, the fill listener, marker parser and resume to the mod, `ways hook pre-compact` and its wiring to the ways hooks, and the Step 0 and Step 4 branches to the wrap skill, with plugin tests on the mock clock and attend unit tests over fixed ledgers.
- The compaction-checkpoint way still fires at 85% in handoff mode. In relay mode the relay's wrap normally comes first.

## Alternatives Considered

- **`/clear` and a fresh instance from the handoff, as #975 sketches.** Rejected for this step: a mod cannot run `/clear` and then start a turn in the new session it creates, and a fresh instance pays a full cold write of its first prefix. The same-process relay keeps the session id, the task list and the ledger. #975 stays open for the team mode, where another session restarts this one.
- **The PreCompact hook alone, with no resume.** Rejected as the relay and kept as the safety net (item 3): it improves every compaction but leaves the session idle afterwards, which is the waiting the operator asked to end.
- **Answer `session.compact` with `{ messages }`, the handoff as the whole compacted conversation.** Deferred to #857. It gives full control of what survives, but it replaces Claude Code's summarizer by hand, it runs only where the mod is not passed over, and the `instructions` path has not yet been shown to lose content.
- **Follow Claude Code's autocompact state when the key is unset.** Rejected on the operator's correction: relay is a specific use of compaction through the wrap, not a general toggle, and tying the two together would turn relay on for every default install, where native autocompact is on.
- **Fold the mode into `after_wrap` as a third value, `relay`.** Rejected: `after_wrap` governs only the idle path's wrap today, the fill trigger belongs to relay alone, and the operator described relay as a switch for a project.
- **Keep the family name and add `mods.attend.keepalive.relay`.** Rejected: it avoids a migration but files a context-fill feature under idle time for good, and the shared wrap keys would keep a name that describes one trigger.
- **Keep ADR-414's lapse after an idle wrap in relay mode.** Rejected: it would leave relay mode waiting on the operator after the most common unattended event, an idle session, which is what the operator asked to end. A session that needs the operator says so in its marker and stops.
- **An independent check before each resume (deferred, not adopted).** Before each resume, a cheap independent reader, such as a `$.model` fork, would read the handoff against the task list and decide whether to continue. It sits between the agent judging itself and a full `/goal` evaluator. Deferred: it adds a model call to every relay, and its value can be measured only once the relay runs. It is the first addition to consider if relays are seen to run past the point where they should have stopped.
- **No cap, stop only on the agent's report.** Rejected: the agent's report is the one check a drifting generation can get wrong, so a count bounds the spend whatever the agent decides.
- **A fill trigger at Claude Code's own threshold.** Rejected: at the native threshold the wrap has little room to read state and write a dense handoff, and with native autocompact on, Claude Code's compaction would race it.
