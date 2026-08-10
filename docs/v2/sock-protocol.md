# Socket Protocol

v2 uses two bidirectional Unix sockets per widget:

- `control.sock`: lifecycle, config, registry updates, close, ping, and RPC.
- `event.sock`: renderer semantic events such as state, selection, context, action, artifact,
  plugin-internal command, and control requests.

Both sockets use newline-delimited JSON. Every frame is one JSON object followed by `\n`.

## Frame Shape

```ts
interface Frame {
  version: 2;
  id: string;
  widgetId: string;
  channel: 'control' | 'event';
  type: string;
  timestamp: number;
  requestId?: string;
  payload: unknown;
}
```

Rules:

- `version` must be `2`.
- `widgetId` must match the widget bound to the socket.
- `channel` must match the socket being used.
- First frame on each socket must be `hello`.
- `hello.payload.token` must match the token in `launch.json`.
- Frames are limited to 256 KiB by default.
- Large data should be sent as an artifact reference, not inline.
- `requestId` is used only for RPC request/response correlation.

## Launch File

The renderer starts with a host-generated launch file path.

Example command:

```bash
candlesticks --launch-file /tmp/ft-financial-canvas-v2/ws/candlesticks-1/launch.json
```

Example launch file:

```json
{
  "version": 2,
  "widgetId": "candlesticks-1780000000000-a1b2c3",
  "kind": "candlesticks",
  "scenario": "kline",
  "title": "000001.SZ Day",
  "token": "token_abc123",
  "runtimeDir": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1780000000000-a1b2c3",
  "controlSocketPath": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1780000000000-a1b2c3/control.sock",
  "eventSocketPath": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1780000000000-a1b2c3/event.sock",
  "configPath": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1780000000000-a1b2c3/config.json",
  "manifest": {
    "name": "candlesticks",
    "description": "Candlestick and volume chart",
    "defaultScenario": "kline",
    "capabilities": {
      "state": true,
      "selection": true,
      "context": true,
      "action": true,
      "artifacts": true
    }
  }
}
```

The renderer should read `configPath` for initial render data.

## Connection Handshake

The host listens on both sockets before starting the renderer process.

Renderer connects to `control.sock` and sends:

```json
{
  "version": 2,
  "id": "ctl_hello_1",
  "widgetId": "candlesticks-1",
  "channel": "control",
  "type": "hello",
  "timestamp": 1780000000000,
  "payload": {
    "token": "token_abc123",
    "kind": "candlesticks",
    "scenario": "kline",
    "pid": 12345
  }
}
```

Renderer connects to `event.sock` and sends:

```json
{
  "version": 2,
  "id": "evt_hello_1",
  "widgetId": "candlesticks-1",
  "channel": "event",
  "type": "hello",
  "timestamp": 1780000000001,
  "payload": {
    "token": "token_abc123",
    "kind": "candlesticks",
    "scenario": "kline",
    "pid": 12345
  }
}
```

After control authentication, host sends `init`:

```json
{
  "version": 2,
  "id": "ctl_init_1",
  "widgetId": "candlesticks-1",
  "channel": "control",
  "type": "init",
  "timestamp": 1780000000002,
  "payload": {
    "launch": {
      "version": 2,
      "widgetId": "candlesticks-1",
      "kind": "candlesticks",
      "scenario": "kline",
      "configPath": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1/config.json",
      "controlSocketPath": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1/control.sock",
      "eventSocketPath": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1/event.sock"
    },
    "config": {
      "code": "000001.SZ"
    }
  }
}
```

Renderer responds when UI is ready:

```json
{
  "version": 2,
  "id": "ctl_ready_1",
  "widgetId": "candlesticks-1",
  "channel": "control",
  "type": "ready",
  "timestamp": 1780000000100,
  "payload": {
    "title": "000001.SZ Day",
    "capabilities": {
      "state": true,
      "selection": true,
      "context": true,
      "action": true,
      "artifacts": true
    }
  }
}
```

## Control Channel Messages

### Host to Renderer

#### `init`

Initial launch and config payload.

```json
{
  "type": "init",
  "payload": {
    "launch": {},
    "config": {}
  }
}
```

#### `update`

Host sends a new config.

```json
{
  "type": "update",
  "payload": {
    "config": {
      "symbol": "ETHUSDT",
      "timeframe": "15m"
    }
  }
}
```

The renderer should update its UI without restarting.

#### `focus`

Host reports visibility and keyboard focus separately. `active` means the Canvas is visible in the
current layout; `focused` means its tmux pane is selected for keyboard input.

```json
{
  "type": "focus",
  "payload": {
    "active": true,
    "focused": false
  }
}
```

In a multi-Canvas layout, several renderers may receive `active: true`, but at most one receives
`focused: true`. `focused` may be omitted if the host only knows pane visibility.

#### `registry`

Host broadcasts widgets in the same owning agent session or Codex thread.

```json
{
  "type": "registry",
  "payload": {
    "activeId": "candlesticks-1",
    "widgets": [
      {
        "id": "candlesticks-1",
        "kind": "candlesticks",
        "scenario": "kline",
        "title": "000001.SZ Day",
        "status": "ready",
        "active": true,
        "capabilities": {
          "selection": true,
          "context": true
        }
      }
    ]
  }
}
```

#### `request.state`

Host asks for renderer state.

```json
{
  "type": "request.state",
  "requestId": "req_1",
  "payload": {
    "key": "visibleRange"
  }
}
```

Renderer responds with `rpc.response`.

#### `request.selection`

Host asks for current selection.

```json
{
  "type": "request.selection",
  "requestId": "req_2",
  "payload": {}
}
```

#### `request.content`

Host asks for editable or inspectable content, if supported.

```json
{
  "type": "request.content",
  "requestId": "req_3",
  "payload": {}
}
```

#### `close`

Host asks renderer to exit.

```json
{
  "type": "close",
  "payload": {
    "reason": "Closed by Canvas tool"
  }
}
```

#### `ping`

Connectivity check.

### Renderer to Host

#### `ready`

Renderer has initialized and rendered enough to be used.

#### `capabilities`

Renderer updates capabilities.

```json
{
  "type": "capabilities",
  "payload": {
    "state": true,
    "selection": true,
    "content": false,
    "context": true,
    "action": true,
    "artifacts": true
  }
}
```

#### `rpc.response`

Response to `request.*`.

Success:

```json
{
  "type": "rpc.response",
  "requestId": "req_1",
  "payload": {
    "ok": true,
    "data": {
      "symbol": "BTCUSDT",
      "timeframe": "1h",
      "visibleRange": ["2026-07-10T00:00:00Z", "2026-07-10T12:00:00Z"]
    }
  }
}
```

Failure:

```json
{
  "type": "rpc.response",
  "requestId": "req_1",
  "payload": {
    "ok": false,
    "error": "state key not available"
  }
}
```

#### `error`

Renderer reports an error.

```json
{
  "type": "error",
  "payload": {
    "message": "failed to load market data",
    "fatal": false
  }
}
```

#### `pong`

Response to `ping`.

## Event Channel Messages

Event messages are semantic. The renderer should not send every cursor movement into prompt delivery. High-frequency UI data should use `state`; prompt-worthy data should use `context` or `action`.

The host sends `event.ack` after successful handling:

```json
{
  "type": "event.ack",
  "payload": {
    "eventId": "evt_123"
  }
}
```

If handling fails:

```json
{
  "type": "event.nack",
  "payload": {
    "eventId": "evt_123",
    "error": "invalid switch target"
  }
}
```

### Renderer to Host

#### `state`

Renderer state for host-side storage or tool queries. It does not enter prompts.

```json
{
  "type": "state",
  "payload": {
    "key": "cursor",
    "label": "active candle",
    "data": {
      "symbol": "BTCUSDT",
      "timeframe": "1h",
      "index": 120,
      "time": "2026-07-10T08:00:00Z"
    }
  }
}
```

#### `selection`

Current user selection. By default this is stored as state. It enters prompts only if `delivery` is set.

```json
{
  "type": "selection",
  "payload": {
    "label": "selected candle range",
    "delivery": "context",
    "text": "The user selected BTCUSDT 1h candles from 08:00 to 12:00 UTC.",
    "data": {
      "symbol": "BTCUSDT",
      "timeframe": "1h",
      "start": "2026-07-10T08:00:00Z",
      "end": "2026-07-10T12:00:00Z"
    }
  }
}
```

#### `context`

Prompt context to append to the next normal user message.

```json
{
  "type": "context",
  "payload": {
    "label": "chart context",
    "text": "BTCUSDT is consolidating near the MA20 after a sharp move.",
    "data": {
      "symbol": "BTCUSDT",
      "timeframe": "1h",
      "indicators": {
        "ma20": 108000,
        "rsi14": 61.2
      }
    }
  }
}
```

Default delivery is context. If `delivery` is `queue` or `steer`, host treats it like an action.

#### `action`

Explicit renderer action that should return a prompt-worthy instruction to the owning host adapter.

```json
{
  "type": "action",
  "payload": {
    "label": "analyze selected range",
    "delivery": "queue",
    "prompt": "Analyze the selected BTCUSDT range and identify likely support/resistance.",
    "data": {
      "symbol": "BTCUSDT",
      "timeframe": "1h",
      "range": ["2026-07-10T08:00:00Z", "2026-07-10T12:00:00Z"]
    }
  }
}
```

`prompt` is preferred. If absent, host builds generic text from `text` and `data`.

#### `artifact`

Large or structured output by reference.

```json
{
  "type": "artifact",
  "payload": {
    "label": "visible candle data",
    "path": "/tmp/ft-financial-canvas-v2/ws/candlesticks-1/visible-candles.json",
    "mediaType": "application/json",
    "delivery": "context",
    "text": "Renderer saved visible 000001.SZ candles to a JSON artifact."
  }
}
```

Use this for large candle arrays, CSV files, screenshots, or any data that may exceed frame size limits.

#### `command`

A namespaced UI command handled directly by a host adapter. It never creates an agent prompt turn.
The host accepts this event only from a manifest marked `internalOnly`; a public renderer receives
`event.nack`.

```json
{
  "type": "command",
  "payload": {
    "name": "social-post.save"
  }
}
```

Commands are still transported exclusively over the authenticated `event.sock`. Renderer-specific
payloads may be included as `data`, but the host should validate them and retain ownership of any
workspace writes or other side effects.

#### `control`

Renderer asks host to switch, close, or cycle widgets.

```json
{
  "type": "control",
  "payload": {
    "command": "next"
  }
}
```

```json
{
  "type": "control",
  "payload": {
    "command": "switch",
    "targetId": "chart-2"
  }
}
```

#### `cancelled`

Renderer reports user cancellation.

```json
{
  "type": "cancelled",
  "payload": {
    "reason": "user pressed escape"
  }
}
```

#### `log`

Renderer log event. It should not be shown in the TUI.

```json
{
  "type": "log",
  "payload": {
    "level": "warn",
    "message": "market data refresh delayed",
    "data": {
      "retryInMs": 1000
    }
  }
}
```

## Delivery Semantics

`delivery` controls whether an event reaches agent prompts.

| Delivery  | Meaning                                                                                                                           |
| --------- | --------------------------------------------------------------------------------------------------------------------------------- |
| omitted   | Message-specific default. `state` does not enter prompts; `context` appends to next user message; `selection` only stores state.  |
| `context` | OpenCode appends to the next normal user message; Codex makes it available to `canvas_wait` and the next `UserPromptSubmit` Hook. |
| `queue`   | OpenCode creates a user turn with `promptAsync` when idle; Codex makes the action available to `canvas_wait` and the next Hook.   |
| `steer`   | Reserved for host steer behavior. OpenCode handles it like `queue`; Codex handles it as a queued action event.                    |

## Prompt Wrapping

The renderer may provide safe text, but the host still wraps context with source metadata before
delivering it to the owning agent conversation.

Example resulting text:

```text
Additional context from interactive widgets:

Canvas context from candlesticks-1 (chart context)
000001.SZ is consolidating after a sharp move with elevated volume.
Data: {"tag":"000001.SZ","timeframe":"1 Day","selectedCount":12}
```

For actions, OpenCode sends the renderer prompt as a new user turn in the original session. Codex
returns it from `canvas_wait` or attaches it to the next user prompt through the plugin Hook.
