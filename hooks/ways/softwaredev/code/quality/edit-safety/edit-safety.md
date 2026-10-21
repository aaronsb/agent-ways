---
description: editing existing code and files without losing work; re-read a file right before editing because the user may have changed it, back up a working version before a rewrite, edit the source and regenerate instead of hand-editing generated output, build on uncommitted changes without clobbering them, narrow to the named scope when told to wait, take one verified step at a time on work that failed before, and delete in small reviewed batches
vocabulary: edit existing file working version backup before rewrite re-read from disk user edited the file in parallel clobber overwrite lost my changes uncommitted changes generated output regenerate build script source template hand-edit pptx docx xlsx pdf jsonl wait hold stop narrow scope smallest change precedent conventions one problem at a time cleanup delete junk files destructive rm -rf before and after counts small step scaffold ahead of need
files: \.(py|ts|tsx|js|jsx|mjs|cjs|go|rs|sh|rb|sql|swift|kt)$
refire: 0.2
scope: agent, subagent
---
<!-- epistemic: convention -->
# Edit Safety

Most lost work comes from an edit made against a stale picture of the file, the scope, or the user's intent. These habits keep the working version recoverable while you change it.

## Smallest fitting change

- Make the smallest, simplest change that fits the project's conventions, and name the precedent you followed ("same shape as the existing retry helper").
- Fix one isolated problem at a time. Treat the working parts as done. A visual refresh changes no behavior. A front-end-only task leaves the backend untouched.
- Touch only what the task names. Do not change prompts, configs, security handling, other systems, or other repos unasked.
- When the user says "wait" or "hold", stop and narrow to the smallest scope they named. Drop the optional extras.
- Read recent git history before re-planning, and check how the fix interacts with recent commits.

## Before you edit

- Re-read the file from disk right before editing. The user may be editing it in parallel, and their edits stay. Look up records by key, not by position.
- Before rewriting something that works or that the user likes, write a dated backup or a new versioned file beside it. Edit in place only when told to. Back up user documents before an in-place edit. Backups sit outside the shipped path, so only one live copy ships.
- Build on uncommitted changes without overwriting them, unless the user calls them disposable.
- Pass the project's documented parameters explicitly instead of relying on script defaults. Apply config at the narrowest scope named, repo-local by default.

## Edit the source, regenerate the output

Edit the source (data file, markdown, template, build script) and regenerate through the script that owns the output. Do not hand-edit generated decks, documents, spreadsheets, PDFs, or data dumps. The next build overwrites a hand edit without warning. Do not overwrite a dated artifact that a report links to. If the user edited a generated file, diff their edits before rebuilding over them.

## No scaffolding ahead of need

For open-ended work, or work that failed before, propose one small step and the check that proves it. Verify it, then propose the next step. Do not build directory trees, pipelines, or docs ahead of need. When the user asks for less, delete rather than move it somewhere else.

## Before saying done

- List the knock-on issues the change created. Ask before fixing them or deleting stale artifacts.
- Sweep for other copies of the value or logic you changed (code/quality/integration).

## Destructive work

- Remove the obvious junk first. Take ambiguous items one at a time or in small groups, and ask about each group. Report before and after counts.
- Do not run `rm -rf` while testing. Suggest only test inputs that cannot damage the machine.
- Never renumber or squash migrations, decision records, or archives (data/migrations/numbering).

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I read the file a few turns ago" | The user may have saved since. Read it again before the edit. |
| "Git has the old version" | Only if it was committed. A working file the user likes may not be. Back it up. |
| "It's faster to fix the generated file directly" | The next build erases the fix. Change the source. |
| "While I'm here I'll tidy this other part" | That is a second change the user did not ask for. List it instead. |

## See Also

- code/quality/integration(softwaredev) — the class sweep, dead code, and no v2 wrappers
- code/quality/versioning(softwaredev) — why a new file name should say what the thing is
- environment/recovery(softwaredev) — classifying a failed attempt before the next one
- data/migrations/numbering(data) — migration numbers are never reused or renumbered
