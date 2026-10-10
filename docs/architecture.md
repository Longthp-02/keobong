# Architecture

## Architecture for AI
- Architecture is part of the prompt: structure, naming, comments, tests and docs are the context every AI session reads.
- Prefer vertical slices / package-by-feature; keep a feature's code close together.
- Avoid generic technical-layer folders (`services/`, `utils/`) unless justified.
- A feature must not use another feature's internals; it calls that feature's public API.
- Shared code only when genuinely reused — rule of three before abstracting.
- Define domain ports (traits) first; adapters implement infrastructure details.
- Keep database, HTTP, framework and SDK types out of domain/application logic.
- Comments explain why, not what.
- Humans own architecture decisions; AI implements within these boundaries.
- Refactor one feature at a time; no big-bang rewrites.

## System Overview
```
[Next.js PWA] --HTTPS/JSON--> [Rust API (Axum)] --> [PostgreSQL + PostGIS]
      |    \                         |
  share links  CDN cache       Google OIDC, Zalo OAuth
  link previews                VietQR payload generation (local)
                               Object storage (venue photos)
```
Modular monolith: one stateless backend binary, one frontend. Horizontal scaling = more instances.

## Backend Layout (planned)
```
backend/
  Cargo.toml
  migrations/
  src/
    main.rs          // wiring only: config, router, state
    config.rs
    matches/  mod.rs domain.rs service.rs repo.rs http.rs
    slots/    ...
    players/  ...
    auth/     ...
    payment/  // VietQR payload only; never moves money
    share/    // share ids, link preview data
    shared/   // only genuinely shared primitives (Money, GeoPoint, Clock)
  tests/       // acceptance tests through the HTTP router against real PostGIS
```
- `domain.rs`: types, invariants, port traits. No Axum, sqlx or serde-on-the-wire concerns.
- `service.rs`: use cases; depends on domain traits only.
- `repo.rs`: sqlx adapter implementing the repository trait.
- `http.rs`: Axum handlers, request/response DTOs, error mapping.

## Scale-Ready Rules
- Stateless API: sessions in Postgres, no in-process state that correctness depends on.
- All state in Postgres; files in object storage.
- Slot claiming is atomic in the database.
- Every list query is paginated and indexed.
- Public match pages and preview images are CDN-cacheable with short TTLs.
- No microservices, brokers or Redis until measurements justify them. Feature modules are the seams for any later split.

## Key Design Decisions
- **Slot claiming (PR 3c):** players pick a team, not a position, so a place is a row inserted on join (own place or named guest) and released by setting `released_at`. The claim transaction runs at READ COMMITTED, locks the match row with `FOR NO KEY UPDATE`, counts active places in the team and inserts the whole group, so concurrent joins queue per match and never overbook. A partial unique index allows one own active place per user per match.
- **Boundary exception (PR 3c, approved by Long 2026-10-05):** `slots/repo.rs` reads and locks the `matches` row directly, because the capacity check and the insert must be in one transaction. It reads only `id`, `slot_count`, `starts_at`, and since PR 3d `total_fee_vnd` and `host_user_id` (price per player and host checks; approved by Long 2026-10-09), and since PR 3d-2 `cancelled_at` (approved by Long 2026-10-10). Everything else goes through public feature APIs.
- **Geo search:** `geography(Point, 4326)` with a GiST index and `ST_DWithin`.
- **Money:** integer VND (`i64`); no floating point.
- **Share ids:** short, random, unguessable; internal ids never appear in URLs.
- **Payment (PR 3d):** display-only. The `payments` feature owns payout accounts and builds the VietQR (EMVCo, NAPAS 247) payload server-side; the browser renders the QR locally, so bank details never reach a QR service. Payment status lives on each place of a party (`awaiting_payment` with `hold_expires_at`, `payment_reported`, `confirmed`). Expired holds need no background job: they count as released in every read, and every locked write first records them as `released_at`.
- **Time:** inject a `Clock` trait; store timestamps as `timestamptz`; display in Asia/Ho_Chi_Minh.
- **Sign-in and sessions:** the API runs the whole OAuth flow (authorization code + PKCE S256, `state` bound to the browser by a short-lived cookie, OIDC `nonce`), storing each attempt in `oauth_login_attempts` (single use, 10 minutes). After sign-in the `daghep_session` cookie (HttpOnly, SameSite=Lax, Secure on HTTPS, 30 days) holds a random 256-bit token; `sessions` stores only its SHA-256. Vercel proxies `/api/*` to the API so the cookie is first-party on daghep.vn. State-changing requests with a foreign `Origin` get 403. Other features require a user with the `auth::AuthenticatedUser` extractor.
- **Shared kernel:** `clock`, `random` and `text` hold cross-feature helpers; features otherwise talk only through each other's public API.
- **Errors:** one error enum per feature mapped to HTTP status in `http.rs`; never `unwrap()` on runtime paths.

## TODO: verify
DB and frontend hosting, maps provider, whether venues get a separate module.
