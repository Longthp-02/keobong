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

## Steps (one PR each)
1. Polls and series get a sport (football, basketball) and format (3x3, 5x5, 5/7/11-a-side) and a venue; venues get sports and a pending/approved state; adding a venue from a Maps link; admin approval in a private chat; public flag; home-page section "Nhóm đang chơi".
2. Join requests: "Xin tham gia" on the group card, the organiser's private approve/decline, approved players answer polls, the group's Zalo link.
3. Venue photos: organisers upload up to 3 photos per venue (Cloud Storage), shown on the group card.

## Open questions
- Admin identity: `ADMIN_USER_IDS` (Đá Ghép user ids) on Cloud Run. TODO: verify with Long.
- Whether a basketball match ("kèo") uses the same venues list as football. Basketball matches stay a separate step (decision 10 in `docs/zalo-bot-plan.md`).
