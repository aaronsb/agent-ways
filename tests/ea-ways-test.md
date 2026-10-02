# Executive Assistant Ways Integration Test

## Instructions for Claude

Read this file with the Read tool — do NOT have the user paste it into chat.

You are running an integration test for the EA progressive disclosure tree. This test verifies that the correct EA ways fire for typical executive assistant prompts, that progressive disclosure works (parent before child), and that sibling ways don't crowd each other.

**Your role**: Follow each step in order. Announce what step you are on, perform the action, then report the result against the expected outcome. Wait for the user to complete each USER step before moving on.

### How to verify what fired

After each user prompt, check **two** signals:

1. **System-reminders injected into your context** — look for any new `<system-reminder>` blocks that appeared after the user's message. These contain the actual way content. Report headings and content you see.

2. **Embedding scoring via CLI** — run this from `~/.claude/`:
   ```bash
   ways author match "the exact prompt the user typed"
   ```
   This prints the ranked candidates with peak, share, confirm and outcome (`fired ✓`, `< gate`, `< confirm`). Report the top 3 results.

Use **both** signals: system-reminders confirm delivery; `ways author match` confirms scoring.

### Report format

```
Step N: [description]
Result: PASS / FAIL / UNEXPECTED
Injected: [what system-reminder content appeared, or "none"]
Embedding top 3:
  #1 way/id  score  description snippet
  #2 way/id  score  description snippet
  #3 way/id  score  description snippet
Detail: [assessment vs expected]
```

After reading this file, begin with Step 1.

---

## Part A: Root and Direct Children

### Step 1 — Root EA way fires on broad prompt

> **USER**: Type exactly: `I need help managing my workspace today`

> **CLAUDE**: Check system-reminders for EA root content (safety rules table, core principles), then run:
> ```bash
> cd ~/.claude && ways author match "I need help managing my workspace today"
> ```
> Report injected content and top 3 matches.

**Expected**: The root EA way fires (threshold 1.8, vocabulary includes: assistant, workspace, manage). You should see the safety rules table and core principles — NOT email triage structure, drafting style guide, or calendar workflow specifics (those are children). Note: the prompt intentionally avoids mentioning specific domains (email, calendar) to prevent pre-firing child ways.

---

### Step 2 — Email triage child fires on triage prompt

> **USER**: Type exactly: `triage my email from the last 24 hours`

> **CLAUDE**: Check system-reminders for email triage content (presentation structure table, filtering rules), then run:
> ```bash
> cd ~/.claude && ways author match "triage my email from the last 24 hours"
> ```
> Report injected content and top 3 matches.

**Expected**: The email triage way fires (threshold 2.0, vocabulary: triage, email, inbox, scan). You should see:
- "## Presentation Structure" table with section descriptions
- "## Filtering Rules" with filter-in and filter-out lists

The root EA way should NOT re-fire (marker exists from Step 1).

---

### Step 3 — Calendar fires on scheduling (not briefing)

> **USER**: Type exactly: `schedule a meeting with the team for tomorrow afternoon`

> **CLAUDE**: Check system-reminders for calendar content (scheduling workflow, availability checking), then run:
> ```bash
> cd ~/.claude && ways author match "schedule a meeting with the team for tomorrow afternoon"
> ```
> Report injected content and top 3 matches.

**Expected**: The calendar way fires (threshold 2.0, vocabulary: schedule, meeting, calendar). You should see availability-check-first workflow and timezone guidance. The briefing way should NOT fire for a scheduling request — if it does, that's a vocabulary overlap issue.

---

### Step 4 — Comms fires on chat prompt

> **USER**: Type exactly: `check my teams chat for any unread messages`

> **CLAUDE**: Check system-reminders for comms content (read-safe/send-approval distinction), then run:
> ```bash
> cd ~/.claude && ways author match "check my teams chat for any unread messages"
> ```
> Report injected content and top 3 matches.

**Expected**: The comms way fires (threshold 2.0, vocabulary: teams, chat, unread, message). You should see the read-safe/send-approval distinction and the "As a Context Layer" section.

---

### Step 5 — Tasks fires on task management prompt

> **USER**: Type exactly: `what tasks are overdue and what should I prioritize`

> **CLAUDE**: Check system-reminders for task lifecycle content (suggest-first patterns), then run:
> ```bash
> cd ~/.claude && ways author match "what tasks are overdue and what should I prioritize"
> ```
> Report injected content and top 3 matches.

**Expected**: The tasks way fires (threshold 2.0, vocabulary: task, overdue, priority, pending). You should see the four lifecycle patterns with example suggestion formats.

---

## Part B: Leaf Ways (Depth 2)

### Step 6 — Email drafting grandchild fires

> **USER**: Type exactly: `draft a reply to that email from the client`

> **CLAUDE**: Check system-reminders for drafting content (style calibration, anti-patterns), then run:
> ```bash
> cd ~/.claude && ways author match "draft a reply to that email from the client"
> ```
> Report injected content and top 3 matches.

**Expected**: The email/drafting way fires (threshold 2.5, vocabulary: draft, reply, compose, style, tone). You should see:
- "## Style Calibration" with observation checklist
- "## Anti-Patterns to Avoid" with AI-generated tells
- "## Iteration Rule" (don't re-create draft on every revision)

The email triage parent may or may not re-fire (depends on marker state). The drafting content is the important signal.

---

### Step 7 — Meeting recap fires on transcript prompt

> **USER**: Type exactly: `pull the transcript from yesterday's standup meeting`

> **CLAUDE**: Check system-reminders for recap content (available content table), then run:
> ```bash
> cd ~/.claude && ways author match "pull the transcript from yesterday's standup meeting"
> ```
> Report injected content and top 3 matches.

**Expected**: The comms/recap way fires (threshold 2.5, vocabulary: recap, transcript, summary, meeting). You should see:
- "## What's Available" table
- "## Using Transcripts" section
- "## Meeting Continuity" for recurring meetings

---

### Step 8 — Time tracking fires on EOD prompt

> **USER**: Type exactly: `log my time for today, I'm wrapping up`

> **CLAUDE**: Check system-reminders for time tracking content (logging workflow), then run:
> ```bash
> cd ~/.claude && ways author match "log my time for today, I'm wrapping up"
> ```
> Report injected content and top 3 matches.

**Expected**: The tasks/time way fires (threshold 2.5, vocabulary: time, log, hours, billable, EOD, wrap up). You should see the 4-step logging workflow (review work → match clients → propose entries → log on approval).

---

## Part C: Differentiation Tests

These test that semantically close ways fire the RIGHT one, not the wrong sibling.

### Step 9 — Intelligence wins on meeting prep (not briefing)

> **USER**: Type exactly: `prepare me for my 2pm meeting with the client`

> **CLAUDE**: Check system-reminders — did you get intelligence or briefing content? Then run:
> ```bash
> cd ~/.claude && ways author match "prepare me for my 2pm meeting with the client"
> ```
> Report injected content and top 3 matches.

**Expected**: The intelligence way fires (description includes "prepare me for a meeting"). You should see the meeting prep checklist (calendar details, recent email threads, chat history, previous occurrence, shared documents, open tasks). The briefing way may also fire — if so, check whether intelligence scored higher (it should).

---

### Step 10 — Briefing wins on catch-me-up (not email triage)

> **USER**: Type exactly: `what does my day look like, catch me up`

> **CLAUDE**: Check system-reminders — did you get briefing or email triage content? Then run:
> ```bash
> cd ~/.claude && ways author match "what does my day look like, catch me up"
> ```
> Report injected content and top 3 matches.

**Expected**: The briefing way fires (description includes "catch me up", "what does my day look like"). You should see:
- "## Standard Roles" table (inbox-scout, ops-scout, lead)
- "## Briefing Structure" priority-ordered sections
- "## After the Briefing" task mutation suggestions

Email triage should NOT be the primary way for this prompt.

---

## Part D: Negative and Crowding Tests

### Step 11 — No false positive on unrelated prompt

> **USER**: Type exactly: `explain how the Rust borrow checker works`

> **CLAUDE**: Check system-reminders for any EA content, then run:
> ```bash
> cd ~/.claude && ways author match "explain how the Rust borrow checker works"
> ```
> Report whether any EA ways appear in the top results.

**Expected**: No EA ways fire. This prompt has zero overlap with EA vocabulary. If an EA way fires, that's a false positive — report which one and its score.

---

### Step 12 — No cross-domain crowding with softwaredev

> **USER**: Type exactly: `review the code quality and refactor the authentication module`

> **CLAUDE**: Check system-reminders for what fired, then run:
> ```bash
> cd ~/.claude && ways author match "review the code quality and refactor the authentication module"
> ```
> Report top 5 matches — are any EA ways present?

**Expected**: Softwaredev ways should fire (code quality, possibly security). NO EA ways should fire — "review", "authentication", and "module" should not pull EA ways into a code review context.

---

## Part E: Summary

### Step 13 — Compile results

> **CLAUDE**: Compile a summary table:
>
> | Step | Test | Expected Winner | Result |
> |------|------|----------------|--------|
> | 1 | Root fires on broad prompt | ea/ (root) | ? |
> | 2 | Email triage child fires | ea/email | ? |
> | 3 | Calendar fires (not briefing) | ea/calendar | ? |
> | 4 | Comms fires on chat | ea/comms | ? |
> | 5 | Tasks fires on overdue | ea/tasks | ? |
> | 6 | Drafting grandchild fires | ea/email/drafting | ? |
> | 7 | Recap fires on transcript | ea/comms/recap | ? |
> | 8 | Time tracking fires on EOD | ea/tasks/time | ? |
> | 9 | Intelligence wins over briefing | ea/intelligence | ? |
> | 10 | Briefing wins on catch-me-up | ea/briefing | ? |
> | 11 | No false positive | Nothing fires | ? |
> | 12 | No cross-domain crowding | Softwaredev only | ? |
>
> Report pass/fail count and observations about:
> - Whether progressive disclosure delivers parent content before child content
> - Whether semantically close siblings (briefing vs calendar, briefing vs intelligence) differentiate correctly
> - Whether leaf ways (depth 2) fire at appropriate specificity without crowding parents
> - Whether EA ways stay silent during unrelated softwaredev workflows
