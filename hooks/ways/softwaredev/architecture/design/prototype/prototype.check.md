---
description: flipping an ADR or design to Accepted whose decisive claim is about an external system, performance, latency, or data volume
vocabulary: accept adr ratify external api latency performance cost data volume assumption prototype probe measure
scope: agent
---

## anchor

You are about to accept a decision whose load-bearing claim is about something outside your codebase. Reasoning ratifies; measurement decides.

## check

Before flipping this to Accepted:
- What is the **single load-bearing claim**, stated as a falsifiable prediction (a number, a yes/no), not a vibe?
- Is that claim about an **external system, latency, cost, or data volume** — something you cannot confirm by reading your own code?
- Have you **run the real system** (a throwaway probe, a measured payload, a triggered event) — or are you accepting from reasoning alone?
- If you measured: does the ADR **cite the evidence**? If you didn't: why is this safe to accept unmeasured?
- Did you probe **each** external assumption the design rests on, or just the first one?
