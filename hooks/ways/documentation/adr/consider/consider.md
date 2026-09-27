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
- When the decision carries `observable` entries, demonstrate them before asking, where you can: run the command, show the output or a screenshot, open the page. Say in `via` what the operator was shown (ADR-307).
- Ask the probes in the conversation, one question each, with enough context to answer without opening the record, and label which ones you are confident on. A probe that exists only in the record was never asked, so the answer to "looks good" covers none of them. With more than one probe or decision pending, ask them through the choice tool as one batch, one question each (choices(meta)).
- A probe may be a canary: a point that is deliberately wrong, harmless if accepted, and a little whimsical. Reveal it right after the operator answers. It never stays in the record and is never about safety.

## When the answer comes

- A short yes is a real answer. Take it as given and move on; do not re-ask the probes.
- Under adr/v1, record a `considered` entry: what was said and via which channel. `covers` lists the probes the answer settled; a bare "looks good" covers none. Add `canary: caught` or `missed` only when a canary was used.
- If the canary was missed, say so once and constructively, offer a smaller set of probes, then proceed on the operator's answer.
- A decision the operator started waits for their consideration before `adr accept`. A decision with no operator basis, grounded in evidence, a standard or upstream, may be accepted by the agent directly.

## Asking in plain words
Field names such as `basis`, `considered` and `level` belong in the record, not in what you say to the operator. Ask the way a teammate would, and give enough context that the question can be answered without opening the file. The shape runs from worst to best:
- A bare reference: "What's the basis for ADR-186?"
- A plain question: "Why did we go this way on ADR-186? Can I quote you?"
- Context, then the question: a sentence or two on what the record decided and what it trades, then why, and whether you can quote them.
Write each ask fresh from the record in front of you; these are shapes, not phrasings to reuse. A request to confirm a record follows the same shape: what it decides, then whether it holds up. Don't ask for a level. Work it out from who made the call.
## Raising a concern

- You may raise a concern at any stage, including after acceptance: a safety issue, reasoning that does not follow, or anything that seems off.
- Keep concerns few and actionable. Each names what would resolve it; batch minor points into one.
- Challenge once. If the operator still says go, proceed and do your best.
- A concern goes in the record as a `concern:` entry. It does not block acceptance, and it stays in the record: it is answered, or withdrawn with a stated reason, never deleted. Long sessions are when concessions slip in unannounced, so leave a concern open until it has an answer.

## See Also

- adr(documentation) — the record format and commands for this project
- trust/autonomy(meta) — when to act without asking
