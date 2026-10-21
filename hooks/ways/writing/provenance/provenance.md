---
description: claims and numbers in a report, deck, analysis, status update, or write-up; tie every claim to its source, trace every figure to a source or recompute it with a script, date measured numbers and label estimates, raise conflicts between sources instead of picking one, invent nothing, and end with an open-items list
vocabulary: provenance source cite citation evidence claim number figure stat metric measured estimate inference unverified dated recompute tally count total cost deck slides report analysis status discrepancy conflict visibility gap allowed numbers rounding drift open items correction skeptical primary record
pattern: \bprovenance\b|where did (that|this) (number|figure|stat) come from|allowed.?numbers|not yet measured|\brecompute\b|to the skeptical
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Provenance for Claims and Numbers

A reader acts on what a report says. Each claim and each number needs a path back to where it came from.

## Every claim has a source

- Tie each action item, status bullet, figure, and claim to its source with a short parenthetical: transcript lines, an email, a decision record, an API pull, a commit, or a `file:line`.
- Check dates, attributions, and counts against the primary record, not the user's dictation or an earlier draft. Flag wording that runs ahead of the evidence, such as "funded" when the record says "in motion".
- Invent no facts, sources, history, filler, work items, or findings. Flag gaps to the author. If everything checks out, say so and do not manufacture problems.
- Do not present inference as fact. Say who said what. Label estimates, inferences, and proposals.
- When sources disagree, raise an open question. Never pick one silently or edit the source file to match. Work from one value and flag the other, for example "using 100 and flagging the 90 in the second export".

## Numbers trace or recompute

- Every figure traces to a named source or a script. A model cannot reliably tally a long list, so the model classifies items by index and code does the counting.
- A statistic over runs counts only the runs that completed.
- Before shipping, recompute a few numbers independently and say how. Audit the copy against the set of numbers the computation allows, to catch rounding drift.
- Quote measured numbers with a date. Mark estimates with `~` or "not yet measured". Say what a measurement includes, such as restart time. Read the project's cost notes before quoting a cost.
- Put a correction in a dated paragraph beside the original. Do not overwrite the original.

## Visibility and recommendations

- State what you can and cannot see, and name the export that would close each gap. A claim that something is unfixable needs examples, a definition of the error, and how you know.
- Check the subject's observed behavior in the right time window before recommending. Drop generic advice the data contradicts.
- When a number or recommendation is challenged, recheck the math instead of defending it. Then explain it to a skeptical reader.

## Working with data

- Pull payloads to scratch files and analyze them with `grep`, `sed -n`, `jq`, or a script. Read a run transcript's tail, and only for failed runs.
- List private or personal files by name, but do not open them.
- Record what the source says even when it looks wrong. An anomaly becomes a review warning, not a silent correction.
- Present data as markdown tables plus a short narrative on trends, concentration, and surprises. Format currency with separators, such as $1,234,567. Do not paste raw JSON.

## Open-items tail

End each deliverable with unverified numbers, discrepancies, facts to confirm with a named person, and decisions for the user. Mark recalled facts "unverified (from memory only)" and flag them before anyone repeats them aloud. Resolve audience-facing unknowns before the audience sees the material, such as titles, logo rights, photo consent, meeting slots, and a QR code tested on a phone.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The user told me the number" | Check it against the primary record. |
| "The two sources are close enough" | Raise the gap as an open question. |
| "I counted them myself" | Have code count them, then recompute a few. |
| "It's a reasonable estimate" | Then label it as an estimate. |

## See Also

- writing(writing) — parent: structure, audience, and format
- research(research) — ranking sources and keeping found apart from inferred
- delivery/groundwork/claims(softwaredev) — a contribution's account of itself is a claim
- code/testing/gates(softwaredev) — a zero result needs a positive control
