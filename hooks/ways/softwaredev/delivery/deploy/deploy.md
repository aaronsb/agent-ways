---
description: before a push, repository creation, or deploy; confirm the active account matches the owner of the repository or cloud project and fix a mismatch yourself, push to every remote, deploy only when asked, pick the target from project context, order the steps so nothing is exposed, and prove the new build is live
vocabulary: deploy deployment push account identity gh auth status whoami switch account wrong account 403 repository not found credential helper direnv remote mirror dry-run staging sandbox production prod promote live url auto-deploy ci cd preflight cost spend visibility private repo create owner org
commands: git\ push|gh\ (auth|repo\ create)|wrangler\ (deploy|publish|whoami)|railway\ (up|deploy|whoami)
pattern: \bgh auth\b|wrong (gh |github |cloud )?(acct|account)|which (github|gh|cloud) account|push(ing)? to (both|every|all) remotes?|deploy (it|this) to (prod|production|staging)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Deploy

A push, a new repository, and a deploy all act through accounts and machines the user owns. Check who you are and where the change is going before each one.

## Identity first

Before a push, a repository creation or lookup, an issue action, or a deploy, read the active identity in full with `gh auth status` or the cloud CLI's whoami command. Match it to the account that owns the repository or project. Record the required account in the project's instructions file so the next session can check it.

- Fix a mismatch yourself. Prefer repo-local credentials, such as a `credential.helper` that runs `gh auth token -u <account>` or a per-directory env file loaded by direnv. Otherwise switch accounts for the step and switch back afterward.
- Never change the global git identity or the keychain to fix one repository.
- Keep each owner's accounts, repositories, and content apart. Work for one owner never touches another owner's account.
- On a 403 or "repository not found", suspect the account first. `git push --dry-run` catches it before anything changes.
- A cloud CLI can hold two identities: a project token in the env file and the machine's own login. Pin the account id in the project config and check which identity the command used.
- Before creating a repository, confirm its name, owner, code owners, and visibility. Default to private.

## Remotes

Run `git remote -v`. Push to every mirror the project keeps and to any remote the user names. Report which remotes received the push.

## Deploy only when asked

- Make no deploy, secret write, data-store write, or paid smoke test unless the user asked for it.
- Run the dry-run mode first. Quote the expected cost and confirm before spending.
- Before the first push to a repository, ask whether CI deploys on push.
- Where make targets or CI own the deploy, use them. If that path is blocked, present options. Never route around it silently.

## Pick the target from context

- A small project with no other users can deploy straight to production with the project CLI. Hand back the URL and the test steps, then wait.
- A live app with real users deploys to a sandbox or test branch first, and existing users stay undisturbed. Promote only when the user says to.
- A dev and prod mapping in the project's instructions wins over both defaults.

## Order the steps

Step zero is a preflight: the identity check, `git log -1` in the deploy directory, and the guard tests. Then order the steps so nothing is exposed while it is unsafe. For example, turn on the access gate before the API key goes in. Each step verifies on its own.

## Prove it shipped

A clean compile is not a deploy. Before you ask the user to test, check that the live URL serves the new build, for example by reading a version string or build hash. Name any target you did not check as unverified.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The push failed, so the repository must be missing" | Check the account first. A wrong account reads the same way. |
| "I'll just change the global git email" | Use repo-local credentials. The global setting breaks every other repository. |
| "The deploy command exited zero" | Load the live URL and find the new build there. |
| "It's a quick deploy, no need to ask" | Deploys spend money and reach users. Wait for the word. |

## See Also

- delivery/release(softwaredev) — publishing a versioned artifact with a rehearsed rollback
- environment/hostparity(softwaredev) — a result here is not a result on the target
- code/security/secrets(softwaredev) — setting secrets during a deploy without printing them
- trust/delegation(meta) — a push goes out under the human's name
- proposals(itops) — a structured approval before a high-risk production change
