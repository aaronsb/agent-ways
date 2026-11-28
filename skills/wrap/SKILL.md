---
name: wrap
description: Wrap up a session at a natural seam — land in-flight work, square the TaskList honestly, write a copy-paste continuation prompt, then hand off to a directed /compact, or in relay mode end with the marker the attend mod compacts and resumes on. In boundary mode the agent runs it when a piece of work lands and ends by calling the attend mod's boundary tool, or, where the tool is not listed, goes back to work with the handoff as a checkpoint. Use when the user says "wrap up", "wrap this up", "let's wrap", "end of session", "checkpoint and compact", "hand off", or invokes /wrap.
allowed-tools: Bash, Read, Grep, Glob, TaskList, TaskCreate, TaskUpdate, TaskGet, AskUserQuestion, Agent, mcp__attend__attend_boundary
---

# Wrap

Close out a session at its end. This is the on-demand sibling of the `meta/compaction-checkpoint` way — the way fires automatically near the context limit; `/wrap` runs the same muscle whenever you decide the session is done.

**Wrap is not merge.** Landing is *iterative* — many merges happen across a session as each piece of work lands. Wrap is *terminal* — it runs once, at the end of the session, or in boundary mode at the end of a piece of work. By the time you wrap, the session's work should already be merged. So `/wrap` does **not** land work; it confirms nothing got stranded, then produces the artifacts that outlive the session.

The deliverable is two things that survive a reset: a **TaskList that honestly mirrors reality**, and a **copy-paste continuation prompt**. A skill cannot run `/compact`, so in handoff mode the skill ends by handing you a ready-to-run `/compact` line. In relay mode it ends with a relay marker line instead, and the attend mod compacts the conversation with the handoff and starts a turn that resumes the work (Step 4). In boundary mode it ends by calling the attend mod's `attend_boundary` tool, and a judge decides whether the session stops, compacts or continues (Step 4).

## Arguments

- `/wrap` — full flow: land work (offer; relay mode lands nothing), square the TaskList, write the handoff, then end with a `/compact` line in handoff mode or the relay marker in relay mode (Step 4).
- `/wrap stay` — wrapping mid-work on purpose: **skip** the land/merge step. Don't land anything; just capture state accurately and produce the handoff. Use when the seam is "I'm pausing", not "I'm done".
- `/wrap handoff` — forces handoff mode for this run, whatever the relay setting says. The wrap ends with a `/compact` line and no relay marker, so the operator can stop a relay-mode session by hand.
- `/wrap boundary` — the agent runs this itself when a piece of work lands: a merge, a release, or a closed task with nothing in flight behind it. It runs with `stay` semantics: it lands nothing and asks nothing, so it does not call AskUserQuestion. It writes the handoff, records the wrap id with attend, and ends by calling `attend_boundary` with the wrap id and the agent's own call (Step 4). Where the tool is not listed, the handoff stands as a checkpoint and the agent goes back to DO NEXT. A `/wrap` without `boundary` never calls the tool, so a wrap the operator types is not judged.

## Step 0 — Read the context gauge and the mode first

Before anything else, check where the session sits and which mode the wrap runs in. The gauge is the same source as `context-status`:

```bash
ways context --json   # used / total tokens, pct filled, total auto-detected from model; `session` is the session id
ways settings get mods.attend.continuity.relay --project "$(git rev-parse --show-toplevel)"   # on or off
```

The wrap runs in one of two modes. **Handoff mode** ends the wrap with a `/compact <focus>` line for the operator to run. **Relay mode** ends it with a relay marker line, and the attend mod then compacts the conversation with the handoff and submits a turn that resumes the work. Pick the mode in this order:

1. The argument `handoff` forces handoff mode.
2. A wrap turn from the attend mod that names relay mode and a wrap id is in relay mode. Use that wrap id (Step 3).
3. Otherwise the setting decides. `on` is relay mode. Any other output, `off` or an error, is handoff mode. A wrap turn from the attend mod that names no relay mode is decided here too.

**In relay mode the skill runs with `stay` semantics, whoever invoked it.** It lands nothing: Step 1 records stranded work in the handoff and offers no merge. It asks nothing: do not call AskUserQuestion, because the operator may be away. It starts no new work. A choice that only the operator can make goes into the handoff's DECISIONS section, and Step 4 says when it ends the relay with the `stop decision` marker.

The fill level calibrates the entire wrap, and is worth saying out loud — the user may be wrapping early *on purpose*, and that's valid. Locate it against the ways trigger curves (the `todos` way fires ~75%, `compaction-checkpoint` ~85%):

- **Early** (well below the curves — e.g. 300k/1M, 30%): lots of headroom. Say so plainly ("we'd be wrapping early — 300k of 1M used"), but **don't talk the user out of it** — end-of-day, task-switching, or banking a clean state are all good reasons. When early, `/compact` reclaims little and costs you the live thread, so in Step 4 *offer the lighter paths*: checkpoint-and-continue (no compact), or `/clear`+paste later. The handoff can be leaner — less accumulated context means less to reconstruct.
- **Late** (near or past the curves; a relay-mode wrap that the attend mod asks for at 75% is a late wrap): compaction is imminent or already mandatory. This is the urgent case — be thorough and quick. A lot is about to be summarized away, so the continuation prompt and TaskList carry more weight; get them down before auto-compact fires on its own.

Let this scale Steps 2–3: **how much detail the handoff needs is proportional to how much context is about to be lost.** A near-full window deserves a dense, complete prompt; an early wrap can be crisp. Knowing the *total* window (1M vs 200k) is what makes that call.

## Step 1 — Loose-ends check (not a merge step)

Wrap assumes the session's work already merged. This step only confirms that and catches anything stranded — it is **not** the place to run a full delivery. Assess in parallel:

```bash
git status --short              # uncommitted changes?
git branch --show-current
git log --oneline main..HEAD    # unmerged commits?
```

- **Clean / already merged** → the expected state. Move on.
- **Something stranded** (uncommitted changes, or commits never PR'd) → this is a guard, not wrap's job. Surface it and let the user decide: invoke `merge` to land it, or leave it parked. In relay mode and boundary mode, treat it as parked.
- **Parked on purpose** (`/wrap stay`, relay mode, boundary mode, or user chooses to leave it) → do **not** merge. Record *exactly* what's uncommitted, on which branch, and why it's parked — verbatim into Step 3 so post-reset-you can resume the partial work cold.

The rule: wrap never silently drops stranded work, and never lands it behind your back. Either the user lands it (via `merge`), or it's described in the handoff.

## Step 2 — TaskList honesty pass (load-bearing)

This is the point of the skill, not a footnote. Post-compaction-you (or a fresh session) inherits the TaskList with **zero memory of this conversation** — so the list must be *true*, not optimistic. Use `TaskList` to read current state, then reconcile it:

**Close the books** (`TaskUpdate`):
- Mark genuinely-finished tasks `completed`.
- Retire stale, obsolete, abandoned, or aspirational tasks — don't carry cruft forward. A task that no longer reflects the plan is noise that will mislead the next session.

**Write what's real** (`TaskCreate`):
- One task per piece of *actual* remaining work.
- Each `description` must be resumable cold: file paths, decisions already made (so they aren't re-litigated), what was tried, and the next concrete step. The `subject` is imperative; `activeForm` is present-continuous.
- Mark the genuine next task `in_progress`.

**Open findings are work, not backlog.** A review or test finding that the session's work owns and that is still unfixed gets a task to fix it, under the project convention it falls under (ADR-614). Do not turn it into an issue to close the books. If something merged with an owned finding open, the task says so and names the PR. When the project's instruction file or its review briefs send findings to a follow-up by default, note the line in the continuation as something to change.

**The bar is honesty.** Before moving on, sanity-check: does any "pending" task describe work that's actually done? Does any real remaining thread have no task? A dishonest list is worse than none — it hands the next session a confident lie.

**Then print the list.** Reconciling silently gives the user nothing to check against. Enumerate every task — finished, in progress, and backlog — with its full description:

```
#1 [IN PROGRESS] <subject>
  <description verbatim: paths, decisions already made, what was tried, next concrete step>

#2 [OPEN] <subject>
  <description>

#3 [OPEN, blocked by #2] <subject>
  <description>

#4 [DONE] <subject>
```

Number in the order the next session should work them, not creation order. Mark blocking relationships inline (`[OPEN, blocked by #2]`). Finished tasks get subject only — enough to confirm they were closed deliberately, not enough to crowd the live work.

Do not compress the descriptions here. This block and Step 3's prompt carry different weight: the prompt is the cold-start narrative, this is the per-thread detail, and a summarized description is exactly the content post-reset-you cannot reconstruct.

## Step 3 — Write the continuation prompt

Produce a single copy-pastable block. Compaction keeps the session alive; this prompt is the harder guarantee — it survives even a `/clear` or a brand-new window. Also save it to `.claude/handoff.wrap.md` at the repository root (`git rev-parse --show-toplevel`), where `start` and a successor session look for it. When `.gitignore` does not cover it, list it in `.git/info/exclude` so the working tree stays clean.

The file's first line is `<YYYY-MM-DD> <short HEAD> wrap=<wrap id>` in both modes, such as `2026-11-26 e6bbfbdd wrap=20261126T214012Z-3f9a1c2e`, and the prompt starts on the next line. The wrap id names this one wrap. The attend mod and the PreCompact hook treat the file as fresh when its first line carries the id of the wrap they judge, so a handoff left over from an earlier wrap is not taken for this one. When a wrap turn from the attend mod names a wrap id, set `WRAP_ID` to that id and skip the two lines that make one. Otherwise make one from the UTC time and the first 8 letters and digits of the session id, or, where no session id is set, the shell's process id and a random number. In either case record the id with attend, in both modes, so the PreCompact hook knows this wrap:

```bash
SID="$(printf '%s' "${CLAUDE_CODE_SESSION_ID:-${CLAUDE_SESSION_ID}}" | tr -cd '0-9A-Za-z' | cut -c1-8)"
WRAP_ID="$(date -u +%Y%m%dT%H%M%SZ)-${SID:-p$$r${RANDOM:-0}}"
attend keepwarm wrap-id --record "$WRAP_ID" >/dev/null 2>&1 || true   # no attend, nothing recorded
echo "$(date +%F) $(git rev-parse --short HEAD) wrap=$WRAP_ID"        # the handoff's first line
```

Shell variables do not survive between Bash calls, so keep the printed id: the relay marker and the boundary call in Step 4 repeat it.

Tailor sections to the work, but cover:

```
Continue work on <project> (<path>) — <one-line what it is and its purpose>.
<build/test status: how to build, what's green>.

RESUME — claude --resume <session id>. Only the transcript holds: <what a cold reload would recover that this handoff leaves out, or "nothing this handoff lacks">.

LANDED — <what shipped this session, with PR #s / commit refs; "merged to main, green">.

STATE — IN FLIGHT — <anything parked: branch, uncommitted files, why; from Step 1>.

INVARIANTS / GOTCHAS — <things NOT recoverable from git: crash-safety rules, ordering
constraints, "X is noise, trust Y", env quirks. Skip if none.>

TASKS — recreate these with the task list tool before starting; they are the working set.
<the Step 2 enumeration verbatim — number, status, subject, full description>.

WORKING MODE — <for complex work only: steer from the main session, delegate the heavy
work to subagents, one worktree per branch. See "Working mode" below. Skip for simple work.>

DO NEXT — <the immediate next step, then 1-3 alternatives, each specific enough to start>.

DECISIONS — <each choice left to the operator, one numbered line each, in this form>. Skip if none.
N. <question> — blocks: <task ids | none> — lean: <the lean> — reversible: <yes | no>

CONVENTIONS — <project-specific workflow worth repeating: branch→PR→review→merge, lint
quirks, verification patterns>.

KEY FILES / BRANCHES — <the handful that matter, with paths>.
```

The `RESUME` line names this session so the operator can reload it cold, and it says what only the transcript holds, so the operator can tell whether the reload is worth its price. The session id is the `session` field of Step 0's `ways context --json`, the same value as `$CLAUDE_CODE_SESSION_ID`. What only the transcript holds is detail this handoff leaves out and a later session would want, such as long tool output, a review's full text, or the reasoning behind a call. Write "nothing this handoff lacks" when the handoff carries everything that matters.

Each `DECISIONS` line states three things after the question. `blocks:` lists the numbers of the tasks in `TASKS` that cannot start until the operator answers, or `none`. `lean:` is the answer you would act on. `reversible:` is `yes` when acting on the lean can be undone later and `no` when it is a one-way step, such as a release, a publish, a force push or a deleted branch.

If a `/goal` is active, **restate the goal condition** at the top, under the first line — it's the anchor the post-compaction turns steer by, and it survives compaction. Never write that the goal is met: only the goal's evaluator judges that.

The `TASKS` section carries the Step 2 enumeration in full and tells the next session to rehydrate it with `TaskCreate`. Pasting this prompt into a `/clear`ed or brand-new window arrives with an empty task list, so spell the instruction out — "recreate these as tasks before starting" — rather than assuming the list travels with the prompt.

Keep it dense and concrete. The gold standard is a prompt a stranger could resume from. Test that before moving on. A stranger holding only the repository and this prompt can: run the project, run the verification gates, find the current plan, and name the next step. Fix the prompt for any of the four that fails.

Number every decision left to the operator, in the prompt and in the chat, so the reply can be "1 and 3". A decision buried in a paragraph gets paraphrased back or missed.

### Working mode

Write the `WORKING MODE` section when the remaining work has complexity: several open tasks, a change across many files, or review-and-fix rounds still to run. It tells the next session to spend its own context on steering and to delegate the rest, so the session lasts across the whole task list:

- The main session steers. It reads reports, makes the calls, writes briefs, talks with the operator, and merges. It does not read large diffs or run long test suites itself.
- Implementation, review, and fan-out reads go to subagents. Each brief names the files, the gates with their pass condition, and the shape of the report to return.
- Each branch of implementation gets its own git worktree, in the location the project uses for worktrees, so parallel agents and the main checkout never share a working tree. List the existing worktrees and what each holds.
- A fix round after the second goes to a freshly briefed agent. A worker resumed through many rounds can run out of context partway through its gates.
- Small edits, single-file fixes, and anything the operator is watching live stay in the main session.

Name the subagent types and the worktree location this project uses, so the next session does not have to rediscover them. Work that is one task in one file gets no `WORKING MODE` section.

## Close-out: what the session taught

Walk four categories. Each item lands in an issue, a design note, or a way edit, or gets the line "nothing of interest, because <reason>":

- A sharp edge that bit, and the tell that would spot it next time.
- A corrected assumption: the session started believing X and the work proved it false.
- A trigger that should have fired and did not (a way, a skill description, a hook).
- A reusable tool the session built: a script, a query, or a check worth running again.

## Step 4 — Hand off

Step 0's mode picks the ending. Handoff mode ends with a `/compact` line for the user, and relay mode ends with the relay marker. A `/wrap boundary` ends with a call to the boundary tool instead, and goes back to work where the tool is not listed (Boundary mode, below).

### Handoff mode (compaction is gauge-dependent)

First, let Step 0 decide whether compaction even applies:

- **Early wrap** → `/compact` is *optional*; it reclaims little and ends the live thread. Lead with the lighter paths: checkpoint-and-continue (keep working in the same session — the TaskList and handoff are already saved), or `/clear`+paste later for a clean switch. Mention `/compact` as available, not as the default.
- **Late wrap** → compaction is the point. Make the directed `/compact` the headline action.

A skill cannot run `/compact`, because Claude Code does not let a hook or a skill invoke a slash command. So when compaction is the move, end by handing the user a one-keystroke directed compaction, and explain why directed beats generic:

> The synthesis above is now the freshest content in context. Run `/compact` with these
> focus instructions and it'll preserve the synthesis as the anchor instead of the system
> keeping whatever it guesses:

Then output the exact line, with focus instructions tailored to what should survive:

```
/compact keep the continuation prompt and TaskList state; <project>'s next step is <X>; drop the exploratory back-and-forth
```

Offer the alternative explicitly: for a hard reset (switching tasks entirely), `/clear` then paste the Step 3 prompt into a fresh session — the old one stays in `/resume`.

Make the `/compact ...` line the **last thing** in your response so it's immediately copyable. Do not claim the session is compacted — you've prepared it; the user pulls the trigger.

### Relay mode

Print no `/compact` line and offer no lighter paths. End the reply with the relay marker as its very last line, written as plain text outside any code block, with nothing after it. The attend mod reads the marker from the last line of the reply, and a missing or malformed marker stops the relay. The marker is one of:

```
relay: continue tasks=<open tasks> goal=<active|none> wrap=<wrap id> head=<short HEAD>
relay: stop <empty|decision|doubt> tasks=<open tasks> goal=<active|none> wrap=<wrap id> head=<short HEAD>
```

Fill in the fields:

- `tasks=` is the number of open tasks after Step 2: the tasks that `TaskList` shows as pending or in progress. Completed and deleted tasks do not count.
- `goal=active` when a `/goal` condition is in force in this conversation's context, that is, one was set and has not been cleared or ended. Otherwise `goal=none`. Do not write `none` because you believe the goal is met, since only the goal's evaluator decides that.
- `wrap=` is the wrap id on the handoff's first line (Step 3).
- `head=` is the short HEAD commit, from `git rev-parse --short HEAD`.

Pick the first reason that holds:

- `stop empty` when no open work remains (`tasks=0`).
- `stop decision` when every open task waits on a decision whose lean is one-way (`reversible: no` in the handoff's DECISIONS section). A decision that blocks only some tasks is no reason to stop: work goes on with the rest, and the agent sends the question to the operator (below). A decision whose lean is reversible is no reason to stop either: work goes ahead on the lean, and the handoff logs the call for the operator to overrule.
- `stop doubt` when your doubt about the direction is high, as core.md defines doubt.
- `continue` otherwise: open tasks remain and DO NEXT can start without the operator.

The agent sends a question that blocks only some tasks itself, with the `attend_send` tool addressed to the operator's member id, or with `attend message send --to <member id> "<question>"`. `attend message awaiting` then counts it as pending, in its `operator` count. In a session without the attend mod, the question stays in the handoff's DECISIONS section.

After a `continue`, the attend mod compacts the conversation with the handoff as the compaction's instructions and submits a turn that tells the agent to read `.claude/handoff.wrap.md` and start its DO NEXT, up to `max_relays` times (5 by default) since the operator's last prompt. After a `stop`, the mod compacts with the handoff, submits no resume, and sends the wrap's reply to the operator over attend. Do not claim the session is compacted or resumed: the mod does both after the reply ends. In a session without the attend mod, nothing acts on the marker, and the handoff waits in the file.

### Boundary mode

A `/wrap boundary` ends with a call to the attend mod's boundary tool, `attend_boundary`, in place of the `/compact` line or the relay marker. The session lists the tool as `mcp__attend__attend_boundary`. Where it appears only as a deferred tool's name, load it the way this session loads deferred tools. Where the tool is not listed at all, because the attend mod is absent, is older than the tool, or does not register it in this session, do not call it. The handoff stands as a checkpoint, and the wrap ends as the `continue` outcome does: print no relay marker and no `/compact` line, and go back to work at DO NEXT.

Write the handoff before the call, because the mod refuses to arm an exit until the file with the wrap's id exists. Call the tool with two inputs:

- `wrap_id` is the wrap id on the handoff's first line (Step 3).
- `own_call` is your own call. `stop` means no open work remains, or every open task waits on a decision whose lean is one-way. `compact` means open work remains and the next step needs little of what the context window holds. `continue` means open work remains and the next step draws on what the window holds.

The mod records your call. A judge, a separate model call that reads only the handoff and the conditions the mod measures, makes the decision. The reply names the outcome. Act on the outcome, not on your own call:

- `continue`: go back to work at DO NEXT. The handoff stands as the checkpoint for an exit that cannot run a wrap. When the reply says the handoff lacks what you see, add what is missing to the handoff, then go back to work.
- `stop`: the mod arms the exit. End the turn and start nothing new. An operator prompt disarms the exit.
- `compact`: end the turn. After the reply ends, the mod compacts with the handoff and then resumes the work or stops.
- `doubt`: end the turn. The mod stops and sends the handoff to the operator over attend.
- A refusal: the reply names the call the mod refused and why, such as a stop while agents are in flight, a `continue` while the session drains, or the `max_relays` cap. Do what the reply says.

Do not claim the session is stopped, compacted or resumed: the mod does each after the reply ends.

## Key Principles

- **The TaskList must be honest** — done is done, stale is gone, real work is captured with resume-detail. That's the load-bearing deliverable.
- **Print the list you squared** — an unenumerated honesty pass is unreviewable; the user should see every task and description without asking.
- **Prepare, don't pretend** — a skill cannot run `/compact`. In handoff mode it hands off a ready `/compact` line; in relay mode the attend mod compacts after the wrap's reply. Never report compaction as done.
- **Directed > generic** — a synthesis-anchored `/compact` keeps what matters; an unanchored one keeps what the system guesses.
- **On-demand** — that's the whole reason this exists alongside the `compaction-checkpoint` way. You choose the seam with `/wrap`, the agent runs `/wrap boundary` when a piece of work lands, or the attend mod asks for a wrap at its idle bound or, in relay mode, at its context fill threshold.
- **The agent's call is recorded, and the judge makes the call** — in boundary mode the agent passes its own call to `attend_boundary`, the mod records it, and the judge decides from the handoff and the measured conditions. The agent acts on the reply.
- **Two survival layers** — TaskList + continuation prompt survive even a hard reset; `/compact` is the lighter same-session path on top. The prompt carries the tasks *and* the instruction to rehydrate them, because a fresh window starts with an empty list.

## Not for

- Authoring the code being wrapped — `/wrap` closes out work, it doesn't write it.
- Replacing the `compaction-checkpoint` way — that fires passively near the limit; this is the on-demand version. They share DNA on purpose.
- Running `/compact` itself — a skill cannot run a slash command. In handoff mode it prepares everything and hands you `/compact`; in relay mode the attend mod compacts after the wrap and starts the resume turn.
- Deciding whether the session stops, compacts or continues after a boundary wrap — the judge decides; the wrap records the agent's own call.

## See Also

- `merge` — Step 1 delegates here to land work.
- `compaction-checkpoint` (meta way) — the automatic sibling that fires near the context limit.
- `todos` (meta way) — the TaskList-at-compaction discipline this enforces on demand.
- `context-status` — check how much room is left before you decide to wrap.
- `attend` — the attend mod, which asks for wraps and, in relay mode, compacts and resumes after them.
