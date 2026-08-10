# File Architecture

This document describes the current Canvas v2 layout and the responsibility of each layer in
`ft financial canvas`.

## Host Layer

Host code runs inside either the OpenCode plugin process or the Codex MCP server process.

```text
src/index.ts
src/hosts/opencode/prompt-bridge.ts
src/hosts/codex/mcp-server.ts
src/hosts/codex/event-broker.ts
src/hosts/codex/event-store.ts
src/hosts/codex/hook.ts
src/canvas/manager/canvas-manager.ts
src/canvas/manager/tmux.ts
src/canvas/manager/event-sink.ts
src/canvas/manifest.ts
src/canvas/protocol.ts
src/canvas/ipc/host-server.ts
```

### `src/index.ts`

OpenCode plugin entrypoint.

Responsibilities:

- Register `canvas_*` tools.
- Create `CanvasManager`.
- Register packaged skill paths and any optional command definitions.
- Forward OpenCode events to `PromptBridge`.
- Forward `chat.message` to `PromptBridge` for context injection.
- Dispose all running canvases when the plugin unloads.
- Enable the OpenCode-only `social-post-card` coordinator.

It should not import or render a canvas UI.

### `src/canvas/manager/canvas-manager.ts`

Main host aggregate.

Responsibilities:

- Validate `canvas_spawn` requests against manifests.
- Create widget IDs and runtime directories.
- Write `config.json` and `launch.json`.
- Start `control.sock` and `event.sock`.
- Build renderer commands from manifests.
- Ask `TmuxManager` to place the renderer process.
- Maintain widget records by host session or Codex thread.
- Route renderer `state`, `selection`, `context`, `action`, `artifact`, `command`, and `control`
  events.
- Enforce session ownership for update/query/switch/close tools.
- Keep `internalOnly` renderers out of public discovery and tool operations.

### `src/canvas/manager/tmux.ts`

Terminal placement layer.

Responsibilities:

- Require the active agent to run inside tmux.
- Spawn the first renderer in a right-side split pane.
- Spawn later renderers in hidden tmux windows.
- Arrange one to four session-owned renderer panes with predefined layouts.
- Track visible panes separately from the focused pane.
- Keep layout state scoped by agent session/thread and hide the previous session's visible panes when
  another session becomes active in the shared tmux host window.
- Switch hidden renderers into the focused visible slot with `swap-pane`.
- Close panes and recover a fallback renderer if needed.

`TmuxManager` accepts pane IDs only from `CanvasManager`; public tools accept Canvas IDs and never
raw tmux commands, pane IDs, shell fragments, window targets, or session targets.

It is intentionally renderer-agnostic.

### `src/canvas/manager/event-sink.ts`

Host-neutral event-delivery boundary used by `CanvasManager`.

Responsibilities:

- Define the minimal tool context shared by both adapters.
- Define normalized renderer context and action events.
- Keep OpenCode and MCP SDK types out of the shared manager.

### `src/hosts/opencode/prompt-bridge.ts`

OpenCode prompt bridge.

Responsibilities:

- Store renderer context events until the next user message.
- Append pending context in `chat.message`.
- Queue renderer action events and deliver them with `client.session.promptAsync`.
- Track busy/idle session state from OpenCode events.

It does not render UI and should not know renderer-specific data schemas.

### `src/hosts/codex/*`

Codex adapter.

Responsibilities:

- Register the public Canvas tools over stdio MCP and derive ownership from `_meta.threadId`.
- Advertise `codex/sandbox-state-meta` and recover the active workspace from `sandboxCwd`.
- Expose `canvas_wait` for renderer interactions needed in the current turn.
- Keep a bounded in-memory event queue and a bounded temporary journal for interactions that cross
  turn boundaries.
- Consume that journal from the `UserPromptSubmit` Hook and return standard `additionalContext`.

The Codex adapter does not configure `onCommand`, so `internalOnly` renderers and
`social-post-card` remain unavailable through Codex.

### `src/social-post/coordinator.ts`

OpenCode-only automatic social draft workflow.

Responsibilities:

- Measure each conversation from its incoming chat/`busy` start until its `idle` event without
  carrying time across turns; an intermediate `retry` remains part of the same elapsed round.
- When one conversation exceeds one minute, read text turns completed by that boundary and generate
  a draft in an ignored temporary child session.
- Spawn the `internalOnly` review card through `CanvasManager`, never through a public tool.
- Handle namespaced save/cancel socket commands and save accepted text under
  `.memory/social-posts/`.

The coordinator owns orchestration and filesystem writes. The card remains an independent renderer
and has no direct access to OpenCode session APIs or the workspace.

### `src/canvas/manifest.ts`

Renderer registry and command builder.

Responsibilities:

- Discover renderer manifests from `canvases/*/config.json`.
- Provide `defaultScenario`, scenario names, capabilities, and the unified launcher command.
- Build shell-quoted renderer commands from manifest placeholders.

It must not import renderer UI code. Packaged and external renderers are discovered through canvas config files.

### `src/canvas/protocol.ts`

Shared v2 protocol types and helpers.

Responsibilities:

- Define protocol version.
- Define frame shape.
- Define host-to-renderer and renderer-to-host messages.
- Provide `createFrame`, `parseFrame`, and `encodeFrame`.

External renderers do not need this TypeScript file, but they must implement the same wire protocol.

### `src/canvas/ipc/host-server.ts`

Host-side socket server.

Responsibilities:

- Start one `control.sock` and one `event.sock` per widget.
- Require authenticated `hello` on both sockets.
- Send `init` over the control channel.
- Handle RPC requests and responses.
- Ack or nack event-channel messages.
- Close and clean up socket files.

## Renderer Layer

Renderer code runs in a child process inside the tmux pane.

```text
canvases/_sdk/ipc/renderer-client.ts
canvases/_sdk/ipc/use-ipc.ts
canvases/_sdk/use-mouse.ts
canvases/launcher.ts
canvases/bun-cli.ts
canvases/<kind>/*
```

### `canvases/launcher.ts`

Unified renderer launcher.

Responsibilities:

- Read `canvases/<kind>/config.json`.
- Dispatch Bun/Ink renderers to `canvases/bun-cli.ts`.
- Dispatch command renderers, such as Rust binaries, through their configured command.
- Keep host manifests stable while renderer implementation technology varies.

### `canvases/bun-cli.ts`

Bun/Ink adapter for packaged TypeScript renderers.

Responsibilities:

- Read `launch.json` and initial config from `configPath`.
- Dynamically import the configured canvas module.
- Render the configured React component with Ink.

Rust or other native renderers do not use this file; they implement the socket protocol directly and are launched by `canvases/launcher.ts`.

### `canvases/_sdk/ipc/renderer-client.ts`

Optional TypeScript helper for renderer authors.

Responsibilities:

- Connect to both host sockets.
- Send authenticated `hello`.
- Parse host frames.
- Provide `sendControl` and `sendEvent`.

External renderers may ignore this helper and implement the socket protocol directly.

### `canvases/<kind>/*`

Current packaged renderers.

Each renderer folder owns its UI, types, scenarios, components, and `config.json`.

The current public packaged folders are `candlesticks`, `chart`, `dag`, `market-table`, `news-list`,
and `security-snapshot`. `social-post-card` is an OpenCode-only
internal renderer. They are child-process renderers rather than host logic. Renderer folders do not
import from each other; Bun/Ink renderers may use the renderer-side helpers in `canvases/_sdk/`.

Example `config.json`:

```json
{
  "schemaVersion": 1,
  "name": "chart",
  "description": "Interactive grouped bar/line chart renderer.",
  "defaultScenario": "bar-graph",
  "scenarios": ["bar-graph"],
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

## Runtime Files

Each widget gets a private runtime directory:

```text
/tmp/ft-financial-canvas-v2/<workspace-hash>/<widget-id>/
  config.json
  launch.json
  control.sock
  event.sock
```

`launch.json` includes connection metadata and an auth token. The directory should be readable only by the current user.

Example:

```json
{
  "version": 2,
  "widgetId": "chart-1780000000000-a1b2c3",
  "kind": "chart",
  "scenario": "bar-graph",
  "token": "token_xxx",
  "runtimeDir": "/tmp/ft-financial-canvas-v2/workspace/chart-...",
  "controlSocketPath": "/tmp/ft-financial-canvas-v2/workspace/chart-.../control.sock",
  "eventSocketPath": "/tmp/ft-financial-canvas-v2/workspace/chart-.../event.sock",
  "configPath": "/tmp/ft-financial-canvas-v2/workspace/chart-.../config.json",
  "manifest": {
    "name": "chart",
    "defaultScenario": "bar-graph",
    "capabilities": {
      "selection": true,
      "context": true,
      "action": true,
      "artifacts": true
    }
  }
}
```

## Dependency Rule

The shared host core may depend on:

- `protocol`
- `manifest`
- `host-server`
- `tmux`
- `event-sink`

The shared host core must not depend on:

- React/Ink renderer components
- renderer-specific schemas
- renderer-specific prompt construction beyond generic event wrapping

Renderers may depend on:

- the protocol contract
- their own UI and domain libraries

Renderers must not depend on:

- OpenCode or MCP SDK internals
- host-specific tool context types
- either host adapter
- tmux management internals
