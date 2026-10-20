---
description: closing the loop before handing work to the user; run and render the work yourself, check that done holds in the artifact the user actually runs, open it where they review it, say what to test and what should happen, name what you could not verify, and wait for their confirmation before committing
vocabulary: handoff hand off done ready to test try it check your work test it yourself open it preview render rendered screenshot pdf docx print deck slides page localhost dev server live url confirm before commit worktree scratch copy rebuilt artifact what to test expected result unverified art director overflow clipping
pattern: test (it|this) yourself|\bjust open it\b|(is|it's) (it |this )?all working|ready for (you|me) to test|open (it|this) (in|on) (the |my )?(browser|editor|viewer)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Handoff

Run the work yourself before you tell the user it is done or ask them to test it. The user's time is the most expensive check in the loop. Spend it on what only they can judge.

## Run it yourself first

- Start the dev server yourself. Hit the endpoints directly. When you connect a new tool server to the session, call each of its tools once.
- Drive a UI with a browser automation tool. For a sign-in, run the browser headed and let the user log in. If automation is blocked at the login, change approach instead of retrying.
- Read database rows and logs in short, searched bursts. Do not follow a long live tail.
- Include negative cases, such as a spoofed sender or a malformed request. Delete test rows afterward.
- When a UI bug survives a fix, stop guessing. Gather full-size screenshots, console errors, and the stored state before the next attempt.

## Render static output and look

A green build says nothing about how a page looks. Render each changed page of a PDF, document, or slide deck to an image (`pdftoppm` for PDF) and read it. Open a document in the viewer its recipient uses. Check print output when the work will be printed. Preview markdown tables the way the host renders them. Judge each page as a demanding art director would: overflow, overlap, clipping, stray boxes, leftover placeholder text. Interactive programs follow the perceptual loop.

## Done means in the artifact the user runs

- The change is merged and rebuilt, wired to the real backend, and its output files sit on disk at plausible sizes.
- Never report progress from a worktree or scratch copy the user will not open.
- Build a desktop app at the path the user launches it from.
- When deploying is in scope, confirm the live site serves the new build.
- Your "done" and a subagent's "done" are claims. Before you relay one, name what would show it false and check that.

## Hand off where they review

Open the artifact without being asked, in the viewer the user reviews that kind of file in. Reasonable defaults are an editor for text and folders, a browser tab for each page, app, or PR, an image viewer for images and PDFs, and a scratch text file for paste-ready text. Viewers the user has named win. A bare path or raw diagram source is not a handoff.

Then give the path or URL, the key controls, what to test, the expected result, and any command only they can run. Watch the logs while they test. Commit, push, or merge only after they confirm.

When the user says "just open it", open it now without a headless check first. Open a rebuilt deck at its first slide.

## Name what you could not verify

Give the gap and the one check that closes it. For example: "An access gate blocks the live page, so I could not load it. Open it signed in and confirm the header shows today's date."

## Before a stakeholder hears done

Check the claim against the artifacts: files delivered, archives current, data processed, work pushed. QA it as the recipient would. List feedback that has not been folded in yet.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The build is green, so it works" | A build proves it compiled. Run it and look. |
| "The user will test it anyway" | They test what you could not. Spend their time there. |
| "It works in my worktree" | The user runs the merged, rebuilt copy. Check that one. |
| "I sent them the path" | Open it where they review it and say what to test. |

## See Also

- code/testing/evaluation(softwaredev) — parent: reproduce the failure and drive the real path
- code/testing/gates(softwaredev) — automated gates that run before the handoff
- delivery/groundwork/claims(softwaredev) — a done report is a claim to check
- environment/hostparity(softwaredev) — verified here is not verified on the target
- evaluative/perceptual(evaluative) — driving and looking at an interactive program
