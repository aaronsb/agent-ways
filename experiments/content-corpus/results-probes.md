# Probe baseline through the real scan

Measured 2026-10-08 on branch `adr-701-probe-scorer`. Question from ADR-701 §9: how many of the committed tree-sampled probes (`tests/probes/tree-sample.tsv`, 131 rows) does the scan the hooks run fire on the expected way, with no must_not sibling ahead of it? This is the baseline. Nothing here tunes a way, a threshold or the corpus, and no CI gate reads the rate yet.

## Method

`ways author probe` runs each row as a fresh session through the scan's own functions: candidate collection, eligibility, the late-interaction matcher with `matching.admission`, the per-way outcome (keyword gate, semantic channel, parent boost) and the admission order. The relevance judge is off. A probe reads no refire stamp and no parent marker and writes no marker, log or decision record. Candidates and the corpus come from this checkout's `hooks/ways`, not the projection; the embedding engine and MiniLM are the installed ones.

- **Prompt lane.** `direct` and `situational` rows. The scan loop's per-way decision is shared code (`prompt_outcome`), so the probe cannot drift from it.
- **Tool lane.** `-tool` rows go through the Bash lane's way matching (`command_hits`, the `PreToolUse` entry) with the prompt as the tool description and an empty command. That lane takes free text, so no row is skipped.
- **Rank and share.** Rank is the expected way's position among the ways that compete, best first. On the late-interaction path the ordering quantity is the summed softmax share. On the single-vector path (below) it is the calibrated probability `g(cos)`, which the share column then shows. Margin is the expected way's quantity minus the best other way's.
- **Stage.** `fired`: the way's body would have been shown. `below-threshold`: it competed and the matcher did not fire it. `not-admitted`, `capped` and `not-confirmed`: it fell at that late-interaction stage. `keyword-gated`: its pattern matched and the keyword floor vetoed it. `masked`: scope or `when:` kept it out of the lane. `state-trigger`: the way fires from a condition, which the Bash lane's semantic matcher skips. `withheld-parent`: it fired only on a parent boost and no parent was shown.
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
- **Skipped:** 1. A row whose expected way cannot fire on its lane by design (scope, `when:`, or a state trigger) is reported `skipped: lane-ineligible`, with the stage that showed it, and left out of every denominator. This replaces the first run of this baseline, which scored `softwaredev/freshness` on the Bash lane as a failure (131 scored, pass 81/131 = 61.8%, top-1 71/131 = 54.2%). It carries `trigger: session-start`, so the Bash lane's semantic matcher skips it.

## Summary

131 probes, 130 scored, 1 skipped. Admission rule `share`.

```
single-vector fallback (late interaction could not run): 129 prompt probes
parent boost exercised by a parent fired in the same probe: 24 probes
parent boost from an earlier turn's parent marker: not exercised (each probe is a fresh session)
skipped: softwaredev/freshness (situational-tool): lane-ineligible (state-trigger)
```

Skipped: `softwaredev/freshness` (situational-tool): lane-ineligible (state-trigger).

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
