# Candlesticks Renderer

Self-contained Rust renderer for the `ft financial canvas` v2 socket protocol. This folder owns its
manifest, launcher, renderer source, vendored charting source, dependency lock file, licenses,
and generated binary packaging layout.

## Runtime Config

Fetch recent candlesticks from the market API:

```json
{
  "tag": "000001.SZ"
}
```

`code` is accepted as an alias of `tag`:

```json
{
  "code": "000001.SZ"
}
```

Provide exactly one of `tag` or `code`, not both.

Read OHLCV data from a local JSON or CSV file:

```json
{
  "tag": "600519.SH",
  "data_file": "/absolute/path/600519.json"
}
```

Optional API settings are `interval_unit`, `interval_value`, `adjust_kind`, and `limit`.
`interval_unit` accepts `Minute`, `Day`, `Week`, `Month`, or `Year`; `adjust_kind` accepts
`None`, `Forward`, or `Backward`.

Stock tags use the six-digit exchange-qualified form, such as `000001.SZ`, `600519.SH`, or a
valid Beijing exchange code ending in `.BJ`.

`canvas_spawn` and `canvas_update` wait for the renderer to accept or reject the config. A failure
is returned directly by the originating tool as `{ "success": false, "id": "...", "error":
"..." }`; correct that same Canvas with `canvas_update` rather than spawning another one.

## Data Files

JSON may be a top-level array or an object containing `data`, `result`, `items`, `rows`, or
`candles`. CSV uses the same field names as headers.

Required fields are `open`, `high`, `low`, `close`, `ts_millis`, and `volume`. Optional fields
are `ts_millis_open` and `turnover`. Decimal values may be JSON numbers or numeric strings;
`timestamp` and `time` are accepted as aliases for `ts_millis`.

## Layout

```text
config.json                 Canvas manifest and command entry
launcher.ts                 Selects the binary for the current platform/architecture
bin/<platform>-<arch>/      Generated renderer binaries (ignored by Git)
Cargo.toml                  Rust crate definition; the repository root owns Cargo.lock
src/                        Renderer, Canvas socket client, and charting source
```

The source repository does not track prebuilt renderer binaries. From the repository root, run
`npm run build:canvases` to build all native renderers and place the current platform binary under
its matching `bin/<platform>-<arch>/` directory.
