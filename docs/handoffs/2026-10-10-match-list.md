# Frontend Handoff: venues and the match list

## What changed
Matches now belong to a venue from a fixed list, and the home page lists upcoming matches with open places.

## Endpoints affected
- New `GET /api/venues` (public, `Cache-Control: public, max-age=300`).
- New `GET /api/matches` (public, `Cache-Control: public, max-age=30`).
- Changed `POST /api/matches`.
- Changed `GET /api/matches/{shareId}` (new field).

## Request changes
- `POST /api/matches` requires `venueId` (a venue slug). `venueName` is ignored; the web app still sends it so an API from before this change keeps working during a deploy.
- `GET /api/matches` query parameters, all optional:
  - `date`: `YYYY-MM-DD`, a local day from today to six days ahead; default all seven days.
  - `type`: `casual`, `competitive` or `beginner_friendly`.
  - `lat` and `lng`: both or neither. The web app rounds them to 3 decimals.
  - `cursor`: the `nextCursor` of the previous page.

## Response changes
- Match views gain `venueAddress` (string, or `null` for matches created before venues).
- `GET /api/venues` returns `{ "venues": [{ "id", "name", "address" }] }`.
- `GET /api/matches` returns `{ "matches": [MatchView & { "placesLeft", "distanceM" }], "nextCursor": string | null }`. `distanceM` is whole metres, or `null` without a position.

## Validation / auth / error changes
- `POST /api/matches` with an unknown or inactive venue: 422 `{ "error": "invalid_match", "field": "venueId" }`. Missing `venueId`: 400 `invalid_request`.
- `GET /api/matches` with a bad parameter: 422 `{ "error": "invalid_query", "field": "date" | "type" | "near" | "cursor" }`.

## Frontend actions required
Done in the same PR: venue picker on `/create`, address on the match page, home page list (`MatchList`).

## Example payloads
`GET /api/matches?date=2026-10-11&lat=10.800&lng=106.740`
```json
{ "matches": [{ "shareId": "demo0002", "venueName": "SSA Sports Center (Amitie Thảo Điền)", "venueAddress": "28 Duyên Hải, An Khánh", "startsAt": "2026-10-11T11:30:00Z", "placesLeft": 9, "distanceM": 778, "...": "other MatchView fields" }], "nextCursor": null }
```

## TODO verify
None.
