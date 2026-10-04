# Daghep

Free, open-source app (daghep.vn) for joining pickup football matches with strangers in Ho Chi Minh City.

A host posts a match with open slots and shares the link in Zalo or Messenger. Players take a slot on team A or B and pay the host directly via VietQR. The app never holds money.

**Status:** pre-MVP — walking skeleton: a match stored in Postgres is served by the Rust API and rendered at `/m/{shareId}`. See `spec.md` and `plan.md`.

## Run locally
Requirements: Docker, Rust (stable), Node 22 with pnpm.

```bash
docker compose up -d db
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/daghep_dev

cd backend
cargo test             # acceptance + unit tests against real PostGIS
cargo run -- migrate   # create tables
cargo run              # API on http://localhost:8080

cd ../frontend
pnpm install
pnpm test
API_BASE_URL=http://localhost:8080 pnpm dev   # web on http://localhost:3000
```

Create a sample match to view at http://localhost:3000/m/demo2026:

```sql
INSERT INTO matches (share_id, venue_name, starts_at, ends_at, format, match_type,
                     level_min_tenths, level_max_tenths, total_fee_vnd, slot_count)
VALUES ('demo2026', 'SSA Sports Center', '2026-10-10T11:30:00Z', '2026-10-10T13:00:00Z',
        'seven_a_side', 'casual', 25, 35, 900000, 14);
```

## Stack
- Backend: Rust (Axum, sqlx)
- Database: PostgreSQL + PostGIS
- Frontend: Next.js PWA

## For contributors and AI agents
Start with `AGENTS.md`, then `CONSTITUTION.md` and `memory-bank/`.

## License
MIT — see `LICENSE`.

Built by [Long Pham](https://www.linkedin.com/in/long-pham-55466920b/).
