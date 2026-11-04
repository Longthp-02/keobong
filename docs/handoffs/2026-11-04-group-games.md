# Frontend Handoff

## What changed
The home page also lists one-off group games ("Kèo nhóm sắp tới") made in Zalo, each linking to its poll page.

## Endpoints affected
- `GET /api/groups`: adds `games`.

## Request changes
None.

## Response changes
- `games`: `[{ "title", "sport": "football"|"basketball"|null, "format": null, "venue": {"name","address"}|null, "days": [{"day":"YYYY-MM-DD","yes":n}], "playTime": "HH:MM"|null, "pollPath": "/p/<share id>" }]`, soonest first, no names. Cached 60 s like `groups`.

## Validation / auth / error changes
None.

## Frontend actions required
Done in the same PR: `getGroups` returns `{ groups, games }` (games default to `[]` for an older API); `GroupList` renders both.

## Example payloads
`{"groups":[],"games":[{"title":"Bóng rổ vui vẻ hôm nay","sport":"basketball","format":null,"venue":{"name":"Sân Lakeview","address":"…"},"days":[{"day":"2026-11-04","yes":3}],"playTime":"19:00","pollPath":"/p/abcDEF12"}]}`

## TODO verify
None.
