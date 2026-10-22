---
description: handing finished work over to the user, a reviewer or whoever takes it over; run and render it yourself first, confirm done in the merged and rebuilt copy they will use, say what to test and the expected result, give them a handover package with the commands that rerun each reported result, and name what you could not verify
vocabulary: handoff hand off handover package hand over pass it on takes over while I'm away they can start using it so someone else can rerun the results ready for you to test open it preview render rendered screenshot pdf slides deck dev server live url worktree scratch copy rebuilt artifact what to test expected result unverified
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Handoff

Before telling someone the work is done, or asking them to test or review it, run it yourself. Their time is the most expensive check in the loop, so spend it on what only they can judge.

## Run it first

- Start the program or server and drive the changed path: call the endpoints, drive the UI with browser automation, call each tool of a newly connected tool server once.
- Include a negative case, such as a malformed or unauthorized request. Remove test data afterwards.
- When a UI bug survives a fix, gather full-size screenshots, console errors and the stored state before the next attempt.

## Render static output and look

A passing build says nothing about how a page looks. Render each changed page of a PDF, document or slide deck to an image and read it for overflow, overlap, clipping and leftover placeholder text. Check it in the viewer the recipient will use. Interactive programs follow evaluative/perceptual.

## Done is in the artifact they run

- The change is merged and rebuilt, wired to the real backend, and its output files exist at plausible sizes.
- Progress in a worktree or scratch copy the recipient will not open is not done.
- When deploying is in scope, confirm the live site serves the new build.
- A subagent's "done" is a claim. Name what would show it false and check that before relaying it.

## What the handoff carries

- The path or URL, what to test, and the expected result.
- Any command only they can run.
- What you could not verify, and the one check that closes the gap: "The live page is behind sign-in, so I could not load it. Open it signed in and confirm the header shows today's date."
- Feedback that has not been folded in yet.

## When someone else takes over or reviews it

Leave a package that works without you. For each number reported, such as a score, a pass count or a timing, give the command that produced it and the inputs, data and versions it ran against. The reviewer or the person taking over reruns that command and gets the same figure without asking you.

Open the artifact for them when they ask, or when it is on a display they are using. Otherwise the path and the test steps are the handoff.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The build is green, so it works" | A build proves it compiled. Run it and look. |
| "The user will test it anyway" | They test what you could not. Spend their time there. |
| "It works in my worktree" | They run the merged, rebuilt copy. Check that one. |
| "The numbers are in the report" | Without the command and its inputs, nobody can rerun them. |

## See Also

- code/testing/evaluation(softwaredev) — parent: reproduce the failure and drive the real path
- code/testing/gates(softwaredev) — automated gates that run before the handoff
- delivery/groundwork/claims(softwaredev) — a done report is a claim to check
- environment/hostparity(softwaredev) — verified here is not verified on the target
- evaluative/perceptual(evaluative) — driving and looking at an interactive program
