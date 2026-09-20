---
status: Proposed
date: 2026-09-19
deciders:
  - aaronsb
  - claude
related:
  - ADR-123
  - ADR-125
  - ADR-126
  - ADR-155
  - ADR-160
  - ADR-161
  - ADR-172
  - ADR-181
---

# ADR-188: PostToolUse delivery for tool-lane ways and retirement of the semantic Bash surface

## Context

Three lanes match ways against tool calls. The Bash lane (`check-bash-pre.sh`, `ways scan command`) runs the `commands:` regex against the command text, the `pattern:` regex against the tool description, and the semantic lane that ADR-155 §4 added, which embeds the command, its description, and the assistant's prose since the last human turn (ADR-160). The Edit and Write lane (`check-file-pre.sh`, `ways scan file`) runs the `files:` regex against the path. Both lanes also score `.check.md` checks. All of it runs on PreToolUse and prints its matches as `{"decision":"approve","additionalContext":...}`.

Issue #528 measured what happens to that output. Across 115 transcripts on one workstation running Claude Code 2.1.x, 17 August to 19 September 2026, Claude Code recorded every PreToolUse hook output carrying a way as a `hook_success` attachment and created no `hook_additional_context` attachment for any of them: 2,922 rows on PreToolUse:Bash, 107 on PreToolUse:Edit, 103 on PreToolUse:Write, 3,132 in total, zero delivered. The same transcripts show PostToolUse output from `check-post.sh` paired one to one: 145 `hook_success` rows carrying a way, 144 `hook_additional_context` rows. The audit rated those PostToolUse fires as followed, in the Debugging and Sub-Agents tool-triggered episodes. UserPromptSubmit output is delivered. Upstream anthropics/claude-code#19432 reports the cause on PreToolUse: `additionalContext` is received and logged and is never injected, and the top-level shape the binary emits is an undocumented PreToolUse field.

The engine's events log counts the same month per way: 6,905 `semantic:bash:en` fires and 496 `bash` (commands regex) fires, 73% of all `way_fired` events. In the session that filed #528, the log shows 65 Bash-lane fires and the model saw none of them. Every `commands:` trigger in the corpus (`git commit`, `gh pr create`, `gh issue`, `oh-my-posh`), every `files:` trigger, and every check scored on these two lanes was silent for the whole measured month.

Two consequences follow.

**The tool lanes suppress the prompt lane.** `show::way_scored` records the fire (`session::record_way_fire`) before it returns the content the caller prints. A silent Bash-lane fire stamps engagement salience to 1.0 for that way (ADR-123). A prompt-lane match on the same way inside the refire half-life (ADR-126) then returns `Suppressed`. The lane the model cannot see mutes the lane it can.

**The semantic Bash surface ran a natural experiment.** For a month it fired on tokenised command text at the volume above and nobody noticed the absence of its output. Its samples in the log are off-topic at high scores: `workstation/shell/prompt` at 0.91 on `head -40 slides decks cohort-01-intro-framing.html`, `ea/email` at 0.86 on an `ssh` listing, `softwaredev/delivery/commits` at 0.90 on a `git log` pipeline. Per Bash call that fired, the median was 1 way, 20% of calls fired 4 or more, and the maximum was 30. ADR-155 §4 added the lane on the premise that the tool description is Claude's statement of intent at act time and that fire and near-miss telemetry would monitor the surface for noise. Delivery was never verified.

Two pieces of work depend on this decision. #525 derives write targets from Bash command text so that `files:` ways fire in auto mode, where edits run through `sed -i`, `tee`, and redirects; it inherits whatever delivery the Bash lane has. The transcript audit's recommendation to prefer tool triggers over prompt triggers rests on the 144 PostToolUse postcheck deliveries, which is a different mechanism from the PreToolUse lanes the recommendation named.

What PreToolUse can still do is refuse. ADR-181 names the guard class: exit 2 with the reason on stderr, which the model receives as the tool result. That is the one PreToolUse channel measured to reach the model, and it carries a refusal with no guidance attached.

The Task lane already solved a version of this problem. `check-task-pre.sh` writes matched way ids to a stash under the session directory and `inject-subagent.sh` drains it on SubagentStart, emitting the canonical `hookSpecificOutput.additionalContext` envelope and logging `way_fired` at that moment. That stash exists because a subagent has no UserPromptSubmit of its own.

### The next-prompt stash probe

The first candidate carrier for tool-lane matches was that same pattern pointed at the main session: stash on PreToolUse, drain into the next UserPromptSubmit output. A probe on a spike branch (worktree `worktree-agent-aa3ba24f71bb47ead`, commit `e2e53243`, kept out of merge) built it: a stash per session and agent at `<sessions_root>/<session>/pretool-stash.<agent>.jsonl`, drained deliver-once into the next prompt, with `way_fired`, engagement, and the marker each recorded once, 288 tests passing. The carrier works. The probe returned four findings that this decision carries:

1. Recording happens at match time in `show::way_scored`, before delivery. A stash that never drains has still consumed the way's first fire and started its refire clock.
2. Subagents never receive UserPromptSubmit, so their stashes are orphaned. They do receive PostToolUse.
3. One-turn lag turns pre-action guidance into post-action guidance, and checks lose their pre-flight meaning under a next-prompt drain.
4. One `git commit` with the description "commit the change" produced 10.4k characters of stashed context, because the semantic Bash lane fired four neighbouring ways on the description. That volume becomes a visible prompt tax the moment any carrier delivers it.

## Decision

**Tool-lane matching moves to PostToolUse and emits the canonical envelope. The semantic lane on the Bash surface retires. Engagement is stamped at delivery. The PreToolUse emitter is deleted.**

1. **`commands:`, `pattern:` on the description, `files:`, and checks run on PostToolUse.** `check-post.sh` already receives `Edit|Write|Bash|Task` on PostToolUse and PostToolUseFailure, with `tool_name`, `tool_input`, and `tool_response` on stdin. It gains a dispatch: for Bash it calls `ways scan command` with `tool_input.command` and `tool_input.description`; for Edit and Write it calls `ways scan file` with `tool_input.file_path`. The matchers are unchanged. The scan commands emit through `emit_hook_context("PostToolUse", ...)`, the same envelope on the same hook that carried the 144 postcheck deliveries and that ADR-161 chose for the queued-message lane. Delivery is same-turn, one tool call after the match, and it reaches subagents, which receive PostToolUse with `agent_id` set. `check-bash-pre.sh` and `check-file-pre.sh` come out of `settings.json`. `strip-session-link-pre.sh` stays on PreToolUse:Bash as a guard.

2. **The semantic lane on the Bash surface retires.** `scan command` stops scoring ways by embedding, and the `semantic:bash:en` and `semantic:bash:multi` channels are removed. This point supersedes ADR-155 §4. The embed pass remains for checks that declare `description` and `vocabulary`, since `check_semantic_score` reads it, and the lookbehind (ADR-160 stage 1) continues to feed that pass. Reasoning that precedes an action still reaches the semantic lane on the next prompt through the response-context channel (ADR-155 §3), where the user's own message anchors relevance.

3. **Engagement is stamped at delivery.** A lane records a fire, stamps engagement, and logs `way_fired` when the body is emitted on a hook event the harness has been measured to deliver. Today those are UserPromptSubmit, PostToolUse, SubagentStart, and SessionStart through `emit_hook_context`. Under point 1 the match and the emit share one process on one delivering hook, so `way_scored`'s stamp-before-return is a delivery-time stamp. Any lane that separates match from delivery, the Task stash today or a future stash, stamps when it drains, or its `way_fired` event carries both a `matched_at` and a `delivered_at` tick. The inline `decision`-bearing emitter in `scan::command` and `scan::file` is deleted, so no path in the binary can print a way body the harness drops. A caller that wants scores without a disclosure uses the scoring path with recording off; `ways introspect` reads recorded events and stamps nothing.

4. **#525 builds on the PostToolUse Bash lane.** Derived write targets from a Bash command feed the same `files:` matcher the Edit and Write lane uses, on the same hook, with trigger `bash-write`. `tool_response` is available there; whether to skip targets when the command failed is #525's call.

5. **Guidance at act time waits on upstream.** PreToolUse carries guards (ADR-181), the Task stash, and `mark-tasks-active.sh`. If anthropics/claude-code#19432 closes with PreToolUse `additionalContext` injected, a follow-up ADR can move `commands:` and `files:` back to act time on measured evidence. The emit-shape fix alone is not adopted now, since upstream reports the documented field is also uninjected.

6. **Acceptance gate.** Delivery of the PostToolUse envelope is settled by the 145-to-144 pairing above and needs no further probe. The load-bearing claim that remains, per the prototype-before-accept way, is that a `commands:` or `files:` way body delivered on PostToolUse is acted on within the same turn. One live session after the build confirms it: a `commands:` fire on `git commit` in the events log, the matching `hook_additional_context` row in the same transcript, and the assistant's next action following the way. That observation is recorded here before the status moves to Accepted.

Reversibility: cheap for points 1, 3, and 4, which move existing matchers between hooks and delete one emitter. Point 2 removes one branch in `scan::command`; way embeddings are untouched, so re-adding it needs no corpus rebuild.

## Consequences

### Positive

- Every `commands:` and `files:` trigger in the corpus reaches the model for the first time, one tool call after the match. Commit guidance arrives before the next commit or the PR; `files:` guidance on the first edit of a file arrives before the second.
- The tool lanes stop muting the prompt lane. A way matched on a tool call is recorded once it is delivered, so a later prompt-lane match inside the refire window is suppressed only when the model has already read the way.
- Silent injection volume drops by 7,401 fires a month (6,905 semantic plus 496 regex). Delivered volume becomes the 496 regex fires, the `files:` fires, and whatever #525 adds. The tail of calls firing 4 or more ways, and the 10.4k-character commit the probe measured, go with the semantic lane.
- Subagents keep tool-lane delivery, since PostToolUse fires inside them and `check-post.sh` already exports `agent_id`.
- Delivery rests on a mechanism measured in this project's own transcripts and rated as followed in the audit.
- One hook process fewer runs before each Bash call executes. The embed pass moves from the pre hook to the post hook.

### Negative

- Guidance arrives after the action. A commit-message way fires after the commit exists; a check written as "before you run X" lands after X ran. Check and way bodies in act-time voice need an authoring pass to read correctly one step later. That pass is follow-up work under this ADR.
- The Bash surface loses its semantic channel, and with it the possibility ADR-155 §4 named of a way reaching the moment before action on Claude's stated intent. The measured lane never delivered and its samples were noisy, so the loss is a possibility. The response-context channel carries the reasoning to the next prompt.
- ADR-155 part 5 planned to read Bash-surface semantic fires as telemetry for pattern demotion. The month of events already in the log remains usable for that; the stream stops with this ADR. The prompt lane's `way_keyword_gated` and `way_nearmiss` streams continue.
- The events log for the measured window counts fires that were never delivered. Analyses over `way_fired` before this ADR ships must treat channels `bash`, `semantic:bash:*`, and `file` as undelivered.

### Neutral

- ADR-155 §4 is superseded by point 2. The rest of ADR-155 stands.
- The transcript audit's finding on tool triggers is re-read as a finding about PostToolUse postchecks. Its recommendation survives on the mechanism this ADR adopts for all tool-lane matches.
- Engagement state stamped by silent fires lives in per-session directories and expires with the session. No migration.
- `ways introspect` and `scripts/hook-fire-detector.sh` gain a delivered-versus-emitted distinction as a follow-up, so a future silent surface is visible within days.
- PreToolUse becomes a refusal surface in this project. Anything wired there either exits 2 with a reason (ADR-181) or writes state for a later hook to drain.
- The probe's spike branch stays unmerged. Its stash-and-drain code is a measured starting point should PostToolUse delivery regress upstream and a main-session fallback be needed.

## Alternatives Considered

- **Stash on PreToolUse, drain at the next UserPromptSubmit (option 1 in #528).** The Task lane's pattern pointed at the main session. The probe above built it and it delivers once. Rejected on the probe's own findings. Latency is bounded by the turn: an autonomous turn runs tens of tool calls with no prompt between them (65 Bash-lane fires in the session that filed #528), and a commit-format way stashed at the first commit drains after the PR is open. Subagents never receive UserPromptSubmit, so their stashes are orphaned. Checks lose their pre-flight meaning. It adds a stash, a drain, a deliver-once record, and the identity questions ADR-172 had to settle for its drain, where PostToolUse adds nothing.

- **Match on PreToolUse, stash, drain on the same call's PostToolUse.** Rejected. `tool_input` is present on PostToolUse, so the match runs there with the same inputs and no stash. A PreToolUse match also fires on a call the permission system then denies, and a stash for it would need a claim and a cleanup per call.

- **Deliver on Stop.** ADR-172's drain point. Rejected. ADR-161 rejected Stop for the queued-message lane because it cannot steer the current turn, and the same holds here. It also inherits the re-entry and livelock bounds ADR-172 records.

- **Keep the semantic Bash lane and deliver it on PostToolUse.** Rejected. The lane's measured samples fire off-topic at high scores, 20% of firing calls emit 4 or more ways, and one plain `git commit` produced 10.4k characters. Delivering that volume would be a regression from the silence the month measured. If a semantic act-time lane is wanted later, it starts from a calibration pass over the logged month, on its own ADR.

- **Fix the emit shape to `hookSpecificOutput.additionalContext` on PreToolUse and re-measure.** Rejected as the fix. Upstream #19432 reports the documented field is also uninjected, and the `emit_hook_context` docstring records the same silent drop on SessionStart for an undocumented shape. It is cheap to try beside the acceptance observation, and a positive result reopens act-time delivery under point 5.

- **Keep recording as it is and let refire absorb the suppression.** Rejected. The suppression is the second defect in #528, and the probe's first finding shows the same defect recurs in any carrier that stamps at match time. A fire nobody read must never count as a disclosure, whatever the delivery mechanism.

- **Retire the tool lanes entirely and rely on prompt triggers.** Rejected. The 144 postcheck deliveries were rated as followed in the audit, `commands:` triggers are the corpus's exact deterministic triggers (ADR-125, ADR-155 part 5), and #525 needs a tool lane to fire file ways in auto mode.
