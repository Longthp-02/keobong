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
- **Venues (PR 3f-list, approved by Long 2026-10-10):** a fixed list in `venues` (slug, name, address, location), owned by the `matches` feature because only matches use it. Creating a match takes a venue slug; one statement checks the venue is active and copies its name and location into the match, so the match keeps its name if the venue is renamed; the address is read from the venue. Venue names and addresses in the seed migration are real place names, so they stay in Vietnamese as data. Old matches have no venue.
- **Match list (discovery feature):** `discovery` owns no tables. It asks `matches::service::upcoming_matches` for a page of candidates (not cancelled, kickoff in the window, ordered by `(starts_at, share_id)`, keyset cursor in microseconds; the cursor's kickoff bounds the scan on `matches_starts_at_idx`) and `slots::service::taken_places` for the held places of those matches in one query, then drops full matches. Pages can therefore be shorter than the page size and still have a next cursor. `slots/repo.rs` reads `matches.share_id`, `id` and `cancelled_at` for this, within the boundary exception below.
- **Geo search:** `geography(Point, 4326)` with a GiST index; the list computes `ST_Distance` from the visitor's rounded position (never stored).
- **Money:** integer VND (`i64`); no floating point.
- **Share ids:** short, random, unguessable; internal ids never appear in URLs.
- **Payment (PR 3d):** display-only. The `payments` feature owns payout accounts and builds the VietQR (EMVCo, NAPAS 247) payload server-side; the browser renders the QR locally, so bank details never reach a QR service. Payment status lives on each place of a party (`awaiting_payment` with `hold_expires_at`, `payment_reported`, `confirmed`). Expired holds need no background job: they count as released in every read, and every locked write first records them as `released_at`.
- **Time:** inject a `Clock` trait; store timestamps as `timestamptz`; display in Asia/Ho_Chi_Minh.
- **Sign-in and sessions:** the API runs the whole OAuth flow (authorization code + PKCE S256, `state` bound to the browser by a short-lived cookie, OIDC `nonce`), storing each attempt in `oauth_login_attempts` (single use, 10 minutes). After sign-in the `daghep_session` cookie (HttpOnly, SameSite=Lax, Secure on HTTPS, 30 days) holds a random 256-bit token; `sessions` stores only its SHA-256. Vercel proxies `/api/*` to the API so the cookie is first-party on daghep.vn. State-changing requests with a foreign `Origin` get 403. Other features require a user with the `auth::AuthenticatedUser` extractor.
- **Match drafts (drafts feature, 2026-10-12):** `POST /api/match-drafts` (signed in) sends a pasted message to the `MatchTextReader` port; the production adapter calls Anthropic's Messages API with a forced tool call so the answer is structured JSON. `MatchDraft::checked` keeps only fields that pass the match rules (known active venue, date within 30 days, end after start within 4 hours, half-step levels, 2–30 places, fee cap) and turns a per-person price into the total. Fields of the wrong type in the model's reply are dropped one by one, and errors never quote the reply. `draft_reads` (user, time) enforces 10 reads per user and 200 overall per rolling 24 hours (one transaction under an advisory lock, so parallel requests cannot exceed them) and deletes rows older than 24 hours; the message is never stored or logged. Without `ANTHROPIC_API_KEY` the endpoint answers 503. No new dependency (reqwest was already used for Google).
- **Zalo bot (bot feature, 2026-10-13):** `POST /api/bot/zalo/webhook` receives Zalo Bot Platform updates (secret header compared in constant time; 404 when the bot is not configured). Text messages become commands (`bot::domain::parse_command`; group messages must start with the bot's tag, which is also the only kind Zalo delivers from groups). Linking: "liên kết" in a private chat stores a hashed 15-minute code and replies with `/bot/link?code=…`; the signed-in web page redeems it with `POST /api/bot/link`, which links one Zalo id to one account and confirms in the private chat. A description is read with `drafts::service::read_draft` (same daily limits, per linked account), completed with the confirmed defaults (casual, levels 1.0–5.0), validated with `NewMatch::validate`, and stored in `bot_drafts` for 30 minutes; the same sender's "ok" in the same chat takes the draft (delete-returning, so a repeated "ok" creates once) and calls `matches::service::create_match`. `bot_updates` drops repeated deliveries. Replies are end-user copy in `backend/messages/vi.json`, compiled in. The webhook is handled within the request (Cloud Run may throttle work after the response).
- **Page-view analytics (2026-10-12):** Vercel Web Analytics (`@vercel/analytics`, `<Analytics />` in the root layout) counts page views, referrers, rough location and device. It sets no cookies; Vercel's dashboard shows only aggregated data and the per-visitor hash is discarded after 24 hours. The privacy policy says so. Added to measure which posts and pages bring visitors. No custom events, and no URL carries personal data (share ids are random), so nothing is redacted.
- **Shared kernel:** `clock`, `random` and `text` hold cross-feature helpers; features otherwise talk only through each other's public API.
- **Errors:** one error enum per feature mapped to HTTP status in `http.rs`; never `unwrap()` on runtime paths.

## TODO: verify
Maps provider (not needed while venues are a fixed list).
