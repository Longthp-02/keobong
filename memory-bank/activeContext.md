# Active Context

> Update after every meaningful AI session.

## Current Focus
Production is live (2026-10-10): API on Cloud Run (`daghep-api`, asia-southeast1) with Neon (Singapore), web on Vercel proxying `/api`. Verified end to end with real Google sign-in: payout account, paid match, join from a second account, VietQR scan, report, confirm, cancel, sign-out. Match page redesigned to the Claude Design canvas (PR #11). Venues and the home match list (PR #12): fixed venue list (3 launch venues in District 2), GET /api/venues, GET /api/matches with day/type filters, distance and cursor paging; home page from the Main artboard. After merging, the API must be released (migrations add venues). Next: Zalo login (PR 3f) once Long activates the Zalo app; host card and per-player levels need profiles.

## Latest Decisions
- Name: Daghep (UI: "Da Ghep" with Vietnamese diacritics in vi.json); domain daghep.vn owned by Long (active). Renamed from "Keo" because "keo bong" reads as football betting slang.
- Stack: Rust (Axum, sqlx) + PostgreSQL/PostGIS backend, Next.js PWA frontend. Rust chosen for near-zero cost at MVP scale and scale-to-zero without slow cold starts.
- Target: ~100 users first, built so scaling is configuration, not rewrite (see `AGENTS.md` Scale-Ready Rules).
- Sign-in: Google and Zalo.
- Repository: public, MIT license.
- Repo is English-only; single exception: UI copy in `frontend/messages/vi.json`.
- Launch area: District 2, Ho Chi Minh City. First venues to try: SSA Sports Center (An Khanh), Football Field An Phu (Nguyen Hoang St.), An Phu Sports Complex (Mai Chi Tho St.).
- Payments: direct player → host via VietQR; the app never holds money.
- Confirmed rules (2026-10-04): 30-minute slot hold, own slot + 2 named guests, host-only no-show marking with admin-reviewed disputes, bank details only for slot holders, self-assessed level.
- Deployment: Cloud Run (asia-southeast1) + Neon (Singapore) + Vercel; PWA first, Capacitor native wrap later if adoption is good.
- Attribution footer links to https://www.linkedin.com/in/long-pham-55466920b/.
- Design reference: Claude Design canvas with match list, match detail (teams A/B, slot buttons), create match, dark mode.
- Level steps of 0.5 confirmed by Long (2026-10-04); enforced in `Level` and a DB CHECK.
- Public match API sends `Cache-Control: public, max-age=30`; the web match page uses ISR (revalidate 30s) so a CDN can absorb share-link bursts.
- Legal pages (2026-10-04): /privacy and /terms (content in vi.json), required for Zalo/Google app registration. Public visibility, deletion/anonymization, minimum age 16 and operator name confirmed by Long (see spec.md Domain Rules). Vercel functions pinned to sin1 via frontend/vercel.json.
- Rules confirmed 2026-10-04 for Step 3: default slots with substitutes 14/18/28 (host can edit), price rounded up to 1,000 VND, any signed-in user can host, host can cancel, player can leave (within 2h of kickoff the host may mark a no-show).
- Step 3 plan approved: 3a create match, 3b Google sign-in + sessions, 3c slots and race-safe claiming, 3d VietQR + payment confirmation + 30-min release + cancel, 3e Zalo login + backend deploy (Cloud Run + Neon).
- Public IDs (not secrets): Zalo App ID 3441994593394325596; Google OAuth Client ID 669288809087-96o9da5roaa70da6m0grt0102ggih5oo.apps.googleusercontent.com. Secrets live only with Long / Secret Manager.
- Skeleton decisions (2026-10-04): malformed and unknown share ids both return 404 `match_not_found`; public match view exposes no internal id and no payment details; match stores explicit `slot_count` and `total_fee_vnd` (no per-player price shown until rounding is confirmed); migrations run via `daghep-api migrate`, not on startup; tests use `#[sqlx::test]` against real PostGIS.

## Open Questions
1. Remaining proposed rules in `spec.md` (slot capacity, price rounding, who can host, cancellations).

## Pending on Long
- Check whether the iNET mailbox lienhe@daghep.vn is a time-limited trial (it is the contact in the legal pages).
- Zalo for Developers: activate the app, verify the domain daghep.vn, set the callback `https://daghep.vn/api/auth/zalo/callback`.

## Next Safe Step
Zalo bot is live (2026-10-25). Long's decisions 6–10 are in `docs/zalo-bot-plan.md`: reminders first, then attendance polls with a daily nudge (sign-in required), then basketball (3x3, 5x5, venues from Long, name stays Đá Ghép). The reminders PR adds `bot_pending_reminders` and `bot_reminders` (migration and limits approved by Long 2026-10-25) and needs `BOT_CRON_SECRET` plus a Cloud Scheduler job after merge (`docs/deploy.md`). Reminders are live and verified (a test reminder posted in Long's group). The attendance poll PR adds `bot_pending_polls`, `bot_polls`, `bot_poll_answers`, `bot_chat_members` and the `/p/<id>` page (migration and limits approved by Long 2026-10-25). Polls state the play time and nudge at 20:00 by default (Long, 2026-10-26; migration `20261026010000_add_poll_play_time.sql` approved by Long 2026-10-26). Weekly polls PR (migration `20261026020000_add_weekly_polls.sql`, approved by Long 2026-10-26) adds "hằng tuần" polls posted every Monday 20:00 until "tắt bảng", and a help message listing every command. Player help PR: "tìm kèo" lists open matches; unlinked organisers get the link privately when Zalo allows. Public groups step 1 PR (migration `20261026030000_add_public_groups.sql` approved by Long 2026-10-26): venues from Maps links with admin approval, sport/format/venue on weekly polls, "công khai", home-page section. After merge Long sets `ADMIN_USER_IDS`. Next: join requests (step 2), venue photos (step 3), real @-mentions test. Still open: the full account-deletion procedure (bot-data SQL is in `docs/deploy.md`).
