---
description: When to surface publishing a repo's onboarding guide as a share link a teammate's own agent can open directly, instead of asking them to clone the repo and hunt for the file
vocabulary: onboarding share link teammate colleague new engineer collaborator handoff hand off bring up to speed get started repo working model share guide invite agent context publish onboard
pattern: onboard|onboarding guide|share .{0,30}(guide|link)|bring .{0,30} up to speed|hand(ing)? off|get .{0,30} started on (the|this) repo
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Onboarding Share

A teammate joining a repo or engagement rarely lacks the *knowledge* — it's usually sitting in an onboarding guide already. What they lack is a cheap way to get it into **their own agent's head**. "Clone the repo, find the file, read it, paste the relevant bits to your Claude" is four steps of friction at exactly the moment someone is least oriented.

If the repo has an onboarding guide (the `ONBOARDING.md` convention), it can be published to a share link the teammate's Claude Code opens directly — one click, and their agent already knows the working model. That bridge is what collapses the friction. The mechanics live in the share tool; this way is only about *when to reach for it*.

## When to surface it

Offer it as an **optional next step**, not a default action, when:

- A new collaborator is coming onto a repo whose working model isn't self-evident from the code (a non-obvious source→build→release flow, feedback routing, where to file what).
- Someone asks "how do I get started here?" — in Slack, a handoff, a kickoff.
- You just wrote or revised an onboarding guide and a specific person needs it.

Frame it the way you'd frame any helpful-but-not-asked-for move: name it, say what it does, and let the human pull the trigger.

## Treat it as an outward publish

This copies the guide to org-hosted infrastructure **outside git**. That puts it under the same bar as any external share:

- **Confirm before publishing.** Don't auto-share. Surface the offer and wait for go.
- **It's a snapshot, not a live mirror.** Edits to the guide after publishing don't propagate — the link keeps serving the old content until you re-publish/update the *same* link. So the habit is: edit the guide → commit → refresh the link.
- **Right guide for the audience.** A client/engagement-specific guide and a scrubbed generic template are different artifacts. Share the one that matches who's receiving it.

## The honest boundary

You know the tool's behavior; you usually *don't* have visibility into the hosted side's access scope — who in the org can open the link, how long it's retained. If that matters before sending it to a specific person, say so and have the human verify org-side rather than guessing.

## See Also

- skills(meta) — the share capability itself is a tool/skill; this way only guides when to surface it
- trust(meta) — outward publishing rides on borrowed reputation; the confirm-first bar comes from there
