---
contract: adr/v1
kind: evidence
capability: matching
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-155
  - ADR-160
  - ADR-188
  - ADR-191
  - ADR-606
  - ADR-706
---

# ADR-707: Measurements of the shell surface, covering the semantic lane's on-topic rate, alternative match surfaces, and a judge reading the described call

## What this record is

Three studies run on 2026-10-22 on one workstation's Bash calls. They ask whether the semantic lane on the Bash surface (ADR-155 §4) brings in guidance that bears on the command, what text a matcher should read instead, and whether the relevance judge can separate on-topic candidates from off-topic ones when it reads a described command. ADR-188 and ADR-706 cite it. It decides nothing.

All three use the same window: the events log and session transcripts from 2026-10-20 01:37Z to 2026-10-22 17:58Z, 8,016 Bash calls. That window is mostly way-tuning and review work in this repository, which over-represents self-reference (61% of the lane's fires came from agent-ways).

## The grading rubric

Fixed before grading. The grader sees the way's id and description, the project, the Bash call's `description` field and the command. Scores, variants and provenance are hidden.

- **ON:** the command performs, or is the direct step of, the activity the way governs, so reading the way could change this command or the next step.
- **ADJ:** the same broad area, but the guidance does not bear on this step.
- **OFF:** the way's subject is not what the command does.

Ties go to the lower grade. A read-only command is ON only when the way governs that act of inspecting.

## Study 1: the semantic lane as it runs

**Method.** Every `way_fired` event in the window on `semantic:bash:en`, joined to its Bash call through the transcript; 5,511 of 5,705 joined. A sample of 248 fires, stratified by way (two per way with 20 or more fires, one below that), graded against the rubric. A second pass re-graded a random 50 from a reshuffled sheet.

**Counts.**
- The lane made 5,705 of the window's 7,034 way fires (81%). The `commands:` regex made 668.
- Subagents made 86% of the lane's fires.
- 149 distinct ways fired; no way exceeded 2.3% of the total.

**On-topic rate.**

| grade | sample | way-weighted estimate | 95% interval | fires in the window |
|---|---|---|---|---|
| ON | 7 / 248 | 3.4% | 1.3–5.5% | about 190 |
| ADJ | 43 / 248 | 19.9% | 16.4–23.5% | about 1,100 |
| OFF | 198 / 248 | 76.7% | 73.3–80.0% | about 4,200 |

Second-pass agreement was 48 of 50 (κ 0.89), from the same grader, who had seen the items.

**The score does not separate.** Across calibrated-score bands from 0.4 to 1.0, ON plus ADJ stays between 16% and 29%. A threshold of 0.7 keeps 2 of the 7 ON fires and leaves OFF near 80%.

**Why it misfires.**
- The matched text is usually not the command. The lane embeds the assistant's prose since the last human turn, the command and its description, reduced by sentence salience. Of 767 untruncated logged surfaces, 56% contained the command's opening characters; the rest matched on prose alone.
- Subagents match the parent's prose. `lookbehind::intent` finds the session's main transcript (`tools/ways-cli/src/cmd/scan/lookbehind.rs:31`), so a subagent's `cat` is scored against what its lead was saying. Of 150 probed subagent fires, 53 surfaces opened with text found only in the parent transcript, and 1 with the subagent's own.
- Read and search commands (`cat`, `sed`, `grep`, `ls`, `git log`) made 53% of fires at 81–83% OFF.

## Study 2: alternative match surfaces

**Method.** The 248 graded fires, re-scored through the engine's own path (`reduce_for_embed`, `way-embed match` on the shipped English corpus, the calibrated `g(s)`). A fire is a fresh scan at p ≥ 0.5, or 0.4 under an ancestor fired in the same scan, with no refire gate. The Python harness matched the engine's probe on 473 of 473 scores to four decimal places, and rebuilt today's surface within 0.005 of the logged score on 220 of 248. Because the 248 were chosen by today's surface, the comparison that counts is a second set: 200 Bash calls drawn uniformly from the window, each variant's fires graded blind by subagent graders from sheets with no scores or variant labels. A third set of 120 calls on which nothing semantic fired measures what a variant would newly catch.

**Surfaces.** A is today's (narration, command and description). B is the description alone. C is the operative verbs (program, subcommand and flags per segment, arguments and paths dropped) plus the description. D is the verbs alone. E is the command with paths stripped. F is the command plus the description. G, H and I come from a segment parse (below): G is the description plus every operative segment's verb as one text; H scores each segment's verb plus the description on its own and keeps each way's maximum; I is the description, then each segment's tldr description in order, joined by its connector in words; I2 is I without the description.

| surface | fires on 200 uniform calls | ON | ON rate | ON+ADJ | gated to calls with an action segment: fires per call, ON rate |
|---|---|---|---|---|---|
| A, today, fresh scan (160 of 1,201 fires graded) | 1,201 | 0 of 160 sampled | 0% of the sample | 11% | — |
| C | 473 | 36 | 7.6% | 34% | 1.17, 13.3% |
| G | 447 | 32 | 7.2% | 34% | 1.16, 11.7% |
| H | 700 | 43 | 6.1% | 32% | 2.08, 9.2% |
| I | 636 | 32 | 5.0% | 26% | 1.84, 7.6% |
| I2 | 485 | 25 | 5.2% | 29% | 1.68, 7.2% |

ON rate is per fire, not per call: C was on topic in 36 of its 473 fires. Today's surface also fires at least 9 of C's 36 on-topic pairs, outside its graded sample, so its on-topic rate is above zero though none of its 160 sampled fires was graded ON.

On the 248, removing narration (A to F) cut OFF fires from 167 to 87, and removing paths and arguments (F to C) cut them to 12. The narration was also the only source of 5 of the 248's 7 ON fires. On the 120-call recall set, C found 52 ON fires (10.6%) that nothing semantic had fired. Of the 98 ON fires C found across the uniform and recall sets, 37 were reached by an existing `commands:` regex, 21 belonged to a way whose regex missed the command, and 40 to ways with no `commands:` field. Of C's 36 on-topic fires on the uniform calls, about three-quarters were regex-shaped, reachable by a repaired or new `commands:` regex, and about 8 were intent-shaped; beyond what regexes reach, C added about 4 intent-shaped on-topic fires per 100 calls.

**A Bash call is several tools.** A shell splitter (quotes, `;`, `&&`, `||`, pipes, `&`, newlines, subshells, command substitution, loop bodies, heredoc bodies dropped) gives each segment a role: navigation (`cd`, `export`, assignments, `mkdir -p`, `source`, plumbing such as `echo` and `sleep`), filter (a pipe consumer such as `head`, `grep`, `jq`, `awk`, `sed` without `-i`), or operative. On the 200 uniform calls:

- segments per call: mean 6.1, median 5, maximum 24;
- operative segments per call: mean 3.1, median 3;
- 122 calls span two or more classes of operative segment (71 two, 51 three or more);
- 62 of the 70 calls with an action segment (publish, version-control write, service, build, file write) also carry a read.

The engine has no shell tokenizer: the `commands:` regex runs on the raw string, so an anchored pattern such as `^gh\ ` misses `gh` after `git push &&`.

**tldr coverage.** tldr-pages `pages.en` (common, linux and osx), read as data from a local cache. A segment looks up `program-sub1-sub2`, then `program-sub1`, then `program`, following aliases, and takes the example whose template shares the most subcommand words and flags. Over all 24,079 operative segments in the window's 8,016 calls:

| source | share |
|---|---|
| subcommand page, matching example | 13.2% |
| subcommand page, summary only | 7.2% |
| program page, matching example | 35.6% |
| program page, summary only | 38.5% |
| unresolved | 5.5% |

Half the segments reach an action-level example. The misses that matter are this project's own binaries (`adr` 152 segments, `dotfiles` 111, `ways` 82, `doc` 16, `attend` 9), every `make <target>` (607 segments, which get make's generic summary) and interpreters and project scripts (2,537 segments). An extract holding each page's summary plus each example's description and template comes to 7,092 pages and 30,909 examples: 3.68 MB raw, 0.98 MB gzipped.

**tldr text did not help inside the embedding scorer.** I and I2 sat at 5% ON, below C. Example descriptions are generic ("Create and switch to a new branch") and draw ways whose aliases share that vocabulary.

## Study 3: the judge reading a described call

**What the judge sees today.** Nothing. The relevance gate (`gate::apply`) has one call site, in the prompt lane (`tools/ways-cli/src/cmd/scan/mod.rs:402` on main at 7fd6c0de). Bash-lane hits are injected unjudged, which departs from ADR-196 §1: that section applies the gate to every way the matcher selected semantically. The events log's 927 `way_judged` lines are 753 from UserPromptSubmit and 174 from PostToolUse.

**Method.** The production judge request (claude-haiku-4-5, the system prompt, instruction and forced `record_judgements` tool of `tools/ways-agent/src/net.rs`, temperature 0, pass threshold 0.3, eight candidates per request, every candidate judged in as many requests as it takes), run twice per arm; a pair passes on the two-trial mean. Grades are the blind grades from studies 1 and 2, plus 575 pairs graded blind by claude-opus-5-5. The ordered described block reads `About to run: <description>`, then one line per `;`, `&&`, newline, loop or subshell unit, in order, with navigation dropped; segments joined by `|` or `||` stay on one line with the symbol (`git log (…) | grep (…) | head (…)`), and each segment reads `program subcommand (description)`, an operative segment with its matched tldr description and a filter with its tldr summary.

The arms:
- **A:** today's candidates, judged on the prompt lane's input (one line of narration);
- **B:** today's candidates, judged on the described block;
- **C:** A's narration with B's block appended;
- **D:** surface I's candidates, judged on the described block;
- **E:** surface C's candidates, judged on the described block.

**Results on the 200 uniform calls.**

| row | injections | ON / ADJ / OFF | ON per call | OFF per call | off-topic share |
|---|---|---|---|---|---|
| today's lane as logged, after refire, unjudged | 340 | 5 / 64 / 271 | 0.025 | 1.36 | 80% |
| A | 75 | 5 / 21 / 49 | 0.025 | 0.245 | 65% |
| B | 46 | 8 / 18 / 20 | 0.040 | 0.100 | 43% |
| surface I, unjudged | 636 | 32 / 133 / 471 | 0.160 | 2.36 | 74% |
| D | 145 | 29 / 69 / 47 | 0.145 | 0.235 | 32% (25–40) |
| surface C, unjudged | 473 | 36 / 125 / 312 | 0.180 | 1.56 | 66% |
| E | 133 | 35 / 70 / 28 | 0.175 | 0.140 | 21% (14–30) |

The rows are not all alike. Today's lane and arms A to C use the fires the events log recorded, after refire held some back. Surfaces I and C and arms D and E are fresh scans with no refire. Fresh against fresh: today's surface fires about 6 ways per call on these calls (1,201 fires, study 2), with on-topic fires between about 0.045 per call (the 9 known) and about 0.14 (the upper bound from 0 of 160 sampled); arm E passes 0.175 on-topic and 0.14 off-topic per call. Refire would lower every fresh row.

The judge's AUC for ON against OFF on its candidates, from prior grades only, was 0.67 for A, 0.92 for B, 0.78 for C, 0.97 for D and 0.98 for E. Adding narration (C) brought off-topic passes back. E beats D on every column; their intervals overlap.

**Cost and time.** About half the uniform calls (96 of 200 for E) carry at least one candidate. E made 0.57 judge requests per Bash call at about $0.00175 per request, about $0.001 per Bash call. Median latency was 1.36 s and p95 5.9 s with eight requests in flight; 9–29% of requests across arms and sets took longer than the prompt lane's 2,000 ms deadline (16–29% on the uniform set, 19% for E). The two trials agreed on 468 of 471 E verdicts.

**API note.** The judge sends `temperature: 0`, so the two trials measure provider nondeterminism only. The claude-api skill bundled with Claude Code 2.1.308 (`claude-api/shared/api-version-migration.md`, "Migrating to API version 2026-10-21", change 1) returns 400 for `temperature`, `top_p` and `top_k` on API version `2026-10-21`. The judge sends `anthropic-version: 2023-06-01` (`tools/ways-agent/src/net.rs:275`), where nothing changes, and the study's requests succeeded. The first bump of the version will drop the parameter, and verdict stability at the default temperature is unmeasured (#1130).

## Limits

- Every grader is a Claude model. Study 1's grader made both passes. Study 2's blind graders were stricter than study 1's: on the original fired ways they agreed on 40 of 40 OFF, 4 of 13 ADJ and 4 of 7 ON. Study 3's Opus grader agreed with prior grades on 80% of 80 control pairs (κ 0.67), never turned OFF into ON, and leaned lenient. Absolute ON rates depend on the grader; comparisons within one grader on the same calls are firmer.
- ON counts are small: 7 in the 248, 36 for C on the uniform set.
- Study 2's surfaces and study 3's arms D and E were fresh scans, with no refire gate; today's lane and arms A to C in study 3 use logged fires, after refire. The window's real injection rate after refire was 0.71 per call against 6.0 for fresh scans.
- The segment parser and verb classes are heuristic, and some tldr lookups are wrong (`sed -n '/A/,/B/p'` reads "Print just a first line").
- Study 3's batches of eight ran in alphabetical order and judged every candidate; production's prompt lane judges the first eight in matcher order and passes the rest unjudged.
- The 248 favour narration and the recall set favours action calls. The uniform set is the fair comparison, and it is one window on one host.

## Spend

Study 3 cost $10.90 at list price, including $2.53 for the Opus grading. Studies 1 and 2 made no API calls outside the session's own subagent graders, whose cost was not recorded.

## Addendum, 2026-10-22: the arm E judge through `$.model.complete`

Appended after the studies above; nothing above is changed.

**Method.** At about 20:14Z on the same machine, Claude Code 2.1.308 loaded a probe mod under `claude -p` and ran its slash command `/judge-probe 20 1`. The command took 20 of study 3's uniform-set calls that carried arm E candidates, rebuilt arm E's judge prompts byte-identical to the study's, and sent each, one at a time, through `$.model.complete` on the session's own account: model claude-haiku-4-5, the study's judge text with an instruction to answer in strict JSON instead of the forced `record_judgements` tool call, timeoutMs 8000. A pair passes at P(yes) ≥ 0.3, as in study 3. The comparison is against arm E's first key-path trial on the same pairs.

**Result.** 20 of 20 calls answered, with no API errors and no parse failures. Every reply came wrapped whole in a ```json fence.

| | key path passes | key path blocks |
|---|---|---|
| `$.model.complete` passes | 20 | 3 |
| `$.model.complete` blocks | 3 | 59 |

The two agreed on 79 of 85 pairs. All six disagreements are pairs graded ADJ; on every ON and every OFF pair the two gave the same verdict. By prior grade, the `$.model.complete` path passed ON 12 of 12, ADJ 10 of 21 and OFF 1 of 52.

Latency, sequential: median 1,241 ms, range 681 to 2,840 ms. Usage: 9,886 input and 2,563 output tokens over 20 calls, 296 to 792 input tokens per call, no cache reads. That is below the key path's roughly 1,160 input tokens per request in study 3; the forced-tool schema the key path sends is the likely difference, unconfirmed.

**Limits.** 20 calls and 85 pairs from the uniform set: a smoke test, not a measurement of how closely the two paths agree. One trial, one model, the Bash lane's judge text only, and the probe sent the prompts itself rather than through a hook-side job. A mod slash command did run under `claude -p`, so a larger comparison can run headless.
