# Tech Context

- **Language/framework:** Rust (stable), Axum on Tokio. Chosen over Spring Boot for near-zero cost at MVP scale (small memory, instant cold start on scale-to-zero) and personal-brand differentiation; scalability is equivalent for this workload.
- **Data access:** sqlx with compile-time checked queries; migrations in `backend/migrations/` via `sqlx migrate`.
- **Database:** PostgreSQL + PostGIS (`geography(Point)`, GiST index).
- **Frontend:** Next.js + TypeScript, PWA manifest; package manager pnpm (TODO: verify). UI copy in `frontend/messages/vi.json`.
- **Auth:** Google OIDC + Zalo OAuth (Zalo for Developers app registration needed). Sessions in Postgres, signed HTTP-only cookie.
- **Observability:** `tracing` with JSON logs; no secrets or bank numbers in logs.
- **Local setup assumptions:** Docker for PostGIS; rustup stable; Node LTS.
- **Test tools:** `cargo test`, testcontainers (PostGIS image), `tokio::test`; Vitest, Testing Library, Playwright.
- **Lint/audit:** rustfmt, clippy `-D warnings`, cargo-deny or cargo-audit; ESLint + TypeScript strict.
- **CI/build:** GitHub Actions — backend fmt/clippy/test, frontend lint/test/build on every push and PR. Multi-stage Docker build to a minimal runtime image.
- **Deployment (decided):** backend on GCP Cloud Run in `asia-southeast1`, min instances 0; Postgres + PostGIS on Neon (Singapore region); frontend on Vercel Hobby (root directory `frontend/`); venue images in GCS; domain daghep.vn, DNS optionally on Cloudflare. Secrets live in GCP Secret Manager / Vercel env vars, never in the repo.
- **Product form:** installable web app (PWA) first. If adoption is good, wrap the same web code as native iOS/Android apps with Capacitor (mainly for push notifications); no rewrite.

## Scaling Path
| Stage | Infra | Change needed |
|---|---|---|
| ~100 users | 1 Cloud Run service scaling to zero; free/small Postgres with PostGIS (Neon or Supabase — TODO: verify) | none |
| ~10k users | Cloud Run autoscaling; paid small Postgres; CDN on public match pages | config |
| 100k+ users | Cloud SQL/AlloyDB + read replicas; Redis cache if measured; WebSocket for live slots | add components, no core rewrite |

## TODO: verify
Crate versions, Rust edition, package manager, maps/geocoding provider. Re-check free-tier limits and regions of Cloud Run, Neon and Vercel before the first deploy.
