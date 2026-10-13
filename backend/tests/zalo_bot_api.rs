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
    Bot {
        app: app_with_bot(pool.clone(), clock.clone(), reader.clone(), chat.clone()),
        chat,
        reader,
        clock,
        pool,
    }
}

fn update(chat_id: &str, chat_type: &str, sender: &str, text: &str, message_id: &str) -> Value {
    json!({ "ok": true, "result": {
        "event_name": "message.text.received",
        "message": {
            "chat": { "id": chat_id, "chat_type": chat_type },
            "text": text,
            "message_id": message_id,
            "date": 1,
            "from": { "id": sender, "is_bot": false, "display_name": "Someone" }
        }
    } })
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

    async fn deliver(&self, body: &Value) -> Option<String> {
        let before = self.chat.sent.lock().unwrap().len();
        let response = call(&self.app, webhook_request(body, Some(BOT_SECRET))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let sent = self.chat.sent.lock().unwrap();
        assert!(sent.len() <= before + 1, "at most one reply per message");
        sent.get(before).map(|(chat, text)| {
            assert_eq!(
                chat,
                body["result"]["message"]["chat"]["id"].as_str().unwrap()
            );
            text.clone()
        })
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

    let in_group = bot.in_group(HOST, "liên kết").await.unwrap();
    assert!(!in_group.contains("/bot/link"), "{in_group}");

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

    let reply = bot.in_group(HOST, "tối nay 19h SSA thiếu 3").await.unwrap();

    assert!(reply.contains("liên kết"), "{reply}");
    assert!(bot.reader.calls.lock().unwrap().is_empty());
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
