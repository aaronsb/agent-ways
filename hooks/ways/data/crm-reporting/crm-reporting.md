---
description: Covers answering business questions with numbers from a CRM or other system of record, such as open pipeline, bookings, win rate, revenue by rep or product, losses, and account history. Read stage and record-type metadata first, find placeholder, test, vendor program, and duplicate-option records, keep resale apart from services and gross apart from margin, lead with the standard figure and then a reconciled cleaned view, and date every number.
vocabulary: crm salesforce hubspot soql opportunity opportunities sales-pipeline open-pipeline bookings closed-won closed-lost win-rate quota forecast rep-performance account-history loss-reasons record-type gross-profit renewals
pattern: \b(sales|open|real|active) pipeline\b|\bwin.?rates?\b|\bclosed.?(won|lost)\b|\bbookings\b|\bsoql\b|\bopportunit(y|ies)\b.{0,40}\b(stage|amount|owner|pipeline)|\b(services|licens\w*|resale)\b.{0,40}\bbook(ed|ings)?\b|\b(booked|bookings)\b.{0,40}\b(services|licens\w*|resale)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Reporting From a CRM

## Read the metadata before the numbers

- Query the stage metadata first. In Salesforce that is `OpportunityStage` with `IsClosed`, `IsWon`, and `ForecastCategoryName`. A stage named like a closed stage can be open, and a closed stage can be cleanup.
- Filter on the won and closed flags, not on guessed stage names.
- List record types and picklist values with record counts. Retired values still sit on old records, and docs list values that no longer exist.
- Confirm a field exists with a describe call before using a field a doc names.

## Find the records that distort totals

| Record | Effect | Handle |
|---|---|---|
| Renewals pre-created in an open stage | Inflate open pipeline | Exclude from pipeline. Report their total separately |
| A cleanup or obsolete closed stage | Inflates losses | Exclude from win rate and say so |
| On-hold deals in an omitted forecast category | Inflate active pipeline | Show in-progress and actively worked side by side |
| Alternative quote options entered as separate deals on the same account and close date | Double count pipeline and losses | Show a second view that keeps only the largest option |
| Vendor program records, such as deal registration or rebates | Look like client deals | Count them and flag them |
| Test records (an internal account or obvious test names) | Pad totals and top-N lists | Flag them. Drop them from rankings |
| Token-amount placeholders and blank amounts | Skew counts and averages | Count them |
| Account rollup fields repeated on every opportunity | Summing them gives nonsense | Sum the opportunity's own amount |

## Split what the business splits

- Keep pass-through resale apart from services. Raw amount rankings can be rankings of resale at a thin margin.
- Show gross profit next to revenue. Check whether cost fields are actuals or a placeholder, such as a fixed percentage of amount filled in by default.
- A grouping field that only applies to one record type leaves a large blank bucket. Report the blank bucket and say why it is blank.

## Fields that mislead

- A default value such as "No" on new records does not mean someone reviewed the record. Blank means never reviewed. Only an explicit positive value is a signal.
- Last-modified dates are useless when an integration user touches every record daily. Use stage history, field history, or activity dates.
- Activity data carries noise: email-open tracking tasks, and one meeting copy per internal attendee.
- A name search can match several accounts and a parent-child tree. Show every match with its totals, or ask which one is meant.
- Created-to-close days for pre-created renewals is not a sales cycle. Split fresh deals from renewals.

## Report it

- Lead with the standard figure a report in the source system would show. Then give the cleaned view, labeled, with the reconciliation ("120 deals, of which 10 are placeholders or vendor program records, leaving 110").
- State the filters in one line.
- Date every number. The data is live and moves during the day.
- Stay read-only. Analysis never writes to the system of record.

## Common rationalizations

| Rationalization | Counter |
|---|---|
| "Open means IsClosed = false." | Check what sits in each open stage. Placeholders can inflate the total. |
| "My cleaned number is more accurate, so I'll lead with it." | Lead with the standard figure. Put yours second and show how they reconcile. |
| "The docs list the stages." | Query the stage metadata. Docs drift. |
| "The field says No, so the deal was reviewed." | Check whether No is the default on new records. |

## See Also

- research(research) — distrust naive reads and verify key figures
