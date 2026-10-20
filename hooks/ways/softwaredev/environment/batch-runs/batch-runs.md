---
description: Running batches of paid generation or API jobs, such as image, video, or model runs. Expand the user's shorthand into the exact job spec, submit each job on its own in parallel, keep blind inputs unread when the run is blind, report each job's path, status, and cost, rerun on "try again" without lecturing, and classify failures by cause.
vocabulary: generation gen batch jobs prompt-file image video resolution duration seed cost credits billing moderation blocked retry rerun try-again again outputs download topped-up reupped while-we-wait blind without-inspecting tally running-total per-job
pattern: prompt[\w-]*\.txt?\b|\b(run|gen|each|again|do)\b.{0,20}\b\d+x\b|\brun another \d+\b|that \^|\breupped\b|\bjust topped up\b|topped up (the )?credits|credits? (ran out|expired)|double the volume|without inspecting
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Batch Runs

## Spec

Launch exactly the spec: prompt file, inputs, count, and settings such as resolution and duration. Expand shorthand literally, without asking:

| Shorthand | Meaning |
|---|---|
| `2a-d` | items 2a, 2b, 2c, 2d |
| `3x each, 1 short and 2 long` | count and length per item |
| `that ^` | the last complete job spec |
| `double the volume` | twice the previous count |
| `4x instead of 10x` | replace the count |

Pass the project's stated standards, such as a required resolution, explicitly. Never rely on a script default. A value the user states for this run wins over the project standard.

The user's run request authorizes its spend, so do not re-confirm it. A paid run you propose yourself still needs the user's go.

## Launch

- Submit each job on its own in the background, all in parallel. Never run fewer than asked. Do not wrap the set in a batch-submission script.
- Use plain shell commands in the main session, staggered to stay under rate limits. Do not hand the jobs to subagents.
- Watch the jobs with a monitor that polls every few seconds with short timeouts. Download each result as it lands, then remove the watcher.

## Blind runs

A run is blind when the project instructions forbid reading inputs, or when the user says "without inspecting".

- Read the prompt file from disk on every run. The user edits it between runs.
- Never read, print, view, or describe the prompts, inputs, or outputs. Pipe them straight to the job. Diagnose failures from metadata only.
- Delete the request files and job logs the project says to clear.

Outside blind runs, checking an output is fine. For private media, say who can see the prompts and outputs, and put auth on any proxy endpoint you stand up.

## Report

Report each job in the same format every run: output path and size, status or failure reason, seed, duration, and cost. Then give the batch total, the running total, and a tally such as `7 of 8 succeeded, $15 total`. For many jobs, use a compact table grouped by prompt and input. Correct any earlier miscount.

## Reruns

- "Try again", "again", and "run that ^ again but 5x" mean rerun the exact previous set with only the named change, right away.
- Re-read the prompt file. Do named pre-steps, such as "delete all logs", first.
- Do not warn that it will fail unchanged. The prompt may have changed, and moderation is not deterministic. Add at most one short pattern note after the results.
- "While we wait" work launches alongside running batches. Label and report each batch on its own.
- A mid-run correction ("I meant prompt2.txt") launches the corrected jobs now and labels the mistaken batch.

## Failures

Classify each failure by job ID and say whether anything was charged.

| Class | Typical sign | Move |
|---|---|---|
| Moderation | often a 400 | Report it. Rerun only when asked. |
| Billing | often a 403 | Stop retrying. Name the console to top up, then wait. |
| Transient | timeout, 5xx | Retry it yourself. |
| Wrong model or endpoint | often a 404 | Fix the call and say so. |

When the user says the credits are "reupped" or "topped up", rerun exactly the set that billing blocked.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It will fail the same way again." | The user may have edited the prompt. Rerun it. |
| "I'll peek at one output to check it." | In a blind run, read metadata only. |
| "I'll submit fewer to save credits." | Run the count the user asked for. |

## See Also

- environment/bounded-execution(softwaredev) — background forms and liveness signals for each job
- environment/recovery(softwaredev) — classify a failure before retrying
