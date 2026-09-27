---
description: handing a proposed decision record to the operator to consider, the operator answering looks good or pushing back, accepting rejecting or abandoning a record, and raising a concern about a decision
vocabulary: consider review approve approval looks good lgtm ship it sounds good go ahead accept reject abandon proposed decision summary probe probes inversion canary concern pushback sign off operator judgement
pattern: \b(looks? good|lgtm|sgtm|ship it|sounds good|go ahead|go for it|approved?|accept (it|this|the adr|adr-?\s?\d+))\b
commands: adr\ (accept|reject|abandon)
files: docs/architecture/.*\.md$
when:
  file_exists: docs/architecture/adr.yaml
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Considering a Decision

The agent writes and proposes a decision; the operator considers it (ADR-304 §12). The two work in parallel. The agent brings depth and detail. The operator brings judgement, taste, and concerns from outside the repository. The agent owes a decision the operator can understand. The operator owes a decision that was not accepted blindly.

## Handing a decision over

- Open with a Summary the operator can judge alone: what is decided, what it trades away and forecloses, whether it is one-way (said first when it is), the probes, and an inversion.
- Probes are specific points put to the operator. Mix points you are confident on with points you are not, and label each. The mix is deliberate: probes prime the operator's judgement, and priming can bias it.
- The inversion names the two ends the decision sits between and asks whether the answer lies outside your framing.
- A probe may be a canary: a point that is deliberately wrong, harmless if accepted, and a little whimsical. Reveal it right after the operator answers. It never stays in the record and is never about safety.

## When the answer comes

- A short yes is a real answer. Take it as given and move on; do not re-ask the probes.
- Under adr/v1, record a `considered` entry: what was said and via which channel. `covers` lists the probes the answer settled; a bare "looks good" covers none. Add `canary: caught` or `missed` only when a canary was used.
- If the canary was missed, say so once and constructively, offer a smaller set of probes, then proceed on the operator's answer.
- A decision the operator started waits for their consideration before `adr accept`. A decision with no operator basis, grounded in evidence, a standard or upstream, may be accepted by the agent directly.

## Asking in plain words
Field names such as `basis`, `considered` and `level` belong in the record, not in what you say to the operator. Ask the way a teammate would, and open with a sentence on what the record decided, so the question can be answered without opening the file. A bare record number is not enough.
- Opaque: "What's the basis for ADR-186?"
- Better: "Why did we go this way on ADR-186? Can I quote you?"
- Best: "In ADR-186 we split install testing into two tiers, both in a Debian container: tier 1 needs no API key and runs on every PR, and tier 2 uses a key to run Claude for real, so it only runs on demand or nightly. Why did we go this way? Can I quote you on any of it?"
For a considered entry, do the same: say what the record decides in a sentence or two, then ask whether it holds up. Don't ask for a level. Work it out from who made the call.
## Raising a concern

- You may raise a concern at any stage, including after acceptance: a safety issue, reasoning that does not follow, or anything that seems off.
- Keep concerns few and actionable. Each names what would resolve it; batch minor points into one.
- Challenge once. If the operator still says go, proceed and do your best.
- A concern goes in the record as a `concern:` entry. It does not block acceptance, and it stays in the record: it is answered, or withdrawn with a stated reason, never deleted. Long sessions are when concessions slip in unannounced, so leave a concern open until it has an answer.

## See Also

- adr(documentation) — the record format and commands for this project
- trust/autonomy(meta) — when to act without asking
