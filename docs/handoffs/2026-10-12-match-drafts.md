# Handoff: match drafts (2026-10-12)

## Endpoint
`POST /api/match-drafts`, signed in, body `{ "text": "<1–500 characters>" }`.

- 200 `{ "draft": { venueId, date, startTime, endTime, format, matchType, levelMin, levelMax, slotCount, totalFeeVnd } }`. Every field may be `null`; `date` is `YYYY-MM-DD` and times are `HH:MM` in Ho Chi Minh City. `Cache-Control: private, no-store`.
- 400 `invalid_request` (malformed body), 401 (not signed in), 422 `invalid_text`, 429 `{ "error": "draft_limit", "scope": "user" | "service" }`, 502 `reader_failed`, 503 `drafts_unavailable` (no API key configured).

## Frontend
`browserReadDraft` in `lib/api.ts` maps these to `DraftResult`; `CreateMatchForm` shows the paste box when given `readDraft` and copies only non-null fields. A draft's place count overrides the format's default count.
