# Handoff: Google sign-in and sessions (PR 3b)

## Endpoint changes
| Method | Path | Result |
|---|---|---|
| GET | `/api/auth/google/start?next=/path` | `303` to Google; sets `daghep_oauth_state` (HttpOnly, Path=/api/auth/google, 10 min). `503 {"error":"sign_in_unavailable"}` when Google is not configured. |
| GET | `/api/auth/google/callback?code&state` | Success: `303` to `FRONTEND_ORIGIN + next`, sets `daghep_session` (HttpOnly, SameSite=Lax, Path=/, 30 days, Secure on HTTPS); ends any session the browser already had and removes expired sessions. Any failure (cancelled, wrong/missing state, expired or reused attempt, bad code, nonce mismatch): `303` to `FRONTEND_ORIGIN/login-failed`, no session. |
| GET | `/api/me` | `200 {"displayName": string\|null, "avatarUrl": string\|null}`, `Cache-Control: private, no-store`; `401 {"error":"unauthenticated"}` otherwise. |
| POST | `/api/auth/logout` | Deletes the session, clears the cookie, `303` to `FRONTEND_ORIGIN/`. |
| POST | `/api/matches` | Now requires a session: `401 {"error":"unauthenticated"}` without one. The signed-in user becomes the host. |

`next` keeps only same-site paths (`/...`, not `//`, visible ASCII without backslash, ≤ 200 chars; percent-encode anything else); anything else returns to `/`.

Any non-GET request with an `Origin` header other than `FRONTEND_ORIGIN` gets `403 {"error":"forbidden_origin"}`. Server-to-server calls (no `Origin`) are unaffected.

## Config changes
- `CORS_ALLOWED_ORIGIN` is renamed `FRONTEND_ORIGIN` (also the redirect base after sign-in).
- New: `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` (secret manager only), `GOOGLE_REDIRECT_URI` — all three or none.
- Production: `GOOGLE_REDIRECT_URI=https://daghep.vn/api/auth/google/callback`, `FRONTEND_ORIGIN=https://daghep.vn`, and Vercel needs `API_BASE_URL` so `/api/*` is proxied to the API. Rewrites are built at build time, so `API_BASE_URL` must be set for builds (redeploy after changing it).
- The API refuses to start when Google is configured without an explicit `FRONTEND_ORIGIN`, or when an HTTPS redirect URI is not on the frontend origin. Plain-HTTP redirect URIs are allowed only on localhost / 127.0.0.1.
- Local development: open the web app at exactly `http://localhost:3000` (not 127.0.0.1); otherwise the browser's `Origin` differs from `FRONTEND_ORIGIN` and sign-out gets a 403.
- Check at first deploy: the Vercel rewrite passes `Set-Cookie` and `Origin` through unchanged.

## Database changes
Migration `20261006000000_create_users_and_sessions.sql` (approved by Long 2026-10-05): `users`, `user_identities`, `sessions` (token hash only), `oauth_login_attempts`, and `matches.host_user_id NOT NULL`. Local databases with older matches must be reset.

## Frontend actions
Done in this PR:
- `next.config.ts` proxies `/api/*` to `API_BASE_URL`.
- `/create` shows a sign-in prompt when signed out, and the account bar with sign-out when signed in.
- The server action forwards the browser's cookies; a `401` shows "session expired".
- New `/login-failed` page.
