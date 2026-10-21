---
contract: adr/v1
kind: decision
verb: change
capability:
  - loop
  - disclosure
  - authoring
  - method
basis:
  - operator: aaronsb
    level: guided
    said: "our adr could introduce a new type of macro that returns its result in the next turn"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21
  - operator: aaronsb
    level: guided
    said: "I think we should use a formula of time against turns to provide a max allows relevancy. if it exceeds that, our 'finally' is a message that says the x macro failed to return in time or turns and it's results are in y (in tmp)"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21
  - operator: aaronsb
    level: guided
    said: "the finally phase of it should indicate that it's abnormal for that to happen and to investigate if reasonable"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21, on the line that replaces a result past its relevance budget
  - operator: aaronsb
    level: guided
    said: "in most repos, the pattern is going to be someone (like me) who we could consider an 'agentic power user' (we don't really want to use that term, but you can get what I mean) where many times I am just pushing things through because it needs to get done. other people are on the repo/project as well, but are new or casual, or just observing. a false signal that causes the power user to have to wait (and never finish) is not effective. so we don't want to block work from happening, but we should have it surfaced and prompted in a way that if there is any pending actions or requests from other users we prioritize a decision (but not necessarily action) on those items"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21, answering no to the codeowners-team probe
  - operator: aaronsb
    level: guided
    said: "declining a pr, or declining to participate is as valid of an answer as sending the response. not doing anything at all will cause the same notifications on a new session to appear. if after multiple sessions more stack up, that's our 'stacking up the snakes' scenario, and it should be more annoying if there's too many of them"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "maybe we add some pressure: after a reasonable amount of PRs have stacked and become unreasonable (I'm not sure what that number is) we add an additional gudiance to dismiss/decline or respond to them."
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "the biggest issue is that when it's a single user reviewing itself, it cant be marked off as self"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "this way we have two more complete scenarios - single contributor that stays honest, and the true collaboration with others model."
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "in both situations, if the github account is an owner or has rights, we can force through any blocks or rules if necessary"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "this override is the 'getting things done' thing that often happens with the agentic power user experience I see"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "the agent /can/ force a merge on its own, especially if it's a single user. but for multiple users, there can be an established convention in the session. one scenario among many might be 'nobody ever seems to reply to the PRs, we can just push them through, ok?' (from the agent) and the operator says yes. this is sort of aligned with the classifier model that claude code uses"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "'making it standing for the repo' if friction is encountered is to configure the repo to not block it"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "writing a memory or a claude skill or something doesn't really solve the root cause"
    via: chat, relayed by the coordinating session
  - operator: aaronsb
    level: guided
    said: "it's a stats of awareness"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21, on the review-request line
  - operator: aaronsb
    level: guided
    said: "or something like that"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21, on the exact wording of the review-request line
  - evidence: "`gh search prs --review-requested=@me --state=open` returned 30 rows with duplicates and no --limit; with --limit 200 and deduplication by repo#number it returned 75. Each run took 4.9 to 6.9 s"
  - evidence: "a way macro runs under MACRO_DEADLINE, 3 s (tools/ways-cli/src/cmd/show/helpers.rs:164); a macro still running then is killed with its process group and its output is discarded"
  - evidence: "six places decide solo versus team with different rules: hooks/ways/softwaredev/delivery/github/macro.sh:84-86 and :122-131, docs/hooks-and-ways/macros.md:54-58, skills/merge/SKILL.md:33-37, commands/project-init.md:51, agents/code-reviewer.md:161-165, and hooks/ways/softwaredev/code/supplychain/automation/automation.md, which names solo with no detection"
  - evidence: "in praecipio/anthropic-partnership the operator authored 19 of the last 21 PRs, which every current rule reads as solo, while CODEOWNERS gives /docs/ and /cert-prep/ four owners and PR #32, by another author, requests four reviewers"
  - evidence: "attend's script sensors can drop a line after the script advanced its own marker: absolute and relative refractory (tools/sensor-trait/src/lib.rs:317-338), the lapsed-cache reset (tools/attend/src/cmd/run/tick.rs:406-413), and a slot magnitude under 3.0 emitted as low priority to stderr only (tick.rs:497-503, tools/attend/src/emit/mod.rs:16-27). The attend mod's hooks (plugins/attend/hooks/lib.ts) run no sensors"
  - upstream: "GitHub REST, create a review for a pull request: a review created without `event` stays PENDING and is visible only to its author until submitted"
  - upstream: "GitHub GraphQL `resolveReviewThread` and `addPullRequestReviewThreadReply` mutations"
  - upstream: "GitHub does not let a PR's author approve it or request changes on it; a review with event COMMENT is allowed, and the author can resolve review threads and set commit statuses through the REST statuses API"
  - precedent: ADR-137
  - precedent: ADR-004
  - precedent: ADR-180
  - precedent: ADR-604
  - upstream: "`gh pr merge --admin` merges past unmet branch protection for an account with admin rights unless the protection enforces for administrators (\"Do not allow bypassing the above settings\"); a ruleset is bypassed only by actors on its bypass list; setting a commit status needs push access"
agent:
  name: claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-21
deciders:
  - aaronsb
amends: [ADR-004#execution-model, 'ADR-604#Note (2026-10-21): rule 2, as ADR-606 amends it']
considered:
  - operator: aaronsb
    said: "sort of like how I can que up messages to send to you and they're relevant and you might get them after you've gotten other context but it still makes sense because the turn that it ran in was nearby"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21, answering whether a deferred result arrives on the next hook even when unrelated
    covers: [next-hook]
  - operator: aaronsb
    said: "in most repos, the pattern is going to be someone (like me) who we could consider an 'agentic power user' (we don't really want to use that term, but you can get what I mean) where many times I am just pushing things through because it needs to get done. other people are on the repo/project as well, but are new or casual, or just observing. a false signal that causes the power user to have to wait (and never finish) is not effective. so we don't want to block work from happening, but we should have it surfaced and prompted in a way that if there is any pending actions or requests from other users we prioritize a decision (but not necessarily action) on those items"
    via: chat, relayed verbatim by the coordinating session, 2026-10-21, answering no to the codeowners-team probe
    covers: [codeowners-team]
  - operator: aaronsb
    said: "deferred items should come back at the start of every session"
    via: chat, relayed by the coordinating session
    covers: [deferred-returns]
  - operator: aaronsb
    said: "let's do this"
    via: chat, accepting ADR-606 before implementation
related:
  - ADR-004
  - ADR-137
  - ADR-180
  - ADR-403
  - ADR-604
observable:
  - "A way declaring `macro_defer:` renders at once under the 3 s path, and its macro's result arrives at the next prompt or tool call with a header naming the way, its trigger and its age. A 6 s `gh search` logs no macro_timeout event."
  - "A deferred result collected past its relevance budget arrives as one line naming the way, the bound it missed, that the miss is abnormal, and the result's path; the file is at that path and the session log holds a deferred_budget_missed event."
  - "The review-request line at session start in this repository counts the top repository as this project and leaves out the vendored llama.cpp submodule (ggml-org/llama.cpp), where the operator has no push access and takes part in no PR, so its open PRs do not enter the open pile."
  - "The involvement check run on praecipio/anthropic-partnership for a change under /docs/ names the four CODEOWNERS and whether each was asked to review, and holds nothing."
  - "In a repository with another person's pending review request, a merge by the operator proceeds after the agent has surfaced the request and recorded the operator's decision on it."
  - "At the start of a new session, an item deferred in an earlier session, a pending-here item or one raised with a PR in hand, is listed again for a decision while GitHub still shows it pending, marked deferred with the date of the deferral; within the session that deferred it, it is not asked again."
  - "An item the operator declined is not listed in later sessions; a review request raised again after the decline is listed as a new item."
  - "With more open items than pending.recur_count, or an oldest older than pending.recur_age, the reminder appears during the session as well as at its start, and work continues without waiting on it."
  - "With the open pile past pending.clear_count or pending.clear_age, the session's first ask is a grouped clearing decision, and one answer to a group writes a marker for each item in it."
  - "Each change of tier logs a pending_tier_entered event naming the tier, the open count and the oldest item's age."
  - "On a PR with no other CODEOWNER, requested reviewer, reviewer, commenter or author, an agent review shows a review/agent commit status that goes from pending to success as its threads are resolved, and returns to pending after a push."
  - "A declined finding's thread carries the reason in a reply before it is resolved."
  - "On a PR with no other CODEOWNER, requested reviewer, reviewer, commenter or author, in a repository where the operator's account can bypass the rule, a merge blocked only by a required approval completes without a question and carries the override comment."
  - "A PR carrying another person's CHANGES_REQUESTED review is treated as scenario B, though GitHub no longer lists that person as a requested reviewer."
  - "When `--admin` or a ruleset bypass is refused, the agent reports the refusal and the rule that blocked it, proposes the configuration change, and does not retry another way."
  - "With CODEOWNERS, PR metadata or `gh` authentication unreadable, the agent treats the PR as scenario B and asks before any force."
  - "In a repository with other people involved, the first forced merge happens only after the operator accepted a convention, and the override comment quotes the convention and names the people whose review was skipped."
  - "After repeated overrides of one rule, the agent proposes a repository configuration change, and once applied the rule no longer blocks."
---

# ADR-606: Deferred macros, and the PR awareness, involvement check and PR feedback way built on them

## Summary

- **Decided:** Ways gain a deferred macro. A way that declares `macro_defer:` starts its macro detached when it fires, renders at once with the last cached result, and the finished result reaches the agent at its next UserPromptSubmit or PostToolUse, once, with a header saying which way ran it, what fired it and how long ago. A result past a relevance budget that combines elapsed time and elapsed turns is replaced by one line saying the miss is abnormal, naming the file holding the result, and telling the agent to investigate when it bears on the work. The first consumer is a session-start line of review-request counts. On the same base: one check of who else is involved replaces six divergent solo/team rules, never gates, and puts other people's pending items in front of the operator for a decision before work goes on. Declining closes an item as fully as acting on it; deferring, or giving no answer, brings it back at every session start, and the reminder grows more prominent as the open pile grows and ages, without ever blocking work. Past a clearing threshold, guidance has the agent put clearing the open pile to the operator as the session's first decision, answered by group rather than item by item; and a PR feedback way runs review findings to closure, with the PR as the ledger, in two scenarios. A single contributor that stays honest: the agent's review posts one thread per finding, each thread closes with evidence or a recorded decision and is resolved, and a `review/agent` commit status, labelled as the author's own claim, stands in for the approval GitHub will not let an author give. True collaboration with others: other people's reviews and the operator's reviews of their PRs run through threads and pending reviews, with the pending-item tiers above. Where the operator's account can bypass a repository rule, the agent may force a merge past an approval-type block on its own when no one else has a stake in the PR, and otherwise only under a convention the operator established, stated or proposed by the agent from evidence and accepted. A failing required check is never forced without the operator. An override lasts for the session and keeps no agent-side exception. Every forced merge is recorded on the PR, and repeated overrides of one rule lead to a proposed change in the repository's own configuration, applied with the operator's yes, so the rule stops blocking.
- **Trades away:** A slow macro's result arrives a turn or more late and can land beside unrelated context. The engine gains detached processes, a results directory and a collector on two hooks. Nothing wakes an idle session when a new review request appears. Reviewer-mode output waits on the operator to submit it. In scenario A the PR's sign-off is a status the author sets on their own work, not an approval. A forced merge weakens enforcement that other people may rely on, and in scenario A the agent forces an approval-type block without asking. Other people's pending items cost the operator a decision at each session start, a deferred item included, and where the repository enforces no review, the agent does not hold work for a CODEOWNER or requested reviewer: their stake is surfaced and decided on, never waited for. A growing open pile is made deliberately more noticeable, in a session that may not be about them.
- **One-way?** No. Reversible now; expensive once ways outside this repository declare `macro_defer:`, since removing the field then breaks their frontmatter.
- **Probes:** *Confident (next-hook):* a slow lookup's result reaches the agent with whatever comes next, a prompt or a tool call, even when that prompt is about something else, as long as it is recent; past that it becomes a one-line pointer to the file. *Not confident (codeowners-team):* asked whether CODEOWNERS entries for other people should make the agent stop for review and refuse `/merge full-sail`; answered no, and the check now surfaces and never gates. *Not confident (deferred-returns):* asked whether a deferred item should stay quiet until work touches it again; answered that deferred items come back at the start of every session.
- **Inversion:** Other people on a repository as a gate that holds the operator's work, or as background the agent never mentions. This sits between: their pending items come first for a decision, work proceeds whatever the decision, and only the repository's own enforcement holds a merge, unless it is overridden under section 5's rules.

## Context

A way's macro runs inside the hook that disclosed the way, under a 3 s deadline that kills it and discards its output. Some context worth giving an agent costs more than that to fetch. A search for the operator's open review requests across GitHub took 4.9 to 6.9 s. ADR-137 rules out raising the deadline: unbounded work runs outside the cycle and leaves a prepared result the cycle reads.

The operator asked for three things. Agents should know which pull requests concern the session: review requests addressed to the operator, and open PRs in the top repository and its git submodules. Agents should work a PR through a feedback loop like the evaluation loop (softwaredev/code/testing/evaluation), one that spans sessions and people. And there should be one answer to who else is involved in a repository or PR. Today six places decide solo versus team with different rules. In most repositories one operator drives most of the work while others on the repository are new, casual or observing; a repository whose PRs are almost all the operator's still has CODEOWNERS and reviewers naming four other people. Treating their presence as a gate leaves the operator waiting on people who will not act, and the work does not finish. Ignoring it leaves their requests unanswered.

The existing paths each fall short. The ways CLI cannot run a 6 s query in a macro. Attend sensors run only under `attend run` or Monitor, not under the attend mod; ADR-137 and the gh-monitor skill retired GitHub watching from attend sensors, although `tools/attend/examples/gh-notifications.sh` still ships as a script sensor; and the proposed command sensors of ADR-403 are unbuilt and lack delivery acknowledgement. A per-feature cache-and-hook pipeline like gh-tasks (ADR-180), which runs synchronously in hooks behind a TTL with no daemon, would work for this feature, but every later slow query would have to copy it, and its fetch still runs on a hook's path when the TTL lapses.

GitHub already records the state a PR loop needs: review threads and whether each is resolved, each review's commit, requested reviewers. Guidance for working that state is missing. A PR with no one else involved shows the gap most: GitHub does not let an author approve their own PR, so an agent review posted under the operator's account leaves the PR with no review decision, and a required-approval rule can never be met. In one such PR a thorough agent review sat in a single comment of fourteen numbered findings with a second comment listing fixes; which findings ended declined, deferred or left could be learned only by matching numbers across prose, and two commits pushed after the review were never re-reviewed. No way covers inline comments, thread replies, resolving a thread, reading a returned review, re-requesting review, or waiting on CI with gh-monitor inside the merge flow.

## Decision

### 1. Deferred macros

**Declaration.** A way declares `macro_defer: <ttl>`, for example `macro_defer: 10m`, beside the existing `macro: prepend|append`. `macro:` keeps meaning where macro output sits in the rendered way. `macro_defer:` makes the run deferred and sets how long a result is reused. A third `macro:` value would merge the two axes, and a deferred way still needs a position for its cached result. An engine that predates the field ignores it and runs the macro synchronously, where a slow macro times out as it does today. The schema (`hooks/ways/frontmatter-schema.yaml`, `display.macro`) gains the field, and `ways author lint` checks its duration.

**Firing.** When the way fires, `render_way` reads the way's cached result for this session and agent:

- Within the TTL, it renders the cached result with its age, `(as of 4 min ago)`, and starts nothing.
- Older or absent, it starts the macro detached in its own process group and session, outside the 3 s deadline, and renders the way with the stale result marked with its age, or with no macro output.

A detached run is bounded at 60 s. An overrun is killed with its process group, logged as `macro_timeout` with `deferred=true`, and reported through the collector as below. One run per way per session and agent is in flight at a time; a fire while one runs starts nothing. A failed run (non-zero exit, or empty output) holds the slot for the TTL, so a failing source is not retried at every fire.

**Location.** Every deferred run writes its stdout to a file under the agent's state directory, `<ways sessions-root>/<session>/deferred/<way>/` for the main agent and `<ways sessions-root>/<session>/agents/<agent>/deferred/<way>/` for a subagent, keyed as disclosure markers are. The sessions root is `$XDG_RUNTIME_DIR/claude-sessions`, or `/tmp/.claude-sessions-<uid>` where that variable is unset, so the files sit in per-user temporary storage. Beside the output, a metadata file records the start time, the trigger (the lane and what matched: `session-start`, `commands: gh pr`, a prompt), the operator turn count at the fire, the exit status, and whether the result was delivered. When the session's state is cleared at SessionStart, delivered results go with it and undelivered ones stay, so the continuing session still collects them. `$XDG_RUNTIME_DIR` does not outlive the login; the `/tmp` fallback can, so at each SessionStart the engine also removes every `deferred/` directory under the sessions root whose newest file is more than 7 days old. When that sweep removes a result that was never delivered, the sweeping session's next collection emits one line naming the way, the count and the age: `<way>: <n> deferred results swept undelivered after <age>`.

**Delivery.** The collector runs in the `ways hook` prompt lane (UserPromptSubmit) and post_tool lane (PostToolUse), whichever comes first after the run starts. For each undelivered run of the current agent it computes the relevance budget below and injects one of:

- the result, under a header such as `[deferred: delivery/review/requests · fired by session-start · ran 40 s ago]`, when the run finished within budget;
- `<way> macro failed (exit <n>) after <s>; its output is at <path>`, when it failed within budget;
- `<way> macro did not return within <time>/<turns> turns, which is abnormal; its output is at <path>. Investigate if it bears on the current work.`, when the budget is spent, whether the run finished, is still running, or was killed at its bound. For a run still in flight the path is where its output will land. Investigating means reading that file and the `macro_timeout` or `deferred_budget_missed` record, and judging whether the bound or the macro is at fault.

A spent budget is logged as a `deferred_budget_missed` event naming the way, the bound, and whether the run had finished, beside `macro_timeout`, so `ways` tooling can count how often it happens. Each run delivers once and is then marked delivered. A result that a refire of the same way renders inline is marked delivered there, so it does not arrive twice. A subagent's deferred result is collected by that subagent's PostToolUse and goes to it alone; a result its subagent never collects stays as that subagent's file and is not handed to the parent.

**Relevance budget.** A result is relevant while

```
age / max_age + turns / max_turns ≤ 1
```

where `age` is wall time since the macro fired and `turns` is operator prompts since the fire. The sum spends one budget on both axes, so a result is dropped for a long idle, for a burst of prompts, or for a moderate amount of each, and the check is one number to test. Defaults: `max_age` 10 minutes, `max_turns` 3. A result collected 4 minutes and one prompt after its fire scores 0.4 + 0.33 and is delivered. A subagent has no operator prompts, so its budget is time alone. Both maxima are ways settings, `deferred_macro.max_age` and `deferred_macro.max_turns`. A per-way override is left out until a way needs one.

**Amendment to ADR-004.** ADR-004's Execution Model section says a macro runs once per session with its output cached with the way marker, and that no timeout or output limit is enforced (ADR-004:104-106). Neither holds today: a macro runs at each fire under the 3 s deadline. This record amends that section: a macro runs synchronously under `MACRO_DEADLINE` unless its way declares `macro_defer:`, in which case it runs as above.

### 2. PR awareness

A way under `softwaredev/delivery/review/` with `trigger: session-start`, `macro: prepend` and `macro_defer: 10m` gives the session a line of counts. It orients the session; it is not a task list. The shape, wording flexible:

```
Review requests: 75 open · 0 for this project · 10 Dependabot · 4 via teams. Open pile: 3 (1 pending here, 2 deferred). /reviews lists them.
```

- **open:** every open PR whose review is requested of the operator directly, deduplicated by repo#number, from `gh search prs "user-review-requested:@me" --state=open --limit 200`.
- **via teams:** requests made only to a team the operator belongs to, counted apart and listed in `/reviews`. They are not counted as needing the operator's response.
- **for this project:** direct requests in this project's repositories, people only, with `(+N Dependabot)` when the project has Dependabot PRs.
- **Dependabot:** PRs authored by `dependabot[bot]` or `app/dependabot` anywhere. Other bots stay in the open count for now.
- **open pile:** what the session-start report below holds, the pending-here items and the deferred items still pending, with the two counts shown apart.
- **pending here:** the items that need the operator's decision in this project's repositories: review requests addressed to the operator directly, and unresolved threads and change requests from others on the operator's PRs.

This project's repositories are the top repository and each git submodule, resolved with `gh repo view` inside it. A submodule counts only where the operator has push access to it or takes part in the PR: authored it, was asked to review it, or commented on it. A vendored upstream submodule with a large PR queue therefore adds nothing to the open pile.

Beside the line, the same deferred macro reports, as a summary line or a listed block by the pressure tiers in section 4, the pending-here items plus every item that carries a deferral marker and is still open, whatever its kind. An item raised with a PR in hand and deferred, such as a CODEOWNER not yet asked, therefore comes back too. Each marked item is checked against GitHub by its id, and one no longer pending is dropped with its marker. A deferred "CODEOWNER not yet asked" item stays pending while that person has neither been requested to review nor reviewed or commented on the PR (section 3, signal 4); once they do any of these, it drops with its marker. Deferred items show `deferred <date>`. The session-start report computes no touched paths: others' PRs on the same paths, and CODEOWNERS not yet asked, become pending items only once a PR is in hand (section 3), and return at session start only through a deferral marker. The report is a GitHub query like the counts, so it arrives through the deferred macro and is subject to the relevance budget like any other result. Review requests in other repositories stay in the counts and in `/reviews`, and do not join this project's pile.

The macro keeps its own cross-session cache of the search under `$XDG_CACHE_HOME/agent-ways/` (`~/.cache/agent-ways/` when the variable is unset), so sessions started together make one query.

The `/reviews` skill is the drill-down: it lists requests grouped by repository, this project first, and opens a chosen PR in reviewer mode. Mirroring review requests or unresolved threads into the task list as ADR-180 mirrors issues is an option, separate from this line and not decided here.

When a session works a PR, the review way's deferred macro reports what changed since the operator last looked: `#32: head moved bd862fd→3ad6dda since your last review`, with the compare range, and new comments and reviews since then. The last-reviewed head is the `commit_id` of the operator's latest review on the PR, so this needs no local ledger.

### 3. One check of who else is involved

One script, `hooks/ways/softwaredev/delivery/github/gh-involved`, replaces the six solo/team rules. For a repository, a PR, or a set of paths it lists the people other than the operator with a stake, the signal that names each, and their pending items. The corpus stops asking "solo or team"; the question is who else is involved and what they are waiting on.

Signals that name a person:

1. CODEOWNERS (root, `.github/` or `docs/`) assigns them a touched path.
2. The PR requests their review.
3. They authored the PR.
4. They submitted a review on the PR, left a review comment or thread on it, or commented on its conversation. GitHub drops a reviewer from the requested reviewers once they submit, so this signal keeps someone who requested changes in view, and an ordinary comment such as "please hold this" counts as well. Bots do not count.
5. They authored a PR in the repository in the last 90 days.
6. They committed in the last 90 days, on any branch, deduplicated by GitHub login where known and by email otherwise, bots excluded.

Pending items are other people's open asks that bear on the work. The first two are the session-start pending-here items (section 2). The last two need a PR in hand and use that PR's touched paths:

- review requests addressed to the operator directly;
- unresolved threads and change requests from others on the operator's PRs;
- others' open PRs touching the same paths as the PR in hand;
- CODEOWNERS of the PR's touched paths who have not been asked to review.

The check never gates. Its output adds no human gate, does not refuse `/merge full-sail`, and does not make the agent wait. What holds a merge stays where `delivery/merge` puts it under ADR-604: the repository's own enforcement (rulesets, branch protection, required reviews), which holds a merge unless it is overridden under section 5's rules, and whether the work sets direction. The merge skill drops "team repos always gate" and full-sail's solo/pair limit; full-sail stays bounded by blast radius, direction and enforcement. The github way uses the check for PR description weight and reviewer suggestions, the code-reviewer agent for whose PR it is reviewing, and `project-init` for what to scaffold. The repository-level counts move into the github way's deferred macro. `docs/hooks-and-ways/macros.md` describes the check in place of its current rule.

The operator's login comes from `gh config get -h github.com user`, which reads local config, with `gh api user` as the fallback. Contributor queries use `--paginate`.

The check fails closed. When CODEOWNERS, the PR's metadata or `gh` authentication cannot be read, an unauthenticated `gh` included, it reports that it could not tell, and the PR is treated as scenario B (section 4): the agent asks before any force. This is ADR-604 rule 3, unreadable means strict.

### 4. The PR feedback way

A new way, `softwaredev/delivery/review/`, sits beside `delivery/merge`, not under it: reviewing someone else's PR never merges, and a PR with other reviewers runs across sessions before merging is in view. `delivery/merge` links to it for the step between review and landing.

The PR is the loop's ledger in both scenarios below. Threads are findings, an unresolved thread is an open one, and a session starting cold reads open findings from GitHub without parsing prose. The way covers inline comments, thread replies, resolving threads, reading a returned review, re-requesting review, and waiting on CI with the gh-monitor skill's `ci` mode in the merge flow.

#### A. Single contributor that stays honest

Scenario A is decided per PR. It holds when section 3's signals 1 to 4 find no one else: no CODEOWNER of a touched path, no requested reviewer, no other author, and no other person who has reviewed or commented on it. Bots do not count. A PR the operator wrote alone that touches a path someone else owns is scenario B, and so is a PR the check could not read. The agent's review subagent supplies the findings.

- **Findings.** The review posts as a formal review with event `COMMENT`, which GitHub allows on one's own PR, with one inline comment per finding. Each finding is a thread.
- **Sign-off per finding: a resolved thread.** Each thread closes with a reply carrying its evidence, as the evaluation way reports it (the scenario replayed, the command, red before the fix and green after), or with the decline or deferral and its reason, and is then resolved. A deferred finding becomes an issue naming the condition that reopens it, the residuals rule of `delivery/issues`.
- **Sign-off for the PR: a commit status.** The agent sets `review/agent` on the head commit with `gh api repos/{owner}/{repo}/statuses/{sha}`, which needs push access to the repository; without it the agent reports the status in its review summary instead:
  - `pending` while threads are open, with the open count in the description;
  - `failure` while a finding the review marked blocking is unresolved;
  - `success` once every thread is resolved or decided, with `target_url` linking to the review;
  - `pending` again when the head moves past the reviewed commit, with an offer to review that range alone.
- **Honesty.** The status is the author's own claim and says so, for example "agent-reviewed; findings settled". No wording suggests independent approval (delivery/groundwork/claims). The operator chooses whether branch protection requires the status as a check.
- **Merge.** The agent merges once no unresolved thread lacks a decision. Every finding needs a decision, not a fix. This holds the agent to its own review; section 3's rule that nothing gates concerns other people's stake.

#### B. True collaboration with others

Someone other than the operator has a stake in the PR: they own a touched path, were asked to review or have reviewed or commented on it, or authored it. The pending items, decision markers and tiers below belong to this scenario, with two modes:

| Mode | Findings from | Output |
|---|---|---|
| Author, others reviewing | other people's review threads, across sessions | one fix per thread; a thread reply with the evidence (command, run count, whether the failure reproduced before the fix, as code/testing/evaluation reports it); the thread resolved through `resolveReviewThread`; review re-requested |
| Reviewer | the diff since the operator's last-reviewed head | a PENDING review with inline comments, which the operator edits and submits |

Formal approvals come from other people. Where a repository requires approvals and the agent's review should count toward them, a separate reviewer identity, a GitHub App or a machine user, can file formal reviews. It is optional: it costs setup per repository or organization and possibly a paid seat.

**Other people's pending items come first, for a decision.** When the agent takes up a PR, and again before it merges, it puts the pending items from section 3 in front of the operator ahead of its own work and asks for a decision on each:

| Decision | Effect |
|---|---|
| Act | The agent does it now. The item closes when the action lands on GitHub: a review submitted, a thread answered or resolved, a PR merged or closed. An action that did not land leaves the item open. |
| Decline | Closes the item. Declining a PR, or declining to take part, is as complete an answer as acting. |
| Hand off | Closes the item for the agent, with the person or agent it went to. |
| Defer | Keeps it open, out of the current session only. |

No answer counts as defer. The agent records the decision in one line in its report. Work proceeds whatever the answer. In a run with no operator present, such as `/goal`, every item stays undecided, and the agent proceeds and lists the items in its report. There is no limit on how many times an item can be deferred.

The operator may make a decline visible on GitHub, by removing themselves as a requested reviewer where they have the access, or by a short reply. Both are optional and outward-facing, so the agent does them only when the operator chooses.

**Open items return every session.** An open item is not asked about again in the session that deferred it or left it unanswered, at a PR's take-up or before a merge. At the next session start every pending-here item, and every deferred item GitHub still shows as pending, is reported again (section 2), and a deferred one carries the mark and the date of its deferral.

**Decision markers.** A decision outlives its session, so it is not kept in the per-session temporary area. The pending items themselves are re-derived from GitHub at each session start, so only a marker is stored: its kind (`deferred`, `declined` or `handed-off`), the item's id (a review request, thread or PR node id, with the person's login for a CODEOWNER not yet asked) with the time the item was raised, the date, the hand-off target where there is one, and an optional note, one line each in `$XDG_STATE_HOME/agent-ways/decisions/<owner>/<repo>.jsonl` (`~/.local/state/agent-ways/...` when the variable is unset). A `declined` or `handed-off` marker keeps its item out of later listings; a `deferred` marker only supplies the mark. Act needs no marker, since GitHub records the action. An item raised again after a decline, such as a re-requested review, has a new raised time and is a new item. GitHub stays the source of what is pending, and a PR reply is not the store: it is public, it exists only when the operator chose one, and some items, such as a CODEOWNER not yet asked to review, have no thread to reply in. A marker whose item is no longer pending on GitHub is removed when the listing is next built.

**Pressure scales with the open pile.** The open pile is what the session-start report holds (section 2): the pending-here items and the deferred items still pending, none with a closing decision. Its prominence follows two measures, the number of open items and the age of the oldest, taken from when GitHub raised it:

| Tier | When | What the operator sees |
|---|---|---|
| Summary | at most `pending.list_count` items (default 3), and the oldest under `pending.list_age` (default 7 days) | one summary line at session start giving the open-pile count |
| Listed | more items, or an older oldest | a listed block at session start, asked before other work is taken up |
| Recurring | more than `pending.recur_count` items (default 10), or the oldest past `pending.recur_age` (default 30 days) | the listed block at session start, and during the session one line giving the count and the oldest's age, at the review way's refire cadence and on the next prompt after a `gh pr` command, until the open pile drops below this tier |
| Clearing | more than `pending.clear_count` items (default 25), or the oldest past `pending.clear_age` (default 60 days) | everything in the recurring tier, the list grouped for clearing, and clearing guidance |

Every tier asks for decisions and none blocks work. "Asked before other work is taken up" means the question comes first; when the operator moves on without answering, that is no answer, and the work goes ahead. The six thresholds are ways settings with the defaults above.

**Clearing.** In the clearing tier the macro's output carries guidance, shipped beside the macro and disclosed only while the tier holds. It tells the agent to put clearing the open pile to the operator as the first decision of the session: decline or dismiss what is stale or will not be done, and respond to the rest. It asks for a decision and holds no work. The list is grouped by repository, author and age band, and the operator can answer a group in one line, such as "decline everything older than 30 days in repo X" or "hand off author Y's PRs to Z". The agent writes the normal marker for each item in the group, so an open pile of a hundred items clears in a few answers and each item's decision stays recorded.

The defaults are a guess: the operator does not know the right number. Past about 25 items an item-by-item list stops being answerable in one sitting, and an item open for two months has usually been passed over more than once. Each change of tier, up or down, is logged to the ways event log as `pending_tier_entered`, naming the tier, the open count, the oldest item's age and the project, so real use can tune the thresholds.

Posting under the operator's account is outward-facing. In reviewer mode nothing posts under the operator's name until they submit: the review is created without `event`, and where the operator already has a pending review on the PR the way adds threads to it, since GitHub keeps one pending review per user per PR. In author mode, thread replies and resolutions follow the repository's norms and the operator's standing authorization.

### 5. Forcing a merge past enforcement

A merge held by branch protection, a ruleset, a required check or a required approval can be forced through with `gh pr merge --admin` or a ruleset bypass only where the operator's account can bypass that rule. `--admin` passes branch protection for an admin unless the protection enforces for administrators ("Do not allow bypassing the above settings"), and a ruleset is bypassed only by actors on its bypass list. When `--admin` or a bypass is refused, the agent reports the refusal and the rule that blocked it, and proposes the configuration change below; it never retries another way. Where one operator drives most of a repository's work, forcing past a rule to finish work is routine, so a forced merge costs at most one question and a one-line record.

The authorization follows the model the operator compared it to, Claude Code's auto-mode classifier: an action is judged against what the operator has authorized, not asked about one at a time. Scenario A is such an authorization, and in scenario B a convention set in the session is. Either lasts for the session. Nothing on the agent's side makes an exception standing: a block that keeps recurring is settled in the repository's configuration, where everyone can see it.

**Scenario A.** The agent may force a merge on its own when the PR is in scenario A (section 4): no other CODEOWNER of a touched path, requested reviewer, author, reviewer or commenter, and the check could read all of them. It forces only approval-type blocks, typically a required approval that only a second account could give, and does not ask. A failing required check is never forced on its own: the agent surfaces it, and the operator may choose to override it. Honesty rests on the trace: the PR comment, the `review/agent` status and the `merge_override` event.

**Scenario B.** The agent forces only under a convention the operator has established.

- **Establishing one.** The operator states it, or the agent proposes it from an observed pattern and the operator accepts, for example "nobody ever seems to reply to the PRs, we can just push them through, ok?" answered yes. A proposal states its evidence, such as reviews requested on N PRs over M weeks with no response from the people asked.
- **Scope.** A convention holds for the session. A later session needs it set again, or the repository changed so the rule no longer blocks.
- **Pending items first.** Another person's open request on the PR is surfaced for a decision (section 4) before any force. Forcing settles the merge and leaves their item open.
- **Without a convention.** When a rule blocks a merge, the agent asks once: the block and the rule, that the operator can override it, what the override skips, and whose review it skips. The answer may be a single override or a convention.

**Recorded every time.** Each forced merge leaves a short comment on the PR naming the rule bypassed and the reason, quoting the convention where one applied, and naming the people whose review was skipped. It is logged as a `merge_override` event with the repository, the rule, the reason, and what authorized it: scenario A, a session convention, or a single answer.

**Repeated friction becomes a repository change.** When the same rule in the same repository has been overridden `merge.override_suggest_after` times (default 3), counted from the `merge_override` events, the agent proposes changing the repository's configuration so the rule stops blocking:

- drop or relax the required approval;
- replace it with a required `review/agent` status check;
- add the operator to the ruleset's bypass list;
- narrow the rule to the paths where other people do review.

The proposal is made once, with its evidence: the overrides and their reasons. In scenario B it names the other people the change affects. Changing a repository's settings is an outward act, so the agent applies it only when the operator says yes, with the operator's rights (`gh api` on the repository's rulesets or branch protection), and records what changed in a PR comment or an issue so collaborators see it. The repository's rules then match how it is worked, and no hidden exception lives on the agent's side. This applies ADR-604 rule 6, which names needlessly strict enforcement as a concern to raise with the change that would fix it.

A rule that keeps blocking is a misfit between the repository's configuration and how the repository is worked. An agent-side record of the exception, whether a memory, a skill or a stored grant, leaves the misfit in place, hides it from collaborators, and has to be carried by every later session. Changing the configuration fixes the cause where everyone can see it. The `delivery/groundwork/permission` way makes a similar point about boundaries another team owns, where a wrapper around the broken step hides the boundary and adds something else to maintain; it is an analogy here, not the rule this rests on.

**Amendment to ADR-604 rule 2.** Rule 2, and `delivery/merge` after it, says a way may add rigor on top of enforcement and never subtracts from it: no skipped required check, no bypass flag, no suggestion to disable a hook or rule to get past it. ADR-604 now carries rule 2 under its own note heading, which this record amends. The change:

- A recorded override may subtract from enforcement for one merge, made by the operator, or by the agent in scenario A or within a convention the operator set in the session. Outside those, the agent still never subtracts from enforcement on its own judgement.
- The clause against suggesting that a rule be disabled is lifted for the configuration proposal above, in both of its cases: after repeated overrides of one rule, and at once when `--admin` or a bypass is refused, since no other route remains. A lasting change to what the repository enforces is made in the repository's configuration, with the operator's yes, and is visible there.

Rules 1 and 3 to 8 of ADR-604 stay in force.

### 6. Requirements on ADR-403

A later step may move PR awareness to an ADR-403 command sensor, which would wake an idle session on a new request. That needs what ADR-403 does not yet specify, recorded here as requirements on it:

- **Delivery acknowledgement.** A script that advances its own marker must learn whether attend delivered the line; today refractory gating, the lapsed-cache reset and low-priority suppression can drop it after the marker moved.
- **Per-repository fan-out,** so one sensor spec covers the top repository and each submodule.
- **Sensors in mod hosts.** The attend mod runs no sensors, so a sensor-based path reaches only sessions under `attend run` or Monitor.

## Consequences

### Positive

- Any way can carry context that takes longer than 3 s to fetch, with no per-feature cache-and-hook pipeline. The github way's repository-health and contributor calls can move off the 3 s path the same way.
- A deferred result that arrives late, fails or overruns is announced once, with its file. A result removed undelivered by the 7-day sweep is announced in one line by the session that swept it. Nothing a macro produced is dropped without a line, except what is lost when the sessions root itself is cleared (logout for `$XDG_RUNTIME_DIR`, reboot for the `/tmp` fallback), where no process remains to report it.
- Deferred macros run in the ways hooks, so they work in attend-mod sessions, which run no attend sensors.
- One check gives the github way, `/merge`, the code-reviewer agent and `project-init` the same view of who else is involved, and a CODEOWNERS entry or a requested reviewer is seen.
- Nothing the check finds holds work. In a repository where one operator drives most of the work while others are new, casual or observing, the operator is not left waiting on them, and their open asks still get an answer.
- No exception lives in hidden agent-side state. A rule that keeps blocking is either overridden visibly for a session or changed in the repository's settings, where collaborators see it.
- The open pile cannot grow silently: it returns every session and gets louder as it grows and ages. Declining is a cheap way out of it, and grouped answers keep a large open pile clearable.
- A PR's state survives session boundaries because it lives on GitHub. Review findings from other people are closed with evidence and an explicit resolve.
- Reviewer-mode output cannot reach other people before the operator reads it.
- A cold session reads a PR's open findings from GitHub's threads without parsing prose, and a scenario-A PR shows its review state as a status beside its CI checks, including whether commits landed after the review.

### Negative

- A result can arrive next to an unrelated prompt; the header and the budget limit the confusion without removing it.
- In a `claude -p` run with no tool calls after the fire there is no later hook, and that run never collects the result. It stays in its file until the 7-day sweep, which reports it in another session's line.
- The engine gains detached processes, a collector on UserPromptSubmit and PostToolUse, and per-agent result files to test and keep from leaking.
- An idle session learns of a new review request at the operator's next prompt, not when it appears.
- The involvement check reads CODEOWNERS and PR metadata, so it costs more than a `git log` count and needs network for its per-PR signals. Offline or unauthenticated, it fails closed, and a PR that would be scenario A is asked about like scenario B.
- Where the repository enforces no review, the agent can merge a change to a path another person owns. The operator's recorded decision is the only trace of it.
- Each surfaced item asks the operator for a decision, which adds a question to a session that would otherwise run straight through. A deferred item asks again at every session start, one more question per session for as long as it stays pending.
- Decision markers are a small piece of local state outside GitHub. They do not travel to the operator's other machines, where a deferred item shows without its mark and a declined or handed-off item is listed again.
- The recurring tier is deliberate annoyance. It interrupts sessions about unrelated work until the open pile shrinks, and the thresholds that set it are a guess until use tunes them.
- The clearing threshold, 25 items or 60 days, is a stated guess as well. Set too low, it turns an ordinary week into a clearing session; set too high, the list is already unworkable when the guidance arrives.
- The `review/agent` status is self-asserted. An operator, or a reader of the repository, could take it for independent approval despite its wording.
- A forced merge weakens enforcement that other people may rely on. In scenario A and under a convention no question precedes it, so the trace is the only check.
- In scenario B a forced merge can skip another person's required review. The agent names that person in the PR comment, which may be the only record they get.
- A session convention can outlive the evidence it rested on within that session. It lapses when the session ends, and the agent re-checks the pattern before proposing to set it again or to change the repository.
- A configuration change to the repository affects everyone who works in it, not only the operator. In scenario B the proposal names who else is affected.
- An agent review makes more API writes: one comment per finding, a reply and a resolve per thread, and a status per change of state.
- A grouped answer applies one decision to items the operator has not each read. A group decline can close an item that deserved a reply.
- A first fire has no cache, so the way's first rendering carries no macro output.

### Neutral

- Follow-ups outside this decision:
  - `tools/attend/src/sensors/script.rs` reads a script's stdout only after it exits (`wait_with_output`, line 110). Output past the pipe buffer stalls the script until the 10 s kill and is lost, and a background child holding stdout can hang the tick loop.
  - The comment in `tools/attend/examples/xdg-downloads.sh:83-90` says a single 2.0 line fires on its own; a slot under 3.0 is emitted as low priority and does not wake Monitor.
  - The github way's macro does not check `gh auth status`, so an unauthenticated session reports "Not a GitHub repository".
  - Stale text to correct: ADR-004:104-106, by an appended note pointing here; `hooks/ways/macro.sh:7` ("no network"), which the github macro already breaks; the solo/team rule at `docs/hooks-and-ways/macros.md:54-58`; the `+name` form and working directory in the sensor authoring guide; the example count in the configuration guide; `scripts/gh-monitor-ci-watch.sh:15-16`, which cites commit 78028f3, absent from this repository's history.
- The per-feature PR pipeline planned before this decision is not built.

## Alternatives Considered

- **A per-feature cache-and-hook pipeline for PR data, like gh-tasks: run synchronously in hooks, gated by a TTL.** Works for this feature. Rejected: every later slow query would have to copy the pipeline, and the fetch still runs on a hook's path whenever the TTL lapses.
- **A third `macro:` value, `deferred`.** One field. Rejected: it merges position and timing, a deferred way still needs a position for its cached result, and an older engine would run the macro synchronously and drop its output because the value matches neither position.
- **Raise MACRO_DEADLINE.** Rejected by ADR-137: it moves the cliff, and every way's hook waits on it.
- **Deliver a deferred result only when its way is relevant again.** Keeps results beside related context. Rejected: the result may never be relevant again in the session, and the operator compared delivery to a message queued mid-turn, which arrives next and still makes sense because its turn was near.
- **Staleness by TTL alone.** Rejected for the operator's time-against-turns budget: a burst of prompts in two minutes moves the conversation further than ten idle minutes.
- **`max(age/max_age, turns/max_turns) ≤ 1` instead of the sum.** Two independent caps. Rejected: a result at 90% of both caps is delivered, though both kinds of drift have nearly run out.
- **An ADR-403 command sensor now.** Wakes idle sessions. Rejected for now: unbuilt, it can drop a line after the script moved its marker, it does not run in mod hosts, and ADR-137 retired GitHub watching from sensors. Kept as a later step with the requirements in section 6.
- **A cap on deferrals, or dropping the defer option after a number of sessions.** Forces a resolution. Rejected: deferring and not answering stay valid answers indefinitely, and the rising prominence of a growing open pile does the forcing without removing a choice.
- **Fix each of the six solo/team rules in place.** Smaller diffs. Rejected: six copies drift again, as they already did.
- **Gate on the check: team paths stop for review and `/merge full-sail` is refused for them.** Honors every named owner. Rejected by the operator: in most repositories one person drives the work while others are new, casual or observing, and a false signal leaves the operator waiting on work that never finishes. The repository's own enforcement already holds a merge where its owners want one held.
- **Reviewer mode posting on another person's PR with `event: COMMENT`.** Saves the operator a click. Rejected: it publishes under the operator's name before they have read it.
- **One top-level review comment with numbered findings.** The current practice, one write. Rejected: a finding's outcome can be learned only by matching numbers across prose, nothing marks the PR as reviewed, and nothing shows that later commits went unreviewed.
- **Record the exception agent-side: a memory, a skill, or a stored grant such as `merge.override_grants`.** Saves the repeated question without touching the repository. Rejected: in the operator's words, it "doesn't really solve the root cause". It leaves the misfit between the rule and how the repository is worked in place, hides the exception from collaborators, and has to be carried by every later session. The repository's configuration is where the cause is fixed.
- **The agent never forces a merge; the operator decides each override.** Keeps every subtraction from enforcement in the operator's hands. Rejected by the operator: on a PR no one else has a stake in it adds a question to routine work, and in a shared one an established convention already carries the authorization.
- **A machine-user reviewer for every repository.** Gives a scenario-A PR a real approval that branch protection counts. Not the default: each repository or organization needs the identity set up, possibly a paid seat and a credential to keep, and it presents the agent's own review as an independent approval. Kept as an option under scenario B.
- **The GitHub MCP server as the client.** Rejected for the reasons in ADR-180: it duplicates `gh` and adds tool definitions to every session.
