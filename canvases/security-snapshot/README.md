# Security Snapshot Renderer

Independent Ratatui renderer for FTShare `stock-security-info`. Runtime configuration arrives only
through Canvas v2 sockets and accepts only an exchange-qualified A-share stock code:

```json
{ "symbol": "600519.SH" }
```

Inline quote, valuation, company, or snapshot fields are rejected. The renderer fetches the
security response from FTShare and accepts `.SH`, `.SZ`, and `.BJ` codes. For local service routing,
set `FTSHARE_SECURITY_BASE_URL`. The config contract is `security-snapshot.schema.json`.

## Interaction

- Left/Right or Tab selects Market, Performance, Valuation, or Capitalization.
- `a` attaches the active section, Enter requests analysis, and `e` exports the full snapshot.
- `q` or Escape requests closure through the event socket.
