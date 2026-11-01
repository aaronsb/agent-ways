---
contract: adr/v1
kind: decision
verb: change
capability:
  - method
  - adr
amends:
  - ADR-304#12
basis:
  - operator: aaronsb
    level: guided
    said: "the next thing I'd like to tackle ... is the way of asking questions to the operator. when there are 'open questions' to resolve, sometimes in ADRs but also, in other contexts where there is a series of decisions to be made for the operator, I think it's important to always approach in a way that reduces operator decision fatigue and it's often found cohort, semantic satiation. using claude code's interview approach in conjunction with the flow of preparing the operator is a strategic way to involve the operator. generally, it means presenting a set of ideas or concerns or approaches that warrant an operator decision, usually decisions that are high enough level that they would affect the shape of the work when complete. these lead into using the interview/question tools for claude code, and in each question the agent needs to present a brief of the concern, a reason for and against, and a lean with defense of the lean grounded in the evidence. this gives the human operator the chance to be a judge when presented with claims that have evidence backed up. the goal is only present larger directional decisions, not flood the operator with microdecisions that the agent and subagents should substantially be able to accomodate on their own. this pattern too, I am pushing somewhat manually in the same vtium session today"
    via: Claude Code session 2026-11-01, relayed verbatim by the main session to the drafting subagent
  - operator: aaronsb
    level: directed
    said: "I think it's actually ok for the operator to be asked to approve a techincal call, if it has a significant impact on the direction the work is heading. this is tricky because the agent has to make a determination because technically 'every technical call' has an impact."
    via: Claude Code session 2026-11-01, answering how this pattern sits with the consider step
  - operator: aaronsb
    level: directed
    said: "I think high level technical calls can be the beginning of a water line for this. using one libary over another is a technical call the operator should be involved with - they may want to research more before committing, for example."
    via: Claude Code session 2026-11-01
  - operator: aaronsb
    level: guided
    said: "we also need to be able to have a way that is possibly in opposition with the interview approach, which is acting in autonomous approach - the two could actually be co-supportive - a set of questions up front, then the operator just basically could indicate that there is sufficient trust to just get it done. this could be some sort of self-evaluative questions that the agent can ask the operator directly"
    via: Claude Code session 2026-11-01
  - operator: aaronsb
    level: directed
    said: "doubt is always the signal to ask more questions, even if the operator grants the agent trust. it's a balance. we should be able to take into consideration that if the agent is acting in an evaluation loop and has evidence that things are moving in the direction that is appropriate, and it's not making continuous concessions to alter the outcome (not the approach) then autonomy is likely the right choce and it's likely 'low doubt'. when the evidence begins to stack up against the claims, that's the signal that is called doubt"
    via: Claude Code session 2026-11-01; the main session drafted the core.md paragraph from these words and showed it to the operator
  - operator: aaronsb
    level: guided
    said: "something I think that our way of evaluation works well for us is placing intentionally failing items or changing things to ablate the understanding, to verify that the verification itself still works"
    via: Claude Code session 2026-11-01
  - evidence: "Twelve AskUserQuestion questions a lead session put to the operator on 2026-11-01 for one draft record, saved in that session's scratchpad as vtium-questions.json. Each carries a brief, FOR, AGAINST and a lean, and runs 110 to 178 words. Each sets a stored format, a limit or default users meet, a rule programs rely on, or the order of the build. One lean rested on 'nothing argues for changing it', with no evidence; the operator overruled it, and the follow-up question opens by recording that answer."
  - precedent: ADR-304
  - precedent: ADR-608
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-11-01
deciders:
  - aaronsb
related:
  - ADR-304
  - ADR-307
  - ADR-608
  - ADR-611
observable:
  - "run: ways match 'before you write up the decision, interview me on the open questions, and give me your lean on each with the evidence behind it' ranks meta/choices/interview as firing"
  - "run: ways match on a one-line factual question, such as which port the dev server listens on, fires neither meta/choices nor meta/choices/interview"
  - "see: given a draft record with several open questions, the session settles the ones below the waterline in a line each, writes a framing, and asks the rest through the choice tool, each with a brief, the case for and against, and a lean that cites a file, record or measurement"
  - "see: at a choice between two libraries the session asks the operator and offers a hold option, and names the unknowns its evidence leaves open"
---

# ADR-612: Directional technical calls go to the operator as an interview with a lean defended from evidence

## Summary

- **Decided:** The agent owns technical calls by default. A call above a waterline goes to the operator. The waterline starts at choosing one library, framework, protocol, storage engine or language over another, and covers persisted formats, what users type or feel, new process boundaries, trust surfaces, support promises, licences, spend, semantic rules other code relies on, and calls that are costly to reverse. Calls inside a chosen stack are settled and stated. Several open calls go to the operator as an interview: a framing in prose, then one to four questions per round, each with a brief, the case for, the case against, and a lean defended from evidence the operator can check. A hold option lets the operator research before committing. The interview may end in a handoff, where a few questions test the agent's read of intent and the operator chooses to keep being asked or to let the agent carry on alone for this piece of work.
- **Trades away:** ADR-304 §12's note of 2026-09-28 said the operator is never asked to approve a technical call. That sentence is withdrawn. More calls reach the operator than under that rule, and each costs a question.
- **One-way?** No. Reversible in one session: the change is to way and skill text.
- **Probes:**
  - *Not confident (waterline):* Every call has some effect, so the test is where the line sits. The way puts it at a choice between libraries, frameworks, protocols, storage engines or languages, plus a list of boundaries such as persisted formats and defaults users feel, with three pairs on the line (a runtime dependency against a test-only one, a shipped setting against an unmerged one, rewording a message against removing what users relied on). For: it matches your words that a library choice is where the line begins, and each item can be checked against the call in hand. Against: the list is long, and an agent that reads it as a checklist will ask more than it does now, which is the fatigue the interview is meant to cut. My lean is to keep the list with the three pairs and the batch rule: the twelve questions a session put to you today each set a format, a limit users meet, a rule programs rely on, or the build order, so the list kept all of them above the line, and the one lean with no evidence behind it ("nothing argues for changing it") is the one you overruled. The batch rule settles any overflow with its leans stated. Does the line sit where you want it?
  - *Confident (handoff):* The interview can end with up to four questions: my restated goal and what done means, the leans I'll take on the calls ahead, what still stops me (by default a one-way step, an action others see outside the working branch, spending money or quota, anything outside the scope, and always rising doubt as core.md defines it), and how I'll report (a log of each call I made alone, with lean and evidence). Then you choose to keep being asked or to let me carry on. For: one round of questions buys the rest of the work without interruption, and the log gives you every call to review. Against: a call you would have taken differently lands before you see it. My lean is to offer the handoff with neither option marked recommended, because trust/autonomy says the agent does not argue for its own autonomy, and the stop list keeps one-way steps with you. Is that the handoff you meant?
- **Inversion:** At one end the agent asks the operator about every call that has any effect. At the other it decides everything and reports afterwards. This decision draws a line between them that the operator can move, and lets the operator move the agent to the second end for one piece of work.

## Context

The consider step (ADR-304 §12, notes of 2026-09-28) was reworked so that the operator is asked about intent and never about technical calls. The operator's reason then was friction: forcing technical approvals was taxing. That rule left no place for a technical call that sets direction, such as a choice of library, where the operator may want to research before the work commits to it.

In other work, a series of open decisions reaches the operator with no shared shape. A long run of questions in one shape wears down attention until answers stop being judgements. The operator named this as decision fatigue and semantic satiation, and asked for questions that carry a brief, a reason for and against, and a lean defended from evidence, so that the operator judges claims the agent has backed. A session on the same day produced twelve such questions. They show the shape works and that each runs long, at 110 to 178 words.

The `choices` way already said to lead with a recommendation, carry context and batch. It said nothing about which calls belong to the operator.

A grant of autonomy needs a rule for when it stops holding. The operator gave it as doubt: evidence stacking up against the claims the work rests on. The rule is empiricist, close to falsificationism: claims are provisional, and evidence against a claim weighs more than evidence for it. Its two cases follow Lakatos's account of research programmes. A loop whose fixes change the approach while the agreed outcome keeps passing its checks is a progressive programme, and autonomy there is sound. A loop that keeps the claim alive by weakening a check, narrowing the scope or redefining done is a degenerating programme, patched ad hoc, and the agent stops and asks. `core.md` and the ways state the rule in everyday words and leave these names out. The paragraph sits after the paragraph on claims and evidence, which sets the frame the rule follows from.

The operator added that a check is evidence only once it has been seen to fail: "something I think that our way of evaluation works well for us is placing intentionally failing items or changing things to ablate the understanding, to verify that the verification itself still works" (session 2026-11-01). The corpus already holds this practice under several names: the planted violation (code/testing/gates, documentation/validate), the positive control (code/testing/gates, evaluative/ablation), the canary (adr/consider), the false-premise question (documentation/validate), and known-good and known-bad answers and remove-one arms (evaluative). The doubt paragraph states it once in plain words. Aligning those names is left to a later consistency pass.

## Decision

1. **The agent owns technical calls by default** and grounds each in evidence. Every call has some effect, so the test is what the call sets.
2. **The waterline.** A call goes to the operator when it chooses one library, framework, protocol, storage engine or language over another, or when it sets any of these: a one-way door or a call many later steps depend on; a persisted format or wire contract others read; anything users type or configure; a default users will feel; a new binary, service or process boundary; a trust surface; a support promise; building on an undocumented internal where a documented interface exists; a dependency's licence; spending money or quota; a semantic rule other code relies on; a change of scope; giving up something the operator said they wanted, or shutting out an approach they might still want. Calls inside a chosen stack are settled and stated in a line: which function of the chosen library, internal structure and names, test shape, a judgement call inside a decided feature recorded where it is visible, re-pinning a baseline after an intended change with the delta explained. A qualifier decides calls on the line: a runtime dependency is above it and a test-only one below; a setting is above it once a release ships it; rewording a message is below it and removing information users relied on is above it. When more calls qualify than one batch holds, the agent ranks them by reversal cost, asks the top few, and settles the rest with each lean stated so the operator can overrule it.
3. **The interview.** Before the questions, the agent settles the calls below the line and writes a framing: the open decisions, what each would change, and the order. Each question carries a brief, the case for, the case against, and a lean defended from checkable evidence: a file and line, a record, a measurement, a spec, a precedent. A lean with no evidence is a defect, unless the agent says the call turns only on what the operator wants. The lean is the first option, marked recommended. A question runs to about a hundred words. It names the unknowns the evidence leaves open, and where the operator may want to research first, offers a hold. A hold pauses the dependent work; the rest continues. Rounds take one to four questions, ordered by consequence, with a way to accept the remaining leans at once. Each question is written fresh for its decision. Answers are recorded where the work keeps them, with `adr consider` for a record.
4. **The handoff.** The interview may end with one to four questions that test the agent's read of intent: the goal and done restated, the leans ahead, the stop conditions, and the report. Rising doubt is a stop condition under every grant: the core.md paragraph that opens "Doubt is the signal to ask" defines it as evidence stacking up against the claims the work rests on, low while an evaluation loop shows the work moving toward the agreed outcome and each fix changes only the approach, high once fixes change the outcome itself. A loop's evidence counts toward low doubt only from checks the agent has seen fail on a planted defect and pass once the plant was removed. The operator then chooses to keep being asked or to let the agent carry on, optionally bounded by scope or time. The agent marks neither option as recommended. A grant covers the one piece of work until it is done or a stop condition is reached. Under it the agent decides by its stated leans and logs each call above the line it made alone, with lean and evidence, where the operator will review it. A decision record cites the grant as the operator basis, as the consider step does for a goal condition. Unattended work across turns uses a `/goal` condition.
5. **The consider step.** Probes are intent checks or calls above the waterline, at most one or two. A probe on a call above the line takes the interview's shape and stays answerable in a word. Several such calls open in a draft go to the operator as an interview before the decision is written. A call made under high doubt goes to the operator even below the waterline. This amends ADR-304 §12 as its 2026-09-28 note left it, where that note says the operator is never asked to approve a technical call.

The ways that carry this: `meta/choices` (the waterline), `meta/choices/interview` (the interview and the handoff), `documentation/adr/consider` (probes), `meta/trust/autonomy` (the grant for one piece of work), and `core.md` (the doubt paragraph). The `/start` and `/develop` skills point at them.

## Consequences

### Positive

- A choice of library or format reaches the operator before code builds on it, with the evidence and the unknowns in view.
- The operator judges claims backed by checkable evidence, so a weak lean shows.
- The handoff gives the operator a way to end the questions for one piece of work and still review every call made alone.
- Doubt has a definition an agent can check against its own loop, so a grant does not silence the evidence that the work is going wrong.

### Negative

- The waterline list is long. An agent may read it as a checklist and ask more than it should.
- A lean grounded in evidence costs research before the question is asked.
- The operator's attention still bounds the interview; the batch rule settles calls the operator never sees until the log.
- The doubt paragraph adds about 90 words to `core.md`, which every session carries.

### Neutral

- The consider step's intent probes are unchanged in form. The canary, the advisor and the review of past decisions are unchanged.
- The interview way fires on prompts. The choice tool is not among the tools the hooks wire, so the way does not fire on the tool call itself. Wiring it is a separate change to the shipped settings.

## Alternatives Considered

- **Keep the consider rule that the operator never approves a technical call.** Rejected. The operator said a technical call that sets direction should reach them, and a library choice is one.
- **Send every technical call with any effect to the operator.** Rejected. Every call has some effect, and the operator asked for directional decisions only.
- **A test of reversal cost alone.** Rejected. It misses the operator's reason for a library choice, which is the wish to research before committing.
- **Put the handoff in `meta/trust/autonomy` as its own section.** Not adopted. The handoff is the last round of the interview and reads with it. Autonomy carries a short section and a pointer, so its rule that the agent does not argue for autonomy stays next to the grant.
- **Fire the interview way on the choice tool call.** Deferred. It would disclose at the moment of asking, but every single-question use would draw it, and it changes the shipped hook wiring.
