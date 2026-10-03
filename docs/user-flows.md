# User Flows

## 1. Host creates a match (happy path)
1. Host signs in.
2. Opens "Create match", enters venue, date/time, format, players already in, total fee, level range, match type, VietQR details.
3. Sees computed price per player.
4. Submits → match is created → host lands on the match page with a share link.

Edge cases to verify: start time in the past; players already in ≥ capacity; fee of zero; missing bank details; venue without coordinates.

## 2. Player joins from a share link (happy path)
1. Player opens the link (no sign-in needed to view).
2. Sees time, venue, price, level, teams A/B with open slots.
3. Taps an open slot → asked to sign in (Google/Zalo) → returns to the same slot.
4. Slot is held for them → VietQR code with amount and memo is shown.
5. Player transfers money in their banking app.
6. Host later marks the slot as paid.

Edge cases to verify: slot taken while signing in; match full; match cancelled; player already in another slot; player outside level range (block or warn? TODO: verify).

## 3. Browse nearby matches
1. Player opens the app; location from browser permission or chosen district.
2. Sees upcoming matches within a radius, grouped by day, with type and format filters.

Edge cases: location denied; no matches (empty state invites them to host).

## 4. Host manages a match
- Marks players paid / unpaid.
- Removes a player (TODO: verify rules).
- After the match, marks attendance; no-shows appear on profiles.
- Cancels the match (TODO: verify notification behavior without push).

## System Flows
- Link preview: crawler requests `/m/{shareId}` → server-rendered Open Graph tags and a generated preview image.
- Slot claim: conditional update in a single transaction; on conflict return 409 with the current slot state.

## TODO: verify
All edge-case behaviors marked above.
