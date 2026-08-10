# Chart Renderer

Standalone Rust renderer for statistical charts over the `ft financial canvas` v2 socket protocol. It
uses Ratatui's native `BarChart` for grouped bars and `Chart` for line data with formatted axes;
the data document chooses either view or combines them.

## Runtime Config

Pass an absolute JSON data path when spawning the Canvas:

```json
{
  "data_file": "/absolute/path/to/chart-data.json"
}
```

`dataFile` is accepted as an alias. Provide exactly one spelling, not both. The complete chart
document contract is in [`chart-data.schema.json`](./chart-data.schema.json), with a runnable
shape in [`example-data.json`](./example-data.json).

The data file owns the title, table name, rows, category axis, numeric series, Y-axis formatting,
colors, and bar geometry. Rows use a wide-table shape: `axes.x.field` identifies the category
column and each `series.field` identifies one numeric column.
When `table.idField` is provided, every row must contain a unique string or numeric ID.

`display.view` is required and controls the layout:

- `bar`: grouped `BarChart` only
- `line`: axis-based `Chart` only
- `both`: grouped bars above the line overview

Ratatui's native `BarChart` stores values as `u64`. This Canvas therefore accepts finite,
non-negative values only and requires `axes.y.min` to be zero when it is provided. Decimal values
are preserved by scaling with `axes.y.format.decimals` before passing them to the widget.

## Interaction

- Left/Right: pan the visible category window
- Up/Down: choose the active series used by the line view
- Mouse click: select an individual bar, or the active series at a category in line view
- Mouse wheel or `+`/`-`: zoom by changing visual spacing and visible category capacity
- `c`: clear selection and transmit the cleared selection
- `a`: attach selected cells, or the visible range, to LLM context
- Enter: queue an LLM analysis action for selected cells or the visible range
- `e`: export selected/visible rows as a JSON artifact
- `q` or Escape: close the Canvas

Selection changes and throttled view state are sent to the controller automatically. The host may
also request current state or selection through Canvas RPC. `canvas_spawn` and `canvas_update`
wait for the document load; invalid config or chart data is returned directly by that tool as
`success: false` with the existing Canvas ID and exact error.

The renderer reloads on Canvas `init` and `update`; it does not watch the filesystem. After
repairing a referenced data file in place, call `canvas_update` on the same Canvas with the same
path to trigger a reload.

## Build

Run `./build` in this directory, or run `../../ftopencode-build`. The build script uses Cargo's
incremental cache and packages the result as `bin/<platform>-<arch>/chart`.
