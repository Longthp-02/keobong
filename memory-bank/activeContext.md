# Active Context

> Update after every meaningful AI session.

## Current Focus
Walking skeleton done (plan Steps 1-2): `GET /api/matches/{shareId}` (Rust/Axum/sqlx/PostGIS) rendered by Next.js at `/m/{shareId}`, with CI. Next: Step 3 (create match, Google sign-in, take a slot).

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
- Skeleton decisions (2026-10-04): malformed and unknown share ids both return 404 `match_not_found`; public match view exposes no internal id and no payment details; match stores explicit `slot_count` and `total_fee_vnd` (no per-player price shown until rounding is confirmed); migrations run via `daghep-api migrate`, not on startup; tests use `#[sqlx::test]` against real PostGIS.

## Open Questions
1. Remaining proposed rules in `spec.md` (slot capacity, price rounding, who can host, cancellations).

## Pending on Long
- Check whether the iNET mailbox lienhe@daghep.vn is a time-limited trial (it is the contact in the legal pages).
- Register the Zalo for Developers login app and the Google OAuth client using https://daghep.vn/privacy and /terms.
- Google OAuth client (localhost redirect first).
- Before deploy: GCP project with billing + budget alert, Neon project (Singapore), Vercel account.

## Next Safe Step
Plan Step 3 with `prompts/plan-first.md`: create-match endpoint + form, then Google sign-in, then race-safe slot claiming.
