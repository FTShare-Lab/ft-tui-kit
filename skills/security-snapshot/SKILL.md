---
name: security-snapshot
description: |
  Interactive Ratatui overview of one FTShare A-share security with market, performance, valuation, and capitalization sections.
  Use when showing a mainland stock snapshot or asking the user to focus analysis on one metric group.
---

# Security Snapshot Canvas

Pass only one exchange-qualified A-share stock code. Never provide price, name, valuation,
performance, company, or other snapshot data in the Canvas config; the renderer retrieves it from
FTShare.

```typescript
canvas_spawn({
  kind: 'security-snapshot',
  scenario: 'overview',
  config: JSON.stringify({ symbol: '600519.SH' }),
});
```

Accepted exchanges are `.SH`, `.SZ`, and `.BJ`; the code must contain exactly six digits. Resolve a
company name to its stock code before spawning. Inline fields and unknown config keys are rejected.
On a symbol or FTShare error, correct the code and update the same Canvas ID.

## Controls

- Left/Right or Tab: select Market, Performance, Valuation, or Capitalization.
- `a`: attach the active section; Enter: request analysis; `e`: export the full snapshot.
- `q` or Escape: close through the socket protocol.

When `canvas_wait` is available, call it with this Canvas ID while waiting for an interactive
selection or analysis request. Otherwise, rely on the host's automatic Canvas event delivery.
