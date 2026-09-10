---
description: a green result on the authoring host is evidence about the authoring host only; the CI runner, container, staging box, or other OS is a separate host until exercised there, and untracked or unpushed files do not exist for it
vocabulary: works locally fails in ci on my machine passes here ci red workflow runner container differs github actions pipeline job target host authoring host parity unpushed untracked uncommitted reproduce ci locally act same image verified unverified
files: \.github/workflows/.*\.ya?ml$|Dockerfile|\.gitlab-ci\.yml
commands: git push
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Host Parity

The build passes on the laptop, the report says "tests pass", and CI goes red an hour later. The laptop and the runner are different systems with different shells, tool versions, path layouts, environment variables, and privileges. A green result on the authoring host is evidence about the authoring host. Claims about the target come from the target.

## Two rules

**Validate where it runs, or say that you did not.** Exercise the change on the target: the CI runner, a container built from the CI image, the staging box, the other OS. When the target can be run locally (`act` for GitHub Actions, a container matching the runner, the same base image), run it there before claiming parity. When no target is reachable, name each one as unverified.

**Untracked means absent.** A file the build resolves from the working tree but that is untracked, ignored, or unpushed does not exist for CI, for a teammate, or for the deployed artifact. A build that passes locally on an uncommitted file is unverified. Before claiming delivery, read `git status` and confirm the artifact sits where the consumer will look for it.

## What differs between hosts

| Difference | Typical symptom |
|---|---|
| OS and shell | `bash` versus `sh`, GNU versus BSD flags, path separators, line endings |
| Tool versions | a flag the runner's older compiler, node, or python lacks |
| Env vars and secrets | a value set in the shell rc that the runner never receives |
| File case sensitivity | `Readme.md` resolves on macOS and fails on Linux |
| Network access | a fetch the runner's egress rules block |
| Working directory | a relative path that assumes the repo root or the home directory |
| Clock and locale | timezone-dependent dates, sort order, number formatting |

## The report shape

Name the hosts. "Verified on the authoring host only; unverified on <target>" is the sentence, written once per target. "Tests pass" with the host unnamed reads as verified everywhere. When a target has been exercised, say which one and how (the CI run URL, the container image tag).

A local model, service, or credential standing in for a remote one falls under the same rule. The substitute serves iteration, and the remote supplies the evidence.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It's the same code" | Same code, different host. Run it there. |
| "CI will catch it" | CI catches it after the push. Report it as unverified until then. |
| "I ran the tests" | Where? Name the host in the report. |
| "The file is right there" | `git status` says whether the runner will see it. |

## See Also

- environment/container-safety(softwaredev) — the container definition the target build runs inside
- environment/config(softwaredev) — env vars and dotenv files that differ per host
- delivery/github(softwaredev) — reading CI checks on the pull request
- freshness/groundtruth(softwaredev) — the executing system is authoritative over descriptions of it
