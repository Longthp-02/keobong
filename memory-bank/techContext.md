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
- **Deployment:** GCP Cloud Run, min instances 0. Frontend on Vercel free tier or Cloud Run (TODO: verify). Images in GCS.

## Scaling Path
| Stage | Infra | Change needed |
|---|---|---|
| ~100 users | 1 Cloud Run service scaling to zero; free/small Postgres with PostGIS (Neon or Supabase — TODO: verify) | none |
| ~10k users | Cloud Run autoscaling; paid small Postgres; CDN on public match pages | config |
| 100k+ users | Cloud SQL/AlloyDB + read replicas; Redis cache if measured; WebSocket for live slots | add components, no core rewrite |

## TODO: verify
Crate versions, Rust edition, package manager, DB provider and its PostGIS support, frontend hosting, maps/geocoding provider.
