# Public groups plan

Status: agreed with Long 2026-10-26 (chat); step 1 migration approved 2026-10-26. Builds on the Zalo bot polls in `docs/zalo-bot-plan.md`.

## Goal
The home page shows groups that really play through Đá Ghép ("🏀 Bóng rổ 3x3 · Sân Lakeview · T7, CN 15:00 · 8 người điểm danh tuần này"), so visitors see the app is alive. Visitors can ask to join; the group's organiser approves.

## Decisions (Long, 2026-10-26)
1. A group is a weekly poll series. It is shown only when its organiser makes it public ("công khai" in the request, or `@Bot DaGhep công khai bảng N`). No member names on the home page.
2. Joining needs sign-in and the organiser's approval in a private Zalo message. An approved player can answer the group's polls and receives the group's Zalo link when the organiser has given one (`@Bot DaGhep link nhóm: https://zalo.me/g/...`).
3. A venue is added from a Google Maps link (`@Bot DaGhep thêm sân Lakeview https://maps.app.goo.gl/...`): the API follows the short link and reads the coordinates. New venues wait for Long's approval before they show publicly (proposed; Long did not object).
4. Venue photos are uploaded by organisers on the web (their own photos); no photos copied from the internet, no paid Places API.

## Built in step 1 (2026-10-26)
- `@Bot DaGhep thêm sân <tên> bóng rổ|bóng đá <link Google Maps>`: the API follows the short link only on Google hosts, reads the pin, and saves a pending venue (`active` stays false, so it is never offered for football matches). Admins in `ADMIN_USER_IDS` get a private notice and answer `duyệt sân N` / `từ chối sân N`; the proposer is told.
- Poll requests now carry sport, format and an approved venue (read by the AI from the request). A weekly poll whose request says "công khai" is public; `công khai bảng N` / `ẩn bảng N` toggle it (linked members).
- `GET /api/groups`: public series with sport, format, approved venue, weekdays, play time and this week's yes-counts and number of players; no names; cached 60 s. The home page shows them under the match list ("Nhóm đang chơi trên Đá Ghép").

## Built in step 2 (2026-10-26)
Approved by Long 2026-10-26 with the migration `20261026040000_add_join_requests.sql` and these limits: at most 5 pending requests per player, 50 waiting for one organiser across their groups, and a declined player may ask the same group again after 7 days. The Zalo Bot API cannot list a group's members (confirmed by Long 2026-10-26), so members are: the organiser, approved players, and linked people seen tagging the bot in the group's chat in the last 60 days. Answering a poll on the web does not count, since a poll link can be forwarded. Others ask to join. A request not answered within 14 days lapses and the player may ask again (TODO: verify with Long, proposed 2026-10-26).
- Each public group has a public id (`/g/<id>`, 12 hex characters, not a secret). The home card links there ("Xem nhóm · Xin tham gia").
- `GET /api/groups/{id}`: the group as on the home page, plus the signed-in viewer's standing (`none`, `pending`, `declined` with `askAgainAt`, or `member` with the Zalo link and this week's poll path). Private, never cached.
- `POST /api/groups/{id}/join` (signed in): saves a pending request and tells the organiser privately in Zalo ("nhận N" / "không nhận N"). Asking again while pending changes nothing. A member gets 200 with their standing.
- The organiser decides with `nhận N` / `không nhận N` in a private chat with the bot (only their own groups' pending requests; in a group the bot asks to do it privately, so the player's name is not posted there). `nhận` alone or `yêu cầu chờ duyệt` lists the waiting requests. The player is told privately when their Zalo account is linked; the web page shows it either way.
- `@Bot DaGhep link nhóm: https://zalo.me/g/…` (the series' organiser, in that chat) saves the group's Zalo invite link, shown only to members. It must be sent in the group itself.
- Answering polls still works for anyone with the poll link, as before; the poll link is what members get.

## Steps (one PR each)
1. Polls and series get a sport (football, basketball) and format (3x3, 5x5, 5/7/11-a-side) and a venue; venues get sports and a pending/approved state; adding a venue from a Maps link; admin approval in a private chat; public flag; home-page section "Nhóm đang chơi".
2. Join requests: "Xin tham gia" on the group card, the organiser's private approve/decline, approved players answer polls, the group's Zalo link.
3. Venue photos: organisers upload up to 3 photos per venue (Cloud Storage), shown on the group card.

## Open questions
- Admin identity: `ADMIN_USER_IDS` (Đá Ghép user ids) on Cloud Run. TODO: verify with Long.
- Whether a basketball match ("kèo") uses the same venues list as football. Basketball matches stay a separate step (decision 10 in `docs/zalo-bot-plan.md`).
