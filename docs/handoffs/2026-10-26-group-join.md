# Frontend Handoff

## What changed
Public weekly groups have their own page `/g/<id>` where signed-in players ask to join; the organiser approves in Zalo. Home-page group cards link there.

## Endpoints affected
- `GET /api/groups`: each group now has `id`.
- `GET /api/groups/{id}`: new.
- `POST /api/groups/{id}/join`: new.

## Request changes
- `POST /api/groups/{id}/join`: no body; needs the session cookie.

## Response changes
- `GET /api/groups/{id}`: the public group fields plus `membership`: `null` (signed out), `{"status":"none"}`, `{"status":"pending"}`, `{"status":"declined","askAgainAt":"<RFC 3339>"}` or `{"status":"member","groupLink":"https://zalo.me/g/…"|null,"pollPath":"/p/<share id>"|null}`. `Cache-Control: private, no-store`.
- `POST …/join` 200: `{"membership": …}` (pending, or member when already one).

## Validation / auth / error changes
- 404 `not_found`: unknown, hidden or malformed id.
- 401 on join without a session.
- 409 `too_soon` with `askAgainAt` (declined within 7 days); 409 `group_busy` (organiser has 50 pending); 429 `too_many_requests` (player has 5 pending).

## Frontend actions required
Done in the same PR: `GroupJoin` component, `/g/[id]` page, card link.

## Example payloads
`GET /api/groups/0123456789ab` → `{"id":"0123456789ab","title":"Ai chơi bóng rổ cuối tuần này?",…,"membership":{"status":"pending"}}`

## TODO verify
- Whether Zalo lets the bot message an organiser or player who has not written to it recently.
