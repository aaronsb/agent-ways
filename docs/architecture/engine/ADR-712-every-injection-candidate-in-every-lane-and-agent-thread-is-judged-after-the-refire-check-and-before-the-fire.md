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
  - precedent: ADR-196
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
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

- **Decided:** Every way a lane is about to inject goes to the relevance judge first, whatever fired it and whichever agent thread it is in. That covers semantic hits, `pattern:` keywords, `pattern_strict` ways, `commands:` and `files:` regexes, `tool_name:` ways, state triggers, postchecks, checks and the subagent dispatch scan. Each lane runs three stages in order: the matcher picks candidates, a read-only refire check sets aside the ones the refire curve holds, and the judge decides on the rest before any fire is recorded. The judge reads the candidate's path and description against the calling agent's own recent context. In enforce mode a blocked way is not injected and keeps its refire budget. In shadow mode the lane injects at once and a detached judge logs the verdicts. A share of the held candidates, `gate.held_sample` (default 0.1), goes to the same detached judge for the record.
- **Trades away:** A declared trigger no longer fires on its own. An author's `commands:` regex or `pattern_strict` keyword now proposes a way, and the judge can keep it out. In enforce mode a tool call with candidates waits on the judge, at most the hook's 6 s budget, about 1 to 2 s in the median case.
- **One-way?** No. The trigger kinds the judge sees are one constant, `gate::JUDGED`, and `gate.mode: shadow` or `off` restores injection without a verdict.
- **Probes:** *Confident (scope):* you want the judge on every candidate in every thread, declared triggers included, so authored regexes now propose rather than inject. *Not confident (context):* the tool lanes feed the judge the agent's prose since its last human turn followed by the call itself, where ADR-707 measured that narration let more off-topic ways pass; is that the "recent context samples" you meant?
- **Inversion:** at one end the matcher decides alone and the judge is advisory; at the other every candidate waits on a model. The decision takes the second end for injection, keeps refire as the decay model in front of it, and moves the wait off the hot path wherever the verdict cannot change what is shown.

## Context

ADR-196 §1 put the judge after the matcher and before a fire is recorded, for ways the matcher selected semantically, and left `commands:`, `files:` and `pattern_strict` ways ungated in the first release. Its addendum added keyword fires on the prompt lane. In the code the gate had one call site, the prompt lane, which runs for the main agent's prompts. The Bash, file, tool, state and dispatch lanes injected without a verdict, in the main thread and in every subagent. A model of one long session put the Bash lane at 95% of what subagents received, and ADR-707 measured the semantic Bash lane at about 3% on topic.

The operator set the scope as every candidate, and named the three stages' roles: the matcher gets reasonably close, the judge decides whether the activity that triggered the match is a go or no go for the steering, and the refire curve, the context decay model, keeps a relevant way from being released again too soon.

## Decision

1. **Every candidate.** Each lane sends every candidate it would inject to the judge: semantic, keyword, `pattern_strict`, `commands:`, `files:`, `tool_name:`, state triggers, postchecks and checks. Which kinds go is one switch, `gate::JUDGED` in `tools/ways-cli/src/cmd/scan/gate.rs`, set to `Every`; its other setting, `Matched`, is ADR-196 §1 as built. This amends ADR-196 §1.
2. **Every lane and thread.** The prompt, queued-message, command (Bash), file, tool, state, post-tool and task (dispatch) lanes all judge, in the main agent and in every subagent. A dispatch is judged against the subagent's brief, so SubagentStart injects only what passed. A `ways_read` pull is the agent's own lookup and is not judged. The core posture that the state lane re-injects is not a candidate and is not judged.
3. **The order.** The matcher picks; the read-only refire check (`gate::refire_open`) sets aside what the refire curve holds; the judge decides on the rest; the fire is recorded only for what is shown. Both orders inject the same set, since a held way is not shown either way; this one only skips judging candidates that refire would hold anyway. The refire check is a step of its own so it can move after the judge.
4. **The judge's input.** The candidate is the way's path and description, as ADR-196 §2 set. The context is the calling agent's own: on the prompt lane, Claude's last reply and the prompt; on a tool lane, the agent's last human turn (a subagent's brief) and then its prose since, with the call in hand as the last turn, so a profile of `turns: 1` reads the call. A Bash call reads as ADR-188's `About to run: <description>` line followed by the command, cut to 400 characters. The judge prompt carries no labelled examples; a labelled set is outside this decision.
5. **Batches.** Every candidate is judged, `max_candidates` (8) to a request in the matcher's order, with all of a call's requests inside one deadline. A request that cannot start before the deadline fails open for its own candidates. Requests run one after another; a call with more than eight candidates waits for each.
6. **The hook's budget.** A PreToolUse or PostToolUse hook is bounded at 10 s. The tool lanes give the judge at most 6 s (`gate::TOOL_WAIT`), the agent's start included, and cut the profile's `timeout_ms` to fit, so a hook fails open before Claude Code would kill it. The prompt and state lanes keep 28 s within their 30 s bound.
7. **Shadow mode off the hot path.** In shadow mode a tool lane injects every candidate at once and hands them, with what became of each, to `ways judge detached`, a process in a session of its own that nothing waits on. It judges them and writes the verdicts to the decision log. The prompt lane still judges shadow mode in its own hook.
8. **Held candidates for the record.** In either mode, each candidate the refire check held is sampled at `gate.held_sample`, a float from 0 to 1 registered in `ways settings` (default 0.1), and the sample goes to the detached judge. Each is logged as `held_refire` with its verdict and `judge_ms`. Nothing is injected from it.
9. **The record.** Every lane writes a decision record when it had a candidate, with the lane as `surface`, the calling `agent`, each way's result, `p_yes` and verdict, and the judge's status and `judge_ms`. A detached judge writes its own record under the same `scan_id`, marked `detached`. Every `way_judged`, `judge_call` and `gate_fallback` line names its `lane` and `agent_id`.

## Consequences

### Positive

- The Bash lane, the largest source of subagent injection, passes through the judge in every thread.
- A shadow-mode day can be read per lane and per agent from `decisions.jsonl` without slowing any tool call.
- The held-candidate sample gives the judge's accuracy on what refire holds, which the refire-first order would otherwise lose.

### Negative

- An author's regex or `pattern_strict` keyword is no longer a guarantee; a way that must always reach the agent has no trigger that bypasses the judge.
- In enforce mode, a tool call with candidates waits for the judge; across a long session that is hours of summed wait, and the deferred delivery of ADR-188 item 3d and ADR-606 remains the way to remove it.
- Each detached judgement is a process and a judge request of its own.
- The tool lanes give the judge the agent's narration, which ADR-707 found let off-topic ways pass; that is a departure from ADR-188 item 3c.

### Neutral

- ADR-188's describer, its ordered segment block and its deferred delivery are not built here; this is the synchronous form.
- Checks have no refire curve of their own, so every matched check is judged; `show::check_within` still decides their decay after the verdict.
- A state lane call with no context at all, such as SessionStart before any turn, has nothing to judge relevance against and passes as with the gate off.

## Alternatives Considered

- **Semantic candidates only, as ADR-196 §1 had it.** Kept as the switch's other setting. The operator chose every candidate.
- **The judge before the refire check.** It would give a verdict on every candidate for the record, at the cost of a judge request for each one refire would hold anyway. The held-candidate sample keeps the accuracy history at a tenth of that cost.
- **Shadow mode on the hot path.** Rejected for the tool lanes: the verdict cannot change what is shown, and about 6.9k Bash calls with candidates in one session would wait 2 to 4 hours in all.
- **Parallel requests for more than eight candidates.** Not taken: requests run in sequence within the one deadline. Parallel requests would shorten a wait past eight candidates, which few calls reach.
