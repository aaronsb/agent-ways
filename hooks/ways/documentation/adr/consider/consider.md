---
description: handing a proposed decision record to the operator, checking the operator's intent, an advisor answering when no one is present or under a goal, reviewing past decisions with the operator, accepting rejecting or abandoning a record, and raising a concern about a decision
vocabulary: canary consider review approve approval looks good lgtm ship it sounds good go ahead accept reject abandon proposed decision summary probe probes intent inversion advisor skeptic goal unattended concern pushback sign off operator recent decisions sessions flag
pattern: \b(looks? good|lgtm|sgtm|ship it|sounds good|go ahead|go for it|approved?|accept (it|this|the adr|adr-?\s?\d+)|review (with me )?(the |my |our )?(recent |last \w+ )?(decisions|adrs|records))\b
commands: adr\ (accept|reject|abandon|consider)
files: docs/architecture/.*\.md$
when:
  file_exists: docs/architecture/adr.yaml
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Considering a Decision

The consider step records the operator's intent (ADR-304 §12 and its notes). The agent owns the technical calls and grounds each one in its `basis`: evidence, a standard, upstream, or a precedent. The operator is never asked to approve a technical call. What goes to the operator is a check that the decision serves what they wanted, and why they wanted it.

## Handing a decision over

- Open with a Summary a reader can judge alone: what is decided, what it trades away and forecloses, whether it is one-way (said first when it is), the probes, and an inversion.
- Probes are intent checks, at most one or two, each in the operator's terms and answerable in a word. Write "you wanted list pages to feel fast; is data up to 30 seconds old fine for that?" in place of "is the latency win real?". Label each probe `*Confident (name):*` or `*Not confident (name):*` for how sure you are that you read their intent right. The tool names probes by that label.
- The inversion names the two ends the decision sits between. Put it to the operator only when it turns on what they want.
- When the decision carries `observable` entries, show them where you can: run the command, show the output, open the page. Say in `via` what was shown (ADR-307).
- Ask the probes as part of your reply, one short line each. With more than one pending, ask them through the choice tool as one batch when it is available. An answer is never required. `adr accept` reads no answer and records nothing on the operator's behalf.

## A canary

- You may add a canary among the intent checks: a point that is deliberately wrong, harmless if accepted, and never about safety. Ask it in the conversation only. It never goes in the record's text.
- It is a signal probe and is never required. If no one answers it, record nothing about it and continue.
- When someone answers, reveal the canary right after, and add `--canary caught` or `--canary missed` to that answer's `considered` entry. When an advisor answers, the canary tests the advisor's confidence; record it the same way on the advisor's entry.
- A missed canary is information. Say so once, and proceed without chasing it or re-asking the probes.

## When the answer comes

- Record what was said with `adr consider N --said "..." --via "..."`, and `--covers NAME...` for the probes the words settled. The tool refuses a probe name the Summary does not have.
- If the operator approves or says to accept before the probes were asked, record their words with no `covers`, do what they said, and put the probes to them in the same reply. Do not wait for the answer.
- Take a short yes as given. A minimal answer, or none, is recorded as said, and you continue without re-asking.
- A later answer is another `considered` entry. A "no" is corrected by appending: supersede the record or write a new one.
- Accept when the work calls for it. A decision with no operator basis may be accepted directly.

## When no one is present

A run under `/goal`, or any unattended run, completes the consider step with no human reply.

- The goal condition is the operator's stated intent. Cite it as the `operator` basis at `level: directed`: `said` quotes the condition verbatim, `via` names the goal and the session.
- You may put the probes to an advisor: a second model, or the `skeptic` subagent. Give the advisor the goal condition or the operator's words, and ask whether the decision serves that intent.
- Record the advisor's answer as a `considered` entry attributed to the advisor: `adr consider N --operator advisor --said "..." --via "<which advisor and model, and the run>"`. Pass `--operator advisor` every time, since the default is the git user.
- That entry is a complete record of an autonomous run. Nothing flags it or asks for a human answer later.

## Reviewing past decisions with the operator

When the operator asks to review the decisions from recent sessions:

- List the records. `adr list --json` gives each record's `date` and `considered` entries. `git log --since=<date> --name-only -- docs/architecture` finds records changed in that span, including ones dated earlier.
- Walk through each one in plain words from its Summary: what it decided and what it trades. Say whether it was considered by the operator, by an advisor, or not at all.
- Record each reaction as a `considered` entry in the operator's name, with `via` naming the review. A flag becomes a `concern:` entry. When they want the decision changed, supersede it or write a new record.

## Asking in plain words

Field names such as `basis`, `considered` and `level` belong in the record. Ask the way a teammate would, with enough context that the question can be answered without opening the file: a sentence on what the record decided and what it trades, then the question. Write each ask fresh from the record in front of you. Don't ask for a level. Work it out from who made the call.

## Raising a concern

- You may raise a concern at any stage, including after acceptance: a safety issue, reasoning that does not follow, or anything that seems off.
- Keep concerns few and actionable. Each names what would resolve it; batch minor points into one.
- Challenge once. If the operator still says go, proceed and do your best.
- A concern goes in the record as a `concern:` entry. It does not block acceptance. It is answered, or withdrawn with a stated reason, and never deleted.

## See Also

- adr(documentation) — the record format and commands for this project
- trust/autonomy(meta) — when to act without asking
