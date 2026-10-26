# Progress

## What Exists
- AI engineering foundation: `AGENTS.md`, `CONSTITUTION.md`, tool wrappers, memory bank, docs, prompt library, skills, PR template, ignore files.
- `spec.md`, `plan.md`, `README.md` (with local run guide), MIT `LICENSE`.
- UI design prototype (external Claude Design canvas).
- Backend (`backend/`): Axum app with `GET /health` and `GET /api/matches/{shareId}`; `matches` feature split into domain / service / repo / http; first migration (`matches` table, PostGIS, GiST index); `daghep-api migrate` subcommand; lazy DB pool for fast cold starts; multi-stage Dockerfile.
- Frontend (`frontend/`): Next.js 16 PWA with home page, `/m/[shareId]` match page (server-rendered, Open Graph tags without payment details), not-found page, web manifest, `messages/vi.json`.
- Tests: backend 76 unit + 109 acceptance/integration; frontend 187.
- Production web: https://daghep.vn on Vercel (www redirects to apex); DNS at iNET.
- Legal pages: `/privacy`, `/terms`, footer links, `LegalPage` tests.
- Create match (PR 3a): `POST /api/matches` (validation, share id generation with retry, Clock port), `pricePerPlayerVnd` in the public view, `/create` form with live price preview. Limits: fee 0–100M VND, 2–30 slots, start within 30 days, max 4 hours, same HCMC day. `/create` hidden in production until `API_BASE_URL` is set.
- Google sign-in (PR 3b): OAuth code flow with PKCE/state/nonce in `backend/src/auth/`, sessions in Postgres (hashed tokens, 30 days), `/api/me`, sign-out, same-origin guard for non-GET requests; `POST /api/matches` requires sign-in and stores `host_user_id`; frontend proxies `/api/*`, sign-in prompt and account bar on `/create`, `/login-failed`. Real Google login not yet exercised (needs the client secret; verify at deploy).
- Slots (PR 3c): `backend/src/slots/` — public roster, own place, join team A/B with up to 2 guests, leave until kickoff; claims lock the match row (`FOR NO KEY UPDATE`, READ COMMITTED) so teams never overbook; `TeamSlots` on the match page refreshes the roster on load and after each action.
- Payments (PR 3d): `backend/src/payments/` payout accounts, bank list and VietQR payload (verified against a published example); paid matches need a payout account; places carry payment status with a lazily expiring 30-minute hold; players report transfers, hosts confirm or reject; `/account/payout`, `PaymentPanel`, `HostPayments`.
- Cancel (PR 3d-2): host-only `POST /api/matches/{id}/cancel` before kickoff; `cancelledAt` in the public view and `cancelled` in the roster; joins, reports and host payment actions refused afterwards; banner, link-preview prefix, refund note, host cancel button with confirmation.
- Zalo bot (2026-10-13, step 2): `bot` feature, webhook, private linking via `/bot/link`, create in chat after "ok"; tables `zalo_links`, `zalo_link_codes`, `bot_drafts`, `bot_updates`.
- Bot link messages (2026-10-26, PR pending): Zalo sends a link with a preview as message.unsupported.received (seen in production logs); its text or caption is read when present, otherwise the bot says it reads only text (at most hourly per group). Group cards on the home page say how to answer in Zalo.
- Bot poll answers in the chat (2026-10-26, merged #24): linked players answer "T7 có, CN không" in the group without an AI read; unlinked players get the link code privately.
- Bot attendance polls (2026-10-26, PR pending): "@Bot DaGhep tạo bảng điểm danh …" → link to /p/<id>; signed-in yes/no per day; daily nudge with counts and names of who has not answered.
- Bot reminders (2026-10-25, merged and verified live): "@Bot DaGhep nhắc …" read by AI, saved on "ok", posted by a 5-minute scheduler; "tắt nhắc" lists and stops; a message with nothing about a match gets a short answer.
- Zalo bot live (2026-10-25): webhook set; parser accepts the bare update Zalo posts as well as the getUpdates envelope.
- Match drafts (2026-10-12): `drafts` feature, `POST /api/match-drafts`, Anthropic reader, daily limits in `draft_reads`; "Dán tin kèo" box on `/create`. Zalo bot steps planned in `docs/zalo-bot-plan.md`.
- Page-view analytics (2026-10-12): Vercel Web Analytics in the root layout; privacy policy updated (no cookies, aggregated dashboard, visitor hash dropped after 24 hours).
- Automatic API releases (2026-10-10): `.github/workflows/deploy.yml` runs after CI on main when `backend/` changed (Workload Identity Federation, `github-deployer` account; one-time setup in `docs/deploy.md`).
- Deploy (2026-10-10): Cloud Run service `daghep-api` + migrate job, Neon, Secret Manager, Vercel `API_BASE_URL`; runbook in `docs/deploy.md`. Production verified end to end.
- Generic 404 page (match pages keep their own); account bar with sign-out on the cached match page, loaded in the browser.
- Venues and match list (2026-10-10, PR #12): `venues` table with 3 launch venues; matches take `venueId` and copy name and location; `discovery` feature lists open matches (7 local days, day/type filters, distance, keyset paging); home page `MatchList`; venue picker on `/create`; address on the match page.
- Match page redesign (2026-10-10, PR #11): green header with live counts, round team slots chosen by tapping, sticky share + join bar with the party price, VietQR note for paid matches.
- CI: `.github/workflows/ci.yml` (backend fmt/clippy/test with PostGIS service; frontend typecheck/test/build).
- Local dev: `docker-compose.yml` with PostGIS.

## Not Built Yet
- Zalo sign-in, level warning (needs profile levels), attendance and no-show marking, link preview image, "my matches" and profile pages, bottom navigation, host card on the match page.
- Playwright end-to-end test (planned with Step 3).
- ESLint and dependency audit in CI.
- Rate limiting for sign-in start and match creation (e.g. Cloud Armor); `__Host-` cookie prefix in production.

## Known Risks
Cold-start user acquisition, Zalo login approval time, slot race conditions, no-show disputes, extra hand-wiring for OAuth/sessions in Rust, database free-tier limits. Backend Dockerfile built and smoke-tested in CI (PR 3e).

## Completed Setup Work
- Foundation docs (2026-10-03).
- Walking skeleton (2026-10-04): all tests, fmt, clippy, typecheck and build pass locally; manual end-to-end run DB → API → web verified.

## Next Steps
1. Step 3 plan: create match (API + form), Google sign-in, take a slot with race-safe claiming.
2. Add Playwright E2E for the create → share → view flow.
3. Profiles with levels, host card and "my matches".
4. Zalo login once the Zalo app is active.
