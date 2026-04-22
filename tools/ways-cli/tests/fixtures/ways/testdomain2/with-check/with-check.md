---
description: supply chain dependency security audit
vocabulary: dependency supply chain package audit vulnerability npm pip cargo crate
pattern: (?i)(supply.?chain|dependency|audit|vulnerability)
commands: ^(npm|pip|cargo)\ (install|add)
scope: agent
refire: 0.15
---
# Supply Chain

Audit dependencies before adding them.
