# Continuance: monitoring skill clusters (start with GitHub)

A handoff for a fresh session. Goal: author **skill cluster(s) for different
monitoring types, starting with a GitHub monitoring cluster** — the right-shaped
home for the GitHub awareness that this session proved is *not* an attend sensor.

## The converged principle (don't re-derive — it cost a whole session)

- **attend sensors** = ambient, cross-agent, **local-world** observation (git,
  peers, processes). They poll on a tick under a hard 10s budget and are
  **uniform across sessions** (no per-agent enrollment).
- **External / session-specific watching** (a PR's CI, your notifications, a
  thread's comments) is **NOT a sensor**. It's a **persistent Monitor the agent
  launches** when the work is relevant — session-scoped, opt-in, bounded to the
  task — or a hook.
- attend itself **runs under Monitor**. Wrapping a GitHub poller in an attend
  sensor inverts the stack: Monitor natively does "poll endpoint → emit line →
  notification" and its tool docs already ship the `gh pr checks` and
  `gh api notifications?since=` poll-loop examples.
- Backing principle: **ADR-137 (boundedness)** — a unit of work in a cycle must
  do bounded work; unbounded/external watching belongs out of the tick.

## What to build

**Primary: a skill** (per the repo's Knowledge Way — "a way teaches behavior; a
skill gives Claude something to run"). The skill launches a *correct* persistent
Monitor for a GitHub-watching task. Optionally a thin **way** later that nudges
proactively (e.g. on `git push` / PR creation → "want CI watched?").

Note the **Skills Way scope caveat**: `skills/` here is live personal scope
(`~/.claude/skills/`) — a skill lands in every project on this machine the moment
it merges.

### GitHub cluster — v1 cases
1. **Watch a PR's CI** → persistent Monitor over `gh pr checks` (or the GraphQL
   `statusCheckRollup`), emit on pass/fail transitions, exit when terminal.
   - Reference logic for the rollup aggregation (FAILURE set, the
     `conclusion // state // "PENDING"` coalesce, terminal-first-run rule) is in
     the **retired** `gh-pr-checks.sh` — recover it:
     `git show 78028f3~1:tools/attend/examples/gh-pr-checks.sh`
2. **Watch notifications** → Monitor over `gh api notifications?since=<marker>`.
   - `tools/attend/examples/gh-notifications.sh` is the seed (the `reason`→magnitude
     table is reusable). It uses `since=` only; **upgrade to conditional requests**
     — GitHub returns `ETag` + `Last-Modified` + `X-Poll-Interval: 60`, and
     `If-None-Match` → `304 Not Modified` (verified live this session). That makes
     idle polls free.
3. (Optional) **Watch a PR/issue for new comments** → the Monitor tool docs'
   canonical example.

### Correct Monitor invocation — bake these into the skill
- Use the **Monitor tool**, not Bash; `persistent: true` for session-length watches.
- Flush every pipe stage (`grep --line-buffered`, `awk fflush()`); never `| head`.
- **Coverage:** emit on *all* terminal states (pass AND fail/cancel/timeout) — a
  monitor that only greps for success is silent through a failure, and silence
  reads as "still running."
- Remote poll intervals 30s+ (rate limits); honor `X-Poll-Interval` when present.
- Guard transient failures (`gh ... || true`) so one bad poll doesn't kill the watch.

## Empirical facts established this session (so they aren't re-litigated)

- **Projects v2 is GraphQL-only; GraphQL has no `ETag`/conditional requests** → a
  board can only be *scanned*. A 967-item board scan = **10.6s** (over the 10s
  tick budget) but only **168 KB** and **10 rate-limit points** — the wall is
  *serial pagination latency*, not data volume.
- **`projects_v2_item` webhooks are org-only**; a **user-owned** project can't
  emit them. (So the "edge-triggered" path needs an org.)
- **Card column moves generate zero notifications** (verified live: moved a card,
  `notifications?since=` returned 0).
- **Notifications API supports cheap conditional polling** (ETag → 304,
  `X-Poll-Interval: 60`). REST issue lists carry ETags too.
- **Prior art:** `gh-notify` (CLI, bash+fzf over the notifications API), `Gitify`
  (tray, polls notifications API) — *all* wrap the Notifications API; **none**
  watch board moves. We're aligned with the whole ecosystem.

## Housekeeping / related decisions

- **`gh-notifications.sh` is the same antipattern** (a notifications poll-loop in
  attend). When the notifications skill lands, decide whether to retire it too.
- **"No per-agent sensor enrollment" is a real attend gap** worth its own
  issue/ADR-note — some sensors should be session-scoped — independent of this.
- Landed this session: ADR-137 (boundedness) + gh-pr-checks retirement in **PR #146**.
  Closed: **#25** (board sensor), **#44** (gh-pr-checks). Both as wrong-shape /
  resolved-by-not-doing.

## Open design questions for the cluster

- Structure: one `gh-monitor` skill with sub-modes (ci / notifications / comments),
  or separate skills under a `monitoring/` parent?
- The user framed it as "skill **clusters** for different monitoring **types**" —
  implies a parent concept (monitoring) with GitHub as the first member, leaving
  room for others (CI systems, deploys, queues). Design for that shape.
- Whether the proactive nudge-way ships now or later.
