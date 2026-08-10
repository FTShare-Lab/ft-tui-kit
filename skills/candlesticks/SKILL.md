---
name: candlesticks
description: |
  Interactive candlestick and volume Canvas for mainland stocks.
  Use when the user wants to inspect K-line data, select candle ranges, or ask the LLM to analyze a visible chart range.
---

# Candlesticks Canvas

The `candlesticks` renderer provides the `kline` scenario.

## Spawn

`config` is a JSON string. It must contain exactly one stock identifier field:

- `tag`
- `code`, an alias of `tag`

Do not provide both fields.

```typescript
canvas_spawn({
  kind: 'candlesticks',
  scenario: 'kline',
  config: JSON.stringify({
    code: '000001.SZ',
    interval_unit: 'Day',
    interval_value: 1,
    adjust_kind: 'None',
    limit: 120,
  }),
});
```

Valid mainland identifiers use a six-digit code and an exchange suffix, for example
`000001.SZ`, `600519.SH`, or a valid Beijing exchange code ending in `.BJ`.

Optional config fields:

- `data_file`: absolute JSON or CSV OHLCV file path
- `interval_unit`: `Minute`, `Day`, `Week`, `Month`, or `Year`
- `interval_value`: positive interval multiplier
- `adjust_kind`: `None`, `Forward`, or `Backward`
- `limit`: number of candles, from 1 to 2000

## Config Recovery

`canvas_spawn` waits for candlesticks to validate the config and load its data. When it returns
`success: false`, read `id` and `error`, correct the reported field, and call `canvas_update` for
that same ID. Do not call `canvas_spawn` again unless the original Canvas was closed.

```typescript
canvas_update({
  id: 'candlesticks-...',
  config: JSON.stringify({ code: '000001.SZ' }),
});
```

`canvas_update` also waits for the renderer and returns `success: false` with the exact error when
the replacement config or its data cannot be applied. Config errors are not delivered as actions.

## Interaction

- Click a candle to select it.
- Press `a` to attach summarized chart context to the next user prompt.
- Press Enter to ask the LLM to analyze the selected or visible range.
- Press `e` to export the visible range as a JSON artifact.

When `canvas_wait` is available, call it with this Canvas ID while waiting for an interactive
selection or analysis request. Otherwise, rely on the host's automatic Canvas event delivery.
