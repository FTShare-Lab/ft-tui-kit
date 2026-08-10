---
name: chart
description: |
  Interactive Ratatui bar and line Canvas backed by a JSON table.
  Use when the user wants a grouped bar chart, line chart, combined chart, selection, or LLM analysis of statistical data.
---

# Chart Canvas

The `chart` renderer provides the `bar-graph` scenario. Despite the scenario name, the data file
decides whether the renderer displays bars, lines, or both.

## Spawn

`config` is a JSON string containing exactly one absolute data path field:

- `data_file`
- `dataFile`, an alias of `data_file`

Do not provide both fields. Create the data file in the user's workspace; do not try to read a
schema from the plugin package.

```typescript
canvas_spawn({
  kind: 'chart',
  scenario: 'bar-graph',
  config: JSON.stringify({
    data_file: '/absolute/path/chart-data.json',
  }),
});
```

## Data Contract

Use this complete document shape:

```json
{
  "schemaVersion": 1,
  "title": "Quarterly revenue",
  "subtitle": "Optional subtitle",
  "table": {
    "name": "revenue_by_quarter",
    "idField": "quarter",
    "rows": [
      { "quarter": "Q1", "actual": 120, "target": 110 },
      { "quarter": "Q2", "actual": 138, "target": 130 }
    ]
  },
  "axes": {
    "x": {
      "field": "quarter",
      "label": "Quarter"
    },
    "y": {
      "label": "Revenue",
      "min": 0,
      "max": 200,
      "format": {
        "decimals": 0,
        "prefix": "$",
        "suffix": "m"
      }
    }
  },
  "series": [
    { "field": "actual", "label": "Actual", "color": "cyan" },
    { "field": "target", "label": "Target", "color": "yellow" }
  ],
  "display": {
    "view": "bar",
    "showLegend": true,
    "showValues": "selected",
    "barWidth": 3,
    "barGap": 1,
    "groupGap": 2
  },
  "metadata": {
    "source": "Optional scalar metadata"
  }
}
```

Required field paths:

- `schemaVersion`
- `title`
- `table.name`
- `table.rows`
- `axes.x.field`
- `axes.y`
- `series[*].field`
- `series[*].label`
- `display`
- `display.view`

Rules:

- `display.view` must be `bar`, `line`, or `both`. Never default to `both`.
- `display.showValues` may be `never`, `selected`, or `always`.
- Every `series[*].field` must reference a finite, non-negative numeric value in every row.
- `axes.y.min`, when present, must be `0`.
- `axes.y.max`, when present, must be a finite number greater than zero.
- `axes.y.format.decimals` must be from 0 to 6.
- `table.idField`, when present, must identify a unique string or number in every row.
- The renderer accepts at most 10,000 rows and 16 series.
- Metadata values must be scalar. Unknown fields are rejected.

## Error Recovery

Both `canvas_spawn` and `canvas_update` wait for the renderer to parse and load the document.
Configuration and data errors are returned by the originating tool, not as Canvas actions:

```json
{
  "success": false,
  "id": "chart-...",
  "status": "error",
  "error": "required field `display.view` is missing"
}
```

Repair the field named by `error`, then update the same Canvas. Do not spawn a second Canvas.

```typescript
canvas_update({
  id: 'chart-...',
  config: JSON.stringify({
    data_file: '/absolute/path/chart-data.json',
  }),
});
```

## Interaction

- Click a bar to select that row and series.
- In line view, use Up/Down to choose the active series and click a category to select it.
- Use Left/Right to pan.
- Use the mouse wheel or `+`/`-` to zoom.
- Press `c` to clear the selection.
- Press `a` to attach selected cells, or the visible range, to LLM context.
- Press Enter to request LLM analysis of selected or visible data.
- Press `e` to export selected or visible rows as a JSON artifact.

When `canvas_wait` is available, call it with this Canvas ID while waiting for an interactive
selection or analysis request. Otherwise, rely on the host's automatic Canvas event delivery.
