//! Acceptance tests for "Bot DaGhep": the Zalo webhook, account linking and
//! creating a match from a chat message. Zalo and the AI reader are fakes.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use common::*;
use daghep_api::bot::polls::RawPoll;
use daghep_api::bot::reminders::RawReminder;
use daghep_api::drafts::domain::RawDraft;
use serde_json::{Value, json};
use sqlx::PgPool;

const GROUP: &str = "zgr-team";
const HOST: &str = "zalo-host";
const OTHER: &str = "zalo-other";

static MESSAGE_IDS: AtomicU32 = AtomicU32::new(1);

struct Bot {
    app: Router,
    chat: FakeChat,
    reader: FakeReader,
    reminders: FakeReminderReader,
    polls: FakePollReader,
    maps: FakeMaps,
    clock: TestClock,
    pool: PgPool,
}

/// What a model might answer for
/// "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k/người".
fn tonight() -> RawDraft {
    RawDraft {
        venue_id: Some("ssa-amitie".to_owned()),
        date: Some("2099-10-01".to_owned()),
        start_time: Some("19:00".to_owned()),
        end_time: Some("20:30".to_owned()),
        format: Some("seven_a_side".to_owned()),
        match_type: None,
        level_min: None,
        level_max: None,
        slot_count: Some(3),
        price_per_player_vnd: Some(60_000),
        total_fee_vnd: None,
    }
}

fn bot(pool: PgPool, answer: RawDraft) -> Bot {
    let chat = FakeChat::default();
    let reader = FakeReader {
        answer: Some(answer),
        ..FakeReader::default()
    };
    let clock = TestClock::at(NOW);
    let reminders = FakeReminderReader::default();
    let polls = FakePollReader::default();
    let maps = FakeMaps::default();
    Bot {
        app: app_with_maps(
            pool.clone(),
            clock.clone(),
            reader.clone(),
            chat.clone(),
            reminders.clone(),
            polls.clone(),
            maps.clone(),
        ),
        chat,
        reader,
        reminders,
        polls,
        maps,
        clock,
        pool,
    }
}

/// A webhook body as Zalo sends it: the update itself, without the
/// `{"ok": true, "result": ...}` envelope that `getUpdates` uses.
fn update(chat_id: &str, chat_type: &str, sender: &str, text: &str, message_id: &str) -> Value {
    json!({
        "event_name": "message.text.received",
        "message": {
            "chat": { "id": chat_id, "chat_type": chat_type },
            "text": text,
            "message_id": message_id,
            "date": 1,
            "from": { "id": sender, "is_bot": false, "display_name": "Someone" }
        }
    })
}

fn webhook_request(body: &Value, secret: Option<&str>) -> Request<Body> {
    let mut request =
        Request::post("/api/bot/zalo/webhook").header(CONTENT_TYPE, "application/json");
    if let Some(secret) = secret {
        request = request.header("X-Bot-Api-Secret-Token", secret);
    }
    request.body(Body::from(body.to_string())).unwrap()
}

impl Bot {
    /// Delivers a message and returns the bot's reply, if any.
    async fn say(
        &self,
        chat_id: &str,
        chat_type: &str,
        sender: &str,
        text: &str,
    ) -> Option<String> {
        let id = format!("m{}", MESSAGE_IDS.fetch_add(1, Ordering::Relaxed));
        self.deliver(&update(chat_id, chat_type, sender, text, &id))
            .await
    }

    /// Delivers a message and returns the bot's reply in that chat. Messages it
    /// sent the sender privately at the same time are in `chat.sent`.
    async fn deliver(&self, body: &Value) -> Option<String> {
        let before = self.chat.sent.lock().unwrap().len();
        let response = call(&self.app, webhook_request(body, Some(BOT_SECRET))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let message = &body.get("result").unwrap_or(body)["message"];
        let chat_id = message["chat"]["id"].as_str().unwrap();
        let sender = message["from"]["id"].as_str().unwrap();
        let sent = self.chat.sent.lock().unwrap();
        let new = &sent[before..];
        // Others may get notices (an admin, a venue's proposer), never a link code.
        assert!(
            new.iter().all(|(chat, text)| chat == chat_id
                || chat == sender
                || !text.contains("/bot/link")),
            "link codes go only to the sender"
        );
        let replies: Vec<_> = new.iter().filter(|(chat, _)| chat == chat_id).collect();
        assert!(replies.len() <= 1, "at most one reply per message");
        if chat_id != sender {
            let private = new.iter().filter(|(chat, _)| chat == sender).count();
            assert!(private <= 1, "at most one private message per message");
        }
        replies.last().map(|(_, text)| text.clone())
    }

    /// What the bot sent `to` privately since `before` messages.
    fn private_since(&self, before: usize, to: &str) -> Vec<String> {
        self.chat.sent.lock().unwrap()[before..]
            .iter()
            .filter(|(chat, _)| chat == to)
            .map(|(_, text)| text.clone())
            .collect()
    }

    async fn in_group(&self, sender: &str, text: &str) -> Option<String> {
        self.say(GROUP, "GROUP", sender, &format!("@Bot DaGhep {text}"))
            .await
    }

    async fn link_request(&self, session: &str, code: &str) -> axum::response::Response {
        let request = Request::post("/api/bot/link")
            .header(CONTENT_TYPE, "application/json")
            .header(COOKIE, format!("daghep_session={session}"))
            .body(Body::from(json!({ "code": code }).to_string()))
            .unwrap();
        call(&self.app, request).await
    }

    /// Links the Zalo account `sender` to the Google account `subject`.
    async fn link(&self, sender: &str, subject: &str) -> String {
        let reply = self
            .say(sender, "PRIVATE", sender, "liên kết")
            .await
            .unwrap();
        let code = link_code(&reply);
        let session = sign_in(&self.app, subject).await;
        assert_eq!(
            self.link_request(&session, &code).await.status(),
            StatusCode::OK
        );
        session
    }
}

fn link_code(reply: &str) -> String {
    let start =
        reply.find("/bot/link?code=").expect("a link in the reply") + "/bot/link?code=".len();
    reply[start..].split_whitespace().next().unwrap().to_owned()
}

async fn matches_in_db(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM matches")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn the_webhook_answers_zalos_update_with_or_without_an_envelope(pool: PgPool) {
    let bot = bot(pool, tonight());

    let bare = update(HOST, "PRIVATE", HOST, "hi", "m-bare");
    let reply = bot
        .deliver(&bare)
        .await
        .expect("a reply to the bare update");
    assert!(reply.contains("Bot DaGhep"), "{reply}");

    let wrapped = json!({ "ok": true, "result": update(HOST, "PRIVATE", HOST, "hi", "m-wrapped") });
    let reply = bot
        .deliver(&wrapped)
        .await
        .expect("a reply to the wrapped update");
    assert!(reply.contains("Bot DaGhep"), "{reply}");
}

#[sqlx::test]
async fn the_webhook_needs_zalos_secret(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let body = update(GROUP, "GROUP", HOST, "@Bot DaGhep hi", "m-secret");

    for secret in [None, Some("wrong-secret")] {
        let response = call(&bot.app, webhook_request(&body, secret)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert!(bot.chat.sent.lock().unwrap().is_empty());

    // Without bot settings the routes do not exist.
    let disabled = app(pool);
    let response = call(&disabled, webhook_request(&body, Some(BOT_SECRET))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn answers_only_messages_that_tag_it_in_a_group(pool: PgPool) {
    let bot = bot(pool, tonight());

    assert_eq!(
        bot.say(GROUP, "GROUP", HOST, "tối nay ai đá không").await,
        None
    );
    let help = bot.in_group(HOST, "").await.unwrap();
    assert!(help.contains("@Bot DaGhep tối nay"), "{help}");
}

#[sqlx::test]
async fn a_repeated_delivery_is_answered_once(pool: PgPool) {
    let bot = bot(pool, tonight());
    let body = update(GROUP, "GROUP", HOST, "@Bot DaGhep hi", "m-repeat");

    assert!(bot.deliver(&body).await.is_some());
    assert_eq!(bot.deliver(&body).await, None);
}

#[sqlx::test]
async fn linking_happens_only_in_a_private_chat_with_a_one_time_code(pool: PgPool) {
    let bot = bot(pool, tonight());

    // In a group the link is never posted there; it goes to the sender privately.
    let before = bot.chat.sent.lock().unwrap().len();
    let in_group = bot.in_group(HOST, "liên kết").await.unwrap();
    assert!(!in_group.contains("/bot/link"), "{in_group}");
    assert!(in_group.contains("nhắn riêng"), "{in_group}");
    let private = bot.private_since(before, HOST);
    assert_eq!(private.len(), 1);
    assert!(private[0].contains("/bot/link?code="), "{}", private[0]);

    let reply = bot.say(HOST, "PRIVATE", HOST, "liên kết").await.unwrap();
    assert!(
        reply.contains(&format!("{FRONTEND}/bot/link?code=")),
        "{reply}"
    );
    let code = link_code(&reply);
    let session = sign_in(&bot.app, "host").await;

    let linked = bot.link_request(&session, &code).await;
    assert_eq!(linked.status(), StatusCode::OK);
    assert_eq!(json_body(linked).await, json!({ "linked": true }));
    // The bot confirms in the private chat (its id is the Zalo user's id).
    let last = bot.chat.sent.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last.0, HOST);
    assert!(last.1.starts_with("✅"));

    let reused = bot.link_request(&session, &code).await;
    assert_eq!(reused.status(), StatusCode::GONE);
    assert_eq!(json_body(reused).await, json!({ "error": "invalid_code" }));
}

#[sqlx::test]
async fn a_link_code_expires_after_fifteen_minutes(pool: PgPool) {
    let bot = bot(pool, tonight());
    let code = link_code(&bot.say(HOST, "PRIVATE", HOST, "liên kết").await.unwrap());
    let session = sign_in(&bot.app, "host").await;

    bot.clock.advance(Duration::minutes(15));

    assert_eq!(
        bot.link_request(&session, &code).await.status(),
        StatusCode::GONE
    );
}

#[sqlx::test]
async fn linking_needs_a_signed_in_user(pool: PgPool) {
    let bot = bot(pool, tonight());
    let code = link_code(&bot.say(HOST, "PRIVATE", HOST, "liên kết").await.unwrap());

    let request = Request::post("/api/bot/link")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "code": code }).to_string()))
        .unwrap();
    assert_eq!(
        call(&bot.app, request).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn an_unlinked_sender_is_told_how_to_start_and_nothing_is_read(pool: PgPool) {
    let bot = bot(pool, tonight());

    let before = bot.chat.sent.lock().unwrap().len();
    let reply = bot.in_group(HOST, "tối nay 19h SSA thiếu 3").await.unwrap();

    // The bot sends the link privately and says so in the group.
    assert!(
        reply.contains("vừa nhắn riêng cho bạn link liên kết"),
        "{reply}"
    );
    assert!(reply.contains("@Bot DaGhep tìm kèo"), "{reply}");
    assert!(bot.private_since(before, HOST)[0].contains("/bot/link?code="));
    assert!(bot.reader.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn when_zalo_refuses_a_private_message_the_group_reply_explains_both_paths(pool: PgPool) {
    let bot = bot(pool, tonight());
    // Zalo refuses messages to people who never wrote to the bot: here the
    // fake refuses every private chat, i.e. any chat that is not the group.
    *bot.chat.refuse_private.lock().unwrap() = true;

    let reply = bot.in_group(HOST, "tối nay 19h SSA thiếu 3").await.unwrap();

    assert!(reply.contains("Tìm kèo để đá"), "{reply}");
    assert!(
        reply.contains("nhắn riêng cho Bot DaGhep chữ \"liên kết\""),
        "{reply}"
    );
}

#[sqlx::test]
async fn players_asking_for_a_match_get_the_open_matches(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    // No open match yet: where to look.
    let none = bot
        .in_group(OTHER, "tối nay có kèo nào ko tui muốn đá ghép")
        .await
        .unwrap();
    assert!(none.contains("chưa có kèo nào đang tìm người"), "{none}");

    // A host creates one through the bot.
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k/người")
        .await
        .unwrap();
    let created = bot.in_group(HOST, "ok").await.unwrap();
    let share = created.split("/m/").nth(1).unwrap().trim().to_owned();

    for ask in ["tui muốn đá", "@Bot DaGhep tìm kèo", "đá ghép"] {
        let text = ask.trim_start_matches("@Bot DaGhep ");
        let list = bot.in_group(OTHER, text).await.unwrap();
        assert!(list.contains("Kèo đang tìm người"), "{list}");
        assert!(list.contains("Thứ 5 01/10 19:00–20:30"), "{list}");
        assert!(list.contains("SSA Sports Center"), "{list}");
        assert!(list.contains("còn 3 chỗ · 60.000đ/người"), "{list}");
        assert!(list.contains(&format!("{FRONTEND}/m/{share}")), "{list}");
    }
    // An unlinked sender whose message has a time still gets the list, and how
    // to post matches in case they are an organiser.
    let with_time = bot.in_group(OTHER, "có kèo 19h không").await.unwrap();
    assert!(with_time.contains("Kèo đang tìm người"), "{with_time}");
    assert!(
        with_time.contains("Bạn là chủ kèo muốn đăng kèo?"),
        "{with_time}"
    );
    // A linked organiser asking without times gets the list too.
    let organiser = bot
        .in_group(HOST, "tối nay có kèo nào không")
        .await
        .unwrap();
    assert!(organiser.contains("Kèo đang tìm người"), "{organiser}");
    assert!(bot.reader.calls.lock().unwrap().len() == 1);
}

#[sqlx::test]
async fn creates_the_match_after_the_same_sender_says_ok(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;

    let summary = bot
        .in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k/người")
        .await
        .unwrap();
    assert!(summary.contains("Thứ 5 01/10 19:00–20:30"), "{summary}");
    assert!(summary.contains("SSA Sports Center"), "{summary}");
    assert!(summary.contains("3 chỗ · 60.000đ/người"), "{summary}");
    assert!(summary.contains("\"@Bot DaGhep ok\""), "{summary}");
    assert_eq!(
        bot.reader.calls.lock().unwrap()[0].0,
        "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k/người"
    );

    // Someone else's "ok" does nothing to it.
    let other = bot.in_group(OTHER, "ok").await.unwrap();
    assert!(!other.contains("/m/"), "{other}");
    assert_eq!(matches_in_db(&pool).await, 0);

    let created = bot.in_group(HOST, "ok").await.unwrap();
    let share_id = created
        .split("/m/")
        .nth(1)
        .expect("a match link")
        .trim()
        .to_owned();
    let response = call(&bot.app, get(&format!("/api/matches/{share_id}"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let view = json_body(response).await;
    assert_eq!(view["slotCount"], 3);
    assert_eq!(view["totalFeeVnd"], 180_000);
    assert_eq!(view["matchType"], "casual");
    assert_eq!(
        (view["levelMin"].clone(), view["levelMax"].clone()),
        (json!(1.0), json!(5.0))
    );
    assert_eq!(view["startsAt"], "2099-10-01T12:00:00Z");

    // A second "ok" does not create it twice.
    let again = bot.in_group(HOST, "ok").await.unwrap();
    assert!(!again.contains("/m/"), "{again}");
    assert_eq!(matches_in_db(&pool).await, 1);
}

#[sqlx::test]
async fn works_in_a_private_chat_without_the_tag(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;

    let summary = bot
        .say(HOST, "PRIVATE", HOST, "tối nay 19h SSA")
        .await
        .unwrap();
    assert!(summary.contains("\"ok\""), "{summary}");
    let created = bot.say(HOST, "PRIVATE", HOST, "ok").await.unwrap();

    assert!(created.contains(&format!("{FRONTEND}/m/")), "{created}");
    assert_eq!(matches_in_db(&pool).await, 1);
}

#[sqlx::test]
async fn asks_for_what_the_message_left_out(pool: PgPool) {
    let answer = RawDraft {
        end_time: None,
        format: None,
        ..tonight()
    };
    let bot = bot(pool, answer);
    bot.link(HOST, "host").await;

    let reply = bot
        .in_group(HOST, "tối nay 19h SSA thiếu 3, 60k")
        .await
        .unwrap();

    assert!(reply.contains("giờ kết thúc"), "{reply}");
    assert!(reply.contains("loại sân"), "{reply}");
    assert!(reply.contains("@Bot DaGhep tối nay"), "{reply}");
    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(!ok.contains("/m/"), "{ok}");
}

#[sqlx::test]
async fn a_paid_match_needs_a_payout_account_first(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;

    let reply = bot
        .in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();

    assert!(
        reply.contains(&format!("{FRONTEND}/account/payout")),
        "{reply}"
    );
}

#[sqlx::test]
async fn a_draft_waits_thirty_minutes(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();

    bot.clock.advance(Duration::minutes(30));

    let late = bot.in_group(HOST, "ok").await.unwrap();
    assert!(!late.contains("/m/"), "{late}");
    assert_eq!(matches_in_db(&bot.pool).await, 0);
}

#[sqlx::test]
async fn the_sender_can_cancel_a_draft(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();

    let cancelled = bot.in_group(HOST, "huỷ").await.unwrap();
    let ok = bot.in_group(HOST, "ok").await.unwrap();

    assert_ne!(cancelled, ok);
    assert!(!ok.contains("/m/"), "{ok}");
    assert_eq!(matches_in_db(&pool).await, 0);
}

#[sqlx::test]
async fn a_relinked_account_cannot_confirm_a_draft_read_under_the_old_link(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();

    // The same Đá Ghép account is now linked to another Zalo account.
    let code = link_code(&bot.say(OTHER, "PRIVATE", OTHER, "liên kết").await.unwrap());
    assert_eq!(
        bot.link_request(&session, &code).await.status(),
        StatusCode::OK
    );

    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(!ok.contains("/m/"), "{ok}");
    assert_eq!(matches_in_db(&pool).await, 0);
}

#[sqlx::test]
async fn the_linked_message_names_the_account(pool: PgPool) {
    let bot = bot(pool, tonight());

    bot.link(HOST, "host").await;

    let last = bot.chat.sent.lock().unwrap().last().cloned().unwrap();
    assert!(last.1.contains("\"Player host\""), "{}", last.1);
}

#[sqlx::test]
async fn others_cannot_cancel_a_draft_and_another_chat_cannot_confirm_it(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();

    bot.in_group(OTHER, "huỷ").await.unwrap();
    let elsewhere = bot
        .say("zgr-elsewhere", "GROUP", HOST, "@Bot DaGhep ok")
        .await
        .unwrap();
    assert!(!elsewhere.contains("/m/"), "{elsewhere}");

    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(ok.contains("/m/"), "{ok}");
    assert_eq!(matches_in_db(&pool).await, 1);
}

#[sqlx::test]
async fn a_free_match_says_so_and_needs_no_payout_account(pool: PgPool) {
    let answer = RawDraft {
        price_per_player_vnd: None,
        total_fee_vnd: Some(0),
        ..tonight()
    };
    let bot = bot(pool, answer);
    bot.link(HOST, "host").await;

    let summary = bot
        .in_group(HOST, "tối nay 19h–20h30 SSA sân 7 thiếu 3, free")
        .await
        .unwrap();

    assert!(summary.contains("3 chỗ · Miễn phí"), "{summary}");
}

#[sqlx::test]
async fn one_sender_gets_twenty_answers_per_hour(pool: PgPool) {
    let bot = bot(pool, tonight());

    for _ in 0..20 {
        assert!(bot.in_group(OTHER, "hi").await.is_some());
    }
    assert_eq!(bot.in_group(OTHER, "hi").await, None);
    assert!(bot.in_group(HOST, "hi").await.is_some());

    bot.clock.advance(Duration::hours(1) + Duration::seconds(1));
    assert!(bot.in_group(OTHER, "hi").await.is_some());
}

#[sqlx::test]
async fn without_a_reader_it_points_to_the_web_form(pool: PgPool) {
    let chat = FakeChat::default();
    let clock = TestClock::at(NOW);
    let app = daghep_api::app(daghep_api::Deps {
        pool: pool.clone(),
        clock: std::sync::Arc::new(clock.clone()),
        google: Some(std::sync::Arc::new(FakeGoogle)),
        frontend_origin: FRONTEND.to_owned(),
        reader: None,
        zalo: Some(std::sync::Arc::new(daghep_api::bot::ZaloSettings {
            reply: std::sync::Arc::new(chat.clone()),
            webhook_secret: daghep_api::config::Secret::new(BOT_SECRET.to_owned()),
            bot_name: "Bot DaGhep".to_owned(),
            reminder_reader: None,
            poll_reader: None,
            maps: None,
            admins: vec![],
            cron_secret: None,
        })),
    });
    let bot = Bot {
        app,
        chat,
        reader: FakeReader::default(),
        reminders: FakeReminderReader::default(),
        polls: FakePollReader::default(),
        maps: FakeMaps::default(),
        clock,
        pool,
    };
    bot.link(HOST, "host").await;

    let reply = bot.in_group(HOST, "tối nay 19h SSA").await.unwrap();
    assert!(reply.contains(&format!("{FRONTEND}/create")), "{reply}");
    let reminder = bot.in_group(HOST, "nhắc t7 8h").await.unwrap();
    assert!(reminder.contains("tạm nghỉ"), "{reminder}");
    // Without a scheduler secret the run endpoint does not exist.
    let run = call(&bot.app, cron_request(Some(CRON_SECRET))).await;
    assert_eq!(run.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn ignores_bodies_it_cannot_use_once_the_secret_is_right(pool: PgPool) {
    let bot = bot(pool, tonight());

    for body in ["", "not json", "{}"] {
        let request = Request::post("/api/bot/zalo/webhook")
            .header(CONTENT_TYPE, "application/json")
            .header("X-Bot-Api-Secret-Token", BOT_SECRET)
            .body(Body::from(body))
            .unwrap();
        assert_eq!(
            call(&bot.app, request).await.status(),
            StatusCode::OK,
            "{body:?}"
        );
    }
    let long_id = "m".repeat(129);
    assert_eq!(
        bot.deliver(&update(GROUP, "GROUP", HOST, "@Bot DaGhep hi", &long_id))
            .await,
        None
    );
    assert!(bot.chat.sent.lock().unwrap().is_empty());
}

// ---- Reminders ("@Bot DaGhep nhắc ...") ----

/// What a model might answer for "nhắc t7 cn 8h hỏi ai chơi bóng rổ".
fn weekend_reminder() -> RawReminder {
    RawReminder {
        repeat: Some("weekly".to_owned()),
        weekdays: vec![6, 0],
        day_of_month: None,
        time: Some("08:00".to_owned()),
        message: Some("Cuối tuần này ai chơi bóng rổ không?".to_owned()),
    }
}

fn cron_request(secret: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/api/bot/reminders/run");
    if let Some(secret) = secret {
        request = request.header("X-Cron-Secret", secret);
    }
    request.body(Body::empty()).unwrap()
}

impl Bot {
    fn reminder_answer(&self, answer: Option<RawReminder>) {
        *self.reminders.answer.lock().unwrap() = answer;
    }

    /// Runs the scheduler once; returns how many reminders it posted.
    async fn run_reminders(&self) -> i64 {
        let response = call(&self.app, cron_request(Some(CRON_SECRET))).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["sent"].as_i64().unwrap()
    }

    fn sent_count(&self) -> usize {
        self.chat.sent.lock().unwrap().len()
    }

    fn last_sent(&self) -> (String, String) {
        self.chat.sent.lock().unwrap().last().cloned().unwrap()
    }

    async fn add_weekend_reminder(&self) {
        self.reminder_answer(Some(weekend_reminder()));
        self.in_group(HOST, "nhắc t7 cn 8h hỏi ai chơi bóng rổ")
            .await
            .unwrap();
        let saved = self.in_group(HOST, "ok").await.unwrap();
        assert!(saved.contains("Đã lưu lịch nhắc"), "{saved}");
    }
}

#[sqlx::test]
async fn a_reminder_is_read_confirmed_and_posted_on_schedule(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.reminder_answer(Some(weekend_reminder()));

    let summary = bot
        .in_group(
            HOST,
            "đặt lịch nhắc mỗi tuần t7 cn 8h hỏi có ai chơi bóng rổ ko",
        )
        .await
        .unwrap();
    assert_eq!(
        bot.reminders.calls.lock().unwrap().as_slice(),
        ["đặt lịch nhắc mỗi tuần t7 cn 8h hỏi có ai chơi bóng rổ ko"]
    );
    assert!(
        summary.contains("mỗi Thứ 7, Chủ nhật lúc 08:00"),
        "{summary}"
    );
    assert!(
        summary.contains("Cuối tuần này ai chơi bóng rổ không?"),
        "{summary}"
    );
    assert!(summary.contains("@Bot DaGhep ok"), "{summary}");
    // Nothing is read as a match.
    assert!(bot.reader.calls.lock().unwrap().is_empty());

    let saved = bot.in_group(HOST, "ok").await.unwrap();
    // NOW is Thursday 07:00 local; the next run is Saturday 08:00.
    assert!(saved.contains("Thứ 7 03/10 08:00"), "{saved}");
    assert_eq!(bot.run_reminders().await, 0);

    bot.clock.advance(Duration::days(2) + Duration::hours(1));
    let before = bot.sent_count();
    assert_eq!(bot.run_reminders().await, 1);
    assert_eq!(bot.sent_count(), before + 1);
    assert_eq!(
        bot.last_sent(),
        (
            GROUP.to_owned(),
            "⏰ Cuối tuần này ai chơi bóng rổ không?".to_owned()
        )
    );
    // Posted once per due time, then again on Sunday.
    assert_eq!(bot.run_reminders().await, 0);
    bot.clock.advance(Duration::days(1));
    assert_eq!(bot.run_reminders().await, 1);
}

#[sqlx::test]
async fn a_reminder_request_asks_for_what_it_left_out(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.reminder_answer(Some(RawReminder {
        time: None,
        ..weekend_reminder()
    }));
    let reply = bot
        .in_group(HOST, "nhắc t7 cn hỏi ai chơi bóng rổ")
        .await
        .unwrap();
    assert!(reply.contains("nhắc lúc mấy giờ"), "{reply}");
    // Nothing is pending: "ok" has nothing to save.
    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(ok.contains("Không có kèo nào đang chờ"), "{ok}");
}

#[sqlx::test]
async fn only_linked_senders_set_reminders(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.reminder_answer(Some(weekend_reminder()));
    let reply = bot.in_group(OTHER, "nhắc t7 cn 8h").await.unwrap();
    assert!(reply.contains("liên kết"), "{reply}");
    assert!(bot.reminders.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn a_chat_keeps_at_most_three_reminders(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    for _ in 0..3 {
        bot.add_weekend_reminder().await;
    }
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    let full = bot.in_group(HOST, "ok").await.unwrap();
    assert!(full.contains("đã có 3 lịch nhắc"), "{full}");
}

#[sqlx::test]
async fn reminders_are_listed_and_stopped_by_number(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let none = bot.in_group(HOST, "tắt nhắc").await.unwrap();
    assert!(none.contains("chưa có lịch nhắc"), "{none}");

    bot.add_weekend_reminder().await;
    let list = bot.in_group(HOST, "tắt nhắc").await.unwrap();
    assert!(
        list.contains("1. mỗi Thứ 7, Chủ nhật lúc 08:00: \"Cuối tuần này ai chơi bóng rổ không?\""),
        "{list}"
    );

    // Unlinked members cannot stop the group's reminders.
    let refused = bot.in_group(OTHER, "tắt nhắc 1").await.unwrap();
    assert!(refused.contains("liên kết"), "{refused}");
    let missing = bot.in_group(HOST, "tắt nhắc 2").await.unwrap();
    assert!(missing.contains("Không có lịch nhắc số 2"), "{missing}");
    let stopped = bot.in_group(HOST, "tắt nhắc 1").await.unwrap();
    assert!(stopped.contains("Đã tắt lịch nhắc số 1"), "{stopped}");

    bot.clock.advance(Duration::days(3));
    assert_eq!(bot.run_reminders().await, 0);
}

#[sqlx::test]
async fn a_new_match_draft_replaces_a_pending_reminder(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.reminder_answer(Some(weekend_reminder()));
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    bot.in_group(HOST, "tối nay 19h SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();
    let created = bot.in_group(HOST, "ok").await.unwrap();
    assert!(created.contains("Đã tạo kèo"), "{created}");
    assert_eq!(matches_in_db(&pool).await, 1);
    let list = bot.in_group(HOST, "xem nhắc").await.unwrap();
    assert!(list.contains("chưa có lịch nhắc"), "{list}");
}

#[sqlx::test]
async fn the_scheduler_needs_its_secret(pool: PgPool) {
    let bot = bot(pool, tonight());
    for secret in [None, Some("wrong-cron-secret-000")] {
        let response = call(&bot.app, cron_request(secret)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[sqlx::test]
async fn a_reminder_that_keeps_failing_is_dropped(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_weekend_reminder().await;
    *bot.chat.failing.lock().unwrap() = true;
    // Saturday and Sunday fail: two failures keep the reminder.
    bot.clock.advance(Duration::days(2) + Duration::hours(1));
    bot.run_reminders().await;
    bot.clock.advance(Duration::days(1));
    bot.run_reminders().await;
    *bot.chat.failing.lock().unwrap() = false;
    let kept = bot.in_group(HOST, "xem nhắc").await.unwrap();
    assert!(kept.contains("1. mỗi Thứ 7"), "{kept}");
    // The third failure in a row (next Saturday) drops it.
    *bot.chat.failing.lock().unwrap() = true;
    bot.clock.advance(Duration::days(6));
    assert_eq!(bot.run_reminders().await, 0);
    *bot.chat.failing.lock().unwrap() = false;
    let list = bot.in_group(HOST, "xem nhắc").await.unwrap();
    assert!(list.contains("chưa có lịch nhắc"), "{list}");
}

#[sqlx::test]
async fn a_message_with_nothing_about_a_match_gets_a_short_answer(pool: PgPool) {
    let bot = bot(pool, RawDraft::default());
    bot.link(HOST, "host").await;
    let reply = bot.in_group(HOST, "hôm nay trời đẹp quá").await.unwrap();
    assert!(reply.contains("chưa thấy thông tin kèo"), "{reply}");
    assert!(!reply.contains("Mình còn thiếu"), "{reply}");
}

#[sqlx::test]
async fn cancel_drops_a_pending_reminder(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.reminder_answer(Some(weekend_reminder()));
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    let cancelled = bot.in_group(HOST, "huỷ").await.unwrap();
    assert!(cancelled.contains("Đã huỷ"), "{cancelled}");
    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(ok.contains("Không có kèo nào đang chờ"), "{ok}");
}

#[sqlx::test]
async fn a_reminder_replaces_a_pending_match_draft(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.in_group(HOST, "tối nay 19h SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();
    bot.reminder_answer(Some(weekend_reminder()));
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    let saved = bot.in_group(HOST, "ok").await.unwrap();
    assert!(saved.contains("Đã lưu lịch nhắc"), "{saved}");
    assert_eq!(matches_in_db(&pool).await, 0);
    let again = bot.in_group(HOST, "ok").await.unwrap();
    assert!(again.contains("Không có kèo nào đang chờ"), "{again}");
}

#[sqlx::test]
async fn relinking_drops_a_pending_reminder(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.reminder_answer(Some(weekend_reminder()));
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    bot.link(HOST, "someone-else").await;
    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(ok.contains("Không có kèo nào đang chờ"), "{ok}");
}

#[sqlx::test]
async fn a_pending_reminder_waits_thirty_minutes_and_is_then_cleared(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    bot.link(HOST, "host").await;
    bot.reminder_answer(Some(weekend_reminder()));
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    bot.clock.advance(Duration::minutes(31));
    // The scheduler run clears it, text included.
    bot.run_reminders().await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM bot_pending_reminders")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(ok.contains("Không có kèo nào đang chờ"), "{ok}");
}

#[sqlx::test]
async fn a_good_send_resets_the_failure_count(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_weekend_reminder().await;
    // Saturday and Sunday fail, next Saturday succeeds, then two more failures
    // keep it: the count started again.
    *bot.chat.failing.lock().unwrap() = true;
    bot.clock.advance(Duration::days(2) + Duration::hours(1));
    bot.run_reminders().await;
    bot.clock.advance(Duration::days(1));
    bot.run_reminders().await;
    *bot.chat.failing.lock().unwrap() = false;
    bot.clock.advance(Duration::days(6));
    assert_eq!(bot.run_reminders().await, 1);
    *bot.chat.failing.lock().unwrap() = true;
    bot.clock.advance(Duration::days(1));
    bot.run_reminders().await;
    bot.clock.advance(Duration::days(6));
    bot.run_reminders().await;
    *bot.chat.failing.lock().unwrap() = false;
    let list = bot.in_group(HOST, "xem nhắc").await.unwrap();
    assert!(list.contains("1. mỗi Thứ 7"), "{list}");
}

#[sqlx::test]
async fn a_reminder_more_than_an_hour_late_is_skipped(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_weekend_reminder().await;
    // Saturday 08:00 is due; the scheduler only runs again at 10:00.
    bot.clock.advance(Duration::days(2) + Duration::hours(3));
    assert_eq!(bot.run_reminders().await, 0);
    // It moved on to Sunday 08:00 and posts then.
    bot.clock.advance(Duration::days(1) - Duration::hours(2));
    assert_eq!(bot.run_reminders().await, 1);
}

#[sqlx::test]
async fn a_match_post_that_mentions_nhac_is_still_a_match(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let reply = bot
        .in_group(
            HOST,
            "tối nay 19h SSA sân 7 thiếu 3, 60k, nhắc ae mang áo trắng",
        )
        .await
        .unwrap();
    assert!(reply.contains("Kèo"), "{reply}");
    assert!(bot.reminders.calls.lock().unwrap().is_empty());
}

// ---- Attendance polls ("@Bot DaGhep tạo bảng điểm danh ...") ----

/// What a model might answer for "tạo bảng điểm danh bóng rổ T7, CN, nhắc lúc 20h".
fn weekend_poll() -> RawPoll {
    RawPoll {
        title: Some("Ai chơi bóng rổ cuối tuần này?".to_owned()),
        days: vec!["2099-10-03".to_owned(), "2099-10-04".to_owned()],
        play_time: Some("18:00".to_owned()),
        // Not stated: the bot nudges at 20:00.
        nudge_time: None,
        weekly: false,
        ..RawPoll::default()
    }
}

fn poll_share_id(reply: &str) -> String {
    let start = reply.find("/p/").expect("a poll link") + 3;
    reply[start..].split_whitespace().next().unwrap().to_owned()
}

impl Bot {
    fn poll_answer(&self, answer: Option<RawPoll>) {
        *self.polls.answer.lock().unwrap() = answer;
    }

    /// Creates the weekend poll in the group as HOST; returns its share id.
    async fn add_weekend_poll(&self) -> String {
        self.poll_answer(Some(weekend_poll()));
        self.in_group(HOST, "tạo bảng điểm danh bóng rổ T7, CN, nhắc lúc 20h")
            .await
            .unwrap();
        let posted = self.in_group(HOST, "ok").await.unwrap();
        poll_share_id(&posted)
    }

    async fn get_poll(&self, share_id: &str, session: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::get(format!("/api/polls/{share_id}"));
        if let Some(session) = session {
            request = request.header(COOKIE, format!("daghep_session={session}"));
        }
        let response = call(&self.app, request.body(Body::empty()).unwrap()).await;
        let status = response.status();
        (status, json_body(response).await)
    }

    async fn answer_poll(
        &self,
        share_id: &str,
        session: Option<&str>,
        answers: Value,
    ) -> (StatusCode, Value) {
        let mut request = Request::put(format!("/api/polls/{share_id}/answers"))
            .header(CONTENT_TYPE, "application/json");
        if let Some(session) = session {
            request = request.header(COOKIE, format!("daghep_session={session}"));
        }
        let body = json!({ "answers": answers }).to_string();
        let response = call(&self.app, request.body(Body::from(body)).unwrap()).await;
        let status = response.status();
        (status, json_body(response).await)
    }

    /// Runs the scheduler once; returns how many poll nudges it posted.
    async fn run_nudges(&self) -> i64 {
        let response = call(&self.app, cron_request(Some(CRON_SECRET))).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["nudged"].as_i64().unwrap()
    }
}

#[sqlx::test]
async fn a_poll_is_read_confirmed_and_posted_with_its_link(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.poll_answer(Some(weekend_poll()));

    let summary = bot
        .in_group(HOST, "tạo bảng điểm danh bóng rổ T7, CN, nhắc lúc 20h")
        .await
        .unwrap();
    assert_eq!(
        bot.polls.calls.lock().unwrap().as_slice(),
        ["tạo bảng điểm danh bóng rổ T7, CN, nhắc lúc 20h"]
    );
    assert!(
        summary.contains("Ai chơi bóng rổ cuối tuần này?"),
        "{summary}"
    );
    assert!(
        summary.contains("Thứ 7 03/10 · Chủ nhật 04/10 · chơi lúc 18:00"),
        "{summary}"
    );
    assert!(
        summary.contains("nhắc người chưa trả lời lúc 20:00"),
        "{summary}"
    );
    assert!(bot.reminders.calls.lock().unwrap().is_empty());

    let posted = bot.in_group(HOST, "ok").await.unwrap();
    assert!(posted.contains(&format!("{FRONTEND}/p/")), "{posted}");
    let (status, poll) = bot.get_poll(&poll_share_id(&posted), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(poll["title"], "Ai chơi bóng rổ cuối tuần này?");
    assert_eq!(poll["days"], json!(["2099-10-03", "2099-10-04"]));
    assert_eq!(poll["playTime"], "18:00");
    assert!(posted.contains("chơi lúc 18:00"), "{posted}");
    assert_eq!(poll["closed"], false);
    assert_eq!(poll["mine"], Value::Null);
    assert_eq!(poll["players"], json!([]));
}

#[sqlx::test]
async fn a_poll_request_asks_for_what_it_left_out(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.poll_answer(Some(RawPoll {
        play_time: None,
        ..weekend_poll()
    }));
    let reply = bot
        .in_group(HOST, "tạo bảng điểm danh T7, CN")
        .await
        .unwrap();
    assert!(reply.contains("chơi lúc mấy giờ"), "{reply}");
}

#[sqlx::test]
async fn players_answer_signed_in_and_everyone_sees_the_table(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let share_id = bot.add_weekend_poll().await;
    let yes_saturday = json!([
        { "day": "2099-10-03", "attending": true },
        { "day": "2099-10-04", "attending": false }
    ]);

    let (status, _) = bot.answer_poll(&share_id, None, yes_saturday.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let nam = sign_in(&bot.app, "nam").await;
    let (status, view) = bot.answer_poll(&share_id, Some(&nam), yes_saturday).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        view["mine"],
        json!({ "2099-10-03": true, "2099-10-04": false })
    );
    assert_eq!(
        view["counts"],
        json!([{ "day": "2099-10-03", "yes": 1 }, { "day": "2099-10-04", "yes": 0 }])
    );

    // Changing an answer replaces it.
    let (_, view) = bot
        .answer_poll(
            &share_id,
            Some(&nam),
            json!([{ "day": "2099-10-04", "attending": true }]),
        )
        .await;
    assert_eq!(
        view["mine"],
        json!({ "2099-10-03": true, "2099-10-04": true })
    );

    let (_, public) = bot.get_poll(&share_id, None).await;
    assert_eq!(
        public["players"],
        json!([{ "name": "Player nam", "answers": { "2099-10-03": true, "2099-10-04": true } }])
    );
    assert_eq!(public["mine"], Value::Null);

    let (status, error) = bot
        .answer_poll(
            &share_id,
            Some(&nam),
            json!([{ "day": "2099-10-05", "attending": true }]),
        )
        .await;
    assert_eq!(
        (status, error["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_day"))
    );
    let (status, _) = bot.get_poll("unknownpoll1", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn the_daily_nudge_counts_and_names_who_has_not_answered(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let share_id = bot.add_weekend_poll().await;
    let nam = sign_in(&bot.app, "nam").await;
    bot.answer_poll(
        &share_id,
        Some(&nam),
        json!([
            { "day": "2099-10-03", "attending": true },
            { "day": "2099-10-04", "attending": true }
        ]),
    )
    .await;
    // Huy answered only Saturday: still missing Sunday.
    let huy = sign_in(&bot.app, "huy").await;
    bot.answer_poll(
        &share_id,
        Some(&huy),
        json!([{ "day": "2099-10-03", "attending": false }]),
    )
    .await;

    // Not yet: NOW is Thursday 07:00; the first nudge is Thursday 20:00.
    assert_eq!(bot.run_nudges().await, 0);
    bot.clock.advance(Duration::hours(13));
    assert_eq!(bot.run_nudges().await, 1);
    let (chat, nudge) = bot.last_sent();
    assert_eq!(chat, GROUP);
    assert!(
        nudge.contains("Ai chơi bóng rổ cuối tuần này? · chơi lúc 18:00"),
        "{nudge}"
    );
    assert!(nudge.contains("Thứ 7 03/10: 1 người có"), "{nudge}");
    assert!(nudge.contains("Chủ nhật 04/10: 1 người có"), "{nudge}");
    // The host tagged the bot in the group; Huy left a day out.
    assert!(
        nudge.contains("Chưa trả lời: Player host, Player huy"),
        "{nudge}"
    );
    assert!(!nudge.contains("Player nam"), "{nudge}");
    assert!(
        nudge.contains(&format!("{FRONTEND}/p/{share_id}")),
        "{nudge}"
    );
    assert_eq!(bot.run_nudges().await, 0);

    // Friday, Saturday and Sunday at 20:00, then never again.
    for _ in 0..3 {
        bot.clock.advance(Duration::days(1));
        assert_eq!(bot.run_nudges().await, 1);
    }
    bot.clock.advance(Duration::days(1));
    assert_eq!(bot.run_nudges().await, 0);

    // Closed after Sunday: no more answers.
    let (status, error) = bot
        .answer_poll(
            &share_id,
            Some(&nam),
            json!([{ "day": "2099-10-04", "attending": false }]),
        )
        .await;
    assert_eq!(
        (status, error["error"].as_str()),
        (StatusCode::CONFLICT, Some("poll_closed"))
    );
    let (_, view) = bot.get_poll(&share_id, Some(&nam)).await;
    assert_eq!(view["closed"], true);
}

#[sqlx::test]
async fn a_chat_keeps_at_most_three_open_polls(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    for _ in 0..3 {
        bot.add_weekend_poll().await;
    }
    bot.in_group(HOST, "tạo bảng điểm danh T7").await.unwrap();
    let full = bot.in_group(HOST, "ok").await.unwrap();
    assert!(full.contains("đã có 3 bảng điểm danh"), "{full}");
}

#[sqlx::test]
async fn only_linked_senders_make_polls_and_cancel_drops_a_pending_one(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.poll_answer(Some(weekend_poll()));
    let refused = bot.in_group(OTHER, "tạo bảng điểm danh T7").await.unwrap();
    assert!(refused.contains("liên kết"), "{refused}");
    assert!(bot.polls.calls.lock().unwrap().is_empty());

    bot.link(HOST, "host").await;
    bot.in_group(HOST, "tạo bảng điểm danh T7").await.unwrap();
    let cancelled = bot.in_group(HOST, "huỷ").await.unwrap();
    assert!(cancelled.contains("Đã huỷ"), "{cancelled}");
    let ok = bot.in_group(HOST, "ok").await.unwrap();
    assert!(ok.contains("Không có kèo nào đang chờ"), "{ok}");
}

#[sqlx::test]
async fn a_poll_replaces_a_pending_draft_and_reminder_and_the_other_way_round(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    let session = bot.link(HOST, "host").await;
    add_payout(&bot.app, &session).await;
    bot.poll_answer(Some(weekend_poll()));
    bot.reminder_answer(Some(weekend_reminder()));

    // A draft, then a poll: "ok" saves the poll, and nothing is left after it.
    bot.in_group(HOST, "tối nay 19h SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();
    bot.in_group(HOST, "tạo bảng điểm danh T7, CN")
        .await
        .unwrap();
    let posted = bot.in_group(HOST, "ok").await.unwrap();
    assert!(posted.contains("/p/"), "{posted}");
    assert_eq!(matches_in_db(&pool).await, 0);
    let again = bot.in_group(HOST, "ok").await.unwrap();
    assert!(again.contains("Không có kèo nào đang chờ"), "{again}");

    // A poll, then a reminder: "ok" saves only the reminder.
    bot.in_group(HOST, "tạo bảng điểm danh T7, CN")
        .await
        .unwrap();
    bot.in_group(HOST, "nhắc t7 cn 8h").await.unwrap();
    let saved = bot.in_group(HOST, "ok").await.unwrap();
    assert!(saved.contains("Đã lưu lịch nhắc"), "{saved}");
    let again = bot.in_group(HOST, "ok").await.unwrap();
    assert!(again.contains("Không có kèo nào đang chờ"), "{again}");

    // A poll, then a match draft: "ok" creates only the match.
    bot.in_group(HOST, "tạo bảng điểm danh T7, CN")
        .await
        .unwrap();
    bot.in_group(HOST, "tối nay 19h SSA sân 7 thiếu 3, 60k")
        .await
        .unwrap();
    let created = bot.in_group(HOST, "ok").await.unwrap();
    assert!(created.contains("Đã tạo kèo"), "{created}");
    let again = bot.in_group(HOST, "ok").await.unwrap();
    assert!(again.contains("Không có kèo nào đang chờ"), "{again}");
}

#[sqlx::test]
async fn relinking_drops_a_pending_poll(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    bot.link(HOST, "host").await;
    bot.poll_answer(Some(weekend_poll()));
    bot.in_group(HOST, "tạo bảng điểm danh T7, CN")
        .await
        .unwrap();
    bot.link(HOST, "someone-else").await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM bot_pending_polls")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test]
async fn the_scheduler_clears_an_unconfirmed_poll_after_thirty_minutes(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    bot.link(HOST, "host").await;
    bot.poll_answer(Some(weekend_poll()));
    bot.in_group(HOST, "tạo bảng điểm danh T7, CN")
        .await
        .unwrap();
    bot.clock.advance(Duration::minutes(31));
    bot.run_reminders().await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM bot_pending_polls")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test]
async fn poll_responses_are_never_cached_and_late_or_closed_nudges_are_skipped(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let share_id = bot.add_weekend_poll().await;

    let response = call(
        &bot.app,
        Request::get(format!("/api/polls/{share_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "private, no-store"
    );

    // Thursday 20:00 is due, but the scheduler only runs at 22:00: skipped.
    bot.clock.advance(Duration::hours(15));
    assert_eq!(bot.run_nudges().await, 0);
    // Friday 20:00 posts as usual.
    bot.clock.advance(Duration::hours(22));
    assert_eq!(bot.run_nudges().await, 1);
}

#[sqlx::test]
async fn a_nudge_due_on_the_last_evening_is_not_posted_after_the_poll_closes(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.poll_answer(Some(RawPoll {
        days: vec!["2099-10-01".to_owned()],
        nudge_time: Some("23:50".to_owned()),
        ..weekend_poll()
    }));
    bot.in_group(HOST, "tạo bảng điểm danh tối nay")
        .await
        .unwrap();
    bot.in_group(HOST, "ok").await.unwrap();
    // Thursday 23:50 is due; the run comes at Friday 00:05, after the close.
    bot.clock
        .advance(Duration::hours(17) + Duration::minutes(5));
    assert_eq!(bot.run_nudges().await, 0);
}

#[sqlx::test]
async fn people_not_seen_for_sixty_days_are_no_longer_named(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.link(OTHER, "other").await;
    bot.in_group(OTHER, "hi").await;
    bot.clock.advance(Duration::days(61));
    bot.reminder_answer(None);
    bot.poll_answer(Some(RawPoll {
        days: vec!["2099-12-02".to_owned()],
        ..weekend_poll()
    }));
    bot.in_group(HOST, "tạo bảng điểm danh").await.unwrap();
    bot.in_group(HOST, "ok").await.unwrap();
    // 2099-12-01 07:00 local; the nudge is at 20:00.
    bot.clock.advance(Duration::hours(13));
    assert_eq!(bot.run_nudges().await, 1);
    let (_, nudge) = bot.last_sent();
    assert!(nudge.contains("Player host"), "{nudge}");
    assert!(!nudge.contains("Player other"), "{nudge}");
}

// ---- Weekly polls and help ----

impl Bot {
    /// Runs the scheduler once; returns how many weekly polls it posted.
    async fn run_weekly(&self) -> i64 {
        let response = call(&self.app, cron_request(Some(CRON_SECRET))).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["weeklyPolls"].as_i64().unwrap()
    }

    async fn add_weekly_poll(&self) -> String {
        self.poll_answer(Some(RawPoll {
            weekly: true,
            ..weekend_poll()
        }));
        let summary = self
            .in_group(
                HOST,
                "tạo bảng điểm danh bóng rổ T7, CN hằng tuần, chơi lúc 18h",
            )
            .await
            .unwrap();
        assert!(
            summary.contains("🔁 Hằng tuần") && summary.contains("lúc 20:00 tối Thứ 2"),
            "{summary}"
        );
        self.in_group(HOST, "ok").await.unwrap()
    }
}

#[sqlx::test]
async fn a_weekly_poll_posts_a_new_poll_every_monday_evening(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let posted = bot.add_weekly_poll().await;
    assert!(posted.contains("Thứ 7 03/10 · Chủ nhật 04/10"), "{posted}");
    assert!(
        posted.contains("Mỗi tuần mình sẽ tự đăng bảng mới lúc 20:00"),
        "{posted}"
    );
    let first = poll_share_id(&posted);

    // NOW is Thursday 07:00 local. Nothing new until Monday 20:00 local.
    bot.clock.advance(Duration::days(4) + Duration::hours(12));
    assert_eq!(bot.run_weekly().await, 0);
    bot.clock.advance(Duration::hours(1));
    assert_eq!(bot.run_weekly().await, 1);
    let (chat, text) = bot.last_sent();
    assert_eq!(chat, GROUP);
    assert!(
        text.contains("Thứ 7 10/10 · Chủ nhật 11/10 · chơi lúc 18:00"),
        "{text}"
    );
    let second = poll_share_id(&text);
    assert_ne!(first, second);
    let (_, view) = bot.get_poll(&second, None).await;
    assert_eq!(view["days"], json!(["2099-10-10", "2099-10-11"]));
    assert_eq!(bot.run_weekly().await, 0);

    // And the Monday after.
    bot.clock.advance(Duration::days(7));
    assert_eq!(bot.run_weekly().await, 1);
    assert!(bot.last_sent().1.contains("Thứ 7 17/10"));
}

#[sqlx::test]
async fn a_weekly_poll_is_listed_and_stopped(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let none = bot.in_group(HOST, "tắt bảng").await.unwrap();
    assert!(none.contains("chưa có bảng hằng tuần"), "{none}");

    bot.add_weekly_poll().await;
    let list = bot.in_group(HOST, "tắt bảng").await.unwrap();
    assert!(
        list.contains(
            "1. mỗi Thứ 7, Chủ nhật · chơi lúc 18:00: \"Ai chơi bóng rổ cuối tuần này?\""
        ),
        "{list}"
    );
    let refused = bot.in_group(OTHER, "tắt bảng 1").await.unwrap();
    assert!(refused.contains("liên kết"), "{refused}");
    let missing = bot.in_group(HOST, "tắt bảng 2").await.unwrap();
    assert!(
        missing.contains("Không có bảng hằng tuần số 2"),
        "{missing}"
    );
    let stopped = bot.in_group(HOST, "dừng bảng 1").await.unwrap();
    assert!(stopped.contains("Đã dừng bảng hằng tuần số 1"), "{stopped}");

    bot.clock.advance(Duration::days(5));
    assert_eq!(bot.run_weekly().await, 0);
}

#[sqlx::test]
async fn a_poll_for_this_week_only_does_not_repeat(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let posted = bot.add_weekend_poll().await;
    assert!(!posted.is_empty());
    bot.clock.advance(Duration::days(5));
    assert_eq!(bot.run_weekly().await, 0);
    let list = bot.in_group(HOST, "tắt bảng").await.unwrap();
    assert!(list.contains("chưa có bảng hằng tuần"), "{list}");
}

#[sqlx::test]
async fn help_lists_every_command(pool: PgPool) {
    let bot = bot(pool, tonight());
    let help = bot.in_group(HOST, "help").await.unwrap();
    for command in [
        "@Bot DaGhep tối nay 19h",
        "@Bot DaGhep tạo bảng điểm danh",
        "hằng tuần",
        "@Bot DaGhep tắt bảng",
        "@Bot DaGhep nhắc mỗi thứ 5",
        "@Bot DaGhep tắt nhắc",
        "@Bot DaGhep ok",
        "liên kết",
    ] {
        assert!(help.contains(command), "{command} in {help}");
    }
}

async fn insert_series(pool: &PgPool, chat: &str, count: i64) {
    for _ in 0..count {
        sqlx::query(
            "INSERT INTO bot_poll_series (chat_id, user_id, title, weekdays, nudge_time,
                 next_post_at, created_at)
             VALUES ($1, (SELECT min(id) FROM users), 'Khác', 1, '20:00', '2100-01-01', now())",
        )
        .bind(chat)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn a_weekly_post_missed_by_an_outage_is_skipped_once_then_resumes(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_weekly_poll().await;
    // Due Monday 05 at 20:00; the scheduler is down until Tuesday 10:00 (14 h late).
    bot.clock.advance(Duration::days(5) + Duration::hours(3));
    assert_eq!(bot.run_weekly().await, 0);
    assert_eq!(bot.run_weekly().await, 0);
    // Three more weeks of outage: nothing late is posted.
    bot.clock.advance(Duration::days(21));
    assert_eq!(bot.run_weekly().await, 0);
    // The next Monday at 20:00 it posts that week's poll, once.
    bot.clock.advance(Duration::days(6) + Duration::hours(10));
    assert_eq!(bot.run_weekly().await, 1);
    assert!(bot.last_sent().1.contains("Thứ 7 07/11 · Chủ nhật 08/11"));
    assert_eq!(bot.run_weekly().await, 0);
}

#[sqlx::test]
async fn a_chat_keeps_at_most_three_weekly_polls(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    bot.link(HOST, "host").await;
    insert_series(&pool, GROUP, 3).await;
    let posted = bot.add_weekly_poll().await;
    assert!(posted.contains("đã có 3 bảng hằng tuần"), "{posted}");
    assert!(posted.contains("/p/"), "{posted}");
}

#[sqlx::test]
async fn a_weekly_post_is_skipped_when_the_chat_has_three_open_polls(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_weekly_poll().await;
    // Three polls for next Saturday, still open on Monday.
    bot.poll_answer(Some(RawPoll {
        days: vec!["2099-10-10".to_owned()],
        ..weekend_poll()
    }));
    for _ in 0..2 {
        bot.in_group(HOST, "tạo bảng điểm danh T7 tuần sau")
            .await
            .unwrap();
        bot.in_group(HOST, "ok").await.unwrap();
    }
    // Monday 05: this week's weekly poll closed, so 2 are open: it posts.
    bot.clock.advance(Duration::days(4) + Duration::hours(13));
    assert_eq!(bot.run_weekly().await, 1);
    // Now 3 are open (2 for Saturday 10 + the new weekly one). Make the next
    // Monday's slot full too: polls for Saturday 17.
    bot.poll_answer(Some(RawPoll {
        days: vec!["2099-10-17".to_owned()],
        ..weekend_poll()
    }));
    // Monday 12 at 08:00: last week's polls are closed; three for Saturday 17
    // are opened.
    bot.clock.advance(Duration::days(6) + Duration::hours(12));
    for _ in 0..3 {
        bot.in_group(HOST, "tạo bảng điểm danh T7").await.unwrap();
        let posted = bot.in_group(HOST, "ok").await.unwrap();
        assert!(posted.contains("/p/"), "{posted}");
    }
    // Monday 12 at 20:00: three open polls, so the weekly post waits.
    bot.clock.advance(Duration::hours(12));
    assert_eq!(bot.run_weekly().await, 0);
}

#[sqlx::test]
async fn a_weekly_poll_that_cannot_be_posted_three_times_is_dropped(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_weekly_poll().await;
    *bot.chat.failing.lock().unwrap() = true;
    bot.clock.advance(Duration::days(4) + Duration::hours(13));
    for _ in 0..3 {
        assert_eq!(bot.run_weekly().await, 0);
        bot.clock.advance(Duration::days(7));
    }
    *bot.chat.failing.lock().unwrap() = false;
    let list = bot.in_group(HOST, "tắt bảng").await.unwrap();
    assert!(list.contains("chưa có bảng hằng tuần"), "{list}");
}

#[sqlx::test]
async fn a_poll_repeats_only_when_the_request_says_so(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    // The reader says weekly, but the request never asked for it.
    bot.poll_answer(Some(RawPoll {
        weekly: true,
        ..weekend_poll()
    }));
    let summary = bot
        .in_group(HOST, "tạo bảng điểm danh T7, CN, chơi lúc 18h")
        .await
        .unwrap();
    assert!(!summary.contains("🔁"), "{summary}");
    let posted = bot.in_group(HOST, "ok").await.unwrap();
    assert!(!posted.contains("🔁"), "{posted}");
    let list = bot.in_group(HOST, "tắt bảng").await.unwrap();
    assert!(list.contains("chưa có bảng hằng tuần"), "{list}");
}

#[sqlx::test]
async fn huy_cancels_a_pending_weekly_poll(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.poll_answer(Some(RawPoll {
        weekly: true,
        ..weekend_poll()
    }));
    bot.in_group(HOST, "tạo bảng điểm danh T7, CN hằng tuần, chơi lúc 18h")
        .await
        .unwrap();
    let cancelled = bot.in_group(HOST, "huỷ").await.unwrap();
    assert!(cancelled.contains("Đã huỷ"), "{cancelled}");
}

// ---- Venues and public groups ----

const LAKEVIEW_LINK: &str = "https://maps.app.goo.gl/Lakeview1";

impl Bot {
    /// HOST (the admin, first account) proposes and approves Lakeview; returns its id.
    async fn add_lakeview(&self) -> i64 {
        let before = self.chat.sent.lock().unwrap().len();
        let reply = self
            .in_group(HOST, &format!("thêm sân Lakeview bóng rổ {LAKEVIEW_LINK}"))
            .await
            .unwrap();
        assert!(reply.contains("Đã gửi sân \"Sân Lakeview\""), "{reply}");
        let notice = self.private_since(before, HOST).join("\n");
        let id: i64 = notice
            .split("(số ")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .and_then(|id| id.parse().ok())
            .unwrap_or_else(|| panic!("an admin notice with the id: {notice}"));
        assert!(notice.contains("🏀 Bóng rổ"), "{notice}");
        let decided = self
            .say(HOST, "PRIVATE", HOST, &format!("duyệt sân {id}"))
            .await
            .unwrap();
        assert!(
            decided.contains(&format!("Đã duyệt sân số {id}")),
            "{decided}"
        );
        id
    }

    async fn groups(&self) -> Value {
        let response = call(
            &self.app,
            Request::get("/api/groups").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["groups"].clone()
    }

    async fn add_public_lakeview_group(&self) -> String {
        self.poll_answer(Some(RawPoll {
            weekly: true,
            sport: Some("basketball".to_owned()),
            format: Some("3x3".to_owned()),
            venue_id: Some(self.lakeview_slug().await),
            ..weekend_poll()
        }));
        let summary = self
            .in_group(
                HOST,
                "tạo bảng điểm danh bóng rổ 3x3 sân Lakeview T7, CN hằng tuần, chơi lúc 18h, công khai",
            )
            .await
            .unwrap();
        assert!(
            summary.contains("🏀 Bóng rổ · 3x3 · 📍 Sân Lakeview · 🌐 Hiện trên trang chủ"),
            "{summary}"
        );
        self.in_group(HOST, "ok").await.unwrap()
    }

    async fn lakeview_slug(&self) -> String {
        sqlx::query_scalar("SELECT slug FROM venues WHERE name = 'Sân Lakeview'")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

#[sqlx::test]
async fn an_organiser_adds_a_venue_and_an_admin_approves_it(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    bot.link(HOST, "host").await;
    bot.link(OTHER, "other").await;

    // OTHER (not an admin) proposes it; the admin gets a private notice.
    let before = bot.chat.sent.lock().unwrap().len();
    let reply = bot
        .in_group(OTHER, &format!("thêm sân Lakeview bóng rổ {LAKEVIEW_LINK}"))
        .await
        .unwrap();
    assert!(
        reply.contains("Đã gửi sân \"Sân Lakeview\" để duyệt"),
        "{reply}"
    );
    let notice = bot.private_since(before, HOST).join("\n");
    assert!(notice.contains("Sân mới chờ duyệt"), "{notice}");
    assert!(notice.contains("10.790500,106.751200"), "{notice}");
    let (status, active): (String, bool) =
        sqlx::query_as("SELECT status, active FROM venues WHERE name = 'Sân Lakeview'")
            .fetch_one(&pool)
            .await
            .unwrap();
    // Pending, and never offered for football matches.
    assert_eq!((status.as_str(), active), ("pending", false));
    let id: i64 = sqlx::query_scalar("SELECT id FROM venues WHERE name = 'Sân Lakeview'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Only an admin may decide.
    let refused = bot
        .say(OTHER, "PRIVATE", OTHER, &format!("duyệt sân {id}"))
        .await
        .unwrap();
    assert!(refused.contains("chỉ dành cho admin"), "{refused}");
    let list = bot
        .say(HOST, "PRIVATE", HOST, "sân chờ duyệt")
        .await
        .unwrap();
    assert!(list.contains(&format!("số {id}: Sân Lakeview")), "{list}");

    let before = bot.chat.sent.lock().unwrap().len();
    let decided = bot
        .say(HOST, "PRIVATE", HOST, &format!("duyệt sân {id}"))
        .await
        .unwrap();
    assert!(decided.contains("Đã duyệt sân số"), "{decided}");
    // The organiser who proposed it is told privately.
    let told = bot.private_since(before, OTHER).join("\n");
    assert!(
        told.contains("Sân \"Sân Lakeview\" đã được duyệt"),
        "{told}"
    );
    let again = bot
        .say(HOST, "PRIVATE", HOST, &format!("duyệt sân {id}"))
        .await
        .unwrap();
    assert!(again.contains("Không có sân chờ duyệt"), "{again}");
}

#[sqlx::test]
async fn a_venue_request_needs_a_maps_link_a_sport_and_a_place_in_vietnam(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    let no_link = bot
        .in_group(HOST, "thêm sân Lakeview bóng rổ")
        .await
        .unwrap();
    assert!(no_link.contains("link Google Maps"), "{no_link}");
    let other_site = bot
        .in_group(HOST, "thêm sân Lakeview bóng rổ http://evil.example/x")
        .await
        .unwrap();
    assert!(other_site.contains("link Google Maps"), "{other_site}");
    let no_sport = bot
        .in_group(HOST, &format!("thêm sân Lakeview {LAKEVIEW_LINK}"))
        .await
        .unwrap();
    assert!(no_sport.contains("bóng đá hay bóng rổ"), "{no_sport}");
    *bot.maps.place.lock().unwrap() = Some(daghep_api::bot::maps::MapsPlace {
        lat: 48.85,
        lng: 2.35,
        label: None,
    });
    let abroad = bot
        .in_group(HOST, &format!("thêm sân Lakeview bóng rổ {LAKEVIEW_LINK}"))
        .await
        .unwrap();
    assert!(abroad.contains("không ở Việt Nam"), "{abroad}");
    *bot.maps.place.lock().unwrap() = None;
    let unreadable = bot
        .in_group(HOST, &format!("thêm sân Lakeview bóng rổ {LAKEVIEW_LINK}"))
        .await
        .unwrap();
    assert!(unreadable.contains("chưa đọc được vị trí"), "{unreadable}");
}

#[sqlx::test]
async fn a_public_weekly_group_shows_on_the_home_page_without_names(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_lakeview().await;
    assert_eq!(bot.groups().await, json!([]));

    let posted = bot.add_public_lakeview_group().await;
    let share_id = poll_share_id(&posted);
    let nam = sign_in(&bot.app, "nam").await;
    bot.answer_poll(
        &share_id,
        Some(&nam),
        json!([
            { "day": "2099-10-03", "attending": true },
            { "day": "2099-10-04", "attending": false }
        ]),
    )
    .await;

    let response = call(
        &bot.app,
        Request::get("/api/groups").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "public, max-age=60"
    );
    let groups = bot.groups().await;
    assert_eq!(
        groups,
        json!([{
            "title": "Ai chơi bóng rổ cuối tuần này?",
            "sport": "basketball",
            "format": "3x3",
            "venue": { "name": "Sân Lakeview", "address": "Sân bóng rổ Lakeview" },
            "weekdays": [0, 6],
            "playTime": "18:00",
            "week": {
                "players": 1,
                "yes": [{ "day": "2099-10-03", "yes": 1 }, { "day": "2099-10-04", "yes": 0 }]
            }
        }])
    );
    assert!(!groups.to_string().contains("Player"), "no names: {groups}");

    // Hidden again, it leaves the home page; shown again, it comes back.
    let hidden = bot.in_group(HOST, "ẩn bảng 1").await.unwrap();
    assert!(hidden.contains("Đã ẩn bảng hằng tuần số 1"), "{hidden}");
    assert_eq!(bot.groups().await, json!([]));
    let shown = bot.in_group(HOST, "công khai bảng 1").await.unwrap();
    assert!(shown.contains("giờ hiện trên trang chủ"), "{shown}");
    assert_eq!(bot.groups().await.as_array().unwrap().len(), 1);
    // Only linked members change it.
    let refused = bot.in_group(OTHER, "ẩn bảng 1").await.unwrap();
    assert!(refused.contains("liên kết"), "{refused}");
}

#[sqlx::test]
async fn weekly_groups_are_private_unless_asked_and_pending_venues_stay_hidden(pool: PgPool) {
    let bot = bot(pool.clone(), tonight());
    bot.link(HOST, "host").await;
    // A weekly group without "công khai" is not listed.
    bot.add_weekly_poll().await;
    assert_eq!(bot.groups().await, json!([]));

    // A public group whose venue is still pending shows without the venue.
    bot.in_group(HOST, &format!("thêm sân Lakeview bóng rổ {LAKEVIEW_LINK}"))
        .await
        .unwrap();
    let slug = bot.lakeview_slug().await;
    bot.poll_answer(Some(RawPoll {
        weekly: true,
        sport: Some("basketball".to_owned()),
        venue_id: Some(slug),
        ..weekend_poll()
    }));
    let summary = bot
        .in_group(
            HOST,
            "tạo bảng điểm danh bóng rổ sân Lakeview T7, CN hằng tuần, công khai",
        )
        .await
        .unwrap();
    assert!(
        !summary.contains("📍"),
        "a pending venue is not offered: {summary}"
    );
    bot.in_group(HOST, "ok").await.unwrap();
    let groups = bot.groups().await;
    assert_eq!(groups.as_array().unwrap().len(), 1, "{groups}");
    assert_eq!(groups[0]["venue"], Value::Null);
    assert_eq!(groups[0]["sport"], "basketball");
}

#[sqlx::test]
async fn a_public_group_counts_the_poll_the_bot_posted_this_week(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.add_lakeview().await;
    bot.add_public_lakeview_group().await;
    // Monday 05 at 20:00 local the bot posts the poll for 10–11/10.
    bot.clock.advance(Duration::days(4) + Duration::hours(13));
    assert_eq!(bot.run_weekly().await, 1);
    let share_id = poll_share_id(&bot.last_sent().1);
    let nam = sign_in(&bot.app, "nam").await;
    bot.answer_poll(
        &share_id,
        Some(&nam),
        json!([{ "day": "2099-10-10", "attending": true }]),
    )
    .await;
    let groups = bot.groups().await;
    assert_eq!(
        groups[0]["week"],
        json!({ "players": 1, "yes": [{ "day": "2099-10-10", "yes": 1 }, { "day": "2099-10-11", "yes": 0 }] })
    );
}

#[sqlx::test]
async fn a_second_group_proposing_the_same_venue_does_not_fail(pool: PgPool) {
    let bot = bot(pool, tonight());
    bot.link(HOST, "host").await;
    bot.link(OTHER, "other").await;
    for sender in [HOST, OTHER] {
        let reply = bot
            .in_group(
                sender,
                &format!("THÊM SÂN Lakeview BÓNG RỔ {LAKEVIEW_LINK}"),
            )
            .await
            .unwrap();
        assert!(reply.contains("Đã gửi sân \"Sân Lakeview\""), "{reply}");
    }
    let first = bot
        .say(HOST, "PRIVATE", HOST, "sân chờ duyệt")
        .await
        .unwrap();
    let ids: Vec<i64> = first
        .split("số ")
        .skip(1)
        .filter_map(|rest| rest.split(':').next()?.parse().ok())
        .collect();
    assert_eq!(ids.len(), 2, "{first}");
    bot.say(HOST, "PRIVATE", HOST, &format!("duyệt sân {}", ids[0]))
        .await;
    let duplicate = bot
        .say(HOST, "PRIVATE", HOST, &format!("duyệt sân {}", ids[1]))
        .await
        .unwrap();
    assert!(duplicate.contains("trùng tên"), "{duplicate}");
    // Once approved, proposing it again says it exists.
    let exists = bot
        .in_group(OTHER, &format!("thêm sân Lakeview bóng rổ {LAKEVIEW_LINK}"))
        .await
        .unwrap();
    assert!(exists.contains("đã có rồi"), "{exists}");
}
