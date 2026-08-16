# DAG Renderer

Standalone Rust/Ratatui renderer for directed acyclic graphs over the `ft financial canvas` v2
socket protocol. Nodes use rounded terminal borders and connections are routed with Braille line
cells.

## Config Sources

Provide the DAG document directly as the Canvas config, or provide exactly one absolute JSON path:

```json
{ "data_file": "/absolute/path/dag.json" }
```

`dataFile` is accepted as an alias. File config cannot contain inline nodes or other fields. The
file contains the same document shape accepted inline. The contract is documented in
`dag-config.schema.json` and the packaged `dag` skill.

Every document requires `schemaVersion`, `title`, `nodes`, and `edges`. Node IDs must be unique;
edges must reference existing nodes; self-edges, duplicate edges, and cycles are rejected.

`layout.direction` accepts `left-to-right` or `top-to-bottom`. Nodes size themselves for content and
outgoing port capacity inside the `minNodeWidth`/`maxNodeWidth` and
`minNodeHeight`/`maxNodeHeight` bounds. `layerGap` is a minimum;
edge labels and outgoing bend lanes expand it automatically. Connections without an explicit `color` are colored by target:
edges entering the same node share a stable generated color, while branches to different targets use
different colors. Outgoing edges receive separate source ports and bend lanes; incoming edges may
merge at a shared target port. Connections use Braille cells; arrows use `→` or `↓`.

## Interaction

- Left-drag anywhere in the graph viewport to pan an oversized graph.
- Click a node without dragging to toggle its selection.
- Arrow keys or `h`/`j`/`k`/`l` pan; mouse wheel pans vertically.
- Tab and Shift+Tab change the active node; Space toggles it.
- `Home` resets the viewport to the graph origin.
- `c` clears selection, `a` attaches context, Enter requests analysis, and `e` exports JSON.
- `q` or Escape closes the Canvas.

Selection and viewport state are sent to the controller. `canvas_spawn` and `canvas_update` wait
for validation and return configuration or data errors directly in the originating tool result.

## Source Layout

```text
src/main.rs          Process and terminal bootstrap only
src/runtime.rs       Canvas lifecycle, config reloads, and request routing
src/app.rs           Renderer state, viewport, selection, and state RPC values
src/layout.rs        Adaptive node sizing, topology placement, and layer gaps
src/render.rs        Rounded nodes, Braille segments, labels, and text arrows
src/routing.rs       Outgoing port/lane allocation and incoming route convergence
src/interaction.rs   Mouse drag/click and keyboard handling
src/actions.rs       Selection, LLM context/action, and artifact export
src/data.rs          Document decoding, validation, and topological ranking
../_sdk-rust/       Shared renderer-side Canvas v2 two-socket runtime crate
```

## Build

Run `./build` in this directory, or run `npm run build:canvases` from the repository root. The build
script uses Cargo's incremental cache and packages the result as `bin/<platform>-<arch>/dag`.
