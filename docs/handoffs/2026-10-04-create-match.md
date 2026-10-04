# Frontend Handoff

## What changed
- New endpoint to create a match.
- The public match view gains `pricePerPlayerVnd`.

## Endpoints affected
- `POST /api/matches` (new)
- `GET /api/matches/{shareId}` (response gains one field)

## Request changes
`POST /api/matches`, JSON body (camelCase):

| Field | Type | Rule |
|---|---|---|
| `venueName` | string | trimmed, 1–120 chars |
| `startsAt` | RFC 3339 UTC | in the future, at most 30 days ahead |
| `endsAt` | RFC 3339 UTC | after `startsAt`, at most 4 hours, same HCMC day |
| `format` | `five_a_side` \| `seven_a_side` \| `eleven_a_side` | |
| `matchType` | `casual` \| `competitive` \| `beginner_friendly` | |
| `levelMin`, `levelMax` | number | 1.0–5.0, steps of 0.5, `levelMax >= levelMin` |
| `totalFeeVnd` | integer | 0–100,000,000 (zero allowed) |
| `slotCount` | integer, optional | any integer is accepted by the parser; outside 2–30 is a `422`; default 14 / 18 / 28 by format |

No authentication yet; sign-in is required from PR 3b. The backend is not deployed yet, so this is not publicly reachable.

## Response changes
- `201 Created` with the public match view (same shape as `GET`).
- `pricePerPlayerVnd` = `totalFeeVnd / slotCount` rounded up to the next 1,000 VND. The API is authoritative; `frontend/lib/pricing.ts` mirrors the rule only for the live preview.

## Validation / auth / error changes
- `422 {"error":"invalid_match","field":"<camelCase field>"}` for the first invalid field.
  Order: `format`, `matchType` (parsed in the HTTP adapter), then `venueName`, `startsAt`, `endsAt`, `levelMin`, `levelMax`, `totalFeeVnd`, `slotCount` (domain validation).
- `venueName` is trimmed; empty, longer than 120 characters, or containing control / bidi-override characters is a `422`.
- `400 {"error":"invalid_request"}` for malformed JSON, wrong types or missing fields.
- `500 {"error":"internal_error"}` otherwise (details only in server logs).

## Frontend actions required
Done in this PR: `/create` page (hidden, and `404`, in production until `API_BASE_URL` is set) (server action calls the API so the API URL stays server-side), `MatchCard` shows the price per player.

## Example payloads
```json
// request
{"venueName":"SSA Sports Center","startsAt":"2099-10-10T11:30:00Z","endsAt":"2099-10-10T13:00:00Z","format":"seven_a_side","matchType":"casual","levelMin":2.5,"levelMax":3.5,"totalFeeVnd":900000}
// 201 response
{"shareId":"9ut86CBoV6","venueName":"SSA Sports Center","startsAt":"2099-10-10T11:30:00Z","endsAt":"2099-10-10T13:00:00Z","format":"seven_a_side","matchType":"casual","levelMin":2.5,"levelMax":3.5,"totalFeeVnd":900000,"slotCount":18,"pricePerPlayerVnd":50000}
```

## Confirmed rules (Long, 2026-10-04)
- Fee cap 100,000,000 VND (DB constraint `matches_total_fee_vnd_max`, migration approved); zero-fee matches allowed.
- `startsAt` must be in the future and at most 30 days ahead, else `422 startsAt`.
- `endsAt` must be after `startsAt`, at most 4 hours later, and on the same Ho Chi Minh City calendar day (no matches past midnight), else `422 endsAt`.
