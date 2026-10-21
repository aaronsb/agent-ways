---
id: 01.025.H
domain: ways
mode: how-to
related:
  - "[[ADR-701]]"
  - "[[ADR-508]]"
  - "[[01.024.R]]"
aliases: []
---

# Contribute a way or skill

This guide takes one way or skill from a draft to a pull request against the shared corpus. It assumes a development checkout ([development.md](../development.md)). The kinds of contribution and the bar each must clear are defined in [Contribution kinds and acceptance bars](../reference/contribution-kinds-and-acceptance-bars.md). The mechanics of a way file (frontmatter, triggers, `refire:`, trees) are in [Writing a way](../hooks-and-ways/extending.md) and the [authoring way](../../hooks/ways/meta/knowledge/authoring/authoring.md) and its children.

## 1. Name the kind

Decide which of the four kinds the draft is: way, skill, tool knowledge or posture. Only ways and skills go into the shared corpus. Send tool knowledge to the tool's own instructions, and keep posture in your user-scope ways under `$XDG_CONFIG_HOME/agent-ways/ways/`. When a draft mixes kinds, keep the part that says how the work is done and move the rest to its home.

## 2. Write it

- Phrase a way as how the work is done when the situation arises ([phrasing a way](../reference/contribution-kinds-and-acceptance-bars.md#phrasing-a-way)).
- Write the pattern, not the incident. Leave out client, employer and person specifics. An anecdote with only the names removed still describes one engagement, so restate it as the general situation it stands for ([pii-free](../../hooks/ways/meta/knowledge/authoring/pii-free/pii-free.md)).
- Keep the register sober. Injected text reads as doctrine, and the model imitates the register of what it reads ([ADR-178](../architecture/practice/ADR-178-register-transfers-by-demonstration-core-md-carries-policy.md)). Run `scripts/check-register.sh` and clear what it reports for your file. The [prose](../../hooks/ways/meta/trust/prose/prose.md) way lists the shapes to cut.
- Put a list of phrases, such as phrases to avoid or example wordings, only in a way that fires when someone asks for that work. A way that refires through long sessions repeats the list each time, and the model picks the phrases up.

## 3. Check the engine before tuning

Run `ways status` and read two lines.

- **Corpus** lists the entries and how many are embedded. The count should cover every installed way. If it is short, run `ways corpus`.
- **Calibration** shows an AUC for each corpus. Without calibration the keyword lane fails open, so every `pattern:` hit fires and the results reflect keyword-only matching. `ways corpus` fits the calibration.

Tune against the full corpus with the core domains enabled. `ways settings get ways.disabled_domains` should print `[]`, and `ways settings list ways.project` shows per-way switches in the current project. A disabled domain hides the overlaps and trigger collisions your way will meet in installs that have it on.

## 4. Tune the trigger

1. Keep `pattern:` to terms of art: exact command names, file names, acronyms. Put common words in `vocabulary:`, where the semantic lane weighs them in context ([keyword-lane](../../hooks/ways/meta/knowledge/authoring/keyword-lane/keyword-lane.md)). `ways author lint <way-dir>` flags common-word patterns.
2. Write the golden prompts without the way's own trigger words. A golden that repeats the vocabulary measures the vocabulary against itself.
3. Have someone else write a held-out set: a person, or a fresh agent given only a one-sentence statement of the situation and not the way ([held-out](../../hooks/ways/evaluative/held-out/held-out.md)). Save it as TSV with the header `prompt	expected_way	kind	role	must_not`.
4. Build a corpus from the checkout and probe the held-out set through the real prompt scan:

   ```bash
   ways corpus --ways-dir hooks/ways --output "$CORPUS"
   ways author probe held-out.tsv --ways-dir hooks/ways --corpus "$CORPUS"
   ```

   The probe runs each prompt as a fresh session with the relevance judge off, and reports where the expected way ranked and which stage decided it.
5. Check that the way does not steal prompts from existing ways. `ways author siblings <way-id>` lists the ways nearest to it. Then export the existing goldens and probe them in a clone of the base branch and in your branch, each against a corpus built from its own tree:

   ```bash
   ways author golden --ways-dir hooks/ways --probes > goldens.tsv
   ways author probe goldens.tsv --ways-dir hooks/ways --corpus "$CORPUS" --tsv > after.tsv
   ```

   Write the base branch's rows to `before.tsv` and diff the two files. A row that passes on the base branch and fails on yours is a prompt your way took from another way. Remedy it in your way's vocabulary or pattern, then probe again.

## 5. Record the engine state

A probe result means little without the state it was measured under. Report it with:

- the commit of the checkout the corpus was built from
- the `Engine`, `Gate`, `Language`, `Corpus` and `Calibration` lines from `ways status`
- the disabled domains (`[]` when none)
- the probe file, its row count, and the pass rate

## 6. Show the effect

For a way, run the ablation described in [the ablation for a way](../reference/contribution-kinds-and-acceptance-bars.md#the-ablation-for-a-way). Each arm must be a clone (`git clone --branch <b> <repo> <dir>`); the matrix refuses a worktree.

```bash
MATRIX_ARMS="without=$BASE without2=$BASE with=$BRANCH forced=$FORCED" \
MATRIX_TRIALS=3 \
TIER2_SCENARIOS="<your-scenario>" \
MATRIX_OUT="$HOME/matrix-out" \
make test-live-matrix
```

A scenario is a directory under `tests/fixtures/docker/scenarios/`; [the fixture notes](../../tests/fixtures/docker/CLAUDE.md) say how to add one. Each run writes its transcripts under `MATRIX_OUT/<model>/<arm>/t<k>`. Grade them with a blind judge as [evaluative/ablation](../../hooks/ways/evaluative/ablation/ablation.md) describes. Tier 2 spends API tokens, so run it locally when the way is ready ([ADR-508](../architecture/platform/ADR-508-the-live-fixture-runs-locally-its-github-workflow-runs-on-manual-dispatch-only.md)).

For a skill, invoke it on a representative task and keep the transcript or the end state it left.

## 7. Open the pull request

Send one way or one skill per pull request. The description carries:

- the kind, stated
- the scenario that would show its effect, and the ablation result for a way or the run for a skill
- probe results against the held-out prompts, with the engine state from step 5
- the output of `ways author lint <way-dir>` and `scripts/check-register.sh`
