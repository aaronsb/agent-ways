---
description: triage my email, scan inbox for unread messages, classify and filter email threads, what needs a reply
vocabulary: triage inbox unread email scan messages filter noise priority action required check email review mail urgent reply thread
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Email Triage

Scan emails across all accounts for a given time period, filter noise, cross-reference with calendar and tasks, and present an intelligent summary.

## Workflow

1. **Determine date range.** Parse relative references ("last 24 hours", "since Monday") into timestamps.
2. **Fetch inbox per account.** Get messages for the period, read full content, extract headers (From, To, Subject, Date), note thread IDs.
3. **Check response status.** Fetch sent messages for the same period. If an inbox message's thread has a sent reply with a later timestamp, the user already responded.
4. **Fetch calendar** for the same period to cross-reference meetings with email threads.
5. **Cross-reference with open tasks.** For each action-required email:
   - Does an open task already cover this? Note the match.
   - Does this email resolve an existing task? Prepare a completion suggestion.
   - Does this email create a new obligation? Prepare a creation suggestion.

## Presentation Structure

| Section | Content |
|---------|---------|
| **Action Required** | Emails needing response that the user has NOT replied to, ranked by urgency |
| **Already Addressed** | Messages with sent replies found in thread. Brief note of what was said. |
| **Calendar Cross-Reference** | Upcoming events with related email threads noted |
| **Sent Items of Note** | Outbound emails that may generate follow-ups |
| **Open Tasks in Play** | Tasks related to people/clients/topics surfaced in this triage |
| **Suggested Task Actions** | Create / Complete / Update / Stale? (see tasks way) |
| **Filtered as Noise** | Brief count of what was excluded |
| **Suggestions** | Scheduling recommendations, deadline reminders, follow-up nudges |

## Filtering Rules

**Filter OUT:** purchase receipts, shipping notifications, marketing, automated alerts, recruiting spam, subscription renewals.

**Filter IN:** real people expecting responses, meeting-related threads, deadlines, invitations, anything mentioning the user by name.

**When uncertain:** include with a note rather than filtering out.

## See Also

- email/drafting(ea) — drafting workflow for outbound email
- trust/voice(meta) — voice and attribution in email
