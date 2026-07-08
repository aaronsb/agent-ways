---
name: merge
description: Land an increment to main through the branch → commit → push → PR → review-gate → merge → cleanup flow. The review gate is a four-square decision (review depth × human gate). Picks up wherever you are in the cycle. Use when the user says "merge this", "land this", "run a review and merge", "ship it", or invokes /merge.
allowed-tools: Bash, Read, Grep, Glob, Agent
---

# Merge — land an increment

Land current work to main. This is the **stable tail** of the develop loop: review →
fix → merge → clean up. Assess the current state and pick up from wherever the user
is. Landing an increment is **not** cutting a release — versioning and publishing are
`/release`.

## Arguments

- `/merge` — default flow; the review gate adapts to the four-square (below)
- `/merge full-sail` — skip the review pause (solo/pair repos, low blast-radius only; team repos and direction-setting changes still gate)

## Assess First

Run these in parallel to find your position in the flow:

```bash
git status --short              # Uncommitted changes?
git branch --show-current       # On main or a feature branch?
git log --oneline main..HEAD    # Commits ahead of main?
git remote show origin 2>&1     # Remote tracking state?
```

Also read repo flavor for the human gate:

```bash
git log --since="90 days ago" --format='%aN' | sort -u | wc -l
```

- **≤2 active**: Solo/pair — lightweight PRs
- **3+ active**: Team — review pause mandatory, suggest reviewers

If the GitHub way already injected `**Context**: Team/Solo/pair project`, use that.

## Flow Steps (skip what's already done)

### 1. Branch (if on main with changes)

```bash
git checkout -b <branch-name>
```

`feature/thing`, `fix/thing`, `refactor/thing`. Use the user's name if given.

### 2. Commit (if uncommitted changes)

Stage and commit in conventional-commit format. Split into atomic commits if there
are genuinely separate concerns. Ask for message direction if intent isn't clear.

### 3. Push

```bash
git push -u origin <branch>
```

### 4. PR

```bash
gh pr create --title "..." --body "$(cat <<'EOF'
## Summary
...

## Test plan
...
EOF
)"
```

Title under 70 chars. Summary 1–3 bullets.

### 5. Review Gate — the four-square

The gate is **not one policy**. Classify the increment on two axes and pick the path
(the full rationale lives in the `delivery/merge` way):

|                          | machine review: **light** | machine review: **deep / swarm** |
|--------------------------|---------------------------|----------------------------------|
| **human gate: none**     | quick `code-reviewer` (or skip for trivial) → merge | swarm review + adversarial verify → agent-gated merge |
| **human gate: required** | single `code-reviewer` → offer to read before merge | swarm review **and** operator approval before merge |

- **X (review depth)** ← complexity and blast-radius: diff size, files touched,
  core-vs-leaf, test coverage, reversibility.
- **Y (human gate)** ← does the work set direction (an ADR, an architecture change →
  human gate required, however small), and has the operator already read it (wrote it
  or reviewed it live → gate already satisfied).

When the classification is ambiguous, **state your read and let the operator decide**
rather than silently picking a corner. Inside a `/goal` loop, bias to the autonomous
corners; outside one, ask. Team repos always gate; `/merge full-sail` is rejected for
them and for direction-setting changes — say why.

**Running a single review:**

```
Agent(subagent_type="code-reviewer", prompt="Review PR #<number> in this repo.
Run gh pr diff <number> to see the changes. Post findings as a gh pr comment.")
```

For the swarm corner, fan `code-reviewer` agents across dimensions (correctness,
security, reuse) and adversarially verify each finding before trusting it.

### 6. Remediate

A review is only worth running if the findings get fixed. Before the merge gate,
**remediate per finding** — apply the fix, or record why a finding is declined. Don't
carry known findings past the gate. "Reviewed" is not "landed clean."

### 7. Merge

```bash
gh pr merge <number> --merge
```

Regular merge (not squash/rebase) unless the user prefers otherwise — preserve the
branch's narrative.

### 8. Cleanup

```bash
git checkout main && git pull && git fetch --prune
git branch -d <branch>
git branch   # verify only main + active work branches remain
```

Always run the full sequence — stale branches and dangling refs accumulate fast.

### 9. Release? (only if warranted)

Landing is not releasing. If the merged change warrants a versioned release (new
feature, breaking change, artifact-bearing project), **suggest `/release`** — don't
auto-publish. Publishing is a one-way door.

## Key Principles

- **Don't ask permission per step** — assess, propose the remaining flow, execute.
- **Pause at decision points**: commit wording, PR description, the review gate.
- **The four-square drives the gate**, not change size alone — a one-line ADR still
  gates on the operator; a big leaf refactor may not.
- **Remediate before merge** — findings that aren't fixed aren't findings, they're debt.
- **If mid-flow**, pick up from current state — don't restart.
- **Merge ≠ release** — landing an increment is the daily act; releasing is `/release`.

## Not for

- Writing the code being merged — this lands finished work, it doesn't author it (see `/develop`).
- Releasing/tagging/publishing artifacts — that's `/release` and the release way.
- Merging on a team repo, or a direction-setting change, without the gate — mandatory there.

## See Also

- `code-review` / `code-reviewer` — the review this borrows at the gate.
- `release` — the heavier sibling: publishing a versioned release.
- `wrap` — Step 1 of wrap delegates here to land stranded work.
- `develop` — the loop whose stable tail this is.
