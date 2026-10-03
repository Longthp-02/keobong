# Active Context

> Update after every meaningful AI session.

## Current Focus
Project foundation: AI context files, spec and plan. No application code yet.

## Latest Decisions
- Name: Keo; candidate domain keobong (TODO: verify availability).
- Stack: Rust (Axum, sqlx) + PostgreSQL/PostGIS backend, Next.js PWA frontend. Rust chosen for near-zero cost at MVP scale and scale-to-zero without slow cold starts.
- Target: ~100 users first, built so scaling is configuration, not rewrite (see `AGENTS.md` Scale-Ready Rules).
- Sign-in: Google and Zalo.
- Repository: public, MIT license.
- Repo is English-only; single exception: UI copy in `frontend/messages/vi.json`.
- Launch area: District 2, Ho Chi Minh City. First venues to try: SSA Sports Center (An Khanh), Football Field An Phu (Nguyen Hoang St.), An Phu Sports Complex (Mai Chi Tho St.).
- Payments: direct player → host via VietQR; the app never holds money.
- Attribution footer links to https://www.linkedin.com/in/long-pham-55466920b/.
- Design reference: Claude Design canvas with match list, match detail (teams A/B, slot buttons), create match, dark mode.

## Open Questions
1. Proposed domain rules listed in `spec.md` (slot expiry, multiple slots per account, no-show disputes, bank detail visibility).
2. Postgres provider (Neon vs Supabase) and frontend hosting.

## Next Safe Step
Implement `plan.md` Step 1 + Step 2 (walking skeleton).
