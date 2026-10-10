# Daghep

Free, open-source app (daghep.vn) for joining pickup football matches with strangers in Ho Chi Minh City.

A host posts a match with open slots and shares the link in Zalo or Messenger. Players take a slot on team A or B and pay the host directly via VietQR. The app never holds money.

**Status:** pre-MVP — signed-in users can create a match at `/create` and share its page at `/m/{shareId}`. See `spec.md` and `plan.md`.

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

The web app proxies `/api/*` to the API (see `frontend/lib/rewrites.ts`).

To sign in locally, start the API with Google settings (see `backend/.env.example`;
the client secret comes from Google Cloud console and is never committed):

```bash
export GOOGLE_CLIENT_ID=... GOOGLE_CLIENT_SECRET=... \
       GOOGLE_REDIRECT_URI=http://localhost:8080/api/auth/google/callback
cargo run
```

Then open http://localhost:3000/create. Without these variables sign-in answers 503.
Since PR 3b every match has a host; reset an older local database with
`DROP DATABASE daghep_dev; CREATE DATABASE daghep_dev;` and `cargo run -- migrate`.

## Deploy
See `docs/deploy.md` (Cloud Run + Neon + Vercel).

## Stack
- Backend: Rust (Axum, sqlx)
- Database: PostgreSQL + PostGIS
- Frontend: Next.js PWA

## For contributors and AI agents
Start with `AGENTS.md`, then `CONSTITUTION.md` and `memory-bank/`.

## License
MIT — see `LICENSE`.

Built by [Long Pham](https://www.linkedin.com/in/long-pham-55466920b/).
