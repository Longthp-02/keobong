# Frontend Handoff

## What changed
The Zalo bot posts reminders on a schedule. Everything happens in the chat; no web page changed. The privacy policy now covers stored reminders (updated 25/10/2026).

## Endpoints affected
- `POST /api/bot/reminders/run`: new. Called by Cloud Scheduler every 5 minutes, not by the web app.

## Request changes
- Header `X-Cron-Secret: <BOT_CRON_SECRET>`; no body.

## Response changes
- `200 {"sent": <number posted>}`.

## Validation / auth / error changes
- `403 {"error":"forbidden"}` without the right secret; `404` when `BOT_CRON_SECRET` or the bot is not configured; `500 {"error":"internal_error"}` on a storage error.

## Frontend actions required
None.

## Example payloads
`curl -X POST -H "X-Cron-Secret: $SECRET" "$API/api/bot/reminders/run"` → `{"sent":0}`

## TODO verify
- Done 2026-10-25: Zalo lets the bot post to a group without a recent message from it.
