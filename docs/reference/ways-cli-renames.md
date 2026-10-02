# ways CLI: renamed commands

The commands were regrouped with no aliases kept ([ADR-507](../architecture/platform/ADR-507-the-ways-commands-regroup-into-operator-commands-and-six-groups-names-another-process-calls-stay-fixed.md)). An old name now fails with a usage error. Commands not listed here kept their names.

| Old | New |
|---|---|
| `ways config show [--json] [--effective]` | `ways settings list [--json] [--effective]` |
| `ways config path` | `ways settings list --json` |
| `ways config init` | removed: `ways settings set` creates the file, `ways settings emit` prints the canonical one |
| `ways config targets` | `ways target list` |
| `ways config target plan\|add\|enable\|disable\|remove <dir>` | `ways target plan\|add\|enable\|disable\|remove <dir>` |
| `ways disable <id>` | `ways settings set ways.project.<id> false` |
| `ways disable --list` | `ways settings list ways.project` |
| `ways enable <id>` | `ways settings unset ways.project.<id>` |
| `ways agent use <profile> [--model <id>]` | `ways settings set gate.engine <profile>`, `ways settings set gate.profiles.<profile>.model <id>` |
| `ways agent mode <mode>` | `ways settings set gate.mode <mode>` |
| `ways agent config` | `ways settings list gate --effective` |
| `ways list` | `ways session ways` |
| `ways introspect list\|replay\|live\|dump\|fires` | `ways session list\|replay\|live\|dump\|fires` |
| `ways reset` | `ways session reset` |
| `ways lint\|template\|match\|tree\|siblings\|suggest\|graph\|reflow` | `ways author lint\|template\|match\|tree\|siblings\|suggest\|graph\|reflow` |
| `ways permissions audit` | `ways author permissions` |
| `ways tune` | `ways tune locale` |
| `ways tune-precision` | `ways tune precision` |
| `ways stats` | `ways tune stats` |
| `ways language` | `ways tune language` |
