# Implementation Plan

## Goal
Ship a free PWA where a host can post a pickup football match and strangers can take open slots via a share link, launching in District 2, Ho Chi Minh City.

## Assumptions
- Stack per `AGENTS.md` (Rust/Axum + PostgreSQL/PostGIS backend, Next.js PWA frontend).
- MVP serves ~100 users; every step follows the Scale-Ready Rules in `AGENTS.md`.
- Monorepo with `backend/` and `frontend/`.
- Single region, low traffic (hundreds of users) for the first months.
- All proposed domain rules in `spec.md` stay `TODO: verify` until Long confirms.

## Non-goals
See `spec.md`. In particular: no money handling, no chat, no native app, no court booking.

## Vertical Slice Strategy
Each slice delivers one user-visible behavior end-to-end (DB → API → UI → test) inside one feature package. Order: match viewing → match creation → joining → payments display → host confirmation → nearby list → auth hardening → link previews → attendance.

## Walking Skeleton
The thinnest real path: a match row seeded in Postgres is served by `GET /api/matches/{shareId}` and rendered by the Next.js page `/m/{shareId}`, with CI running a backend acceptance test against real PostGIS and a frontend Playwright smoke test.

## Steps

### Step 1 — Project skeleton / tooling
Monorepo layout, Cargo backend (Axum) with one health endpoint, Next.js frontend with `messages/vi.json`, Docker Compose for PostGIS, first sqlx migration, multi-stage Dockerfile, GitHub Actions running fmt, clippy, `cargo test` and `pnpm test`. MIT license already in place.

### Step 2 — Walking skeleton
`matches` feature: domain model, `MatchRepository` trait, sqlx adapter, read endpoint by share id, frontend page rendering it. One acceptance test per layer.

### Step 3 — First real feature slice
Create match (unauthenticated stub user behind a feature flag until auth lands) → returns share id → page shows teams with open slots. Then Google sign-in, then take a slot with race-safe claiming.

### Step 4 — Tests and verification
Acceptance tests for each flow in `docs/user-flows.md`; concurrency test for slot claiming; wrong-user tests for host actions.

### Step 5 — Review and hardening
Second-pass AI review, threat-model walkthrough, link preview, Zalo login, rate limits, deploy.

## Tests Needed
- Integration: sqlx repository adapters against real PostGIS (testcontainers).
- Acceptance: API flows through the Axum router (`tower::ServiceExt::oneshot`) against a real DB.
- Concurrency: N parallel claims on the last slot → exactly one wins.
- Unit: fee split, level-range checks, VietQR payload builder.
- Frontend: component tests (Vitest) and one Playwright flow per user flow.

## Risks
See `spec.md`.

## Open Questions
1. Unpaid slot expiry rule.
2. Can one account take several slots (bringing friends)?
3. DB provider (Neon vs Supabase) and frontend hosting.

## Recommended First Coding Task
Step 1 + Step 2 only: scaffold the monorepo and deliver the walking skeleton (`GET /api/matches/{shareId}` → `/m/{shareId}`), with CI green. Nothing else.
