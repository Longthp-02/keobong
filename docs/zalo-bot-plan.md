# Plan: create and fill matches from Zalo chat (Bot DaGhep)

Status: approved by Long on 2026-10-12. Step 1 shipped (PR #15); step 2 is in the `zalo-bot` PR.

## Goal
An organiser runs a match without leaving Zalo. In a group that has "Bot DaGhep"
they write "@Bot DaGhep tối nay 19h SSA thiếu 3 người, 60k/người", confirm with
"@Bot DaGhep ok", and the bot creates the match and posts its link. Players answer
"@Bot DaGhep +1" and the bot keeps the list up to date. The web app keeps working
as it does today for everyone who prefers the link.

## Tested with the real bot (2026-10-12, Long, via getUpdates)
- `getMe`: `account_type` BASIC, `can_join_groups` true. Long added the bot to a test group.
- A group message that tags the bot arrives: `chat.chat_type` `GROUP`, `chat.id` like `zgr-…`, `text` starts with the literal `@Bot DaGhep`, `from.id` and `from.display_name` identify the sender.
- A group message without the tag does not arrive (two tries, no update within the wait). So the bot only ever sees messages that tag it.
- A private message arrives with `chat_type` `PRIVATE`; `chat.id` equals the sender's `from.id`.
- Docs: webhook POST with header `X-Bot-Api-Secret-Token`; `sendMessage` 1–2,000 characters; `sendPhoto` takes an image URL; webhook and getUpdates are exclusive; group support is Beta.
- The same person has the same `from.id` in a group and in a private chat (Long's test, 2026-10-12: `815bb2…` in both), so a link made privately works in groups.
- TODO: verify whether the bot can message a user privately before that user has written to it, and Zalo's webhook timeout and retry policy.
- TODO: verify free-plan limits.

## Flows
### One-time setup per organiser (only in a private chat)
1. The organiser writes "liên kết" to the bot privately. In a group, the bot answers "Nhắn riêng cho Bot DaGhep chữ 'liên kết' để bắt đầu" instead.
2. The bot replies with a one-time link (`daghep.vn/bot/link?code=…`, 15 minutes, single use). The organiser signs in with Google, and the Zalo id is linked to their account. Paid matches also need the payout account, set once on the web as today.
3. Why private only: a link posted in a group could be opened by someone else, who would then receive that organiser's matches and payments.

### Create a match in a group or privately
1. "@Bot DaGhep tối nay 19h SSA thiếu 3 người, 60k/người" → the reader turns it into a draft and the bot answers with a summary: "Kèo 19:00 hôm nay, SSA Amitie, sân 7, 3 chỗ, 60k/người. Trả lời '@Bot DaGhep ok' để tạo."
2. Only the same sender's "@Bot DaGhep ok" within 30 minutes creates the match through `matches::service::create_match` (same rules as the web). The bot posts the link and the list.
3. Missing or unclear details → one short question. Unknown venue → "Đá Ghép hiện có 3 sân: …". Not linked → the setup message above.

### Join and leave from the chat (step 3, needs new domain rules)
- "@Bot DaGhep +1" adds the sender; "+2" adds the sender and one guest (limit 2 guests, as on the web); "-1" removes them before kickoff. The bot answers with the list, e.g. "Kèo 19:00 SSA, 2/3: Nam, Hùng. Còn 1 chỗ."
- The bot applies it to the newest open match created in that group. If the group has several, it asks which.
- A chat player has no Đá Ghép account: the place stores the Zalo id and display name.

## Design
- New feature `bot` (`backend/src/bot/{domain,service,repo,http}.rs`). `POST /api/bot/zalo/webhook` on the Cloud Run URL; constant-time secret check; text events only; the message is handled within the request (Cloud Run may throttle work after the response) and repeated deliveries are dropped by message id.
- Port `MatchTextReader`: text + today + venue list → draft fields. Adapter: Anthropic Messages API, small model, forced tool call with a JSON schema. The output is validated with the existing match rules.
- Port `ChatReply`: Zalo `sendMessage`.
- Tables: `zalo_links` (Zalo id ↔ user, unique both ways), `zalo_link_codes` (hashed code, expiry), `bot_drafts` (sender, chat, checked fields, expiry), `bot_updates` (message id, sender, time: duplicates and the 20-per-hour reply limit). Step 3 adds a match ↔ group table and chat places in the slots tables.
- The web paste box on `/create` reuses the reader (`POST /api/match-drafts`).

## Limits, cost, secrets
- 10 reads per sender and 200 in total per rolling 24 hours, counted in Postgres (`draft_reads`).
- Secrets in Secret Manager only: `daghep-zalo-bot-token`, `daghep-zalo-bot-webhook-secret`, `daghep-anthropic-api-key`.

## Privacy policy changes
The bot only receives messages that tag it or are sent to it privately. Those messages pass through Zalo and the AI provider to be read. Drafts are deleted after 30 minutes. Chat players' Zalo display names appear in the public match list, like names on the web.

## Decisions (Long, 2026-10-12)
1. AI provider: Anthropic (Claude, small model). Long created the key (Secret Manager `daghep-anthropic-api-key`) with a small prepaid balance and spending limit.
2. Create in the chat after "@Bot DaGhep ok" from the same sender (no web click).
3. "thiếu 3 người, 60k/người" means a match with 3 places at 60,000 VND each (total 180,000), not the whole pitch.
4. Chat players without an account (step 3) transfer to the organiser as they do today, no 30-minute hold, and the organiser marks them paid on the web.
5. Rate limits: 10 reads per user and 200 in total per rolling 24 hours.

## Steps (one PR each)
1. Reader + drafts + paste box on `/create` (web only).
2. Webhook, private linking, create in chat with "ok", post the link. Players join through the link as today.
3. "+1 / -1" chat places and list updates (after decision 4).

## Tests
- Unit: tag parsing ("@Bot DaGhep" prefix, "+1", "+2", "-1", "ok"), draft validation, secret check, code expiry.
- Acceptance (`#[sqlx::test]`): webhook with a fake reader and a fake chat reply, no network; only the same sender can confirm; expired drafts and codes are refused; a code works once; rate limits.
- Frontend: paste box, `/bot/link` sign-in page.
- A fixed set of real organiser messages in Vietnamese to check the reader by hand before launch.

## Risks
- Group support is Beta; Zalo may change limits or terms.
- The AI can misread a message; the summary + "ok" step is the safety net.
- Chat players without accounts weaken no-show tracking.
