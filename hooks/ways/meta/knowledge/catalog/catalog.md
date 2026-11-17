---
pattern: \bways? (catalog|index|inventory)\b|\bavailable ways\b|\b(installed|which|what) ways (are|exist|do (we|you) have)\b|\blist (all|every) (the )?(installed |available )?ways?\b
macro: prepend
scope: agent
requires: ["Bash(awk:*)", "Bash(find:*)", "Bash(sed:*)", "Bash(sort:*)"]
refire: 0.3
---
<!-- epistemic: convention -->
# Ways Catalog

The list above names every way in the shipped corpus by its id, relative to its domain. An id with a slash is a child of the way before the slash. A domain marked "also a way" has a way of its own, whose id is the domain name. Project ways under `.claude/ways/` and user ways under `$XDG_CONFIG_HOME/agent-ways/ways/` are not listed here.

To read one way, call `ways_read` with its full id, such as `meta/knowledge/catalog`. To find ways by topic, call `ways_search`.

## See Also

- knowledge(meta) — parent: how ways fire and where they live
