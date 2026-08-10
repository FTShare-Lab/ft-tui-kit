# News List Renderer

Interactive Ratatui renderer for FTShare semantic news search. It fetches current-year news from the
latest 15 days and renders each result in a bordered, vertically scrollable card with title, source,
publish time, summary, relevance score, and original article URL.

## Config

```json
{
  "query": "人工智能",
  "limit": 20,
  "startTime": "2026-08-01T00:00:00+08:00",
  "endTime": "2026-08-03T23:59:59+08:00",
  "highlights": [
    {
      "newsId": "606732245083885569",
      "reason": "Material policy signal"
    }
  ]
}
```

`query` is required. `limit` defaults to 20 and must be between 1 and 50. Time fields are optional
ISO 8601 values. Highlight IDs must be decimal strings because FTShare news IDs exceed JavaScript's
safe integer range. Unknown fields are rejected. The contract is `news-list.schema.json`.

The renderer calls `GET /api/v1/market/data/semantic-search-news`. Set `FTSHARE_BASE_URL` to override
the default `https://market.ft.tech/gateway` base URL.

## Interaction

- Click the search field, type a query, and click Search or press Enter.
- Up/Down/Page/Home/End and the mouse wheel scroll through news cards.
- Click a card or press Space to select up to five articles. Click Explain selected or press Enter
  to ask the owning AI session to explain them.
- Click the blue underlined URL, or press `o`, to open the original article with the system browser.
- Press `a` to attach selected news as context, `c` to clear selection, `/` to focus search, and
  `q` to close.

Interactive searches remain inside the renderer. AI explanations carry bounded article excerpts
marked as untrusted external source material. The `news` state key returns compact result metadata
without full article content so the AI can choose stable IDs and apply highlights with
`canvas_update`.
