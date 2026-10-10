# Project Spec

## Project Name
Daghep — domain daghep.vn (purchase in progress)

## Problem
In Ho Chi Minh City, teams that book a mini pitch often end up short of players, and individuals who want to play have no team. Today this is matched by hand in Facebook and Zalo groups ("pitch 7 needs 2 more players"): posts get buried, nobody tracks who paid, and no-shows are common. No widely used Vietnamese app lets a single player book one slot in a pickup match the way LaBOLA (Japan), Footy Addicts (UK) or Playtomic (padel) do.

## Target Users
- **Players** who want to join a match alone or with a friend.
- **Hosts**: a player or team that has booked a pitch and needs to fill open slots, or an organizer who books a pitch and sells every slot.
- **Venues** (later): pitch owners filling idle hours. TODO: verify whether venues are in MVP.

## Goal
Make it effortless to fill an open slot in a real football match, for free. Success = real matches played because of the app, and people knowing Long built it.

## MVP Scope
1. Sign in with Google or Zalo.
2. Host creates a match: venue, date/time, format (5/7/11-a-side), players already in, total pitch fee, level range, match type, VietQR payment details.
3. Match page with a share link and a rich link preview for Zalo/Messenger; joinable from the link.
4. List of upcoming matches near the user, filtered by day, match type and format.
5. Player joins a specific open slot on team A or B, then pays the host directly via VietQR.
6. Host confirms payments received and, after the match, marks attendance / no-shows.
7. Player profile showing matches played and no-shows.
8. Footer attribution "Free · Built by Long" linking to Long's LinkedIn (https://www.linkedin.com/in/long-pham-55466920b/).

## Non-goals
- Holding, escrowing, refunding or moving money in any way.
- In-app chat, push notifications, native mobile apps.
- Court booking or venue calendar management.
- Tournaments, leagues, rankings.
- Sports other than football.
- Monetization.

## Current State
Walking skeleton implemented (see `memory-bank/progress.md`). A clickable design exists (Claude Design canvas for this project: match list, match detail with team slots, create match, dark mode list).

## Desired Behavior
See `docs/user-flows.md`.

## Core User Flows
1. Host creates a match → gets a share link → posts it in a Zalo group.
2. Player opens the link → sees teams and open slots → signs in → takes a slot → pays host via VietQR.
3. Host marks the player as paid.
4. After the match the host marks who attended.
5. Player browses nearby matches and joins one.

## Domain Rules
Confirmed:
- The service is free and never handles money; payment goes player → host directly.
- Football only in MVP. Formats: 5-a-side, 7-a-side, 11-a-side.
- Match shows two teams (A and B); a player picks a team, not a numbered position (confirmed by Long 2026-10-05). Places split evenly; team A takes the odd one.
- Player level is a number from 1.0 (beginner) to 5.0 (semi-pro) in steps of 0.5; a match declares a level range.
- Match types: casual, competitive, beginner-friendly.
- Sign-in methods: Google and Zalo.
- Launch area: District 2 (Thu Duc City), Ho Chi Minh City. First venues to try: SSA Sports Center, Football Field An Phu, An Phu Sports Complex.
- Open source under MIT; repository is public.
- MVP sized for ~100 users; architecture must scale without rewrites.
- Slot hold: joining a paid match holds the place for 30 minutes. If the player has neither reported a transfer nor had it confirmed by the host within 30 minutes, the place is released automatically.
- Payments (confirmed by Long 2026-10-06): the host saves one payout account on their profile (bank, account number, account name) and reuses it; a paid match cannot be posted without it (free matches can). A player who taps "I transferred" stops the 30-minute countdown and waits for the host; the host confirms ("received") or reports it missing, which releases the party's places. Free matches hold places immediately with no countdown. The host's own party in their own match needs no transfer (confirmed by Long 2026-10-09). The host can report a transfer missing only after the player reported it, and only before kickoff (confirmed by Long 2026-10-09); later cases go to no-show marking. One transfer covers the player and their guests; the memo is `DAGHEP <payment code>`, and the host sees the same code in their list.
- One account may hold its own slot plus up to 2 named guests in the same match.
- Only the host marks no-shows. A marked player can dispute it; disputes are reviewed by the admin (Long).
- The host's VietQR / bank details are shown only to players who currently hold a slot in that match — never on the public match page or in link previews.
- Player level is self-assessed at sign-up. Peer rating after matches is post-MVP.
- Public visibility (2026-10-04): anyone, signed in or not, can see match details (venue and location, time, type, level, fee, slot count), participants' display names and avatars, guest names, and player profiles (display name, avatar, level, matches played, no-shows).
- Account deletion (2026-10-04): delete name, avatar, email, payout details and Google/Zalo link; keep match and slot history anonymized; technical logs auto-delete after 30 days.
- Minimum age 16 (2026-10-04).
- Confirmed by Long (2026-10-04): no-show disputes go to lienhe@daghep.vn and the admin removes wrong marks (in-app "Dispute" button later); no phone number collected in MVP (revisit if users evade no-show marks with new accounts); scam or harassment accounts can be locked; browser location is used only for the nearby search, not saved to the profile (may appear in technical logs up to 30 days); on account deletion, guest names the user entered are deleted and shown as "Guest".
- Operator named in legal pages: Pham Trinh Hoang Long (personal project); contact lienhe@daghep.vn.

- Slot capacity (2026-10-04): default includes substitutes so players can rotate — 5-a-side 14 (10 + 4), 7-a-side 18 (14 + 4), 11-a-side 28 (22 + 6). The host can change it.
- Price per player (2026-10-04): total fee ÷ slot count, rounded up to the nearest 1,000 VND; the host keeps the small remainder.
- Venues and the match list (confirmed by Long 2026-10-10): hosts choose from a fixed list of venues (launch: SSA Sports Center (Amitie Thảo Điền), Sân bóng An Phú Quận 2, Khu thể thao An Phú); other venues are added on request via lienhe@daghep.vn. The home page lists matches for today and the next six days in Ho Chi Minh City, only those with open places, not cancelled and not started, by kickoff time; filters by day and match type; distance shown when the visitor shares their position.
- Who can host (2026-10-04): any signed-in user. Venues use the same flow; a "verified venue" badge comes later.
- Cancellation (2026-10-04): the host can cancel a match and slot holders see "match cancelled" on the page; refunds are between players and host (the app holds no money). A player can leave before kickoff; leaving within 2 hours of kickoff lets the host mark a no-show.
- Cancellation details (built in PR 3d-2; confirmed by Long 2026-10-10): only before kickoff, cannot be undone; "[Đã huỷ]" in link previews; afterwards nothing changes any more: no joining, leaving, reporting a transfer or confirming/rejecting payments, holds stop expiring, and no QR is shown, so the host's list stays complete for refunds.

- Joining (confirmed by Long 2026-10-05): the host does not get a place automatically and joins like anyone else. A player may bring up to 2 named guests in the same team, in one request; the whole group fits or nobody is added. To change guests, leave and join again. A player can leave until kickoff; joining and leaving close at kickoff. A player outside the match's level range is warned but not blocked (needs self-assessed levels on profiles, not built yet).
- Sign-in (PR 3b): Google sign-in is required to create a match; anyone can view matches without signing in. A sign-in lasts 30 days (confirmed by Long 2026-10-05).
- Match limits (confirmed by Long 2026-10-04): total fee 0–100,000,000 VND (zero-fee matches are allowed); 2–30 slots; a match starts in the future and at most 30 days ahead; lasts at most 4 hours; starts and ends on the same day in Ho Chi Minh City time (no matches past midnight).

## Data / State Needed
User (identity provider, display name, level, stats), Match (venue, location point, start/end, format, type, level range, fee, host, status, share id), Slot (match, team, position, player, joined at, payment status, attendance), Host payment profile (bank BIN, account number, account name).

## External Integrations
- Google OAuth 2.0 / OpenID Connect.
- Zalo login (Zalo for Developers app registration required; TODO: verify scopes and review process).
- VietQR: generate payment QR payloads from bank BIN + account + amount + memo (TODO: verify spec and whether to render locally or via a public image API).
- Maps/geocoding for venue location (TODO: verify provider and cost).

## Security / Privacy Notes
See `docs/threat-model.md`. Key points: authorization on every host action, race-safe slot claiming, bank account numbers treated as sensitive, no secrets in the repo, link previews never leak private data.

## Acceptance Criteria
- [ ] A signed-in user can create a match and receives a share link.
- [ ] Opening the share link without signing in shows the match, teams and open slots.
- [ ] A signed-in player can take an open slot; the slot count updates for everyone.
- [ ] Two players claiming the same slot at the same moment never both succeed.
- [ ] A player can hold at most their own slot plus 2 named guests in the same match.
- [ ] A held slot whose payment the host has not confirmed within 30 minutes is released and becomes claimable again.
- [ ] The VietQR code is returned only to players holding a slot; the public match page and link preview never include bank details.
- [ ] A player marked as no-show can file a dispute that the admin can resolve.
- [ ] The match page shows a VietQR code with the correct amount and memo.
- [ ] Only the host can mark payment and attendance; other users get 403.
- [ ] The match list shows upcoming matches within a radius, sorted by start time.
- [ ] The link preview in Zalo shows venue, time, open slots and price.

## Risks
- Cold start: no matches → no users. Mitigation: Long hosts the first matches in District 2.
- Zalo login approval may delay launch; Google-only fallback.
- No-show abuse and host disputes.
- Rust ecosystem needs more hand-wiring for OAuth/sessions than mainstream frameworks (slower first weeks).
- Database free tiers may pause or throttle; monitor before launch.

## Test Plan
See `docs/testing-strategy.md`.

## Rollback / Recovery Plan
Every deploy is a container image tag; roll back by redeploying the previous tag. sqlx migrations are forward-only and additive during MVP; destructive migrations need explicit approval and a backup first.

## TODO: verify
- Domain daghep.vn purchase (in progress).
- Remaining "proposed" domain rules above.
