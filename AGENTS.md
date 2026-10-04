# AGENTS.md — Daghep (pickup football matchmaking)

Start every AI session by reading this file, then `CONSTITUTION.md`, `memory-bank/activeContext.md` and `memory-bank/progress.md`.

## What This Is
Daghep is a free, open-source (MIT) web app (PWA) that lets individual players in Ho Chi Minh City join pickup football matches with strangers and split the pitch fee. Hosts post a match with open slots; players join via a shareable link. The app never holds money: players pay the host directly via VietQR. Built as a personal-brand project by Long. MVP target: ~100 users in District 2, designed to scale without rewrites.

## Stack (TODO: verify crate versions at scaffold time)
- Backend: Rust (stable), Axum, Tokio, sqlx, serde, tower-http, tracing
- Database: PostgreSQL + PostGIS; migrations with `sqlx migrate`
- Frontend: Next.js (TypeScript), installable PWA; UI strings in `frontend/messages/vi.json`
- Auth: Google OIDC + Zalo OAuth; sessions stored in Postgres, referenced by a signed HTTP-only cookie
- Tests: `cargo test` with `#[sqlx::test]` against real PostGIS (`DATABASE_URL`); Vitest + Testing Library on frontend; Playwright E2E from Step 3
- Quality: rustfmt, clippy (`-D warnings`), cargo-deny/cargo-audit
- CI: GitHub Actions running all tests on every push and PR
- Deploy: container on GCP Cloud Run (scale to zero); managed Postgres with PostGIS (TODO: verify provider)

## Key Directories (planned)
- `backend/` — Axum app, one module per feature: `src/<feature>/{domain,service,repo,http}.rs`
- `backend/migrations/` — sqlx migrations
- `frontend/` — Next.js PWA
- `docs/`, `memory-bank/`, `prompts/`, `.github/skills/` — AI and project context

## Architecture / Patterns
- Modular monolith; vertical slices by feature (`matches`, `slots`, `players`, `auth`, `payment`, `share`).
- Features never use another feature's internals; call its public `service` API.
- Define traits (ports) in `domain` before writing adapters.
- `domain` and `service` code has no Axum, sqlx, HTTP or SDK types.
- Prioritize server performance: indexed geo queries, pagination, no N+1, small binary, fast startup.

## Scale-Ready Rules (non-negotiable)
- The API is stateless: no in-memory sessions, caches or locks that correctness depends on.
- All state lives in Postgres; uploaded files go to object storage, never the local disk.
- Slot claiming is atomic in the database (conditional update + unique constraints).
- Every list endpoint is paginated and backed by an index.
- Public match pages and link previews are cacheable by a CDN.
- No microservices, message brokers or Redis until a measured need exists.

## Non-Negotiables
- Human defines correctness. Do not invent domain behavior; write `TODO: verify`.
- If requirements are unclear or conflicting, ask the human immediately.
- Everything in the repo is in English: code, comments, docs, commits. Single exception: end-user UI copy in `frontend/messages/vi.json`.
- No hardcoded secrets; config via env vars / secret manager. Never log secrets or bank account numbers.
- Never swallow failures silently (no `.unwrap()`/`.expect()` on fallible runtime paths; use `Result` and `tracing`).
- No new dependency without a stated reason. No renames of objects, files or endpoints unless required.
- The app must never take custody of money.

## Commands
- Local DB: `docker compose up -d db` (PostGIS on localhost:5432)
- Backend env: `export DATABASE_URL=postgres://postgres:postgres@localhost:5432/daghep_dev`
- Backend tests: `cd backend && cargo test` (needs `DATABASE_URL`; each test gets its own database)
- Backend lint: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- Run migrations: `cargo run -- migrate`; run API: `cargo run` (port 8080)
- Frontend: `cd frontend && pnpm install && pnpm test && pnpm typecheck && pnpm build`
- Run web: `API_BASE_URL=http://localhost:8080 pnpm dev` (port 3000)

## Task Workflow
1. Non-trivial work: plan first (`prompts/plan-first.md`).
2. Behavior change: choose test strategy before coding (`prompts/tdd-ai-workflow.md`).
3. Bug fix: reproducing failing test first, confirm it fails, fix root cause.
4. New major feature: walking skeleton first.
5. Backend changes that affect the frontend: write a note in `docs/handoffs/`.
6. After implementation: remind the human to run a second-pass AI review.

## Context Maintenance Protocol
After every meaningful session update `memory-bank/activeContext.md` and `memory-bank/progress.md` (see `.github/skills/context-maintenance/SKILL.md`). Change this file only for stable, project-wide rules.
