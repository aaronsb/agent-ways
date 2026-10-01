#!/usr/bin/env python3
"""Write bench_items.jsonl: 8 synthetic conversations x 6 candidate ways = 48 items.

`expect` is the author's intended answer, used only to sanity-check input
orientation; this is not a labelled evaluation set.
"""
import json
from pathlib import Path

WAYS = {
    "adr": "Agent Decision Records (ADRs) — creating, managing, and referencing ADRs for technical choices, how reversible a decision is, a deliberate deviation from a standard, and superseding an accepted ADR",
    "migrations": "database schema migrations, table and column alterations, rollback procedures",
    "readme": "README authoring, project overview, getting started guide, README structure",
    "api": "designing REST APIs, HTTP endpoints, API versioning, request response structure",
    "incident": "production incidents — something is down or broken, alert triage, escalation tiers and MTTR targets, fixing forward versus rolling back, and what has to exist before an incident closes",
    "calendar": "schedule a meeting, check my availability, block time on my calendar, create calendar events, find a free slot",
    "mermaid": "Mermaid diagrams, flowcharts, sequence diagrams, state diagrams, diagram styling, palette and color choices for light and dark themes",
    "commits": "git commit messages, conventional commit format, scope and subject line, commit body explaining why",
    "testing": "test structure and coverage, fixtures, unit versus integration tests, flaky tests",
    "email": "triage my email, scan inbox for unread messages, classify and filter email threads, what needs a reply",
}

LONG_DIFF = (
    "Here is the current state of the users table migration. The previous migration "
    "0042_add_email_verified added a nullable boolean, and 0043 backfilled it from the "
    "auth provider export, but the backfill ran in a single transaction and locked the "
    "table for four minutes in staging. I want to split it so the column add, the "
    "backfill in batches of 5k rows, and the NOT NULL constraint are three separate "
    "migrations, and each one has a down() that actually restores the previous shape "
    "rather than raising IrreversibleMigration. Also the index on (tenant_id, email) "
    "should be created CONCURRENTLY, which means it cannot run inside the migration "
    "transaction, so the framework's atomic=True default has to be turned off for that "
    "file only. Can you check what the ordering should be and whether the down path for "
    "the constraint is safe if the backfill has partially completed?"
)

CONVOS = [
    ("db-migration", [
        ("user", "The staging deploy took the site down for four minutes last night during the migration step."),
        ("assistant", "That lines up with migration 0043: it backfills email_verified in one transaction and holds a lock on users the whole time. Splitting it would remove the long lock."),
        ("user", LONG_DIFF),
        ("assistant", "Order: 0044 adds the nullable column, 0045 backfills in 5k batches outside a single transaction, 0046 adds NOT NULL after a validation query, and the concurrent index goes in its own non-atomic file. The down for 0046 only drops the constraint, so a partial backfill is safe to roll back."),
    ], {"migrations": "yes", "incident": "yes", "adr": "no", "readme": "no", "calendar": "no", "api": "no"}),
    ("api-versioning", [
        ("user", "We need to add a v2 of the /orders endpoint because mobile clients want cursor pagination and the old offset pagination can't change."),
        ("assistant", "A path prefix /v2/orders keeps v1 untouched. Cursor pagination returns next_cursor in the body and accepts ?cursor= and ?limit=."),
        ("user", "Should the cursor be opaque or should it expose the created_at timestamp? Partners have asked to construct cursors themselves."),
        ("assistant", "Opaque, base64 of (created_at, id). Exposing raw timestamps makes the sort key part of the public contract, which blocks changing it later."),
    ], {"api": "yes", "adr": "yes", "migrations": "no", "calendar": "no", "email": "no", "mermaid": "no"}),
    ("readme-onboarding", [
        ("user", "New contributors keep asking how to run the project locally. The README is just a title and a license badge."),
        ("assistant", "A getting-started section with prerequisites, clone, install, and a first command that proves it works would cover most of those questions."),
        ("user", "Write that section. Prereqs are Rust 1.80, cmake, and a C++ compiler. The first command should be `make test`."),
        ("assistant", "Drafted: Prerequisites lists Rust 1.80+, cmake 3.20+, and a C++17 compiler; Quick start runs git clone, cd, make setup, then make test, and says what a passing run prints."),
    ], {"readme": "yes", "api": "no", "incident": "no", "calendar": "no", "migrations": "no", "commits": "no"}),
    ("calendar-assistant", [
        ("user", "Can you find 30 minutes with Priya and Marcus next week? Not Monday."),
        ("assistant", "Tuesday 14:00-14:30 and Thursday 10:00-10:30 are free for all three of you. Priya has a hard stop at 15:00 Tuesday."),
        ("user", "Thursday. Call it 'Q3 roadmap sync' and attach the planning doc."),
        ("assistant", "Created 'Q3 roadmap sync' Thursday 10:00-10:30 with Priya and Marcus, planning doc attached, invites sent."),
    ], {"calendar": "yes", "email": "no", "adr": "no", "migrations": "no", "api": "no", "testing": "no"}),
    ("outage", [
        ("user", "Checkout is returning 502s for about 30% of requests since 09:12. PagerDuty fired twice."),
        ("assistant", "The 502s come from the payments sidecar; its pods are OOMKilled every few minutes since the 09:05 deploy that raised the batch size."),
        ("user", "Do we roll back the deploy or bump the memory limit and push forward?"),
        ("assistant", "Roll back: it is one command, restores a known-good state, and the memory change needs a load test we have not run. Then write the timeline while it is fresh."),
    ], {"incident": "yes", "migrations": "no", "readme": "no", "calendar": "no", "mermaid": "no", "adr": "no"}),
    ("diagram", [
        ("user", "The architecture doc has an ASCII box diagram of the hook pipeline that nobody can read in dark mode."),
        ("assistant", "A Mermaid flowchart renders natively on GitHub and adapts to the theme if we avoid hard-coded fill colors."),
        ("user", "Do it: SessionStart -> core.md, UserPromptSubmit -> matcher -> ways, PreToolUse -> command/file lanes."),
        ("assistant", "Replaced it with a flowchart LR of three subgraphs, one per hook event, using classDef with theme-neutral stroke colors only."),
    ], {"mermaid": "yes", "readme": "no", "incident": "no", "calendar": "no", "email": "no", "api": "no"}),
    ("commit-tests", [
        ("user", "The flaky test in scan_tests.rs failed again on CI. It passes locally every time."),
        ("assistant", "It reads a temp dir without sorting the entries, and directory order differs between ext4 on CI and APFS locally. Sorting the paths makes it deterministic."),
        ("user", "Fixed. Commit it."),
        ("assistant", "Staged scan_tests.rs. Proposed message: 'test(scan): sort temp dir entries so ordering does not depend on the filesystem'."),
    ], {"testing": "yes", "commits": "yes", "incident": "no", "calendar": "no", "readme": "no", "migrations": "no"}),
    ("inbox", [
        ("user", "Morning. Anything urgent in my inbox?"),
        ("assistant", "Three unread need a reply: legal's contract redline (due today), a customer escalation from Acme, and the offsite headcount question. The rest are newsletters."),
        ("user", "Draft a reply to the Acme one saying we're on it and will update by end of day."),
        ("assistant", "Draft created in the Acme thread: acknowledges the export failure, says engineering is investigating, promises an update by 17:00."),
    ], {"email": "yes", "calendar": "no", "adr": "no", "api": "no", "migrations": "no", "mermaid": "no"}),
]


def main():
    out = Path(__file__).resolve().parent / "bench_items.jsonl"
    with out.open("w") as f:
        for cid, turns, ways in CONVOS:
            t = [{"role": r, "text": x} for r, x in turns]
            for wid, expect in ways.items():
                f.write(json.dumps({"id": f"{cid}/{wid}", "summary": WAYS[wid],
                                    "turns": t, "expect": expect}) + "\n")
    print(out)


if __name__ == "__main__":
    main()
