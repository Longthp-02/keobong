# Active Context

> Update after every meaningful AI session.

## Current Focus
Project foundation: AI context files, spec and plan. No application code yet.

## Latest Decisions
- Name: Daghep (UI: "Da Ghep" with Vietnamese diacritics in vi.json); domain daghep.vn (to be purchased by Long). Renamed from "Keo" because "keo bong" reads as football betting slang.
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

## Open Questions
1. Remaining proposed rules in `spec.md` (slot capacity, price rounding, who can host, cancellations).

## Pending on Long
- Buy daghep.vn, then register the Zalo for Developers login app.
- Later, before deploy: GCP project with billing + budget alert, Neon project (Singapore), Vercel account.

## Next Safe Step
Implement `plan.md` Step 1 + Step 2 (walking skeleton).
