---
contract: adr/v1
kind: evidence
capability: matching
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-188
  - ADR-196
  - ADR-707
  - ADR-708
---

# ADR-709: A spike of the prompt-lane handoff to a mod judge, covering the prompt.submit bracket, session identity and model latency

## What this record is

A spike run on 2026-10-22 on the maintainer's machine, before the build of ADR-708's prompt-lane handoff. It asks whether a ways mod can answer a job that a UserPromptSubmit settings hook writes and then waits on, inside the same prompt, and how long `$.model.complete` takes at the judge's prompt sizes. ADR-708 cites it. It decides nothing.

## Setup

Eleven `claude -p` runs on Claude Code 2.1.308, each with `--plugin-dir <spike mod>`, `--settings <spike settings>` and `--setting-sources project,local`, so the operator's user settings were not loaded. The settings file registers one UserPromptSubmit command hook, a shell script standing in for the ways scan: it writes a job file named by its stdin `session_id` and waits up to 8000 ms for an answer file. The mod hooks `prompt.submit`, whose `next(e)` runs that settings hook. Two shell scripts stand in for `ways judge take` (wait for the job file) and `ways judge answer` (write the answer file). Each run logs millisecond timestamps from the driver, the mod and the scripts.

The runs: concurrent (twice), sequential (the positive control), no job, model (twice), three latency batches, and a `-p --resume` of the first concurrent run's session.

## Findings

1. **A process started beside `next(e)` progresses while the hook blocks inside it.** The mod starts `next(e)` and, without awaiting it, runs the take script through `$.process.run`. In run 171859 the hook wrote its job, the take script found it, the mod wrote the answer, and the hook reported the answer 22 ms after writing its job. The `-p --resume` run repeated it at 23 ms. In the positive control the mod awaited `next(e)` before starting the take script, and the hook timed out at 8000 ms with no answer.
2. **`$.process.spawn` can be stopped when `next(e)` settles.** In the no-job run the hook exited without writing a job. The mod had started the take script through `$.process.spawn`; when `next(e)` settled it called the stream's `return()`, which resolved, and the take script received SIGTERM 27 ms after the settle. `$.process.run` takes a `timeoutMs` and no abort.
3. **The mod's session id is the hook's.** `$.session.id()` equalled the `session_id` on the hook's stdin in every run, including the `-p --resume` run, which reported the resumed session's id.
4. **`prompt.submit` fires under `claude -p --plugin-dir`.** It reported origin `sdk` and a budget of 10000 ms. `next.budget.remainingMs` stayed between 9998 and 10000 at the handler's return, after an 8 s blocked `next(e)` in the control and after a `$.model.complete` call of about 2 s in the model runs. Neither the time spent inside `next(e)` nor a `$` call was charged to the mod's budget.
5. **`$.model.complete` latency at judge sizes.** Each call sent one of ADR-707's arm E judge prompts with 1 to 8 candidates and the strict-JSON instruction, one call at a time. Every call answered and every reply parsed.

   | model as sent | resolved to | n | min | median | p90 | max | over 2000 ms |
   |---|---|---|---|---|---|---|---|
   | `haiku` | claude-haiku-5-5 | 50 | 932 | 1938 | 2703 | 3327 | 23 of 50 |
   | `claude-haiku-4-5` | claude-haiku-4-5 | 20 | 670 | 1162 | 1797 | 2258 | 2 of 20 |

   Times are in ms. Under Claude Code the alias `haiku` resolved to claude-haiku-5-5, which thinks: its replies carried 98 to 638 output tokens (median 342) against 30 to 269 (median 142) for claude-haiku-4-5. Latency tracks output tokens; for the alias it fits about 664 ms plus 3.9 ms per output token (r = 0.90). A further batch of 20 with the alias at effort `low` stayed in the same range.

## Not tested

- `/clear` and an in-session `/resume`. Both change the session id with no `session.start`, as the attend mod's comment in plugins/attend/hooks/register.tsx records; this spike did not exercise either.
- An interactive session. Every run was `claude -p`.
- Running beside the operator's full user settings, with the ways hooks and the attend mod loaded.
- The real `ways judge` verbs and the real scan. The stand-ins wrote and read files only.

## Limits

One machine, one Claude Code build, one afternoon. The latency batches are sequential calls from one session and say nothing about concurrent judging. The 22 ms handoff is the stand-in scripts' cost and excludes the model call, which dominates.

The runs, logs and scripts are in the session scratch directory `handoff-spike/`, kept outside the repository.
