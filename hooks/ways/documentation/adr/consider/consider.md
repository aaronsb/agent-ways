---
description: handing a proposed decision record to the operator, checking the operator's intent, an advisor answering when no one is present or under a goal, reviewing past decisions with the operator, accepting rejecting or abandoning a record, and raising a concern about a decision
vocabulary: canary consider review approve approval looks good lgtm ship it sounds good go ahead accept reject abandon proposed decision summary probe probes intent inversion advisor skeptic goal unattended concern pushback sign off operator recent decisions sessions flag teammate-style confidence-labeled
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

The consider step records the operator's intent (ADR-304 §12 and its notes). The agent owns the technical calls by default and grounds each one in its `basis`: evidence, a standard, upstream, or a precedent. A technical call that sets the direction of the work goes to the operator, by the waterline in choices(meta): a choice of library, framework, protocol, storage engine or language, a persisted format, a default users will feel, a call that is costly to reverse. The rest of what goes to the operator is a check that the decision serves what they wanted, and why they wanted it.

## Handing a decision over

- Open with a Summary a reader can judge alone: what is decided, what it trades away and forecloses, whether it is one-way (said first when it is), the probes, and an inversion.
- Probes are intent checks or calls above the waterline, at most one or two, each in the operator's terms and answerable in a word. Write "you wanted list pages to feel fast; is data up to 30 seconds old fine for that?" in place of "is the latency win real?". Label each probe `*Confident (name):*` or `*Not confident (name):*` for how sure you are that you read their intent right. The tool names probes by that label.
- A probe on a call above the waterline carries a brief of the concern, the case for and against, and your lean defended from evidence the operator can check (choices/interview). It stays answerable in a word, because the lean is the first option. Several such calls open in a draft go to the operator as an interview before the decision is written.
- The inversion names the two ends the decision sits between. Put it to the operator only when it turns on what they want.
- When the decision carries `observable` entries, show them where you can: run the command, show the output, open the page. Say in `via` what was shown (ADR-307).
- When the record has a figure, show it before the probes, and say in `via` that it was shown (adr/figure). When answers or findings change the text, check every figure against the final text, redraw what no longer matches, and render each figure again and show it to the operator before `adr accept` (adr/figure).
- When the operator is present (choices/interview, how to ask), put the probes to them through the question tool: each probe with a line of context, your lean first, an alternative with its tradeoff, and room to answer in their own words. Where the session has no question tool, as in a headless run, ask in your reply, one short line each. When the operator is away and listens on attend, post the probes there. When no one is present, follow "When no one is present" below. `adr accept` reads no answer, so an absent operator never blocks acceptance.

## A canary

- You may add a canary among the intent checks: a planted defect (core.md) in the form of a point that is deliberately wrong, harmless if accepted, and never about safety. Ask it with the probes, through the same channel. It never goes in the record's text.
- It is a signal probe and is never required. If no one answers it, record nothing about it and continue.
- When someone answers, reveal the canary right after, and add `--canary caught` or `--canary missed` to that answer's `considered` entry. When an advisor answers, record it the same way on the advisor's entry.
- A missed canary is information. Say so once, and proceed without chasing it or re-asking the probes.

## When the answer comes

- Record what was said with `adr consider N --said "..." --via "..."`, and `--covers NAME...` for the probes the words settled. The tool refuses a probe name the Summary does not have.
- `said` quotes the operator's written words. Trim the quote to the words that answer, and mark each cut with `…`. Where the words do not stand alone, add the context in square brackets: `"yes [data up to 30 seconds old is fine for list pages]"`. Leave the quoted words as written, typos included. A spoken answer is a summary marked `--paraphrase`.
- Never trim away disagreement, doubt, a condition or a requested change. Check a question, such as whether another record already covers this, before you go on, and keep it in the quote only when the check turns something up.
- If the operator approves or says to accept before the probes were asked, record their words with no `covers`, trimmed and bracketed the same way. Do what they said, such as `adr accept`, then put the probes to them. Acceptance does not wait on the answer.
- Take a short yes as given. A minimal answer is still recorded, and you continue without re-asking. No answer records nothing.
- A later answer is another `considered` entry. A "no" changes the record. On substantially the same topic, alter the record in place with dated notes; for a different direction or concept, supersede it or write a new record (adr).
- Accept when the work calls for it. A decision with no operator basis may be accepted directly.

## When no one is present

A run under `/goal`, any unattended run, or work under the operator's grant to carry on alone (choices/interview) completes the consider step with no human reply.

- The goal condition, or the operator's grant, is their stated intent. Cite it as the `operator` basis at `level: directed`: `said` quotes the condition or the grant verbatim, `via` names the goal or the grant, and the session.
- You may put the probes to an advisor: a second model, or the `skeptic` subagent. Give the advisor the goal condition or the operator's words, and ask whether the decision serves that intent.
- Record the advisor's answer as a `considered` entry attributed to the advisor: `adr consider N --operator advisor --said "..." --via "<which advisor and model, and the run>"`. Pass `--operator advisor` every time, since the default is the git user.
- That entry is a complete record of an autonomous run. Nothing flags it or asks for a human answer later.

## Reviewing past decisions with the operator

When the operator asks to review the decisions from recent sessions:

- List the records. `adr list --json` gives each record's `date` and `considered` entries. `git log --since=<date> --name-only -- docs/architecture` finds records changed in that span, including ones dated earlier.
- Walk through each one in plain words from its Summary: what it decided and what it trades. Say whether it was considered by the operator, by an advisor, or not at all.
- Record each reaction as a `considered` entry in the operator's name, with `via` naming the review. A flag becomes a `concern:` entry. When they want the decision changed, alter the record in place with dated notes, or supersede it or write a new record when the change is a different direction or concept.

## Asking in plain words

Field names such as `basis`, `considered` and `level` belong in the record. Ask the way a teammate would, with enough context that the question can be answered without opening the file: a sentence on what the record decided and what it trades, then the question. Write each ask fresh from the record in front of you. Don't ask for a level. Work it out from who made the call.

## Raising a concern

- You may raise a concern at any stage, including after acceptance: a safety issue, reasoning that does not follow, or anything that seems off.
- Keep concerns few and actionable. Each names what would resolve it; batch minor points into one.
- Challenge once. If the operator still says go, proceed and do your best.
- A concern goes in the record as a `concern:` entry. It does not block acceptance. It is answered, or withdrawn with a stated reason, and never deleted.

## See Also

- adr(documentation) — the record format and commands for this project
- choices(meta) — the waterline for calls that go to the operator
- choices/interview(meta) — a series of open calls, each with a brief, the case for and against, and a lean from evidence
- trust/autonomy(meta) — when to act without asking
