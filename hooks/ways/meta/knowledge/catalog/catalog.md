---
description: listing every way that exists, the full catalog of way paths by domain, which ways are available and where one lives
vocabulary: catalog list available ways index which exist inventory every all paths domain tree children
pattern: (list|show|which|what) .{0,20}ways\b|ways? (catalog|index|inventory)|available ways
macro: prepend
scope: agent
requires: ["Bash(awk:*)", "Bash(find:*)", "Bash(sed:*)", "Bash(sort:*)"]
refire: 0.3
---
<!-- epistemic: convention -->
# Ways Catalog

The list above names every way in the shipped corpus by its path. A path with a slash is a child of the way before the slash. Project ways under `.claude/ways/` and user ways under `$XDG_CONFIG_HOME/agent-ways/ways/` are not listed here.

To read one way, call `ways_read` with its path. To find ways by topic, call `ways_search`.

## See Also

- knowledge(meta) — parent: how ways fire and where they live
