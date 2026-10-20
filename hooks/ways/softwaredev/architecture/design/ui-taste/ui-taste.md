---
description: designing UIs and demo apps that look curated rather than vibe-coded or AI-generated; cut filler copy, start light and minimal unless the user sets a look, use no emojis, use the host's native design system and familiar idioms, scope loading indicators to the working region, check responsive layouts, and hold the same bar on every screen
vocabulary: ui design look and feel layout frontend demo app screen page loading indicator spinner streaming button dropdown modal dialog alert emoji icon svg light minimal monochrome curated polished enterprise saas vibe coded ai generated fluff filler copy tagline too colorful responsive mobile portrait landscape design system style guide native idioms progressive disclosure status text
files: \.(css|scss|sass|less|vue|svelte)$|(^|/)(components|styles)/[^/]+$
pattern: vibe.?coded|\bai.?(fluff|generated look)|feel(s)? curated|\bspinner\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# UI Taste

UIs that go in front of clients and leadership get judged in seconds. The common complaints are that a screen looks vibe coded, AI generated, or not curated. Build so those words never come up.

## Curated, Not Generated

- Cut taglines, filler copy, "how to use this" sections, inline notes, and one-liners nobody asked for. Filler tends to survive the first pass, so check again.
- Use progressive disclosure instead of walls of text. Drop legends and labels when the meaning is already obvious.
- Consistent design tokens can still look generated. Render the UI and critique the composition from the screenshot, not only from the tokens.
- For a second version, design from the user's stated goals. Do not port the first version screen by screen.

## Default Look

When the user has not set a look, start here:

- Light, minimal, and close to monochrome. Modern fonts and modest type sizes. Use bold and italics for scanning instead of heavy callouts, loud color, or oversized layouts.
- For demos, aim for a professional enterprise SaaS feel. If the user names a reference product, match its restraint.
- Use dark mode only when the user asks for it.
- A style change must not change behavior.

## No Emojis

Keep emojis out of the UI: buttons, dropdowns, file chips, and labels. Use clean SVG icons and plain labels ("Download"). Skip decorative animations that do not show state.

## Native Over Bespoke

| Do | Don't |
|----|-------|
| Use the host's design system and style guide (for example Atlassian's design system inside a Jira or Confluence app), with only subtle extra styling | Ship bespoke widgets or an obviously embedded foreign UI |
| Use in-app dialogs and modals | Use browser `alert()` or `confirm()` |
| Reuse known idioms: spreadsheet-style selection, undo, comment threads | Invent interactions that need instructions |

Ask whether a user of a familiar tool would need instructions. If yes, redesign.

## Responsiveness and Progress

- Check desktop, phone portrait, and phone landscape.
- Stream model responses, and show clearly whether the agent is working or at rest so the app never looks hung.
- Put the working indicator inside the region doing the work, such as the response panel. Never let a spinner take over the whole UI.
- Keep status text short, varied, and written for the user, based on what the system is actually doing. Leave out internal phase names.

## Consistency

Users judge a product by its weakest screen, so hold the same bar on every one. When extending existing work, match its writing style, visual grammar, and scale. If a brief conflicts with what is established, follow the established work and flag the difference. Record settled style rules in the README.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A short intro helps people understand the page" | If the page needs an intro, the layout is unclear. Fix the layout. |
| "Emojis make it friendlier" | They make it look generated. Use icons and plain labels. |
| "A full-screen spinner is simplest" | It hides the rest of the app and looks hung. Scope it to the working region. |
| "The tokens are consistent, so the design is fine" | Look at the rendered screen. Consistency is not composition. |

## See Also

- architecture/design(softwaredev) — the parent: deliberation before building
- code/testing/evaluation/screens(softwaredev) — capturing screens at the sizes users run
- visualization/charts(softwaredev) — charts inside the UI
