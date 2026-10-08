# Probe baseline through the real scan

Measured 2026-10-08 on branch `adr-701-probe-scorer`. Question from ADR-701 §9: how many of the committed tree-sampled probes (`tests/probes/tree-sample.tsv`, 131 rows) does the scan the hooks run fire on the expected way, with no must_not sibling ahead of it? This is the baseline. Nothing here tunes a way, a threshold or the corpus, and no CI gate reads the rate yet.

## Method

`ways author probe` runs each row as a fresh session through the scan's own functions: candidate collection, eligibility, the late-interaction matcher with `matching.admission`, the per-way outcome (keyword gate, semantic channel, parent boost) and the admission order. The relevance judge is off. A probe reads no refire stamp and no parent marker and writes no marker, log or decision record. Candidates and the corpus come from this checkout's `hooks/ways`, not the projection; thresholds, admission and toggles come from the operator's global config, as the hook reads them; the embedding engine and MiniLM are the installed ones.

- **Prompt lane.** `direct` and `situational` rows. The scan loop's per-way decision is shared code (`prompt_outcome`), so the probe cannot drift from it.
- **Tool lane.** `-tool` rows go through the Bash lane's way matching (`command_hits`, the `PreToolUse` entry) with the prompt as the tool description and an empty command, so free text is scored there. The lane tries a way's `commands` and description `pattern` first and skips a state-triggered way's semantic match. A state-triggered expected way with a regex is scored (a regex miss is a failure, stage `regex-miss`); one with no regex cannot fire there and is skipped.
- **Rank and share.** Rank is the expected way's position among the ways that compete, best first. On the late-interaction path the ordering quantity is the summed softmax share. On the single-vector path (below) it is the calibrated probability `g(cos)`, which the share column then shows. Margin is the expected way's quantity minus the best other way's.
- **Stage.** `fired`: the way's body would have been shown. `below-threshold`: it competed and the matcher did not fire it. `not-admitted`, `capped` and `not-confirmed`: it fell at that late-interaction stage. `keyword-gated`: its pattern matched and the keyword floor vetoed it. `masked`: scope or `when:` kept it out of the lane. `state-trigger`: the way fires from a condition and has no regex for the Bash lane to try. `regex-miss`: a state-triggered way's regex did not match. `not-embeddable`: no vocabulary and no pattern, so no channel can fire it. `withheld-parent`: it fired only on a parent boost and no parent was shown.
- **Pass.** The expected way fires and no `must_not` way ranks ahead of it. Top-1 is rank 1.

```
ways corpus --ways-dir hooks/ways --output "$TMP/corpus"
ways author probe --ways-dir hooks/ways --corpus "$TMP/corpus"          # table + summary
ways author probe --ways-dir hooks/ways --corpus "$TMP/corpus" --tsv   # one TSV row per probe
```

Two runs of the command produce identical output.

## What the run exercised

- **Late interaction ran for 1 of 130 prompt probes.** The matcher chunks the surface into sentences and needs at least two. The other 129 golden prompts are one sentence, so the scan used its single-vector fail-safe for them, as the hook does: a way fires when `g(cos)` clears `semantic_fire_probability`. Their `peak` and `confirm` are empty, and the admission stages (`not-admitted`, `capped`, `not-confirmed`) can occur only for the one probe that ran late interaction. This baseline is mostly a measure of the single-vector gate. A multi-sentence probe set would be needed to measure admission and body confirmation.
- **Parent boost.** Exercised where a parent fires in the same probe and lowers its child's bar (24 probes). A boost from an earlier turn's parent marker is not exercised: each probe is a fresh session, and the probe does not fake a marker.
- **Skipped:** 1. A row whose expected way cannot fire on its lane by design is reported `skipped: lane-ineligible` and left out of every denominator. That covers `masked` (scope or `when:`), `state-trigger` with no regex, and `not-embeddable`. A way the operator's config turns off (`disabled_domains` or a toggle) is `skipped: disabled`. The summary prints the config's disabled domains; this run had none. The first run of this baseline scored `softwaredev/freshness` on the Bash lane as a failure (131 scored, pass 81/131 = 61.8%, top-1 71/131 = 54.2%). It carries `trigger: session-start` and no regex.

## Summary

Output of the command above, after the table of rows:

```
probes: 131 total, 130 scored, 1 skipped · admission: share · project: /home/aaron/Projects/ai/harness/agent-ways
operator config: disabled domains: none
pass = the expected way fires and no must_not way outranks it; top-1 = the expected way ranks first

group                   scored    pass     pass%   top-1    top-1%   fires    fires%
overall                    130      81     62.3%      71     54.6%      86     66.2%
role=leaf                   58      43     74.1%      38     65.5%      45     77.6%
role=parent                 17       8     47.1%       8     47.1%       9     52.9%
role=root                   55      30     54.5%      25     45.5%      32     58.2%
kind=direct                 51      44     86.3%      40     78.4%      45     88.2%
kind=situational            79      37     46.8%      31     39.2%      41     51.9%

single-vector fallback (late interaction could not run): 129 prompt probes
parent boost exercised by a parent fired in the same probe: 24 probes
parent boost from an earlier turn's parent marker: not exercised (each probe is a fresh session)
skipped: softwaredev/freshness (situational-tool): lane-ineligible (state-trigger)
```

Failing probes by stage:

| Stage | Probes |
|---|---|
| below-threshold | 42 |
| fired | 5 |
| keyword-gated | 1 |
| not-admitted | 1 |

## Failing probes

49 of 130 scored. `rank` is the expected way's position, `-` when it was not ranked. `sibling_over` lists the `must_not` ways that ranked ahead of it. A `fired` stage with a non-empty `sibling_over` means the way fired but lost the ranking to a sibling.

| expected_way | role | kind | rank | share | stage | sibling_over |
|---|---|---|---|---|---|---|
| data/migrations/idempotent | leaf | situational | 3 | 0.3233 | below-threshold |  |
| data/schema-docs | leaf | situational | 2 | 0.8775 | fired | data/migrations |
| documentation/adr | parent | situational | 6 | 0.3436 | below-threshold |  |
| documentation/adr/migration | leaf | situational | 34 | 0.1089 | below-threshold | documentation/adr/consider |
| documentation/api | leaf | situational | 2 | 0.1572 | below-threshold |  |
| ea/comms | parent | situational | 1 | 0.2747 | below-threshold |  |
| ea/comms/recap | leaf | situational | 3 | 0.1534 | below-threshold |  |
| ea/intelligence | leaf | direct | 3 | 0.5209 | fired | ea/briefing |
| ea/tasks | parent | situational | 6 | 0.1457 | below-threshold |  |
| itops/policy | root | situational | 67 | 0.0178 | below-threshold | itops/incident,itops/proposals,itops/runbooks |
| itops/proposals | root | situational | 6 | 0.4087 | below-threshold |  |
| itops/runbooks | root | situational | 71 | 0.0217 | below-threshold | itops/incident,itops/proposals |
| meta/choices | root | situational | 7 | 0.1000 | below-threshold | meta/introspection |
| meta/develop | root | situational | 2 | 0.1172 | not-admitted |  |
| meta/goals | root | situational | 5 | 0.7464 | fired | meta/wrap |
| meta/introspection | root | situational | 18 | 0.2486 | below-threshold | meta/choices |
| meta/knowledge | root | situational | 15 | 0.2976 | below-threshold | meta/introspection,meta/memory |
| meta/memory | root | situational | 13 | 0.3406 | below-threshold | meta/start |
| meta/start | root | direct | 10 | 0.0390 | below-threshold | meta/memory,meta/wrap |
| meta/start | root | situational | 31 | 0.0840 | below-threshold | meta/deployment |
| meta/subagents | root | direct | 29 | 0.0976 | below-threshold | meta/deployment,meta/introspection,meta/workflows |
| meta/subagents | root | situational | 39 | 0.1003 | below-threshold | meta/deployment,meta/develop,meta/introspection,meta/knowledge,meta/skills,meta/start,meta/think |
| meta/think | root | direct | 38 | 0.0522 | below-threshold | meta/choices,meta/memory,meta/subagents |
| meta/think | root | situational | 3 | 0.2034 | below-threshold | meta/develop |
| meta/trust | root | situational | 1 | 0.3959 | below-threshold |  |
| meta/wrap | root | direct | 15 | 0.1102 | keyword-gated | meta/skills |
| meta/wrap | root | situational | 3 | 0.8385 | fired | meta/start |
| research | root | situational | 6 | 0.1529 | below-threshold | data |
| softwaredev/architecture | root | situational | 2 | 0.3605 | below-threshold |  |
| softwaredev/architecture/design | parent | situational | 38 | 0.0351 | below-threshold |  |
| softwaredev/architecture/threat-modeling | leaf | direct | 1 | 0.4644 | below-threshold |  |
| softwaredev/architecture/threat-modeling | leaf | situational | 7 | 0.3738 | below-threshold |  |
| softwaredev/code | root | situational | 16 | 0.0708 | below-threshold | softwaredev/environment,softwaredev/freshness |
| softwaredev/code/errors | leaf | situational | 6 | 0.0704 | below-threshold |  |
| softwaredev/code/quality | parent | situational | 14 | 0.1815 | below-threshold |  |
| softwaredev/code/security/auth | leaf | situational | 17 | 0.0133 | below-threshold | softwaredev/code/security/contributions,softwaredev/code/security/secrets |
| softwaredev/code/security/injection/prompt | leaf | situational | 11 | 0.1130 | below-threshold |  |
| softwaredev/code/supplychain | parent | situational | 3 | 0.3402 | below-threshold |  |
| softwaredev/code/supplychain/depscan | parent | situational | 5 | 0.4170 | below-threshold |  |
| softwaredev/code/supplychain/depscan/node/lockfile | leaf | situational | 16 | 0.0706 | below-threshold |  |
| softwaredev/code/testing | parent | situational | 5 | 0.7008 | fired | softwaredev/code/supplychain |
| softwaredev/code/testing/gates | parent | situational | 1 | 0.4212 | below-threshold |  |
| softwaredev/code/testing/gates/assertions | leaf | situational | 3 | 0.3483 | below-threshold |  |
| softwaredev/delivery | root | situational | 23 | 0.0841 | below-threshold | softwaredev/code,softwaredev/visualization |
| softwaredev/delivery/release | leaf | situational | 3 | 0.3895 | below-threshold | softwaredev/delivery/groundwork |
| softwaredev/environment | root | situational | 3 | 0.1675 | below-threshold |  |
| workstation/shell/tools | leaf | situational | 20 | 0.0292 | below-threshold | workstation/shell/shellrc |
| writing | root | direct | 32 | 0.0788 | below-threshold |  |
| writing | root | situational | 6 | 0.3097 | below-threshold |  |

## Reading this

- **Direct against situational.** Direct prompts name the topic and pass at a far higher rate than situational ones, which describe the circumstance. The misses are mostly `below-threshold`: the way competed and the calibrated probability stayed under the bar.


## Joined probes

Measured 2026-10-08 on branch `adr-701-multi-sentence-probes`. The baseline above is mostly a single-vector measurement: late interaction needs at least two sentence chunks and 129 of 130 prompt probes had one. The joined set (`tests/probes/tree-sample-joined.tsv`, 79 rows) gives each sampled way a two-sentence surface, so admission, sidecar body confirmation and capping run.

### Set

`ways author golden --ways-dir hooks/ways --probes --joined` selects the same ways as `--probes`, with the same role and `must_not`. Each row's prompt is the way's first `situational` golden prompt, then a space, then the way's first `direct` golden prompt. The situational prompt keeps its own trailing `.`, `!` or `?`; a `.` is added only when it has none. The kind is `joined`. A way without both prompts has no row; tool-surface rows are not used. The chunker splits at `.`, `!` or `?` followed by whitespace (`reduce::split_sentences`), so each joined prompt yields at least two chunks (`meta/develop` yields three, because its situational prompt has a period inside it). The drift test asserts at least two chunks for every shipped row, using the scan's own `chunk_surface` count, which also applies the 12-character minimum and dedup. The 79 rows are 33 roots, 17 parents and 29 leaves. Parents appear because the golden sidecars give them a direct prompt, which `--probes` leaves out.

### Command

```
ways corpus --ways-dir hooks/ways --output "$TMP/corpus"
ways author probe tests/probes/tree-sample-joined.tsv --ways-dir hooks/ways --corpus "$TMP/corpus"
```

`ways author probe` now prints a `path` column (`late`, `single` or `bash`) and a `path:` count line in the summary.

### Summary

Output of the command above, after the table of rows:

```
probes: 79 total, 79 scored, 0 skipped · admission: share · project: /home/aaron/Projects/ai/harness/agent-ways
operator config: disabled domains: none
pass = the expected way fires and no must_not way outranks it; top-1 = the expected way ranks first

group                   scored    pass     pass%   top-1    top-1%   fires    fires%
overall                     79      55     69.6%      64     81.0%      57     72.2%
role=leaf                   29      26     89.7%      26     89.7%      27     93.1%
role=parent                 17      11     64.7%      13     76.5%      11     64.7%
role=root                   33      18     54.5%      25     75.8%      19     57.6%
kind=joined                 79      55     69.6%      64     81.0%      57     72.2%

path: 79 late interaction, 0 single-vector fallback (late interaction could not run), 0 bash lane (scored probes)
parent boost exercised by a parent fired in the same probe: 0 probes
parent boost from an earlier turn's parent marker: not exercised (each probe is a fresh session)
```

### Stage of the expected way

| Stage | Probes |
|---|---|
| fired | 57 |
| not-confirmed | 12 |
| not-admitted | 8 |
| below-threshold | 2 |
| capped | 0 |

`not-confirmed` is a way admitted by the matcher whose sidecar body cosine did not confirm it. `below-threshold` here means the way had no row at all: the match runs at threshold 0.0, so it was outside every chunk's top 8 matches (`TOP_K_PER_CHUNK`). Of the 24 failures, 22 did not fire and 2 fired but were outranked by a `must_not` sibling (`meta/wrap`, `softwaredev/delivery/release`).

### Failing probes

| Expected way | Role | Rank | Stage | sibling_over | Peak | Confirm |
|---|---|---|---|---|---|---|
| `ea` | root | 1 | not-confirmed | - | 0.5305 | 0.3286 |
| `ea/comms` | parent | 1 | not-confirmed | - | 0.5017 | 0.3353 |
| `ea/intelligence` | leaf | 1 | not-confirmed | - | 0.3903 | 0.3051 |
| `ea/tasks` | parent | 2 | not-admitted | - | 0.2299 | - |
| `itops/proposals` | root | 2 | not-admitted | - | 0.4484 | - |
| `meta/develop` | root | 1 | not-admitted | - | 0.3772 | - |
| `meta/memory` | root | 1 | not-confirmed | - | 0.3431 | 0.2528 |
| `meta/start` | root | - | below-threshold | meta/memory,meta/wrap | - | - |
| `meta/subagents` | root | - | below-threshold | meta/deployment,meta/introspection,meta/knowledge | - | - |
| `meta/think` | root | 7 | not-admitted | meta/develop | 0.2419 | - |
| `meta/workflows` | root | 1 | not-confirmed | - | 0.3444 | 0.3201 |
| `meta/wrap` | root | 5 | fired | meta/start | 0.3787 | - |
| `research` | root | 6 | not-admitted | data | 0.3454 | - |
| `softwaredev/architecture` | root | 1 | not-confirmed | - | 0.3804 | 0.2138 |
| `softwaredev/architecture/design` | parent | 4 | not-admitted | - | 0.3200 | - |
| `softwaredev/architecture/threat-modeling` | leaf | 2 | not-confirmed | - | 0.3093 | 0.3255 |
| `softwaredev/code/supplychain` | parent | 1 | not-confirmed | - | 0.3828 | 0.3425 |
| `softwaredev/code/testing/gates` | parent | 1 | not-confirmed | - | 0.6682 | 0.3054 |
| `softwaredev/delivery` | root | 14 | not-admitted | softwaredev/code,softwaredev/environment | 0.2654 | - |
| `softwaredev/delivery/groundwork` | parent | 1 | not-confirmed | - | 0.4640 | 0.2617 |
| `softwaredev/delivery/release` | leaf | 2 | fired | softwaredev/delivery/groundwork | 0.4291 | 0.5639 |
| `softwaredev/environment` | root | 1 | not-confirmed | - | 0.4439 | 0.1415 |
| `workstation/pkghistory` | root | 1 | not-confirmed | - | 0.3895 | 0.3085 |
| `writing` | root | 15 | not-admitted | - | 0.2507 | - |

### Against the single-sentence baseline

Same 79 ways. The single-sentence baseline rows for these ways are 79 situational prompts (37 pass, 31 top-1, 41 fire; 78 single-vector, 1 late) and 51 direct prompts (44 pass, 40 top-1, 45 fire; all single-vector). The joined rows are 55 pass (69.6%), 64 top-1 (81.0%), 57 fire, all late interaction. Per way, 23 ways that did not pass on their situational prompt pass joined; 5 that passed on their situational prompt fail joined (`ea`, `ea/intelligence`, `meta/workflows`, `softwaredev/delivery/groundwork`, `workstation/pkghistory`, all `not-confirmed`); 7 that passed on their direct prompt fail joined. The two baselines use different surfaces and different paths, so the pass counts compare the set as a whole, not a single variable. The probe parent-boost count is 0 on the joined set against 24 on the baseline.

### Join rule change

The first version of the set replaced the situational prompt's trailing `.`, `!` or `?` with a `.`. The rule now keeps the terminator the prompt has and adds a `.` only when it has none. Two rows changed text (`collaboration/onboarding-share` and `meta/introspection`, each a situational prompt ending in `?`). The probe was rerun on the regenerated set: no result moved. Every number, stage count and failing row above is identical to the first run.
