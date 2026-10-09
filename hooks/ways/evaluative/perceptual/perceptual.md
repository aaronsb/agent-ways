---
description: a perceptual evaluative loop for an interactive program such as a game, web page or GUI app; drive it headlessly with simulated input, force state through development-build handles, wait on game or app state rather than time, take screenshots and say what they show
vocabulary: play it myself after each change to see whether the screen still renders right phone held upright game browser headless chrome puppeteer playwright cdp devtools screenshot screenshots canvas webgl three.js render frame animation viewport phone tablet portrait simulated input keypress click tap touch dev build debug handle window global force state smoke scenario playtest look at it say what you saw sleep timing
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Perceptual Loop

The product is a running program a person looks at. The agent drives it headlessly, reads its state, takes screenshots, and looks. Nothing can call the result right without someone reading the image, so the agent is one of the instruments here, and the shared core in `evaluative` keeps that honest.

## Make the program drivable

- **Expose state in development builds.** A global handle the harness can read (scene, score, player position, open dialog), behind the build's dev flag so it never ships. Read numbers before reading pixels.
- **Expose handles that set state.** Jump to a level, force a spawn, grant invulnerability, skip a banner. A scenario that sets state directly reaches the case in a second, where playing to it takes minutes and varies between runs.
- **Add counters to make behaviour countable.** When a test needs "the player bounced twice", give the code a bounce counter. The counter is part of the dev surface.
- **Drive real input.** Send key and pointer events through the browser's or toolkit's input path. A test that calls the movement function directly skips the input handling it should be exercising.

## Wait on state, never on time

Poll the exposed state until the condition holds, then act or capture. A fixed sleep drifts: capture calls cost time, timers run differently headless, and a screenshot lands after the animation it was meant to catch. Anchor each capture to an observed state value.

## Look, and say what you saw

After a visible change, capture the affected screens and read the images. Describe what each one shows in a sentence, and compare it to what the change should produce. If it is off, fix and capture again. A change is not called right from the code alone.

Capture at the sizes people use: a phone in portrait, a tablet, a desktop window. Layout breaks at the edges, and a screenshot that frames only the playfield misses the title, menus and overlays where it broke.

## A scenario per feature

Each feature adds a named scenario that drives to it, asserts on state, and contributes captures. Scenario names, debug keys, and the playtest list derive from one registry so they cannot drift apart. A bug a person hits in play becomes a scenario before the fix.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- code/testing/evaluation(softwaredev) — drive the path the real program takes
- code/testing/evaluation/screens(softwaredev) — the same loop for terminal screens
