---
name: news-list
description: |
  Interactive FTShare semantic news search with scrollable news cards, clickable original links, stable-ID AI highlights, multi-selection, context attachment, and AI explanation actions.
---

# News List Canvas

Use `news-list` with scenario `search` when the user asks to search, browse, compare, highlight, or
explain recent financial news. The FTShare endpoint only covers the current year and latest 15 days;
do not imply a broader archive.

```typescript
canvas_spawn({
  kind: 'news-list',
  scenario: 'search',
  config: JSON.stringify({
    query: '人工智能',
    limit: 20,
  }),
});
```

Optional `startTime` and `endTime` accept ISO 8601 values. Keep `limit` between 1 and 50. The
renderer retrieves news itself, so never invent or inline article rows.

## AI Highlights

After the initial search, read compact result metadata with `canvas_state` using state key `news`.
Choose articles by their returned `newsId`, then update the same Canvas ID with the complete search
config and a `highlights` array:

```typescript
canvas_update({
  id,
  config: JSON.stringify({
    query: '人工智能',
    limit: 20,
    highlights: [
      {
        newsId: '606732245083885569',
        reason: 'Directly addresses AI accountability',
      },
    ],
  }),
});
```

Always pass `newsId` as a string because FTShare IDs exceed JavaScript's safe integer range. Keep
reasons short and grounded in the returned title, source, time, summary, and relevance score. When
only highlights change, the renderer updates them without fetching the same search again.

## Interaction

- The top search field is clickable and supports direct user searches inside the renderer.
- News results are bordered cards; the list supports mouse-wheel and keyboard scrolling.
- Clicking a card selects it. Clicking Explain selected or pressing Enter queues an explanation
  request in the owning AI session. Clicking the underlined URL opens the original article.
- `a` attaches selected news as context, `c` clears selection, and `o` opens the current URL.

When `canvas_wait` is available, wait with this Canvas ID for context or explanation actions.
Otherwise rely on the host's normal Canvas event delivery.
