---
contract: adr/v1
kind: decision
verb: constrain
capability: method
basis:
  - operator: aaronsb
    level: directed
    said: "ways should defer to external system enforcement"
    via: chat, session 418e1be3, 2026-09-30
  - operator: aaronsb
    level: directed
    said: "if someone has a strict policy to review, then we should follow that. at some point, we need to stop second guessing what's best (what you've quoted is our 'way') and instead use any sort of git hooks or policy that is mechanistic in nature to enforce actions"
    via: chat, session 418e1be3, 2026-09-30, on the version-bump review exemption (#659)
  - operator: aaronsb
    level: directed
    said: "each repo is different, and we have to remember this way gets applied to any project not just our project"
    via: chat, session 418e1be3, 2026-09-30
  - operator: aaronsb
    level: directed
    said: "and the deferral should allow to question poor system configuration not just accept it"
    via: chat, session 418e1be3, 2026-09-30
  - operator: aaronsb
    level: directed
    said: "because ways may influence actually setting up the system as well as using it"
    via: chat, session 418e1be3, 2026-09-30
  - operator: aaronsb
    level: directed
    said: "a good environment is low friction"
    via: chat, session 418e1be3, 2026-09-30
  - precedent: ADR-175
agent:
  name: claude
  model: claude-opus-5-5
status: proposed
date: 2026-09-30
deciders:
  - aaronsb
related:
  - ADR-175
observable:
  - The merge way reads a repository's review enforcement before its own review gate, on GitHub, GitLab and in repository files, and treats an unreadable policy as review required (#659).
  - In a repository whose branch rules require an approving review, a version-bump PR is reviewed.
---

# ADR-604: Ways defer to external system enforcement

## Summary

- **Decided:** When a system outside the ways mechanically enforces a policy over an action (forge branch rules, CODEOWNERS, required checks, git hooks, CI gates, the harness's permission system), that enforcement is the policy. A way reads it before applying its own judgement, adds rigor on top of it, and never weakens or routes around it. A way's judgement governs only where nothing enforces, and an enforcement it cannot read counts as the strict reading. Deferring governs how the agent acts under a configuration. It leaves the way free to question that configuration, and when the task is setting the system up, the way's guidance shapes what is proposed.
- **Trades away:** Uniform behavior across projects, and the ability to relax a project's strict rule even where the way judges it wasteful. Each way that guides an enforceable action carries detection work for more than one host.
- **One-way?** No. It is guidance text, reversible in one session.
- **Probes:** *Confident (strict-wins):* a project that requires review on every PR gets a review on version bumps too, even though our default skips it. *Not confident (propose-mechanism):* where a project has no mechanism and a rule keeps coming up, the way offers to add one (a hook, a branch rule, a CI check) and installs nothing unasked.
- **Inversion:** Ways as the policy everywhere, or ways as advice that yields to everything. The decision sits between: enforcement first, ways fill the gaps and judge the configuration itself.

## Context

Ways are installed once per user and disclosed in every project that user opens. Each project sets its own policy, and the strict ones encode it in mechanisms: protected branches that require approvals, CODEOWNERS, required status checks, pre-commit hooks, CI gates. A way written from one project's habits carries that project's defaults into all the others.

The merge way's review gate surfaced the conflict. It gained an exemption that lets a version-only release bump merge without review. In a project that requires review on every change, that exemption, applied as written, would have the agent skip a review the project's own rules demand. The way would be second-guessing a policy the project had already settled mechanically.

## Decision

1. **Read before judging.** Before a way's judgement decides an action that a project can enforce (merging, committing, pushing, releasing, adding a dependency, formatting), the way reads what the project enforces for it.
2. **Enforcement decides.** What the project enforces is the policy. A way may add rigor on top of it and never subtracts from it: no skipped required check, no bypass flag, no suggestion to disable a hook or rule to get past it.
3. **Unreadable means strict.** When the enforcement cannot be read (no forge CLI, no access, an unfamiliar host), the way assumes the stricter reading.
4. **Detect, don't assume.** A way names how to find the forge, the default branch and the project's tooling, and gives detection for each host it supports. It says which hosts it does not cover.
5. **Offer a mechanism for a rule that keeps recurring.** When a way's rule matters enough to keep restating in a project, the way offers to encode it as a mechanism the project owns. It installs nothing without the operator's go-ahead, and a mechanism the project adopts then outranks the way under rule 2.
6. **Question the configuration, follow it while it stands.** Deferring does not mean endorsing. A way names enforcement that looks weak, broken, contradictory or needlessly strict as a concern to the operator, with the change that would fix it. Weak or broken: a default branch with no protection, a required check that never runs, CODEOWNERS pointing at people who left. Needlessly strict: friction that catches no defect, such as a required review on a version-only bump. Until the operator changes the configuration, the agent acts under it.
7. **Setting up the system is the way's ground.** When the task is to configure the enforcement itself (initializing a repository, setting branch rules, adding hooks or CI gates), there is no prior policy to defer to for that change, and the way's guidance shapes the proposal. A good environment is low friction: the proposal is the lightest mechanism that enforces what the project needs, and each rule in it names the defect it stops. The change goes to the operator before it lands, since it binds everyone who works in the project.
8. **The harness is an external system.** A denial from Claude Code's permission system or its classifier is enforcement. A way does not route around it, which is how ADR-175 treats the harness's delegation gate.

## Consequences

### Positive

- A project gets its own policy, and the ways stop overriding it.
- A rule the operator cares about can move out of prose into a mechanism that holds whether or not a way fires.
- Ways that guide enforceable actions say what they detect, so a gap in host coverage is visible.

### Negative

- Reading enforcement adds a step before each such action, and forge-specific detection needs upkeep as forges change their APIs.
- Where a policy cannot be read, the strict reading can cost a review or a check the project did not require.
- A way that judges configuration can be wrong about a project's reasons. Rule 6 keeps that judgement to a raised concern, so the operator's knowledge of those reasons decides.

### Neutral

- Existing ways need a sweep for places their defaults could override a project's enforcement (#660). The merge way was brought in line first (#659).

## Alternatives Considered

- **Ways as authoritative defaults, overridden by project-local ways.** Rejected: every project would have to restate its policy in the ways format, while the forge and the hooks already hold it.
- **Ways as advice only, deferring to everything.** Rejected: where nothing enforces, the way is the only guidance the agent has, and it has to be followed.
- **Ways install their own enforcement globally.** Rejected as a default: hooks installed by ways in every project would make the ways the enforcer and outrank each project's choice. Rule 5 keeps this as a per-project offer.
