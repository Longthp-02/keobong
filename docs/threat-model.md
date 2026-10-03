# Threat Model

## Feature / System
Keo MVP: accounts, matches, slots, VietQR display, host payment confirmation, attendance, share links.

## What could go wrong?
- Two players get the same slot (race condition).
- A non-host marks payments or attendance (IDOR).
- A host maliciously marks players as no-shows.
- Fake matches used to collect transfers (scam hosts).
- Bank account numbers scraped from public match pages.
- Spam match creation; slot squatting by bots holding all slots.
- Link preview leaking private data or injecting markup.
- OAuth misconfiguration (open redirect, missing state/nonce).

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

## What should be logged or monitored?
Auth failures, 403s, slot conflicts, match creation rate per user, no-show markings per host. Never log bank account numbers or tokens.

## What must never be exposed to the client?
Other users' emails and provider ids, internal database ids, secrets, full bank details to non-joined users (TODO: verify visibility rule).

## TODO: verify
Bank detail visibility, rate limits, no-show dispute process, scam-host reporting.
