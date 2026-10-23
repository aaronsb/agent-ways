---
contract: adr/v1
kind: decision
verb: change
capability:
  - method
  - loop
amends:
  - ADR-609#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "since the much simplified issue assignment direction we landed on is a github concern like this one, I think it's ok for pushes, PRs, reviews of those PRs, and additional pushes to the branch to accomodate review outcomes and even amending or editing the pr to happen when necessary without an explicit grant. it's the branch merge to a consceuqential branch that needs a grant stated, either in claude.md or by the user for the session or a goal."
    via: Claude Code session, 2026-10-23
  - operator: aaronsb
    level: directed
    said: "Yes, all issue writes"
    via: Claude Code session 2026-10-23, choice tool answer on whether issue writes need no grant
  - operator: aaronsb
    level: guided
    said: "own pr branch only, but that is contingent on when adr 609 was written"
    via: Claude Code session 2026-10-23, choice tool answer on force pushes to the agent's own pull request branch
  - evidence: 'Claude Code 2.1.309 binary, read with strings on 2026-10-23. The lean Bash text is unchanged from the 2.1.306 text ADR-609 cites: "Commit or push only when the user asks. If on the default branch, branch first." A mode-conditional block, not in the default interactive prompt, reads: "Commit, push, and create or update pull requests without asking whenever that is the natural next step of the task. This overrides the general instructions to only commit, push, or open a pull request when the user explicitly asks. Still review what you are staging, keep secrets out of commits, and never force-push or rewrite shared history unprompted." The same block says: "Editing code, choosing an approach, pushing, and landing a PR once it is approved can all be reverted, so proceed. Ask first only before a one-way door".'
  - precedent: ADR-609
  - precedent: ADR-601
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-23
deciders:
  - aaronsb
related:
  - ADR-609
  - ADR-601
  - ADR-180
---

# ADR-611: Working-branch GitHub writes need no grant, and merging into a consequential branch does

## Summary

- **Decided:** In a repository the user owns, writes that stay on a working branch or in the issue tracker go ahead without an ask or a grant when they are the natural next step of the work: commits on a non-default branch, pushing that branch, opening, editing, reviewing and commenting on its pull request, pushing review fixes to it, and every issue write. Merging into a consequential branch (the default branch, a release branch, or a protected branch) needs the user's ask, a goal condition, or a CLAUDE.md grant, as ADR-609 item 3 sets out. Force pushes, history rewrites, release tags and writes to repositories the user does not own stay asked for each time.
- **Trades away:** Pushes and pull requests appear without a per-write yes, so collaborators and CI see work earlier and more often. A user who wants every push asked for has to say so in their CLAUDE.md.
- **One-way?** No. Reversible: way text and this record, both changed in one session.
- **Probes:** *Confident (merge line):* Working-branch pushes, pull requests and issue writes go ahead without asking, and only merging into main, a release branch or a protected branch waits for your word. Is that the line? *Not confident (force push):* Claude Code's current text still says "never force-push or rewrite shared history unprompted", so a force push to the agent's own pull request branch stays asked for each time. Does that settle your condition?
- **Inversion:** At one end, every outward write waits for the user's word. At the other, every write including merges proceeds when it is the next step. This sits between them, at the branch the rest of the project builds on.

## Context

ADR-609 put every commit, push, pull request write and issue write behind the user's ask, a goal condition or a CLAUDE.md grant, citing the Claude Code 2.1.306 texts. In practice the gate fires on writes that are easy to undo: a pushed working branch can be deleted, a pull request closed or edited, an issue relabelled or reopened. What other people build on is the consequential branch, and the write that changes it is the merge. Claude Code 2.1.309 carries a mode-conditional text that draws the same line, proceeding on pushes and pull requests and holding force pushes and shared history. The operator asked for this line in the corpus.

## Decision

1. **Working writes need no grant.** In a repository the user owns, these go ahead when they are the natural next step of the work: commits on a non-default branch; pushing that branch; opening, editing, reviewing and commenting on its pull request; pushing commits that answer review; and filing, assigning, labelling, commenting on and closing issues.
2. **Merging into a consequential branch needs a source.** The default branch, a release branch and any branch with protection rules are consequential. A merge into one needs the user's ask in the conversation, a goal condition that names it, or a CLAUDE.md grant, as ADR-609 item 3 states. Without one, the agent offers the merge in one line, as ADR-609 item 5 states.
3. **ADR-609 item 7 stands.** Force pushes, history rewrites, release tags and writes to repositories the user does not own are asked for each time. This includes a force push to the agent's own pull request branch, because Claude Code's text still says "never force-push or rewrite shared history unprompted".
4. **A CLAUDE.md line can narrow this.** A user who wants pushes or issue writes asked for each time says so in their CLAUDE.md, and that line wins over the way.
5. **ADR-609 items 1, 5 and 6 change accordingly.** Item 1's scope of gated writes shrinks to merges into consequential branches plus item 7's list. Item 5's offer applies to those. Item 6's suggested lines reduce to the merge line and a narrowing example.

## Consequences

### Positive

- A branch, its pull request, review rounds and issue upkeep run without a stop for permission at each write.
- The gate sits on the merge, the write other people build on.

### Negative

- The interactive Bash text still says "Commit or push only when the user asks". The way's text sits against it, and a session may still ask out of the harness's caution.
- More pushes mean more CI runs. A project with an expensive pipeline, such as this one's four-platform builds, keeps its own cadence in its CLAUDE.md or memory.

### Neutral

- Way edits: `meta/trust/delegation` (the git row and Standing Grants), `meta/codify`, `softwaredev/delivery/commits/prepare`, `softwaredev/delivery/issues` and `softwaredev/delivery/github/follow-through`, wherever they offer an issue or commit write.

## Alternatives Considered

- **Keep ADR-609 as written.** Rejected: it gates writes that are easy to undo, and the operator asked for the gate at the merge.
- **Suggested CLAUDE.md lines only.** Each user copies a line to get this behaviour. Rejected: the operator asked that these writes need no explicit grant.
- **Allow force pushes to the agent's own pull request branch.** Rejected for now: Claude Code's text holds force pushes for a prompt, and the operator made this contingent on what ADR-609 rested on.
