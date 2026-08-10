---
name: dag
description: |
  Interactive directed acyclic graph Canvas with draggable panning, rounded nodes, and Braille connections.
  Use when the user wants to visualize dependencies, pipelines, execution plans, lineage, or causal flow.
---

# DAG Canvas

The `dag` renderer provides the `display` scenario. It validates that every edge references an
existing node and that the resulting directed graph is acyclic.

## Inline DAG

The Canvas config may be the DAG document itself:

```typescript
canvas_spawn({
  kind: 'dag',
  scenario: 'display',
  config: JSON.stringify({
    schemaVersion: 1,
    title: 'Research pipeline',
    nodes: [
      { id: 'prices', label: 'Market prices', description: 'Daily OHLCV', color: 'cyan' },
      { id: 'features', label: 'Feature build', color: 'yellow' },
      { id: 'report', label: 'Agent report', color: 'green' },
    ],
    edges: [
      { from: 'prices', to: 'features', label: 'returns' },
      { from: 'features', to: 'report' },
    ],
    layout: {
      direction: 'left-to-right',
    },
  }),
});
```

## File DAG

For larger graphs, create the same DAG document as a JSON file in the user's workspace, then pass
exactly one absolute path field. `dataFile` is an alias of `data_file`; do not provide both and do
not mix file and inline fields.

```typescript
canvas_spawn({
  kind: 'dag',
  scenario: 'display',
  config: JSON.stringify({
    data_file: '/absolute/path/dag.json',
  }),
});
```

## Document Schema

Required paths:

- `schemaVersion`: must be `1`
- `title`: non-empty string, at most 160 characters
- `nodes`: 1 to 500 node objects
- `nodes[*].id`: unique non-empty string, at most 128 characters
- `nodes[*].label`: non-empty string, at most 160 characters
- `edges`: 0 to 2,000 edge objects
- `edges[*].from`: ID of an existing source node
- `edges[*].to`: ID of an existing target node

Optional node fields are `description`, `color`, and scalar `metadata`. Optional edge fields are
`label`, `color`, and scalar `metadata`. Self-edges, duplicate source/target pairs, unknown node
references, and cycles are rejected. Edge labels must be single-line strings. Omit an edge's `color`
to group connections by target: all edges entering one node share a stable generated color, while
edges entering different targets use different color slots. Set `color` only when a specific semantic
color is required. Outgoing edges from one node use separate source ports and bend lanes; incoming
edges may converge on the same target port and retain their shared target color.

Optional layout:

```json
{
  "layout": {
    "direction": "left-to-right",
    "minNodeWidth": 12,
    "maxNodeWidth": 48,
    "minNodeHeight": 3,
    "maxNodeHeight": 8,
    "layerGap": 8,
    "nodeGap": 2,
    "padding": 2
  }
}
```

- `direction`: `left-to-right` or `top-to-bottom`
- `minNodeWidth` / `maxNodeWidth`: per-node adaptive width bounds for content and vertical-layout ports
- `minNodeHeight` / `maxNodeHeight`: per-node adaptive height bounds for content and horizontal-layout ports
- `layerGap`: minimum of 4 to 24 cells; edge labels and outgoing bend lanes expand the actual gap
- `nodeGap`: 1 to 12 cells
- `padding`: 0 to 8 cells

Unknown document fields are rejected.

Node boxes adapt independently to their label, ID, and wrapped description. Edge labels are
centered in a reserved section of the connection, and the actual layer gap grows to fit them.
Only the bendable connection segments use Braille cells; arrowheads are ordinary `→` or `↓`
terminal characters.

## Error Recovery

`canvas_spawn` and `canvas_update` wait for parsing, reference validation, and cycle detection.
Errors are returned by the originating tool as `{ success: false, id, status, error }`, not as DAG
actions. Repair the exact field or edge reported and update the same Canvas ID.

## Interaction

- Left-drag the graph viewport to pan when it overflows the pane.
- Click a node without dragging to toggle selection.
- Arrow keys or `h`/`j`/`k`/`l` pan; the mouse wheel pans vertically.
- Tab and Shift+Tab choose the active node; Space toggles it.
- `Home` resets the viewport, and `c` clears selection.
- `a` attaches the selected neighborhood or visible viewport to LLM context.
- Enter requests DAG analysis; `e` exports the current context as JSON.

When `canvas_wait` is available, call it with this Canvas ID while waiting for an interactive
selection or analysis request. Otherwise, rely on the host's automatic Canvas event delivery.
