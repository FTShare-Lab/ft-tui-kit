---
name: canvas
description: |
  **The primary skill for terminal TUI components.** Covers spawning, controlling, and interacting with terminal canvases.
  Use when displaying financial candlesticks, market tables, security snapshots, news lists, bar/line charts, or dependency DAGs.
---

# ft financial canvas Toolkit

**Start here when using terminal canvases.** This skill covers the overall workflow, canvas types, and IPC communication.

## Example Prompts

Try asking your coding agent things like:

**Market analysis:**

- "Show the recent candlesticks and volume for 000001.SZ"
- "Compare the top securities in an interactive market table"

**Structured data:**

- "Plot quarterly revenue and cost as grouped bars"
- "Visualize this dependency graph and let me select nodes"

## Overview

Canvas provides interactive terminal displays (TUIs) that can be spawned and controlled via Canvas tools. Each canvas type supports multiple scenarios for different interaction modes. Packaged renderers capture mouse input by default and send prompt-worthy selections back as context.

## Available Canvas Types

| Canvas              | Purpose                                                                                         | Scenarios                          |
| ------------------- | ----------------------------------------------------------------------------------------------- | ---------------------------------- |
| `candlesticks`      | Inspect mainland stock candles/volume and request chart analysis                                | `kline`                            |
| `market-table`      | Inspect FTShare quote rankings and select securities                                            | `quotes`                           |
| `news-list`         | Search recent FTShare news, open sources, highlight stories, and request explanations           | `search`                           |
| `security-snapshot` | Inspect one FTShare security's market, performance, valuation, and capitalization metrics       | `overview`                         |
| `chart`             | Inspect grouped bars, lines, or both from a JSON table                                          | `bar-graph`                        |
| `dag`               | Inspect dependency graphs with draggable panning and selectable nodes                           | `display`                          |

## Canvas Tools

### `canvas_spawn`

Spawn a canvas in a tmux split pane.

```typescript
canvas_spawn({
  kind: 'chart',
  scenario: 'bar-graph',
  config: JSON.stringify({
    data_file: '/absolute/path/chart-data.json',
  }),
})
// Returns: { success: true, id: "chart-1234567890-abc123", ... }
```

The call waits for the renderer to apply the initial config. On validation or data-load failure it
returns `{ success: false, id, error }`; use that `id` with `canvas_update` rather than spawning a
second Canvas.

### `canvas_update`

Send updated config to a running canvas.

```typescript
canvas_update({
  id: 'chart-1234567890-abc123',
  config: JSON.stringify({ data_file: '/absolute/path/updated-chart-data.json' }),
})
// Returns only after this update is accepted or rejected:
// { success: true, id, status: "ready" }
// { success: false, id, status: "error", error: "..." }
```

### `canvas_selection`

Get the renderer-defined selection from a canvas.

```typescript
canvas_selection({ id: 'chart-1234567890-abc123' });
```

### `canvas_content`

Get full content from a renderer that advertises the `content` capability. The current public
renderers do not expose editable content; keep this generic tool for compatible custom renderers.

### `canvas_close`

Close a running canvas.

```typescript
canvas_close({ id: 'chart-1234567890-abc123' });
```

### `canvas_wait` (Codex)

After spawning a Canvas that requires user input, call `canvas_wait` to keep the current Codex turn
open until the renderer emits context or an action. Pass `id` to wait for one Canvas, or omit it to
wait for any Canvas owned by this thread. A timeout is normal; call it again while interaction is
still expected.

```typescript
canvas_wait({ id: 'chart-1234567890-abc123', timeout_ms: 30000 });
```

Treat an `action` event's `text` as the user's requested next step. Treat a `context` event as
additional task context. Context not consumed by `canvas_wait` is attached to a later user prompt by
the Codex hook. OpenCode does not expose `canvas_wait`; it routes both event types through its native
session adapter.

### `canvas_layout`

Arrange one to four widgets from the current session without exposing raw tmux commands. Supported
presets are `single`, `columns`, `rows`, `main-left`, `main-right`, `main-top`, `main-bottom`, and
`grid`.

```typescript
canvas_layout({
  layout: 'columns',
  ids: ['chart-a', 'chart-b'],
  focus: 'chart-a',
});
```

See the `canvas-layout` skill for layout selection and multi-Canvas workflow.

## v2 Socket Communication

Interactive canvases communicate via two Unix domain sockets: `control.sock` for lifecycle/config/RPC and `event.sock` for semantic events.

**Renderer -> Host event examples:**

```typescript
{ type: "selection", payload: { label, text, data, delivery: "context" } }
{ type: "context", payload: { label, text, data } }
{ type: "action", payload: { label, prompt, delivery: "queue" } }
```

**Host -> Renderer control examples:**

```typescript
{ type: "update", requestId, payload: { config } }
{ type: "request.selection", requestId, payload: {} }
{ type: "close", payload: { reason } }
```

Renderers acknowledge config application with a `ready` or `error` control frame carrying the
same `requestId`. Config failures belong in the originating tool result, not in an `action` event.

## Chart Data Contract

For `kind: "chart"`, config must contain exactly one absolute JSON path under `data_file` or
`dataFile`. Create that data file in the user's workspace; do not read a schema from the plugin
package. Use this shape:

```json
{
  "schemaVersion": 1,
  "title": "Quarterly revenue",
  "table": {
    "name": "revenue_by_quarter",
    "idField": "quarter",
    "rows": [
      { "quarter": "Q1", "actual": 120, "target": 110 },
      { "quarter": "Q2", "actual": 138, "target": 130 }
    ]
  },
  "axes": {
    "x": { "field": "quarter", "label": "Quarter" },
    "y": { "label": "Revenue", "min": 0, "format": { "decimals": 0 } }
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
  }
}
```

Required paths are `schemaVersion`, `title`, `table.name`, `table.rows`, `axes.x.field`, `axes.y`,
`series[*].field`, `series[*].label`, `display`, and `display.view`. `display.view` must be `bar`,
`line`, or `both`; never assume `both`. Series row values must be finite, non-negative numbers.
Unknown fields are rejected. On `{ success: false, id, error }`, repair the named field and call
`canvas_update` for that same `id`.

## Requirements

- **tmux**: Canvas spawning requires a tmux session
- **Terminal with mouse support**: For click-based interactions
- **Bun**: Runtime for executing canvas commands

## Skills Reference

| Skill               | Purpose                                                             |
| ------------------- | ------------------------------------------------------------------- |
| `candlesticks`      | Candlestick config, interaction, and tool-result error recovery     |
| `market-table`      | FTShare quote normalization, table contract, and interaction        |
| `news-list`         | FTShare semantic news search, stable-ID highlights, and interaction |
| `security-snapshot` | FTShare security-info normalization and snapshot interaction        |
| `canvas-layout`     | Safe multi-Canvas tmux layouts and comparison workflow              |
| `chart`             | Inline chart data contract, view selection, and error recovery      |
| `dag`               | Inline/file DAG schema, topology validation, panning, and selection |
