# Usage

This document explains how to use the `ft financial canvas` host and how to write Canvas v2
renderer processes.

## OpenCode User Flow

During local development, load the plugin source from the OpenCode workspace. This example assumes
the workspace and `opencode-canvas-main/` are sibling directories:

Example config:

```jsonc
{
  "plugin": ["../opencode-canvas-main/src/index.ts"],
  "permission": {
    "canvas_*": "allow",
  },
}
```

OpenCode should run inside tmux. The plugin does not take over the parent terminal process;
`ftopencode` handles that before OpenCode starts.

```bash
../opencode-canvas-main/ftopencode
```

From OpenCode, the model can call:

- `canvas_renderers`
- `canvas_spawn`
- `canvas_update`
- `canvas_selection`
- `canvas_content`
- `canvas_state`
- `canvas_list`
- `canvas_switch`
- `canvas_next`
- `canvas_layout`
- `canvas_close`

## Codex User Flow

Build `dist/codex-mcp.js` and `dist/codex-hook.js`, then install the repository as the
`ft-financial-canvas` plugin through a configured local Codex marketplace. Start a new thread after
installing or updating so that Skills, MCP tools, and Hooks are loaded together.

Codex must also run inside tmux. `ftcodex` starts or attaches a tmux session before launching it:

```bash
../opencode-canvas-main/ftcodex
```

The launcher uses Codex inline mode (`--no-alt-screen`) by default so conversation output remains
in the tmux pane scrollback. Pass `--alt-screen` to `ftcodex` to restore the full-screen TUI.

Codex receives the same public tools plus `canvas_wait`. The MCP adapter derives ownership from
Codex `_meta.threadId`; callers cannot provide or override a thread ID. The MCP server advertises
`codex/sandbox-state-meta` so it can retain the active workspace in its normalized tool context.

When a task needs an interactive choice in the current turn, call:

```json
{
  "id": "chart-1780000000000-a1b2c3",
  "timeout_ms": 30000
}
```

`canvas_wait` returns one queued renderer context/action or a normal timeout result. Events not
consumed by `canvas_wait` are journaled under the system temporary directory and attached by the
plugin's `UserPromptSubmit` Hook to the next prompt in that thread. The user must approve the
plugin Hook before this cross-turn fallback runs.

The OpenCode-only social draft coordinator and its `social-post-card` internal renderer are not
registered by the Codex adapter.

## Tool Summary

### `canvas_renderers`

Lists registered renderer manifests.

Use this when the model is unsure which renderer or scenario exists.

### `canvas_spawn`

Starts a renderer process in the managed right-side tmux canvas.

Input:

```json
{
  "kind": "chart",
  "scenario": "bar-graph",
  "title": "Quarterly revenue",
  "config": "{\"data_file\":\"/absolute/path/chart-data.json\"}",
  "activate": true
}
```

Output:

```json
{
  "success": true,
  "id": "chart-1780000000000-a1b2c3",
  "kind": "chart",
  "scenario": "bar-graph",
  "paneID": "%7",
  "visible": true,
  "launchFile": "/tmp/ft-financial-canvas-v2/ws/chart-.../launch.json",
  "controlSocketPath": "/tmp/ft-financial-canvas-v2/ws/chart-.../control.sock",
  "eventSocketPath": "/tmp/ft-financial-canvas-v2/ws/chart-.../event.sock"
}
```

### `canvas_update`

Sends a new JSON config to the renderer over `control.sock`.

```json
{
  "id": "chart-1780000000000-a1b2c3",
  "config": "{\"data_file\":\"/absolute/path/updated-chart-data.json\"}"
}
```

### `canvas_selection`

Requests current renderer selection through `request.selection`.

The shape is renderer-defined.

### `canvas_content`

Requests renderer content through `request.content`.

Only renderers with `content: true` capability are expected to support this.

### `canvas_state`

Requests renderer state through `request.state`.

```json
{
  "id": "candlesticks-1",
  "key": "visibleRange"
}
```

### `canvas_list`

Lists widgets owned by the current OpenCode session or Codex thread.

### `canvas_switch`

Switches the visible right-side pane to a specific widget without restarting it.

### `canvas_next`

Cycles keyboard focus through widgets visible in the current layout.

### `canvas_layout`

Arranges one to four widgets owned by the current agent session/thread. It accepts only Canvas IDs and
predefined layouts: `single`, `columns`, `rows`, `main-left`, `main-right`, `main-top`,
`main-bottom`, and `grid`. It never accepts raw tmux commands or pane IDs.

```json
{
  "layout": "columns",
  "ids": ["chart-1", "chart-2"],
  "focus": "chart-1"
}
```

In a multi-Canvas layout, `canvas_switch` focuses an already visible Canvas. Switching to a hidden
Canvas replaces the currently focused visible slot while preserving the rest of the layout.

### `canvas_close`

Closes a widget and cleans up its socket/runtime files.

### `canvas_wait` (Codex only)

Waits up to 55 seconds for a context or action event owned by the current Codex thread. `id` is
optional; when present it filters events to one Canvas. Timeouts and client cancellation are
reported as successful wait results rather than renderer failures.

## Renderer Config

A renderer is registered by adding `canvases/<kind>/config.json`.

Conceptual shape:

```ts
interface CanvasConfig {
  schemaVersion?: number;
  name: string;
  description?: string;
  defaultScenario: string;
  scenarios: string[];
  internalOnly?: boolean;
  capabilities: {
    state?: boolean;
    selection?: boolean;
    content?: boolean;
    context?: boolean;
    action?: boolean;
    artifacts?: boolean;
    command?: boolean;
  };
  entry:
    | { type: 'bun' | 'bun-ink'; module: string; export?: string }
    | { type: 'command'; command: string[]; showCommand?: string[] };
}
```

`internalOnly: true` reserves a renderer for plugin orchestration. Internal renderers are omitted
from `canvas_renderers`, rejected by public Canvas tools, and cannot be run through `canvas show`.
They still run as independent processes and must use the same two authenticated sockets. The
`command` capability is intended for their namespaced, host-handled UI commands; it is not a prompt
delivery mechanism.

The host derives its manifest from this file and launches every renderer through:

```bash
bun run canvases/launcher.ts renderer <kind> --launch-file <launch.json>
```

Example internal Bun/Ink config:

```json
{
  "schemaVersion": 1,
  "name": "social-post-card",
  "description": "Plugin-internal review card for an automatically generated technical social post.",
  "defaultScenario": "review",
  "scenarios": ["review"],
  "internalOnly": true,
  "capabilities": {
    "command": true
  },
  "entry": {
    "type": "bun-ink",
    "module": "./index.tsx",
    "export": "SocialPostCard"
  }
}
```

Example Rust-style command config:

```json
{
  "schemaVersion": 1,
  "name": "candlesticks",
  "description": "Interactive candlestick and volume chart for mainland stocks.",
  "defaultScenario": "kline",
  "scenarios": ["kline"],
  "capabilities": {
    "state": true,
    "selection": true,
    "context": true,
    "action": true,
    "artifacts": true
  },
  "entry": {
    "type": "command",
    "command": ["bun", "{canvasDir}/launcher.ts", "--launch-file", "{launchFile}"]
  }
}
```

Supported command-entry placeholders:

| Placeholder    | Meaning                                                               |
| -------------- | --------------------------------------------------------------------- |
| `{pluginRoot}` | Root directory of the local or packaged `ft financial canvas` plugin. |
| `{launchFile}` | Host-generated launch file path.                                      |
| `{widgetId}`   | Widget instance ID.                                                   |
| `{kind}`       | Renderer name.                                                        |
| `{scenario}`   | Scenario name.                                                        |
| `{runtimeDir}` | Widget runtime directory.                                             |
| `{canvasDir}`  | The renderer's `canvases/<kind>` directory.                           |

The unified host command is shell-quoted by the host. A command-entry renderer is spawned by `canvases/launcher.ts`.

## Renderer Implementation Steps

A renderer binary should:

1. Accept `--launch-file <path>`.
2. Read and parse `launch.json`.
3. Read initial config from `launch.configPath`.
4. Connect to `launch.controlSocketPath`.
5. Connect to `launch.eventSocketPath`.
6. Send authenticated `hello` on both sockets.
7. Wait for `init` on the control socket.
8. Render the TUI in the process stdout/stderr attached to the tmux pane.
9. Send `ready` on the control socket.
10. Send semantic events on the event socket.
11. Respond to `request.state`, `request.selection`, and `request.content` with `rpc.response`.
12. Exit on `close`.

## Minimal Renderer Pseudocode

```text
launch = read_json(argv["--launch-file"])
config = read_json(launch.configPath)

control = unix_connect(launch.controlSocketPath)
event = unix_connect(launch.eventSocketPath)

send(control, {
  version: 2,
  id: new_id(),
  widgetId: launch.widgetId,
  channel: "control",
  type: "hello",
  timestamp: now(),
  payload: {
    token: launch.token,
    kind: launch.kind,
    scenario: launch.scenario,
    pid: process_id()
  }
})

send(event, {
  version: 2,
  id: new_id(),
  widgetId: launch.widgetId,
  channel: "event",
  type: "hello",
  timestamp: now(),
  payload: {
    token: launch.token,
    kind: launch.kind,
    scenario: launch.scenario,
    pid: process_id()
  }
})

loop:
  read control frames
  if frame.type == "init":
    render(config)
    send ready
  if frame.type == "update":
    config = frame.payload.config
    rerender(config)
  if frame.type == "request.selection":
    send rpc.response with current selection
  if frame.type == "close":
    exit

on user attaches context:
  send event frame type "context"

on user confirms an action:
  send event frame type "action"
```

## Prompt Integration

Renderer events enter an agent conversation only through its host adapter:

- `state`: stored/queryable, never prompt-injected.
- `selection`: stored by default; prompt-injected only when `delivery` is set.
- `context`: OpenCode appends it to the next normal prompt; Codex returns it through
  `canvas_wait` or the next `UserPromptSubmit` Hook.
- `action`: OpenCode queues a prompt when the session is idle; Codex returns it through
  `canvas_wait` or the next `UserPromptSubmit` Hook.
- `artifact`: stored by reference and optionally delivered as context/action.

The host always uses the OpenCode session or Codex thread captured when the widget was spawned. It
does not inject text into whichever TUI input happens to be focused.

## Current Packaged Renderers

The project currently provides these manifests:

| Name                | Scenarios                          | Notes                                                                                                                        |
| ------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `candlesticks`      | `kline`                            | Rust candlestick/volume chart. Selection, explicit context, analysis actions, and JSON artifacts use the v2 socket protocol. |
| `chart`             | `bar-graph`                        | Rust grouped bar/line chart. The data file selects `bar`, `line`, or `both`.                                                 |
| `dag`               | `display`                          | Rust DAG renderer with rounded nodes, Braille connections, draggable panning, and inline/file input.                         |
| `market-table`      | `quotes`                           | Rust FTShare quote table for rankings and security selection.                                                                |
| `news-list`         | `search`                           | Rust FTShare semantic news cards with direct search, scrolling, source links, stable-ID highlights, and AI explanations.     |
| `security-snapshot` | `overview`                         | Rust single-security market, performance, valuation, and capitalization view.                                                |

They use the same socket contract as any future renderer.

### Packaged Renderer Mouse Behavior

The packaged renderers enable terminal mouse reporting by default. This prevents native terminal
selection from crossing tmux panes and lets the renderer decide what semantic selection should be
sent back to the owning agent session/thread.

- `news-list/search`: clicking a card updates selection, clicking its underlined URL opens the source in the system browser, and the mouse wheel scrolls cards.

## Candlesticks Renderer

`candlesticks` is a packaged command renderer, not host code. Its manifest, launcher, Rust
source, build script, and platform binaries live in `canvases/candlesticks/`.

Its config requires exactly one stock identifier field: `tag`, or `code` as an alias. Both
`canvas_spawn` and `canvas_update` wait for the renderer to load the config and data. Validation,
market API, and data-file failures return `{ success: false, id, error }` from the originating tool;
they are not converted into LLM actions. The returned ID remains available for a corrected
`canvas_update`.

Capabilities:

```json
{
  "state": true,
  "selection": true,
  "context": true,
  "action": true,
  "artifacts": true
}
```

Current state keys:

- `visibleRange`
- `selectedRange`
- `zoom`

Current event behavior:

- Pan, zoom, and selection changes emit `state`.
- Clicking candles emits `selection`.
- Pressing `a` attaches summarized chart context.
- Pressing Enter requests LLM analysis through `action`.
- Pressing `e` writes visible candles and emits an `artifact` reference.

Do not stream every market tick into prompt delivery. Keep high-frequency market updates inside renderer state or artifacts.
