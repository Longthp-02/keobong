# Handoff: host cancels a match (PR 3d-2)

## Endpoint changes
| Method | Path | Auth | Result |
|---|---|---|---|
| POST | `/api/matches/{id}/cancel` | host | `204` (also when already cancelled); `403 not_host`; `404 match_not_found`; `409 match_started` at or after kickoff. |
| GET | `/api/matches/{id}` | public | Adds `cancelledAt` (`null` or RFC 3339). |
| GET | `/api/matches/{id}/slots` | public | Adds `cancelled` (boolean). |
| POST | `/api/matches/{id}/slots` | session | `409 match_cancelled`. |
| POST | `/api/matches/{id}/slots/mine/report-payment` | session | `409 match_cancelled`. |
| POST | `/api/matches/{id}/payments/{code}/confirm` and `/reject` | host | `409 match_cancelled`. |

`GET .../slots/mine` returns `payment: null` for a cancelled match. `DELETE .../slots/mine` returns `409 match_cancelled`. Holds stop expiring at the moment of cancellation, so the roster, the players' own places and the host's payment list stay exactly as they were, for refunds.

## Database changes
Migration `20261009000000_add_match_cancellation.sql` (approved by Long 2026-10-10): `matches.cancelled_at timestamptz NULL`. Cancelling takes the same `FOR NO KEY UPDATE` lock on the match row as claims, so no join can slip in during a cancellation.

## Frontend actions
Done in this PR:
- "Kèo đã huỷ" banner on the match card, and a "[Đã huỷ]" prefix in link-preview titles.
- `TeamSlots` shows the cancellation, plus a refund note for players who owed money.
- `HostPayments` has a "Huỷ kèo" button with inline confirmation. After cancelling it keeps the list for refunds but drops the actions.
