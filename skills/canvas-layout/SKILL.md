---
name: canvas-layout
description: Arrange one to four Canvas widgets in safe predefined tmux layouts. Use when the user wants two charts or images visible together, side-by-side comparison, stacked canvases, a main view with supporting views, a grid, a different Canvas split, or restoration to a single Canvas. Also use when the user explicitly asks to change the tmux/Canvas layout. Do not use it for arbitrary tmux configuration or shell commands.
---

# Canvas Layout

Arrange already-spawned Canvas widgets with `canvas_layout`. The tool accepts Canvas IDs only and
never accepts raw tmux commands or pane IDs.

## Workflow

1. Spawn every required Canvas and retain each successful `id`. Prefer `activate: false` after the
   first Canvas to avoid unnecessary switching.
2. Call `canvas_layout` with the IDs in visual order.
3. Set `focus` to the Canvas that should receive keyboard input.
4. If a spawn or layout fails, preserve the existing Canvases and repair the reported input.
5. Restore the usual view with `layout: "single"` and exactly one ID when comparison is complete.

## Layout choice

- `single`: one Canvas only.
- `columns`: two to four equal-priority views side by side; prefer for two comparable charts.
- `rows`: two to four stacked views; prefer for a chart plus table or snapshot.
- `main-left`, `main-right`, `main-top`, `main-bottom`: first ID is the main Canvas; remaining IDs
  occupy the auxiliary region. `mainPercent` may be 40–80 and defaults to 60.
- `grid`: two to four equal-priority views; prefer for three or four compact Canvases.

Avoid more panes than remain readable in the current terminal. The tool enforces a maximum of four.

## Examples

```typescript
canvas_layout({
  layout: 'columns',
  ids: [left.id, right.id],
  focus: left.id,
});
```

```typescript
canvas_layout({
  layout: 'main-top',
  ids: [candles.id, snapshot.id, table.id],
  mainPercent: 65,
  focus: candles.id,
});
```

Never invoke `tmux` directly to arrange managed Canvas panes. Do not pass IDs belonging to another
agent session or Codex thread; the tool rejects them.
