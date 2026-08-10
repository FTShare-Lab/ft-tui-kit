---
name: market-table
description: |
  Interactive Ratatui table for FTShare market rankings and locally saved quote results.
  Use for A-share leaders/laggards, activity, turnover, market-cap rankings, stock lists, screening results, and selecting securities for analysis.
---

# Market Table Canvas

Use `market-table` with scenario `quotes`. Never put quote rows or invented market data directly in
`config`. Choose exactly one of the following data paths.

## Fixed presets

Pass one of these exact English strings; do not create other preset names:

- `a-share-top-gainers`: A-share gainers Top 20
- `a-share-top-losers`: A-share losers Top 20
- `a-share-most-active`: A-share turnover amount Top 20
- `a-share-highest-turnover-rate`: A-share turnover-rate Top 20
- `a-share-largest-market-cap`: A-share market-cap Top 20

```typescript
canvas_spawn({
  kind: 'market-table',
  scenario: 'quotes',
  config: JSON.stringify({ preset: 'a-share-top-gainers' }),
});
```

## Local FTShare result

For custom filters, boards, sorting, or result sizes, call the FTShare
`stock-daec-stocks` Skill through Python and save its stdout JSON to a local file. Do not copy the
returned `items` into `canvas_spawn`. Pass only the absolute file path:

```bash
python <FTSHARE_RUN_PY> stock-daec-stocks \
  --board xshg --filter 'name.contains("银行")' \
  --page 1 --page_size 50 --order_by "change_rate desc" \
  > /absolute/workspace/path/bank-quotes.json
```

```typescript
canvas_spawn({
  kind: 'market-table',
  scenario: 'quotes',
  config: JSON.stringify({
    data_file: '/absolute/workspace/path/bank-quotes.json',
  }),
});
```

`dataFile` is an alias of `data_file`; never provide both. The file must be an FTShare-style object
with a non-empty `items` array and must not exceed 16 MiB. Prefer a bounded page over all 5000+
stocks. On config/data failure, repair the file or config and update the same Canvas ID.

## Controls

- Arrow/Page/Home/End: navigate; Space: toggle row; `c`: clear selection.
- `a`: attach selected rows or cursor row; Enter: request analysis; `e`: export JSON.
- `q` or Escape: close through the socket protocol.

When `canvas_wait` is available, call it with this Canvas ID while waiting for an interactive
selection or analysis request. Otherwise, rely on the host's automatic Canvas event delivery.
