# Threat Model

## Feature / System
Daghep MVP: accounts, matches, slots, VietQR display, host payment confirmation, attendance, share links.

## What could go wrong?
- Two players get the same slot (race condition).
- A non-host marks payments or attendance (IDOR).
- A host maliciously marks players as no-shows.
- Fake matches used to collect transfers (scam hosts).
- Bank account numbers scraped from public match pages. Mitigated (PR 3d): payout accounts are returned only by `GET /api/me/payout` (owner) and inside `GET /api/matches/{id}/slots/mine` while the caller holds an unpaid place; tested that public views never contain them; `Debug` output redacts them.
- Spam match creation; slot squatting by bots holding all slots.
- Link preview leaking private data or injecting markup.
- OAuth misconfiguration (open redirect, missing state/nonce). Mitigated (PR 3b): PKCE + single-use `state` bound to the browser + `nonce`; ID token issuer/audience/expiry checked; return paths limited to same-site paths; tested in `backend/tests/auth_api.rs`.
- Account takeover through account linking: the Google `email_verified` claim is ignored today because accounts are identified only by provider `sub`. It must be required before any future linking of Google and Zalo accounts by email.
- Zalo bot (2026-10-13): a forged webhook call (mitigated: secret header compared in constant time, 403 otherwise); someone else linking a Zalo account to their Đá Ghép account and so hosting, and being paid for, its matches (mitigated: link codes only in the private chat, 256-bit, hashed, single use, 15 minutes; linking needs a click on a page that names the account; the chat confirmation names the account; drafts made under an older link cannot be confirmed); a victim tricked into forwarding their own link code is not prevented, only made visible by the named confirmation; group members confirming or cancelling another sender's draft (drafts are keyed by sender and chat); spam tagging (20 answers per sender per hour, AI reads limited per linked account); the bot token leaking (it is in request URLs, which are never logged). Reminders (2026-10-25): someone triggering reminder posts (the run endpoint needs `X-Cron-Secret`, compared in constant time, and only posts reminders that are due, each at most once per due time); group members flooding a chat with reminders (only linked senders, at most 3 per chat, 200 characters, AI reads limited per account); a reminder outliving the bot's place in a group (dropped after 3 failed sends). Weekly polls (2026-10-26) are stopped only by linked members and are capped at 3 per chat. Polls (2026-10-26): answers need a signed-in account and are public to anyone with the 8–16 character random link; answers for days outside the poll or after it closes are refused; a group roster holds linked senders seen tagging the bot and people who answered its polls, within the last 60 days; because a poll link can be forwarded, someone outside the group can answer and then be named in that group's nudges until 60 days after their last answer.
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
Rate limits, scam-host reporting. Account deletion must also delete the user's bot data; the SQL is in `docs/deploy.md` ("Account deletion request: Zalo bot data"), and the rest of the procedure is not written yet. Zalo's webhook timeout and retry policy.
