# Plan: create matches from a chat message (Zalo bot)

Status: draft, waiting for Long's approval. No code yet.

## Goal
An organiser types the message they already write today, for example
"tối nay 19h sân SSA thiếu 3 người, 60k/người", either in a Zalo group that
has "Bot DaGhep" or in a private chat with the bot. The bot answers with a
short summary and a link that opens the create form already filled in. After
the organiser confirms on the web, the match link can be shared as usual.

The same text reader also powers a "paste your message" box on `/create`, so
the feature is useful before the Zalo part is finished.

## What we know (Zalo Bot Platform docs, read 2026-10-12)
- Individuals can create bots; the name must start with "Bot". Created: "Bot DaGhep".
- Updates arrive by webhook (POST, header `X-Bot-Api-Secret-Token` carries our secret) or by `getUpdates`; not both.
- Each message has `chat.chat_type`: `PRIVATE` or `GROUP` (groups are Beta). Replies go to `chat.id` with `sendMessage` (1–2,000 characters, optional markdown).
- Long added the bot to a test group on 2026-10-12; it joined.
- TODO: verify whether group webhooks deliver every message or only messages that mention the bot, and how a mention appears in `text`.
- TODO: verify free-plan limits (paid plans exist).

## Design
1. **Reply with a pre-filled link instead of creating the match in the chat.** The web flow already handles sign-in, the payout account, validation and the host's confirmation, so the bot needs no account linking in v1 and never sees bank details.
2. **New feature `bot`** (`backend/src/bot/{domain,service,repo,http}.rs`):
   - `POST /api/bot/zalo/webhook`: checks the secret header in constant time, ignores non-text events, and in groups ignores messages that do not mention the bot. Those are dropped without logging their text.
   - Port `MatchTextReader`: text + today's date + venue list → a draft (venue slug, date, start and end, format, match type, level range, total fee, places). Adapter: Anthropic Messages API with a small model and a strict JSON schema. Its output is validated with the existing match rules, never trusted.
   - Port `ChatReply`: adapter for Zalo `sendMessage`.
   - Table `match_drafts` (random id, fields, source `web`/`zalo`, created_at, expires after 24 hours). The link is `https://daghep.vn/create?draft=<id>`.
3. **`/create?draft=<id>`** pre-fills the form; the organiser checks it and submits as today.
4. **Web paste box** on `/create`: calls `POST /api/match-drafts` (signed in) with the text and fills the form from the draft.
5. **Missing or unclear details**: the bot asks one short follow-up question (unknown venue → "Hiện Đá Ghép mới có 3 sân: …"; no time → asks for the time). The form also leaves unknown fields empty.

## Limits and cost
- Per sender: TODO: verify (proposal: 10 messages read per day); whole service: TODO: verify (proposal: 200 per day), counted in Postgres. Over the limit the bot says so.
- Only messages that mention the bot (or private messages to it) reach the AI provider.

## Secrets (Secret Manager, never in chat or repo)
`daghep-zalo-bot-token`, `daghep-zalo-bot-webhook-secret`, `daghep-anthropic-api-key`.

## Privacy policy changes
Messages sent to the bot pass through Zalo and the AI provider to be read; drafts are deleted after 24 hours; the bot only reads group messages that mention it.

## Decisions for Long
1. AI provider: Anthropic (Claude, small model). Long creates the API key and billing.
2. The bot replies with a pre-filled link; the match is created on the web (no account linking in v1).
3. TODO: verify. How to read "thiếu 3 người, 60k/người": a match with 3 places at 60,000 VND each (total 180,000), or the full pitch?
4. TODO: verify. The rate limits above.
5. Later (v2): after the match is created, the bot posts the match link back into the group.

## Steps (one PR each)
1. Draft reader + `match_drafts` + `POST /api/match-drafts` + paste box and `?draft=` on `/create`.
2. Zalo webhook + group mention filter + replies + rate limits; set the webhook to the Cloud Run URL.
3. v2: post the match link back to the group.

## Tests
- Unit: draft validation (bad venue, past time, fee limits), mention filter, secret check.
- Acceptance (`#[sqlx::test]`): webhook with a fake reader and a fake chat reply (no network); expired drafts are not returned; rate limits.
- Frontend: paste box fills the form; `?draft=` pre-fills; an expired draft shows a message.
- A small fixed set of real organiser messages (Vietnamese) to check the reader by hand before launch.

## Risks
- Group support is Beta and Zalo may change limits or terms.
- AI can misread a message; the human confirmation on the web is the safety net.
