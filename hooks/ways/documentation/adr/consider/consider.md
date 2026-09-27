---
description: handing a proposed decision record to the operator to consider, the operator answering looks good or pushing back, accepting rejecting or abandoning a record, and raising a concern about a decision
vocabulary: consider review approve approval looks good lgtm ship it sounds good go ahead accept reject abandon proposed decision summary probe probes inversion canary concern pushback sign off operator judgement
pattern: \b(looks good|lgtm|ship it|sounds good|go ahead|approved?|accept (it|this|the adr))\b
commands: (docs/scripts/)?adr\ (accept|reject|abandon)
scope: agent
refire: 0.2
---
<!-- epistemic: convention -->
# Considering a Decision

The agent writes and proposes a decision; the operator considers it (ADR-304 §12). The two work in parallel. The agent brings depth and detail. The operator brings judgement, taste, and concerns from outside the repository. The agent owes a decision the operator can understand. The operator owes a decision that was not accepted blindly.

## Handing a decision over

- Open with a Summary the operator can judge alone: what is decided, what it trades away, whether it is one-way (said first when it is), the probes, and an inversion.
- Probes are specific points put to the operator. Mix points you are confident on with points you are not, and label each. The mix is deliberate: probes prime the operator's judgement, and priming can bias it.
- The inversion names the two ends the decision sits between and asks whether the answer lies outside your framing.
- A probe may be a canary: a point that is deliberately wrong, harmless if accepted, and a little whimsical. Reveal it right after the operator answers. It never stays in the record and is never about safety.

## When the answer comes

- A short yes is a real answer. Take it as given and move on; do not re-ask the probes.
- Under adr/v1, record a `considered` entry: what was said, via which channel, which probes the answer covers, and `canary: caught` or `missed`.
- If the canary was missed, say so once and constructively, offer a smaller set of probes, then proceed on the operator's answer.
- A decision the operator started waits for their consideration before `adr accept`.

## Raising a concern

- You may raise a concern at any stage, including after acceptance: a safety issue, reasoning that does not follow, or anything that seems off.
- Keep concerns few and actionable. Each names what would resolve it; batch minor points into one.
- Challenge once. If the operator still says go, proceed and do your best.
- A concern is answered or withdrawn with a stated reason, in the record. Long sessions are when concessions slip in unannounced, so hold a concern until it has an answer.

## See Also

- adr(documentation): the record format and commands for this project
- trust/autonomy(meta): when to act without asking
