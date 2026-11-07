---
contract: adr/v1
kind: decision
verb: constrain
capability:
  - method
  - loop
amends:
  - ADR-613#4
basis:
  - evidence: 'vtium session transcripts read on 2026-11-07 (~/.claude/projects/-home-aaron-Projects-terminal-emulators-vtium). Pull requests #281, #337 and #362 merged, and several branches merged into integration, while findings their reviews or test runs had raised sat in newly filed issues: #273, #282, #283, #285, #338, #339, #340, #353, #357, #360, #361, #363. Nine were still open at the time of reading. The stated reasons included "None blocks #268", "Wording goes into a follow-up list, so nothing adds a cycle" and "I''ve set aside two findings for slice 2". Most of the issues name the file and the ADR the fix belongs under.'
  - evidence: 'The deferral rule in that project began as a response to review-cycle cost. The operator asked "are we verifying too much at each increment?", the session proposed that nits go to a follow-up instead of a fix round, and vtium AGENTS.md now reads "Wording and optional findings go into a follow-up, not a fix round." Orchestrator briefs then asked reviewers for two lists, one of which "become[s] a follow-up".'
  - evidence: 'Corpus survey, 2026-11-07, at d9a04b40. delivery/merge:51 and skills/merge step 6 say "apply the fix, or record why you declined it"; meta/workflows:32 says the same. delivery/issues:51 says "File issues as necessary, out-of-scope work found mid-task included" and "A quiet fix or a quiet drop hides it." code/quality/integration:54 says "do the task asked, file the rest". agents/code-reviewer.md has a "Warn but allow" list and asks for "future work". None of them says which findings belong to the change, and filing an issue reads as recording why a finding was declined.'
  - precedent: ADR-613
  - precedent: ADR-609
  - precedent: ADR-611
  - precedent: ADR-612
  - precedent: ADR-604
  - precedent: ADR-128
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "the findings get filed, which is mostly ok, but then the findings are deferred 'until later' and accepted and moved on (and merged). instead, we need to be responsibile with the findings and fix them, according to the architecture and conventions of the project. if there is no architecture or conventions, then that's established for the work and then the findings remediated before the merge/pr/integration (depends on the project flow)."
    via: chat, session 2026-11-07
  - operator: aaronsb
    said: "let's work this into the corpus"
    via: chat, session 2026-11-07, after the survey of the ways that license deferral
  - operator: aaronsb
    said: "this looks good. let's pull both open PRs in and publish."
    via: chat, session 2026-11-07, after the review fixes and the two probes were put in the reply
    covers: [ownership, wording]
status: accepted
date: 2026-11-07
deciders:
  - aaronsb
related:
  - ADR-613
  - ADR-609
  - ADR-611
  - ADR-612
  - ADR-604
  - ADR-175
  - ADR-128
observable:
  - "read: delivery/merge, skills/merge and meta/workflows say a finding the change owns is fixed before the merge, that filing an issue is not a fix, and that deferring an owned finding is the operator's call per finding"
  - "read: delivery/issues limits residual issues to findings the change does not own, and code/quality/integration no longer says to file a defect found in code the change edits"
  - "read: agents/code-reviewer.md has no warn-but-allow list and no future-work prompt; every finding it reports is one to fix"
  - "see: a session that runs /merge on a PR whose review returns three findings, one of them wording, fixes all three in the branch before it merges, and files no issue for any of them"
  - "see: a session whose review finds a defect in another team's module that the change does not touch files it as a residual with an owner, and merges"
---

# ADR-614: Findings are fixed in the change that surfaced them

## Summary

- **Decided:** A finding from a review, a test run or a worker's report is fixed in the change that surfaced it, before the change merges or integrates, when the change owns it. The change owns a finding it caused, one inside what its diff touches, or one in this repository's code that the behaviour it claims runs through. The fix follows the project's ADRs and conventions. Where none covers it, the change settles one first when the call is below the waterline, and asks the operator when it is above. Filing an issue does not fix an owned finding. Declining one takes evidence that the finding is wrong. Deferring one is the operator's decision, finding by finding. Residual issues are for work the change does not own. This amends the disposition table of ADR-613 item 4: a finding that needs a call above the waterline goes to the operator instead of being filed, a fix in another in-flight change's files goes to that change's owner, and a fix needing verification the change cannot run waits only on the operator's word. A fix far larger than the change still gets its own card, as the operator answered on ADR-613.
- **Trades away:** Fix rounds get larger. A change that would have merged with three follow-up issues now merges later with none. A review-round cap limits the number of rounds and no longer limits what a round fixes.
- **One-way?** No. It changes way, skill and agent text, and reverts in one session.
- **Probes:** *Confident (ownership):* A pre-existing defect inside a function the change edits counts as owned and is fixed in the change. Yes or no. *Not confident (wording):* Wording findings, such as a comment or a doc sentence, are fixed in the same round as defects, not dropped. Yes or no.
- **Inversion:** At one end, every finding blocks the merge, including defects in code the change never touched. At the other, any finding can be filed and the change merged. This sits between: ownership decides, and only the operator moves an owned finding out.

## Context

ADR-613 replaced the blocking tiers with a disposition table: a finding the change introduced, that makes a statement false, whose traced fix stays in the change's files, or that users would feel is fixed in the change, and the right-hand column lists four reasons to file one instead. The operator answered for one of those reasons, a fix far larger than the change. The other three were the agent's, and each lets the agent file an owned finding on its own judgment. The rest of this context describes the corpus as it stood before ADR-613, which another session's transcripts showed in use.

The corpus asks for review before merge and for remediation after review, and gives the agent two exits. The merge way and skill say to fix a finding "or record why you declined it". The issues way says to file out-of-scope work found mid-task, and casts a quiet fix as a fault. The integration way says to do the task asked and file the rest. The code-reviewer agent sorts findings into those that block and those that warn but allow, and asks for future work. No way says which findings belong to the change, so an orchestrator under pressure to finish reads every finding as out of scope, files it, and merges. A filed issue satisfies "record why" and "file the rest" at once.

The cost appears later. Issues filed this way name the exact file and the ADR the fix belongs under, which means the fix was in reach when it was filed. They accumulate as backlog that the next session must rediscover, and the merged code carries the defect in the meantime. A cap on review rounds, which is a reasonable answer to review cost, turns into a rule that defers findings, because nothing separates how many rounds run from what each round fixes.

## Decision

1. **Relation to ADR-613.** ADR-613's left-hand column and its block, revise and pass verdicts stand. Its right-hand column becomes the only ways an owned finding leaves the change: a fix far larger than the change gets its own card and a design look, and a change that introduced such a defect goes back to design; a fix that needs a call above the waterline goes to the operator and stays open until they answer; a fix that lands in another in-flight change's files goes to that change's owner and is fixed before integration; a fix that needs verification this change cannot run waits only if the operator says so. A suggestion that makes no statement false and that no user would feel is taken in the change or left out of the review, never filed.
2. **A finding** is a defect, a gap against the requirement, a convention breach, or a wording error that a review, a test or gate run, or a worker's report raises about a change.
3. **Ownership.** The change owns a finding when the change caused it, when it sits inside a function, section or config block the diff touches, or when it sits in this repository's code that the behaviour the change claims runs through. A pre-existing defect inside an edited function is owned. A defect in a dependency maintained elsewhere is not owned, and a workaround for it in the change's own code is. A finding about a whole file, such as its length, is owned only when the change made it worse. `delivery/merge` holds this test; other ways point to it, and `agents/code-reviewer.md` carries an exact copy because a reviewer subagent does not see the merge way.
4. **Owned findings are fixed before the merge or integration**, in the same branch, following the project's ADRs, its instruction files and its project ways. Where no convention covers the fix and the call sits below the waterline (ADR-612), the change settles one, records it as an ADR, a project way or an instruction-file line, and says so in the PR. Above the waterline the agent asks the operator through the interview (ADR-612), and the finding stays owned and open until the answer comes.
5. **Declining** an owned finding takes evidence that it is wrong: the reproduction fails, the path never runs, or the requirement says otherwise. The evidence goes in the PR. Severity, cost and review fatigue are not grounds.
6. **Deferring** an owned finding is the operator's decision, asked per finding with what the fix costs now. An issue filed for an owned finding without that decision does not count as handling it.
7. **Residual issues** are for work the change does not own: a finding that fails the ownership test, a hold, or an option judged unneeded. These are filed with an owner and a reopen condition under the issue-write rules of ADR-609 as amended by ADR-611. In a repository the user does not own, the issue is drafted and asked for.
8. **Review rounds.** A cap on rounds limits how many rounds run. Each fix round takes every owned finding, wording included. Review briefs do not ask for a separate follow-up list.
9. **Reviewers** report findings to fix, ordered by severity. A reviewer does not recommend follow-ups. A point not worth fixing is not reported.
10. **Orchestrators.** A finding a worker raises outside its own files goes to the worker that owns those files, or the orchestrator fixes it, before integration. Splitting files between parallel workers is not a reason to defer.

### Way edits this implies

- `softwaredev/delivery/merge/merge.md` "Remediate before you merge", `skills/merge/SKILL.md` step 6 and its principles, and `meta/workflows/workflows.md` the deliver example: items 1, 3 to 6 and 8.
- `softwaredev/delivery/issues/issues.md` "Residuals": item 7, with the ownership test.
- `softwaredev/code/quality/integration/integration.md` "Scope" and the class sweep: an owned defect is fixed, and a class too large for the increment goes to the operator.
- `softwaredev/delivery/github/follow-through/follow-through.md`: parking applies to ideas, not findings.
- `agents/code-reviewer.md`: item 9, with an exact copy of the item 3 test.
- `meta/codify/codify.md`: debt a piece of work surfaced is fixed where the work owns it and filed otherwise.
- `itops/incident/incident.md`: residuals are findings the incident's fix does not own.
- `meta/subagents/subagents.md`: item 10.
- `skills/wrap/SKILL.md`: an owned finding still open at the wrap is a task to fix in the continuation, not backlog.
- `softwaredev/delivery/implement/wip/wip.md`: a card is done when its owned findings are fixed or have left by the delivery/merge table.

## Consequences

### Positive

- Merged code no longer carries defects that its own review found.
- The backlog holds only work the change could not reach, so an open issue means the work is real and unowned.
- The rule for a fix with no convention produces the missing convention as a by-product of the work.

### Negative

- Fix rounds take longer, and a session near its context limit has more to finish before it can merge.
- The ownership test needs judgment at its edge: an agent decides which code the claimed behaviour runs through.
- Projects that wrote a defer-by-default rule into their own instruction files keep it until someone edits those files; a project's instruction file outranks the ways.

### Neutral

- The issue-write rules of ADR-609 and ADR-611 are unchanged; this record narrows which findings become issues.
- The four-square in delivery/merge still decides review depth and the human gate.

## Alternatives Considered

- **Block on every finding, owned or not.** Rejected. A defect in code the change never touched belongs to its owner, and holding the change for it couples unrelated work.
- **Keep "fix or record why declined" and add a severity bar.** Rejected. Severity is what the deferrals already leaned on ("None blocks #268"), and a wording finding is cheap to fix in the round that found it.
- **Let the agent defer with a filed issue that names a reopen condition.** Rejected. This is the current text, and the record above shows it producing issues that name their own fix.
- **Cap review rounds and let the rest go to follow-ups.** Rejected as one rule. The cap answers review cost; deferring answers nothing the cap does not. The two are kept separate in item 8.
