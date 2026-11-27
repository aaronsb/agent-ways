---
contract: adr/v1
kind: decision
verb: change
capability: disclosure
amends: ADR-196#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "that's a significant bug then. judge runs across all lanes agents and subagents"
    via: "the operator, 2026-11-20, quoted in issue #1287"
  - operator: aaronsb
    level: directed
    said: "b) everything. judge is checking relevancy against recent context samples. it should be fed examples and makes the determination to allow the ways injection or not"
    via: "#ways-decay-control, 2026-11-20, answering whether the judge sees only semantic candidates or every candidate"
  - operator: aaronsb
    level: directed
    said: "the match is to match and get reasonably close (in the realm of appropriate) the judge is to determine the relevancy of the activity that triggered the match to the steering ( a go or no go) and the refire is to prevent a repeated release of the same way to the agent, even if it was relevant because the refire curve is our context decay model of understanding"
    via: "#ways-decay-control, 2026-11-20"
  - operator: aaronsb
    level: directed
    said: "that makes sense to check refire first and ignore. then we don't pay an evaluation. on the other hand, we loose historical data on accurateness of the fire so I'm not sure which way to go there"
    via: "#ways-decay-control, 2026-11-20, on the order of the refire check and the judge"
  - operator: aaronsb
    level: directed
    said: "ok this makes sense and i agree now too"
    via: "#ways-decay-control, 2026-11-20, agreeing to the refire check first, then the judge, then the recorded fire, with a detached judge on a 0.1 sample of held candidates"
  - evidence: "issue #1287: gate::apply had one call site, the prompt lane (tools/ways-cli/src/cmd/scan/prompt.rs:327), which serves the main agent's prompts; the command, file, tool, state and task lanes injected unjudged in every agent thread. A model of vtium session 6dda20bd put the Bash lane at 95% of subagent injection (8.49M of 8.97M tokens over 180 subagents), none of it judged, and about 4% of the main thread's injection through the judge"
  - evidence: "latency sizing, #ways-decay-control 2026-11-20: about 6.9k Bash calls with at least one candidate in one vtium session; at 1 to 2 s per judge call that is 2 to 4 hours of blocked Bash time summed across agents, so shadow mode judges off the hot path"
  - evidence: "tools/ways-cli/tests/judge_lanes.rs: a subagent's Bash call, the main agent's Bash call and the file lane each reach a stand-in judge host through `ways judge take` and `ways judge answer`; an enforced block is not injected and records no fire; shadow mode injects everything and logs `would_block` with `judge_ms`; with no engine the lanes inject as before"
  - evidence: "~/.local/state/agent-ways/events.jsonl on ways 1.54.0 under `gate.engine: claude-code`: a PreToolUse command-lane call logged `gate_fallback` with reason `deadline: unclaimed`, `gate_ms` 502 and 3 candidates for the main agent, while the prompt lane in the same session was judged; the ways-judge mod claimed jobs only around `prompt.submit`"
  - precedent: ADR-196
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Request + call"
    via: "the operator's pick in a multiple-choice question in the head session, 2026-11-20; options: request + call (the last human turn or subagent brief plus the call, no assistant narration; agent-written label, marked Recommended), request + narration + call (as built), or a recent-turns window"
    covers: [context]
  - operator: aaronsb
    said: "let's get all 5 in fix and published asap"
    via: "operator, 2026-11-27, agent-ways session, on the #1372 investigation's five decisions"
status: accepted
date: 2026-11-20
deciders:
  - aaronsb
related:
  - ADR-160
  - ADR-188
  - ADR-196
  - ADR-606
  - ADR-701
  - ADR-707
  - ADR-708
---

# ADR-712: Every injection candidate in every lane and agent thread is judged, after the refire check and before the fire

## Summary

- **Decided:** Every way a lane is about to inject goes to the relevance judge first, whatever fired it and whichever agent thread it is in. That covers semantic hits, `pattern:` keywords, `pattern_strict` ways, `commands:` and `files:` regexes, `tool_name:` ways, state triggers, postchecks, checks and the subagent dispatch scan. Each lane runs three stages in order: the matcher picks candidates, a read-only refire check sets aside the ones the refire curve holds, and the judge decides on the rest before any fire is recorded. The judge reads the candidate's path and description against the calling agent's own request and the call in hand. In enforce mode a blocked way is not injected and keeps its refire budget. In shadow mode the lane injects at once and a detached judge logs the verdicts. A share of the held candidates, `gate.held_sample` (default 0.1), goes to the same detached judge for the record.
- **Trades away:** A declared trigger no longer fires on its own. An author's `commands:` regex or `pattern_strict` keyword now proposes a way, and the judge can keep it out. In enforce mode a tool call with candidates waits on the judge, at most the hook's 6 s budget, about 1 to 2 s in the median case.
- **One-way?** No. The trigger kinds the judge sees are one constant, `gate::JUDGED`, and `gate.mode: shadow` or `off` restores injection without a verdict.
- **Probes:** *Confident (scope):* you want the judge on every candidate in every thread, declared triggers included, so authored regexes now propose rather than inject. *Not confident (context):* the tool lanes as first built fed the judge the agent's prose since its last human turn followed by the call itself, where ADR-707 measured that narration let more off-topic ways pass; is that the "recent context samples" you meant? The operator answered request and call, with no narration, and §4 records it.
- **Inversion:** at one end the matcher decides alone and the judge is advisory; at the other every candidate waits on a model. The decision takes the second end for injection, keeps refire as the decay model in front of it, and moves the wait off the hot path wherever the verdict cannot change what is shown.

## Context

ADR-196 §1 put the judge after the matcher and before a fire is recorded, for ways the matcher selected semantically, and left `commands:`, `files:` and `pattern_strict` ways ungated in the first release. Its addendum added keyword fires on the prompt lane. In the code the gate had one call site, the prompt lane, which runs for the main agent's prompts. The Bash, file, tool, state and dispatch lanes injected without a verdict, in the main thread and in every subagent. A model of one long session put the Bash lane at 95% of what subagents received, and ADR-707 measured the semantic Bash lane at about 3% on topic.

The operator set the scope as every candidate, and named the three stages' roles: the matcher gets reasonably close, the judge decides whether the activity that triggered the match is a go or no go for the steering, and the refire curve, the context decay model, keeps a relevant way from being released again too soon.

## Decision

1. **Every candidate.** Each lane sends every candidate it would inject to the judge: semantic, keyword, `pattern_strict`, `commands:`, `files:`, `tool_name:`, state triggers, postchecks and checks. Which kinds go is one switch, `gate::JUDGED` in `tools/ways-cli/src/cmd/scan/gate.rs`, set to `Every`; its other setting, `Matched`, is ADR-196 §1 as built. This amends ADR-196 §1.
2. **Every lane and thread.** The prompt, queued-message, command (Bash), file, tool, state, post-tool and task (dispatch) lanes all judge, in the main agent and in every subagent. A dispatch is judged against the subagent's brief, so SubagentStart injects only what passed. Every dispatch that matched leaves a stash, with no ways in it when the judge blocked them all, so the judge's blocks never leave a matched dispatch without one. A dispatch that matched nothing still writes no stash. A SubagentStart claims the oldest stash, so a subagent from an unmatched dispatch, started in parallel with others, can still take another dispatch's stash. That gap predates this decision, which leaves it as it is. A `ways_read` pull is the agent's own lookup and is not judged. The core posture that the state lane re-injects is not a candidate and is not judged.

   **Note, 2026-11-20: the Claude Code host.** Under `gate.engine: claude-code` each lane's request is a job that the ways-judge mod claims and judges (ADR-708 §5). As released in ways 1.54.0 the mod claimed jobs only around `prompt.submit`, so every tool lane's job went unclaimed and failed open with `deadline: unclaimed` after the 500 ms claim window; the stand-in host in `tools/ways-cli/tests/judge_lanes.rs` hid it. The mod now serves `tool.call` as well, whose chain runs the PreToolUse, PostToolUse and PostToolUseFailure hooks in the main agent and in every subagent, and keeps a take waiting for as long as either chain runs, so several jobs from one call are each claimed. The state lane's SessionStart call (startup, compaction, resume, `/clear`) has no mod event around it, so no host could claim its job.

   **Note, 2026-11-20: SessionStart under the claude-code engine.** At SessionStart the gate decides before it resolves the session's engine, because the mod registers as host only at its first event, after SessionStart. A session held on `claude-code`, or one with nothing held whose `gate.engine` names a claude-code profile, skips the judge handoff: the state lane writes no job, waits on no claim window, logs no `gate_fallback`, resolves and holds nothing, and injects its candidates unjudged, as with the gate off. A resolution made there would find no host, so it would hold the session on a key engine for its whole life or log `engine_unavailable`. The operator's call, since a job written there can never be claimed. A session held on a key engine (anthropic, openrouter) still judges at SessionStart, as does one whose `gate.engine` names a key engine.

   **Note, 2026-11-20: detached judges under the claude-code engine.** Items 7 and 8 start a detached judge after the hook has returned, and under `claude-code` no host is open then: its job could be claimed only by a later event's take, and because a take claims the oldest job it would queue ahead of a hot-path job and push that job past its claim window. Under `claude-code` no detached judge starts, so the held sample of item 8 and the tool lanes' shadow verdicts of item 7 need a key engine for now. The session logs `judge_detach_skipped`, with reason `claude-code engine has no host outside a hook`, once. A tool lane's shadow-mode decision record then carries judge status `detach_skipped` in place of `detached`, so a reader knows no verdicts follow; `ways tune stats` counts it as neither a judged scan nor a fallback. A key engine starts the detached judge as before.
3. **The order.** The matcher picks; the read-only refire check (`gate::refire_open`) sets aside what the refire curve holds; the judge decides on the rest; the fire is recorded only for what is shown. Both orders inject the same set, since a held way is not shown either way; this one only skips judging candidates that refire would hold anyway. The refire check is a step of its own so it can move after the judge.
4. **The judge's input.** The candidate is the way's path and description, as ADR-196 §2 set. The context is the calling agent's own. On the prompt lane it is Claude's last reply and the prompt, as before. On a tool lane it is the request and the call: the agent's last human turn (a subagent's brief), then the call in hand as the agent's turn. The agent's narration is not sent, the operator's pick (Request + call). A Bash call reads as ADR-188's `About to run: <description>` line followed by the command, cut to 400 characters; a file edit reads as `About to edit: <path>`. The request carries `min_turns`, so both turns are read whatever the profile's `turns`. A dispatch is judged against the subagent's brief alone, and the state lane against the prompt in hand, else the last human turn. The judge prompt carries no labelled examples; a labelled set is outside this decision.

   **Note, 2026-11-21: finding the request (#1298).** As released in ways 1.54, a tool lane read the request from the last 64 KB of the agent's transcript. A subagent's brief is the first line of its transcript, and Claude Code writes some 300 KB of setup records after it before the first tool call, so in vtium session b84a8f13 none of 625 subagent calls reached the judge with its brief: each judge read the call alone. The lookup now reads back from the transcript's end a page at a time, up to 32 MB, and falls back to the transcript's first line, which is a subagent's brief. The request is the newest record that asks the agent for something: a prompt the operator typed (`origin.kind: human`, or a `queued_command` attachment with `humanTurn` when typed during a turn), a lead's message to a running subagent (`origin.kind: coordinator`, as a user record or a `queued_command` attachment, which Claude Code marks `isMeta`), the goal condition a `/goal` run continues on (`origin.kind: auto-continuation`), or a record with no origin, such as a brief. A task notification, a plugin's message and a peer session's message arrive without being asked for, and are not the request. Neither are Claude Code's own records: `isMeta` reminders and skill bodies, a compaction summary, slash-command and shell echoes, an interrupt, and its `api-request-blob` copies of the composed request. Every judged record outside the prompt lane, the detached judge's included, carries `request: found` or `request: none` in its judge block.
5. **Batches.** Every candidate is judged, `max_candidates` (8) to a request in the matcher's order, with all of a call's requests inside one deadline. A request that cannot start before the deadline fails open for its own candidates. Requests run one after another; a call with more than eight candidates waits for each.

   **Note, 2026-11-27: one request per scan (#1372).** The operator decided to replace the batches. In one session on the command lane, 43% of fires had no verdict: 576 `gate_fallback` rows said `deadline: before call`, 189 said `model_error: aborted`, and 4,694 fires came from candidates past the first eight, whose second batch could not start inside the 2000 ms deadline (2,127 second-batch requests aborted). The judge passed 15% of the semantic candidates it saw and 43% of the regex ones, and the semantic matcher produced 91% of the unjudged fires. A tool-lane scan now sends one judge request holding up to the profile's `tool_max_candidates` (22) candidates, within the profile's `tool_timeout_ms` (3000 ms). The gate's loop and each judge job count down the same deadline, where before the loop assumed 1500 ms more than a job was given. The hook's 6 s `TOOL_WAIT` stays the outer bound. When a candidate gets no verdict, because it is past the cap, the request timed out or failed, or no judge host answered, a semantic candidate is blocked and a regex, declared, `pattern:` or check candidate passes; each is logged as `way_unjudged` with its reason. The failure that opened the gap, a lane failing open, therefore holds only for candidates that an author wrote an explicit trigger for. ADR-197's per-request cap and its open overflow question are settled by the same decision. Each outcome row carries the command's first 80 characters, with secrets redacted, so a misfire can be traced to its command. The rows have a field for the judge's critique, which stays empty until the judge returns one. The operator also decided that the semantic match and the judge see the command's operative verbs plus its description, not the agent's prose, which builds ADR-188 items 3b and 3c. Item 4's judge input for a Bash call changes with it. The 3000 ms `tool_timeout_ms` budget sits inside the 6 s `TOOL_WAIT` ceiling that item 6 gives a tool lane. Item 5 read: "Every candidate is judged, `max_candidates` (8) to a request in the matcher's order, with all of a call's requests inside one deadline."
6. **The hook's deadline.** Claude Code kills a hook at its bound and drops its output, and a fire recorded before the kill stands for text nobody saw. Each hook therefore takes one deadline when it starts, a second inside its bound: 9 s on the 10 s hooks (the PreToolUse and PostToolUse adapters, check-response, clear-markers) and 28 s on the 30 s ones (check-prompt, check-state, check-queued, inject-subagent). Every slow step takes only what is left of it. The postchecks leave 5 s for what follows them. The judge takes at most 6 s on a tool lane (`gate::TOOL_WAIT`), the agent's start included, and leaves 1.5 s for showing the ways; the profile's `timeout_ms` is cut to fit, and past it the lane fails open. Each macro runs for the lesser of its 3 s and what is left less 0.75 s, and is skipped when that is under 0.1 s. A way that would be shown in the last 0.25 s is held, with no fire recorded.
7. **Shadow mode off the hot path.** In shadow mode a tool lane injects every candidate at once and hands them, with what became of each, to `ways judge detached`, a process in a session of its own that nothing waits on. It judges them and writes the verdicts to the decision log. The prompt lane still judges shadow mode in its own hook.
8. **Held candidates for the record.** In either mode, each candidate the refire check held is sampled at `gate.held_sample`, a float from 0 to 1 registered in `ways settings` (default 0.1), and the sample goes to the detached judge. Each is logged as `held_refire` with its verdict and `judge_ms`. Nothing is injected from it.
9. **The record.** While the gate is on, every lane writes a decision record when it had a candidate, with the lane as `surface`, the calling `agent`, each way's result, `p_yes` and verdict, and the judge's status and `judge_ms`. A detached judge writes its own record under the same `scan_id`, marked `detached`. Every `way_judged`, `judge_call` and `gate_fallback` line names its `lane` and `agent_id`.

## Consequences

### Positive

- The Bash lane, the largest source of subagent injection, passes through the judge in every thread.
- A shadow-mode day can be read per lane and per agent from `decisions.jsonl` without slowing any tool call.
- The held-candidate sample gives the judge's accuracy on what refire holds, which the refire-first order would otherwise lose.

### Negative

- An author's regex or `pattern_strict` keyword is no longer a guarantee; a way that must always reach the agent has no trigger that bypasses the judge.
- In enforce mode, a tool call with candidates waits for the judge; across a long session that is hours of summed wait, and the deferred delivery of ADR-188 item 3d and ADR-606 remains the way to remove it.
- Each detached judgement is a process and a judge request of its own.
- The judge on a tool lane does not see the agent's reasoning, so a way relevant to what the agent is thinking but not to the request or the call is blocked.

### Neutral

- ADR-188's describer, its ordered segment block and its deferred delivery are not built here; this is the synchronous form.
- Checks have no refire curve of their own, so every matched check is judged; `show::check_within` still decides their decay after the verdict.
- A state lane call with no context at all, such as SessionStart before any turn, has nothing to judge relevance against and passes as with the gate off.

## Alternatives Considered

- **Semantic candidates only, as ADR-196 §1 had it.** Kept as the switch's other setting. The operator chose every candidate.
- **The judge before the refire check.** It would give a verdict on every candidate for the record, at the cost of a judge request for each one refire would hold anyway. The held-candidate sample keeps the accuracy history at a tenth of that cost.
- **Shadow mode on the hot path.** Rejected for the tool lanes: the verdict cannot change what is shown, and about 6.9k Bash calls with candidates in one session would wait 2 to 4 hours in all.
- **Request, narration and call, as first built.** Rejected by the operator: ADR-707 measured narration letting off-topic ways pass.
- **A window of recent turns.** Rejected by the operator for the same reason, and because the request and the call are what the way is judged against.
- **Parallel requests for more than eight candidates.** Not taken: requests run in sequence within the one deadline. Parallel requests would shorten a wait past eight candidates, which few calls reach.
