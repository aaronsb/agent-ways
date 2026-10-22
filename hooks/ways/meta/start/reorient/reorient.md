---
description: Finding what the user said or wrote in an earlier session and giving it back word for word with where and when it was said, instead of reconstructing a paraphrase, and reorienting read-only
vocabulary: word for word verbatim exact wording not a paraphrase earlier session transcript history what I told you weeks ago my own words back read-only reorient
pattern: \b(exact|verbatim) prompt\b
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Reorientation Is Read-Only

Whether or not the start skill runs, rebuilding context changes nothing. Edit, create, and commit nothing until the user asks for a change. If you spot a fix while summarizing, note it and raise it after the report.

## Finding what the user did before

When the user asks for something they did or wrote before ("find the exact prompt I used"), search the session history and local files and return the verbatim original with its source and date. Do not reconstruct an approximation when the original can be found. A recap covers the original prompt (quoted), what was tried, what worked, and the lessons.

## See Also

- start(meta) — parent: routing the session open to the start skill
- freshness(softwaredev) — docs that lag the code they describe
