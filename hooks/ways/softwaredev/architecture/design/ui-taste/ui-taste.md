---
description: building or restyling a user interface so it looks crafted rather than generated or templated; cut filler copy, follow the user's stated look or the host product's component library and style guide, use familiar interaction idioms, keep progress indicators inside the region doing the work, check phone and desktop layouts, and hold the same bar on every screen
vocabulary: ui frontend interface screen page dashboard landing page look and feel visual styling polish polished looks generic looks like a template bland default styling personality distinctive ai generated vibe coded slop filler copy tagline emoji icons component library style guide native idioms modal dialog loading spinner responsive mobile layout consistency
pattern: vibe.?coded|\bai slop\b
files: \.(css|scss|sass|less)$
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# UI Taste

A screen that looks generated loses the reader before they use it. The usual tells are filler copy, emoji in controls, a palette nobody chose, bespoke widgets that need explaining, and a spinner over the whole app.

## Cut the filler

- Cut taglines, "how to use this" sections, inline notes and labels that restate the obvious. Filler survives the first pass, so check again.
- Use progressive disclosure instead of walls of text.
- Consistent design tokens can still compose badly. Render the screen and critique the screenshot.
- For a second version, design from the user's goals. Do not port the first version screen by screen.

## The look

- Use the look the user stated, or the host product's design system and style guide when the UI lives inside another product. When neither exists, ask, or propose one restrained direction and say that you chose it.
- Use icons and plain labels in controls, not emoji. Keep animation that shows state and drop the rest.
- A style change does not change behaviour.

## Familiar over bespoke

| Do | Don't |
|----|-------|
| Use the host's components, with only light extra styling | Ship bespoke widgets, or a foreign UI embedded in the host |
| Use in-app dialogs | Use browser `alert()` or `confirm()` |
| Reuse known idioms: selection, undo, comment threads | Invent interactions that need instructions |

If a user of a familiar tool would need instructions, redesign.

## Progress and layout

- Check desktop, phone portrait and phone landscape.
- Stream model output, and show whether the app is working or idle so it never looks hung.
- Put the working indicator inside the region doing the work, never over the whole UI.
- Status text says what the system is doing, in the user's terms, without internal phase names.

## Every screen

Users judge a product by its weakest screen. When extending existing work, match its writing, visual grammar and scale. Where a brief conflicts with the established work, follow the established work and flag the difference.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A short intro helps people understand the page" | If the page needs an intro, the layout is unclear. Fix the layout. |
| "Emoji make it friendlier" | They make it look generated. Use icons and plain labels. |
| "A full-screen spinner is simplest" | It hides the rest of the app and looks hung. Scope it to the working region. |
| "The tokens are consistent, so the design is fine" | Look at the rendered screen. Consistency is not composition. |

## See Also

- architecture/design(softwaredev) — parent: deliberation before building
- code/testing/evaluation/screens(softwaredev) — capturing screens at the sizes users run
- evaluative/perceptual(evaluative) — driving the running UI and looking at it
- visualization/charts(softwaredev) — charts inside the UI
