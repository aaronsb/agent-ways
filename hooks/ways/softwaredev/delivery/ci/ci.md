---
description: running CI once per tree so a tree that passed is not tested again on a later push, merge or bump, test result caching keyed by tree hash, merge queues, triggers and path filters that do not fan out across every workflow, checks that only wait for a runner, and the build infrastructure for running gates in parallel on a local host or in CI
vocabulary: ci pipeline workflow runner rerun re-run duplicate run same tree tree hash test result caching merge queue presubmit postsubmit path filter lockfile trigger fan out deploy per tag bump pr per component wall clock slow runner queued pending parallel gates nextest doctests build directory target directory compiler cache sccache ccache tmpfs disk volume concurrency core count longest first timeout under load cancelled concurrency group
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: heuristic -->
# CI

This way covers how CI runs the ambient pass, the one full pass over the combined tree (code/testing/gates/attention), once per tree, and how the hosts that run gates are set up. Each duplicate run adds CI's wall-clock time to the agent's wait.

## One run per tree

A tree is the exact content of a commit's files, named by its tree hash (`git rev-parse HEAD^{tree}`). A tree that passed is not tested again by a later push, merge or bump of the same tree. A local run counts as that tree's run, and is repeated only when the tree changed.

- **Test result caching.** Store each passing result under the tree hash, and have the workflow look the key up before it runs. On a hit, the job reports a pass and stops. Key a workflow on the tree hash of the paths it reads (`git rev-parse HEAD:<path>`), so a change elsewhere in the repository does not invalidate its result.
- **Merge queues.** A merge queue runs presubmit testing on the combined tree of the target branch and the pull request, then merges exactly that tree. The postsubmit run on the target branch then sees a tree that already passed, and the cache answers it.
- **A push to main.** A regular merge of a branch that was up to date with main produces a commit whose tree equals the branch head's tree. That tree passed on the pull request, so the run on main is a cache hit, not a second full run.
- **Superseded runs.** Put each workflow in a concurrency group keyed by branch, with cancel-in-progress on, so a new push cancels the run on the old tree.

## Infrastructure for parallel gates

These apply wherever several gates run at once, on a local host or on a CI runner.

- **Run test binaries in parallel.** A runner such as cargo-nextest runs each test in its own process across all cores. It does not run doctests, so run those as a separate gate (`cargo test --doc`).
- **Keep build directories warm.** Give each gate a persistent build directory that survives between runs, and a shared compiler cache such as sccache or ccache. A cold build repeats the work the previous run already did.
- **Prune build directories no current gate owns.** A directory left by a finished branch or a removed worktree holds gigabytes that no gate reads.
- **Build on a disk volume.** A RAM-backed tmpfs holds build directories in memory, so they compete with the compilers and tests for the same memory, and the host swaps or kills processes when it runs out.
- **Give each parallel gate its own build and target directories.** For cargo, set `CARGO_BUILD_BUILD_DIR` for intermediate artifacts and `CARGO_TARGET_DIR` for final artifacts, per gate.
- **Size concurrency to the core count, and start the longest gates first.** More gates than cores makes each one slower. A long gate started last finishes last, so the whole set waits on it.
- **Set test timeouts from a run under load.** Set the slow-test timeout from a run under the full parallel load, for example with nextest's `slow-timeout` and `terminate-after`.

## Watch for

| You notice | It means | Do this |
|---|---|---|
| You are about to run the local tests again on a tree CI already passed | That tree already has its run | Compare `git rev-parse HEAD^{tree}` with the tree the passing run tested. When they match, cite that run. Run again only when the tree changed. |
| CI starts a full run on main for a tree the pull request already tested | The postsubmit run repeats the presubmit run | Cancel the duplicate run, or let the test result cache answer it. A run cancelled as a duplicate is not a failure (delivery/github/follow-through). |
| A bump PR whose diff is only the version line and its lockfile entry starts the whole suite | Only the checks that read the version line can be affected | Run the checks the version line reaches, such as the build that stamps the version into the artifact, and let the cache answer the rest. |
| You are about to open one bump PR per component | Each bump PR runs the full pipeline and waits in its own queue | Open one bump PR that bumps every component in the release, so the release pays for one run. |
| A lockfile-only change wakes every build workflow | Each workflow whose path filter lists the lockfile starts, whether or not it builds from the changed entries | List a shared file, such as a lockfile or a top-level build manifest, only in the workflows that build from it, or have the job check which entries changed before it builds. |
| A tag push starts one docs deploy per tag on the same commit | The deploy triggers on each tag, and one commit carries a tag per component | Trigger the deploy on the commit, or skip a deploy whose commit SHA already deployed. |
| Release builds sit queued behind duplicate runs on main | The duplicates hold the runners the release needs | Cancel the duplicate runs. The push-to-main and superseded-runs rules above keep them from coming back. |
| A merge has waited an hour on a check that is queued, not started, on a runner known to be slow or flaky | A check that only waits for a runner has not failed | Merge on the checks that ran, using the command in delivery/merge, and record the waiting check as discovered (code/testing/gates). A check that is running, a pending check on any other runner, and a failed check still gate. |
| Several agents run gates in parallel and share one build or target directory | The gates wait on its lock or overwrite each other's artifacts | Give each gate its own build and target directories, as the infrastructure list above sets out. |
| Tests fail on timeouts only while gates run in parallel | The timeout was tuned on an idle host, so the failure reads as a flaky test | Retune the slow-test timeout, as the infrastructure list above sets out. |

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Main should run its own full suite to be safe" | If main's tree is the tree that passed on the pull request, the second run tests nothing new. |
| "More parallel gates finish sooner" | Past the core count, each gate runs slower and timeouts start to fail. |

## See Also

- code/testing/gates/attention(softwaredev) — focal and ambient verification, and who runs each
- code/testing/gates(softwaredev) — gate states, including discovered, and the planted-defect rule
- delivery/merge(softwaredev) — the green-CI command and the waiting-check rule
- delivery/github/follow-through(softwaredev) — confirming a tree passed once after the merge
- delivery/release(softwaredev) — the bump PR and the release build
- delivery/deploy(softwaredev) — proving the live build changed after a deploy
- subagents/lead(meta) — the lead's signals, including duplicate helpers across workers
