---
description: Render ANSI terminal charts from data — bar, line, sparkline, histogram, table
vocabulary: chart visualize graph sparkline histogram plot trend metric compare bar line table render data display summary distribution hbar spark values series braille ansi terminal
pattern: chart|visuali[sz]|graph|sparkline|histogram|plot|bar.?chart|trend|metric
embed_threshold: 0.32
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Charts Way

## chart-tool

Render ANSI terminal charts by piping JSON to `~/.claude/hooks/ways/softwaredev/visualization/charts/chart-tool`.

### Chart Types

| Type | Input | Best For |
|------|-------|----------|
| `spark` | `values: [...]` | Inline trend at a glance |
| `hbar` | `data: {label: val}` | Comparing named quantities |
| `bar` | `data: {label: val}` | Vertical bar comparison |
| `line` | `values: [...]` | Time series, trends (braille) |
| `table` | `data: {label: val}` | Compact labeled values |
| `hist` | `values: [...]` | Distribution of raw values |

### Usage

Pipe JSON via stdin:

```bash
echo '{"type":"hbar","data":{"GET":120,"POST":45},"title":"Requests"}' | ~/.claude/hooks/ways/softwaredev/visualization/charts/chart-tool
```

### JSON Schema

```json
{
  "type": "bar|hbar|spark|line|table|hist",
  "title": "optional title",
  "data": {"label": value},
  "values": [number, ...],
  "labels": ["x1", "x2"],
  "width": 60,
  "height": 15,
  "color": "auto|red|green|blue|cyan|magenta|yellow|white",
  "format": "human",
  "bins": 10
}
```

- `data` is for bar/hbar/table (label→value map; table also accepts string values)
- `values` is for spark/line/hist (number array)
- `format`: `"human"` uses K/M suffixes (e.g., `31k`, `1.2M`). Default uses commas.
- All optional fields have sensible defaults

### When to Use

When asked to visualize data, show trends, compare values, or display metrics — render a chart instead of printing raw numbers. Pick the chart type that fits:

- **Quick trend** → `spark`
- **Compare categories** → `hbar` (few items) or `bar` (many items)
- **Time series** → `line`
- **Distribution** → `hist`
- **Compact summary** → `table`
