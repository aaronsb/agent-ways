---
description: Render Mermaid diagrams as terminal art using mmaid — flowcharts, sequences, state machines, ER diagrams, pie charts, gantt, git graphs, and more
vocabulary: mermaid diagram flowchart sequence state class er entity relationship pie chart gantt timeline kanban mindmap git graph block treemap quadrant render terminal visualize architecture
pattern: mermaid|diagram|flowchart|sequence.*diagram|state.*diagram|er.*diagram|class.*diagram|gantt|mindmap|visuali[sz]e.*architecture
embed_threshold: 0.35
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Terminal Diagrams with mmaid

## Finding mmaid

Resolution order — use the first one found:

1. **System PATH**: `mmaid` (installed via AUR, brew, `go install`)
2. **XDG cache**: `~/.cache/claude-ways/user/mmaid` (downloaded by our tooling)
3. **Not found**: suggest installation

```bash
# Check if available
command -v mmaid || ~/.cache/claude-ways/user/mmaid --version
```

If not installed:
- **Arch Linux**: `yay -S mmaid` (AUR, when published)
- **Go**: `go install github.com/aaronsb/mmaid-go/cmd/mmaid@latest`
- **Download**: `bash ~/.claude/tools/mmaid/download-mmaid.sh`

## Usage

Pipe Mermaid syntax via stdin:

```bash
echo 'flowchart LR
    A[Start] --> B{Decision}
    B -->|yes| C[Done]
    B -->|no| D[Retry]' | ~/.cache/claude-ways/user/mmaid -t blueprint
```

Or render a file: `mmaid diagram.mmd -t slate`

## Choosing the Right Diagram Type

| Content | Use | Not |
|---------|-----|-----|
| Request/response flows, temporal sequences | `sequenceDiagram` | flowchart |
| State transitions, lifecycles | `stateDiagram-v2` | flowchart |
| Decision logic, branching paths | `flowchart` | sequence |
| Class/entity relationships | `classDiagram` or `erDiagram` | flowchart |
| Project schedules | `gantt` | flowchart |
| Git branching strategies | `gitGraph` | flowchart |
| Proportional breakdown | `pie` | bar chart |
| 2x2 matrix/prioritization | `quadrantChart` | flowchart |
| Hierarchical exploration | `mindmap` | flowchart |
| Task boards | `kanban` | table |
| Chronological events | `timeline` | gantt |
| Data comparison | `xychart-beta` | pie |
| Proportional area | `treemap-beta` | pie |
| System architecture | `block-beta` or `flowchart` | sequence |

The most common mistake is using flowchart for everything. If the content has a time axis, it's a sequence diagram. If things transition between states, it's a state diagram.

## Themes

Use `-t THEME` for color. Recommended for terminal readability:

| Theme | Style |
|-------|-------|
| `blueprint` | Blue technical drawing (solid backgrounds) |
| `slate` | Grey neutral (solid backgrounds) |
| `gruvbox` | Warm retro (solid backgrounds) |
| `monokai` | Dark with vivid highlights |
| `mono` | Black and white, no color |
| `amber` | Retro terminal amber |

Themes with solid backgrounds (`blueprint`, `slate`, `sunset`, `gruvbox`, `monokai`) support depth-based region coloring in subgraphs.

## Flags

| Flag | Effect |
|------|--------|
| `-t THEME` | Color theme |
| `-a` / `--ascii` | ASCII-only (no Unicode) |
| `-m` / `--markdown` | Wrap output in fenced code block |
| `--insert FILE:LINE` | Insert output into file after line N |
| `--padding-x N` | Horizontal node padding (default: 4) |
| `--padding-y N` | Vertical node padding (default: 2) |
| `--sharp-edges` | Sharp corners on edge routing |
| `--demo TYPE` | Show sample diagram |

## When to Use mmaid vs chart-tool

- **mmaid**: Structural diagrams — architecture, flows, relationships, states, schedules
- **chart-tool**: Data visualization — bar charts, sparklines, histograms, line plots

If the user asks to "visualize" something, consider whether the data is structural (mmaid) or quantitative (chart-tool).

## GitHub Compatibility Note

When writing Mermaid for GitHub markdown (not terminal rendering), use `<br>` instead of `\n` for line breaks in node labels — GitHub's renderer doesn't support `\n`.
