---
description: running CI once per tree so a tree that passed is not tested again on a later push, merge or bump, test result caching keyed by tree hash, merge queues, triggers and path filters that do not fan out across every workflow, checks that only wait for a runner, and the build infrastructure for running gates in parallel on a local host or in CI
vocabulary: ci pipeline workflow runner rerun re-run duplicate run same tree tree hash test result caching merge queue presubmit postsubmit path filter lockfile trigger fan out deploy per tag bump pr per component wall clock slow runner queued pending parallel gates nextest doctests build directory target directory compiler cache sccache ccache tmpfs disk volume concurrency core count longest first timeout under load cancelled concurrency group
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: heuristic -->
# CI

At agent pace, CI's wall-clock time is the longest wait in the loop, so every duplicate run lands on the critical path. code/testing/gates/attention sets out the two kinds of verification: focal checks that a worker chooses for one change, and one ambient pass over the combined tree where branches come together. This way covers how CI runs that ambient pass once, and how the hosts that run gates are set up.

## One run per tree

A tree is the exact content of a commit's files, named by its tree hash (`git rev-parse HEAD^{tree}`). A tree that passed is not tested again by a later push, merge or bump of the same tree. A local run counts as that tree's run, and is repeated only when the tree changed. Each pipeline that tests everything on its own is complete, and together they repeat the same work on the critical path, which is suboptimization (Deming): each part is tuned at the cost of the whole.

- **Test result caching.** Store each passing result under the tree hash, and have the workflow look the key up before it runs. On a hit, the job reports a pass and stops. Key a workflow on the tree hash of the paths it reads (`git rev-parse HEAD:<path>`), so a change elsewhere in the repository does not invalidate its result.
- **Merge queues.** A merge queue runs presubmit testing on the combined tree of the target branch and the pull request, then merges exactly that tree. The postsubmit run on the target branch then sees a tree that already passed, and the cache answers it.
- **A push to main.** A regular merge of a branch that was up to date with main produces a commit whose tree equals the branch head's tree. That tree passed on the pull request, so the run on main is a cache hit, not a second full run.
- **A bump PR.** A release bump changes the version line and the lockfile entry. Run the checks the version line reaches, such as the build that stamps the version into the artifact, and let the cache answer the rest.
- **Superseded runs.** Put each workflow in a concurrency group keyed by branch, with cancel-in-progress on, so a new push cancels the run on the old tree.

## Triggers that do not fan out

- **Path filters on shared files.** A path filter that lists a lockfile or a top-level build manifest wakes every workflow that lists it, so a one-line dependency change runs every build. List a shared file only in the workflows that build from it, or have the job check which entries changed before it builds.
- **Deploy once per commit.** When one commit carries several tags, such as one tag per component, a deploy triggered on each tag runs once per tag. Trigger the deploy on the commit, or skip a deploy whose commit SHA already deployed.
- **One bump PR per release.** Each bump PR runs the full pipeline and waits in its own queue. One pull request that bumps every component in the release pays for one run.

## Checks that only wait for a runner

A check that is only waiting for a runner does not gate a merge, and a failed check does. delivery/merge has the command that reads the checks this way, and the waiting check is recorded as discovered (code/testing/gates).

## Infrastructure for parallel gates

These apply wherever several gates run at once, on a local host or on a CI runner.

- **Run test binaries in parallel.** A runner such as cargo-nextest runs each test in its own process across all cores. It does not run doctests, so run those as a separate gate (`cargo test --doc`).
- **Keep build directories warm.** Give each gate a persistent build directory that survives between runs, and a shared compiler cache such as sccache or ccache. A cold build repeats the work the previous run already did.
- **Prune build directories no current gate owns.** A directory left by a finished branch or a removed worktree holds gigabytes that no gate reads.
- **Build on a disk volume.** A RAM-backed tmpfs holds build directories in memory, so they compete with the compilers and tests for the same memory, and the host swaps or kills processes when it runs out.
- **Give each parallel gate its own build and target directories.** Two gates that share one either wait on its lock or overwrite each other's artifacts. For cargo, set `CARGO_BUILD_BUILD_DIR` for intermediate artifacts and `CARGO_TARGET_DIR` for final artifacts, per gate.
- **Size concurrency to the core count, and start the longest gates first.** More gates than cores makes each one slower. A long gate started last finishes last, so the whole set waits on it.
- **Set test timeouts to survive load.** A timeout tuned on an idle host fails when parallel gates share the cores, and the failure reads as a flaky test. Set the slow-test timeout from a run under the full parallel load, for example with nextest's `slow-timeout` and `terminate-after`.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Main should run its own full suite to be safe" | If main's tree is the tree that passed on the pull request, the second run tests nothing new. |
| "The lockfile touches everything, so everything should rebuild" | Only the workflows that build from the changed entries can be affected by them. |
| "The macOS runner is still queued, so the PR is not green" | A queued check has not failed. Record it as discovered and merge on the checks that ran. |
| "More parallel gates finish sooner" | Past the core count, each gate runs slower and timeouts start to fail. |

## See Also

- code/testing/gates/attention(softwaredev) — focal and ambient verification, and who runs each
- code/testing/gates(softwaredev) — gate states, including discovered, and the planted-defect rule
- delivery/merge(softwaredev) — the green-CI command and the waiting-check rule
- delivery/github/follow-through(softwaredev) — confirming a tree passed once after the merge
- delivery/release(softwaredev) — the bump PR and the release build
- delivery/deploy(softwaredev) — proving the live build changed after a deploy
