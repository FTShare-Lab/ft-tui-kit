# ft financial canvas: Canvas v2

Canvas v2 is the current renderer-host architecture used by `ft financial canvas`.

The shared host core owns tmux placement, widget lifecycle, session ownership, and renderer IPC.
OpenCode and Codex adapters register host-specific tools and event delivery. Renderers run as
independent child processes, draw their own TUI in a tmux pane, and communicate with the host
through two authenticated Unix sockets.

This boundary allows renderers to use TypeScript, Rust, Go, Python, or any other runtime that can
read `launch.json` and exchange newline-delimited JSON over Unix sockets.

## Documents

- [File Architecture](./file-architecture.md): module boundaries and ownership.
- [Socket Protocol](./sock-protocol.md): frame format, channels, messages, and examples.
- [Usage](./usage.md): host tools, renderer manifests, and renderer implementation guidance.

## Core Model

```text
OpenCode tool or Codex MCP call
      |
      v
ft financial canvas host
  - shared CanvasManager
  - manifest lookup
  - runtime files
  - control.sock + event.sock
  - tmux pane/window management
  - host event sink
      |
      v
independent renderer process
  - reads launch.json
  - connects both sockets
  - renders a TUI
  - sends state/selection/context/action events
```

The host does not interpret renderer-specific UI state beyond generic protocol routing. It owns the
target OpenCode session or Codex thread and the delivery mode, so a renderer cannot redirect an
event to another conversation.

## Packaged Renderers

All packaged renderers use the same manifest and socket contract:

- Public Rust/Ratatui renderers: `candlesticks`, `chart`, `dag`, `market-table`, `news-list`,
  `security-snapshot`.
- Internal Bun/Ink renderer: `social-post-card` (OpenCode only).

They are renderer processes, not special cases in the plugin host. New financial renderers should
follow the same folder and protocol contract rather than adding domain-specific UI logic to `src/`.
