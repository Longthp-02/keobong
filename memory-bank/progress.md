# Progress

## What Exists
- AI engineering foundation: `AGENTS.md`, `CONSTITUTION.md`, tool wrappers, memory bank, docs, prompt library, skills, PR template, ignore files.
- `spec.md`, `plan.md`, `README.md` (with local run guide), MIT `LICENSE`.
- UI design prototype (external Claude Design canvas).
- Backend (`backend/`): Axum app with `GET /health` and `GET /api/matches/{shareId}`; `matches` feature split into domain / service / repo / http; first migration (`matches` table, PostGIS, GiST index); `daghep-api migrate` subcommand; lazy DB pool for fast cold starts; multi-stage Dockerfile.
- Frontend (`frontend/`): Next.js 16 PWA with home page, `/m/[shareId]` match page (server-rendered, Open Graph tags without payment details), not-found page, web manifest, `messages/vi.json`.
- Tests: backend 58 unit + 80 acceptance/integration; frontend 109.
- Production web: https://daghep.vn on Vercel (www redirects to apex); DNS at iNET.
- Legal pages: `/privacy`, `/terms`, footer links, `LegalPage` tests.
- Create match (PR 3a): `POST /api/matches` (validation, share id generation with retry, Clock port), `pricePerPlayerVnd` in the public view, `/create` form with live price preview. Limits: fee 0–100M VND, 2–30 slots, start within 30 days, max 4 hours, same HCMC day. `/create` hidden in production until `API_BASE_URL` is set.
- Google sign-in (PR 3b): OAuth code flow with PKCE/state/nonce in `backend/src/auth/`, sessions in Postgres (hashed tokens, 30 days), `/api/me`, sign-out, same-origin guard for non-GET requests; `POST /api/matches` requires sign-in and stores `host_user_id`; frontend proxies `/api/*`, sign-in prompt and account bar on `/create`, `/login-failed`. Real Google login not yet exercised (needs the client secret; verify at deploy).
- Slots (PR 3c): `backend/src/slots/` — public roster, own place, join team A/B with up to 2 guests, leave until kickoff; claims lock the match row (`FOR NO KEY UPDATE`, READ COMMITTED) so teams never overbook; `TeamSlots` on the match page refreshes the roster on load and after each action.
- Payments (PR 3d): `backend/src/payments/` payout accounts, bank list and VietQR payload (verified against a published example); paid matches need a payout account; places carry payment status with a lazily expiring 30-minute hold; players report transfers, hosts confirm or reject; `/account/payout`, `PaymentPanel`, `HostPayments`.
- CI: `.github/workflows/ci.yml` (backend fmt/clippy/test with PostGIS service; frontend typecheck/test/build).
- Local dev: `docker-compose.yml` with PostGIS.

## Not Built Yet
- Zalo sign-in, host cancels a match (PR 3d-2), level warning (needs profile levels), attendance and no-show marking, nearby list, link preview image, deployment.
- Playwright end-to-end test (planned with Step 3).
- ESLint and dependency audit in CI.
- Rate limiting for sign-in start and match creation (e.g. Cloud Armor); `__Host-` cookie prefix in production.

## Known Risks
Cold-start user acquisition, Zalo login approval time, slot race conditions, no-show disputes, extra hand-wiring for OAuth/sessions in Rust, database free-tier limits. Backend Dockerfile not yet built in CI (verify before first deploy).

## Completed Setup Work
- Foundation docs (2026-10-03).
- Walking skeleton (2026-10-04): all tests, fmt, clippy, typecheck and build pass locally; manual end-to-end run DB → API → web verified.

## Next Steps
1. Step 3 plan: create match (API + form), Google sign-in, take a slot with race-safe claiming.
2. Add Playwright E2E for the create → share → view flow.
3. Deploy after Long sets up GCP, Neon and Vercel.
