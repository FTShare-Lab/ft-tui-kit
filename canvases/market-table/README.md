# Market Table Renderer

Independent Ratatui renderer for FTShare quote tables. Runtime configuration arrives only through
Canvas v2 sockets. The renderer rejects inline quote data.

## Config

Use exactly one fixed preset:

```json
{ "preset": "a-share-top-gainers" }
```

Supported presets are `a-share-top-gainers`, `a-share-top-losers`, `a-share-most-active`,
`a-share-highest-turnover-rate`, and `a-share-largest-market-cap`. Every preset fetches 20 rows
from FTShare. `FTSHARE_BASE_URL` may override the default service base URL.

Alternatively, use Python to save an FTShare `stock-daec-stocks` JSON response and pass its absolute
path:

```json
{ "data_file": "/absolute/path/a-share-quotes.json" }
```

The file must be at most 16 MiB and contain an object with a non-empty `items` array. Each item
requires `symbol` and `name`; numeric strings are accepted. The config contract is
`market-table.schema.json`.

## Interaction

- Arrow/Page/Home/End keys navigate the table; Space toggles rows and `c` clears selection.
- `a` attaches selected rows (or the cursor row), Enter requests analysis, and `e` exports JSON.
- `q` or Escape requests closure through the event socket.
