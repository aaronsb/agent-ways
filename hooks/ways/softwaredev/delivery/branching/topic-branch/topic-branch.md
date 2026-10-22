---
description: steps for cutting the branch in your own checkout before editing; check git status, pull, git switch -c, bring along uncommitted changes someone else made, never switch branches in a shared checkout or worktree, follow-up commits on the open PR's branch, and the fork and no-remote cases
vocabulary: git switch -c show-current carry along someone else's uncommitted changes shared checkout subagents parallel sessions follow-up commits open pr fork no remote
scope: agent
refire: 0.1
---
<!-- epistemic: heuristic -->
# Branch Before the First Edit

When the work calls for a branch, cut it before the first edit. This holds in the main session, in your own checkout. In a worktree or a checkout shared with other agents (subagents, agent teams), never switch branches.

1. Run `git status` and `git branch --show-current`. Find the default branch with `git symbolic-ref refs/remotes/origin/HEAD` or `gh repo view --json defaultBranchRef`.
2. On the default branch, pull it, then run `git switch -c <type>/<short-topic>`. Use the name the user gave, if any.
3. State the branch name in one line and keep going. A local branch is cheap to undo, so there is no need to stop and ask.
4. Carry uncommitted changes you did not make onto the new branch (delivery/commits/prepare).

## One Branch per Piece of Work

- Push follow-up commits to the open PR's branch.
- Start unrelated work on a new branch cut from a freshly pulled default branch.

## Exceptions

- The user names main as the target, as in "commit to main". Do exactly that.
- The repository has no remote. Branch and commit locally.
- The repository is a fork. Push the branch to the fork and open the PR against upstream.

## See Also

- delivery/branching(softwaredev) — parent: when to branch and how to name it
- meta/subagents/agent-teams(meta) — why teammates in a shared checkout do not switch branches
- delivery/commits/prepare(softwaredev) — uncommitted changes you did not make, and staging by name
