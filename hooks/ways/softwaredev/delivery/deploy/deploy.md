---
description: before pushing, creating a repository, or deploying, confirm the active account is the one that owns the repository or cloud project; after a deploy, prove the live URL or running service serves the new build before calling it shipped
vocabulary: deploy deployment ship hotfix to prod staging live site url running service is the code you shipped new build old bundle cdn cache version hash build id really up active login whoami gh auth status 403 credential helper
commands: git\ push|gh\ repo\ create
pattern: \bgh auth\b|wrong (gh |github |cloud )?(acct|account)|which (github|gh|cloud) account
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Deploy

A push, a new repository, and a deploy each act through an account the user owns and land on a system other people use. Two checks bracket them: who you are before, and what is running after.

## Identity before the push

Before a push, a repository creation, or a deploy, read the active identity in full with `gh auth status` or the cloud CLI's whoami command. Match it to the account that owns the repository or project.

- On a 403, "permission denied", or "repository not found", suspect the account before the repository. `git push --dry-run` shows it before anything changes.
- Fix a mismatch for this repository only: a repo-local `credential.helper`, or a per-directory env file. Do not change the global git identity or the system keychain to fix one repository.
- A cloud CLI can hold two identities at once, a project token from an env file and the machine's own login. Check which one the command used.
- Keep each owner's accounts apart. Work for one owner never runs under another owner's account.
- When the project has a required account, record it in the project's instructions file so the next session can check it.

## Prove the new build is live

A clean build or a deploy command that exited zero shows the deploy ran, not that users get the new code. Before reporting it shipped, or asking the user to test:

- Fetch the live URL or query the running service and find the new build there: a version string, a build hash, a changed asset name.
- Suspect caches and CDNs when the old build still answers. Report it as not yet live rather than done.
- Name each target you did not check as unverified.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The push failed, so the repository must be missing" | A wrong account reads the same way. Check the account first. |
| "I'll change the global git email" | That breaks every other repository. Use repo-local credentials. |
| "The deploy command exited zero" | Load the live URL and find the new build there. |

## See Also

- delivery/release(softwaredev) — publishing a versioned artifact with a rehearsed rollback
- environment/hostparity(softwaredev) — a result here is not a result on the target
- code/security/secrets(softwaredev) — setting secrets during a deploy without printing them
- trust/delegation(meta) — a push goes out under the human's name
