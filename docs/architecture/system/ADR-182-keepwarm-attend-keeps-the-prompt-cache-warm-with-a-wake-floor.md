---
status: Accepted
date: 2026-09-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-136
  - ADR-172
---

# ADR-182: Keepwarm: attend keeps the prompt cache warm with a wake floor

## Context

Claude Code's main conversation is cached on the 1-hour prompt-cache tier. Every request reads the cached prefix and refreshes its timer. A request that arrives after the hour lapses rewrites the whole prefix at the cache-write rate, which on a 200k-token session under Fable 5.1 costs about four dollars against five cents for a warm turn.

The `cache-tax` mod (karanb192/claude-code-mods) solves this with a function hook: at 50 idle minutes it sends a tool-less `$.model.fork` over the session's transcript, reads the reply's usage to confirm the cache was read rather than rewritten, and repeats until an armed window ends. It also drops one cold send with the price on screen. Function hooks are early access, gated behind an environment flag, and nothing in that surface is available to a settings hook or to attend.

Attend already wakes the session. Every line it emits to stdout becomes a Monitor notification, which Claude Code turns into a turn over the full context. That turn is an API request over the same prefix, so it reads the cache and resets the timer. A peer message, a git change, and a scheduled wakeup all do the same. The cache-tax ping and an attend notification are the same operation with different plumbing.

Attend also already reads the transcript. The context sensor (ADR-113) shells to `ways context --json`, which parses the per-message `usage` object that carries `cache_read_input_tokens` and `cache_creation_input_tokens`. The warm-or-cold verdict cache-tax reads from its fork's reply is on disk one poll after any wake.

## Decision

**A `keepwarm` sensor in attend emits one wake when the session has been idle for 50 minutes, while an armed window is open and the context is large enough to be worth it. The transcript is the clock and the scorecard. The agent's contract is one sentence: answer a keepwarm line with one word and no tools.**

1. **The clock is the transcript.** Idle time is measured from the timestamp of the last assistant line in the session transcript. That covers every waker: user turns, peer messages, drained inbox messages, cron firings, and keepwarm itself. Attend does not keep its own emission clock for this.

2. **The floor.** With a window armed, context at or above 50k tokens, and 50 minutes since the last assistant line, the sensor emits one line at medium priority. Medium is the lowest priority `emit_batch` writes to stdout, so the line wakes Monitor. The sensor rides the message lane with the peers sensor: the wake is a timed floor, so it bypasses the action-potential refractory that throttles observation noise. One wake per idle stretch. If no assistant line follows within the remaining ten minutes, the sensor disarms and records why.

3. **The verdict comes from the next usage line.** After a wake, the first new assistant line is checked: a cache read with a cache write under a tenth of the read means the cache was warm and the wake did its job. A read of zero, or a write of a tenth or more, means the cache was already gone. The sensor then stops and records the reason, so an armed window never keeps paying for rewrites.

4. **Cold writes are scored.** Any new assistant line whose cache write is at least half the previous context size, with previous context over 20k, is a paid cold write. The sensor records it and, if no window covers the next three hours, arms one. That mirrors cache-tax: after one rewrite, the session is held warm so the second is not paid today.

5. **Arming is a CLI verb.** `attend keepwarm on [WINDOW]` arms a window (default six hours), `attend keepwarm off` disarms, `attend keepwarm status` prints the cache state, context size, cold price, window remaining, and the session's cold writes. The window is stored per session under attend's own cache directory. The CLI is the contract (ADR-136): the arm file is attend-owned state.

6. **`ways context --json` grows a `usage_tail`.** The last assistant usage entries, each with timestamp, model, input, cache read, cache write, output, and tier. It also gains `--session <id>` so a sensor pinned to one session never reads a peer's transcript in the same project. The transcript parser stays in one place.

7. **The agent contract lives in the three synchronized guidance files.** The attend skill primer, the runtime disclosure, and the attend way each carry the same paragraph: a line beginning `keepwarm:` asks for one word and no tools, and it is not a prompt to investigate.

8. **Prices are a dated table.** Cache read, 1-hour cache write, and output rates per model family, from the list prices on the decision date. The table is for the status card and the recorded cost of a cold write. A model not in the table prices as unknown.

## Consequences

### Positive

- A session left open across lunch comes back to a warm cache for the price of one or two cache reads.
- No function hooks, no environment flag, no fork. The mechanism is a Monitor notification, which attend already owns.
- The verdict is stronger than the fork's. A fork must prove empirically that it shared the main cache. A Monitor wake is a turn in the main conversation, so it shares the prefix by construction.
- Every wake leaves a usage line, so the same parse scores cold writes whether or not keepwarm caused the turn.

### Negative

- A wake appends to the transcript. Each ping adds a user line and an assistant line, on the order of a hundred tokens. A six-hour window adds under a thousand.
- A model at high effort may think before answering with one word. The guidance forbids deliberation, and the output is billed whatever it says.
- On a subscription the dollars are a yardstick. How a cache read weighs against the five-hour and weekly limits is not documented. The status card prints the arithmetic and the operator decides.
- The refuse-once guard is not in this decision. Dropping a cold send needs the `UserPromptSubmit` hook to return a block decision. That is a hook change with its own ADR.

### Neutral

- Attend's chattiness now has a visible unit price: every notification is a cache read. The governor's calm-channel posture (ADR-136) already encodes that cost without naming it.
- The cache clock is an input attend could use for its own emission timing. A low-value disclosure delivered at 65 idle minutes pays the rewrite; the same one at 55 is nearly free. Holding cold-time disclosures is a governor decision left for a later ADR.
- The plugin author reports that Claude Code passes resume fields to settings hooks on `SessionStart`, including seconds since the last response and an estimated cache-write cost. This ADR does not depend on them. If they exist, the cold-resume warning belongs in the SessionStart hook.

## Alternatives Considered

- **Port cache-tax as a function-hook plugin.** Rejected. The surface is early access and off by default. Attend already has the timer, the wake, and the transcript parse.
- **A fork-style side request from attend.** Rejected. Attend has no model access, and a side request would have to prove it shared the main cache. A wake is the main cache.
- **Fold keepwarm into the context sensor.** Rejected. The context sensor rides the event lane with its refractory. A timed floor needs the message lane, and the two sensors have different disclosure shapes.
- **Arm by default.** Rejected. A ping costs money on an API key and an unknown amount of rate limit on a subscription. Arming is an operator act, with the auto-arm after a paid cold write as the one exception, because that session has already shown it comes back.
- **Attend keeps its own emission clock.** Rejected. Only the transcript sees every waker.
