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
- **Slot claiming:** one row per slot, unique `(match_id, team, position)`; claim with `UPDATE ... SET player_id = $1 WHERE id = $2 AND player_id IS NULL`, checking rows affected; unique `(match_id, player_id)` if one slot per player is confirmed.
- **Geo search:** `geography(Point, 4326)` with a GiST index and `ST_DWithin`.
- **Money:** integer VND (`i64`); no floating point.
- **Share ids:** short, random, unguessable; internal ids never appear in URLs.
- **Payment:** display-only; the backend builds the VietQR payload; payment status is a host-asserted flag.
- **Time:** inject a `Clock` trait; store timestamps as `timestamptz`; display in Asia/Ho_Chi_Minh.
- **Errors:** one error enum per feature mapped to HTTP status in `http.rs`; never `unwrap()` on runtime paths.

## TODO: verify
Session cookie details, DB and frontend hosting, maps provider, whether venues get a separate module.
