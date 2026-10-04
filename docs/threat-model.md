# Threat Model

## Feature / System
Daghep MVP: accounts, matches, slots, VietQR display, host payment confirmation, attendance, share links.

## What could go wrong?
- Two players get the same slot (race condition).
- A non-host marks payments or attendance (IDOR).
- A host maliciously marks players as no-shows.
- Fake matches used to collect transfers (scam hosts).
- Bank account numbers scraped from public match pages.
- Spam match creation; slot squatting by bots holding all slots.
- Link preview leaking private data or injecting markup.
- OAuth misconfiguration (open redirect, missing state/nonce). Mitigated (PR 3b): PKCE + single-use `state` bound to the browser + `nonce`; ID token issuer/audience/expiry checked; return paths limited to same-site paths; tested in `backend/tests/auth_api.rs`.
- Account takeover through account linking: the Google `email_verified` claim is ignored today because accounts are identified only by provider `sub`. It must be required before any future linking of Google and Zalo accounts by email.
- Session theft or forgery. Mitigated: 256-bit random tokens, only hashes stored, HttpOnly + SameSite=Lax + Secure cookies, 30-day expiry, sign-out deletes the session; cross-site POSTs refused by an `Origin` check.

## Who could attack or misuse this?
Scam hosts, griefers holding slots, scrapers, bots, curious users enumerating ids.

## What data or operation must be protected?
Host bank details, user identities, payment/attendance flags, host-only actions, session tokens.

## What access rules must always hold?
- Only the host can edit, cancel, mark payment or mark attendance on their match.
- A user can only release their own slot.
- Internal ids never appear in URLs; share ids are unguessable.

## What inputs are untrusted?
Every request body, query, path, header, cookie; venue names and descriptions; OAuth callback parameters; anything shown in link previews.

## What abuse cases must be tested?
- Wrong-user requests to every host endpoint return 403.
- Parallel claims on one slot: exactly one succeeds.
- Claiming when the match is full, cancelled or in the past.
- Oversized and script-containing text fields are rejected or escaped.
- Rate limit on match creation and slot claims (TODO: verify limits).
- VietQR endpoint returns 403 for users without a held slot.
- Unconfirmed holds are released after 30 minutes.
- Guest limit: a fourth slot for the same account is rejected.

## What should be logged or monitored?
Auth failures, 403s, slot conflicts, match creation rate per user, no-show markings per host. Never log bank account numbers or tokens.

## What must never be exposed to the client?
Other users' emails and provider ids, internal database ids, secrets, bank details or VietQR payloads to anyone not currently holding a slot in that match.

## TODO: verify
Rate limits, scam-host reporting.
