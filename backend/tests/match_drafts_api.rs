//! Acceptance tests for reading a pasted organiser message into a match draft.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use chrono::{Duration, NaiveDate};
use common::*;
use daghep_api::drafts::domain::RawDraft;
use serde_json::{Value, json};
use sqlx::PgPool;

fn draft_request(session: Option<&str>, body: Value) -> Request<Body> {
    let mut request = Request::post("/api/match-drafts").header(CONTENT_TYPE, "application/json");
    if let Some(session) = session {
        request = request.header(COOKIE, format!("daghep_session={session}"));
    }
    request.body(Body::from(body.to_string())).unwrap()
}

async fn read(app: &Router, session: &str, text: &str) -> Response {
    call(app, draft_request(Some(session), json!({ "text": text }))).await
}

/// What a model might answer for "tối nay 19h SSA thiếu 3 người, 60k/người, sân 7".
fn tonight_at_ssa() -> RawDraft {
    RawDraft {
        venue_id: Some("ssa-amitie".to_owned()),
        date: Some("2099-10-01".to_owned()),
        start_time: Some("19:00".to_owned()),
        end_time: None,
        format: Some("seven_a_side".to_owned()),
        match_type: None,
        level_min: None,
        level_max: None,
        slot_count: Some(3),
        price_per_player_vnd: Some(60_000),
        total_fee_vnd: None,
    }
}

fn reader(answer: Option<RawDraft>) -> FakeReader {
    FakeReader {
        answer,
        ..FakeReader::default()
    }
}

async fn reads_in_db(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM draft_reads")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn fills_the_form_from_the_message(pool: PgPool) {
    let fake = reader(Some(tonight_at_ssa()));
    let app = app_with_reader(pool, TestClock::at(NOW), fake.clone());
    let session = sign_in(&app, "host").await;

    let response = read(
        &app,
        &session,
        "  tối nay 19h SSA thiếu 3 người, 60k/người, sân 7 ",
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
    assert_eq!(
        json_body(response).await,
        json!({ "draft": {
            "venueId": "ssa-amitie",
            "date": "2099-10-01",
            "startTime": "19:00",
            "endTime": null,
            "format": "seven_a_side",
            "matchType": null,
            "levelMin": null,
            "levelMax": null,
            "slotCount": 3,
            "totalFeeVnd": 180000
        } })
    );
    // The reader gets the trimmed message, today in Ho Chi Minh City and the active venues.
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![(
            "tối nay 19h SSA thiếu 3 người, 60k/người, sân 7".to_owned(),
            NaiveDate::from_ymd_opt(2099, 10, 1).unwrap(),
            vec![
                "ssa-amitie".to_owned(),
                "an-phu-nguyen-hoang".to_owned(),
                "khu-the-thao-an-phu".to_owned()
            ],
        )]
    );
}

#[sqlx::test]
async fn leaves_out_what_the_reader_got_wrong(pool: PgPool) {
    let answer = RawDraft {
        venue_id: Some("some-other-pitch".to_owned()),
        date: Some("2099-09-30".to_owned()),
        slot_count: Some(99),
        ..tonight_at_ssa()
    };
    let app = app_with_reader(pool, TestClock::at(NOW), reader(Some(answer)));
    let session = sign_in(&app, "host").await;

    let body = json_body(read(&app, &session, "kèo nè").await).await;

    assert_eq!(body["draft"]["venueId"], Value::Null);
    assert_eq!(body["draft"]["date"], Value::Null);
    assert_eq!(body["draft"]["slotCount"], Value::Null);
    // Without a valid place count the price per person cannot become a total.
    assert_eq!(body["draft"]["totalFeeVnd"], Value::Null);
    assert_eq!(body["draft"]["startTime"], "19:00");
}

#[sqlx::test]
async fn needs_a_signed_in_user(pool: PgPool) {
    let fake = reader(Some(tonight_at_ssa()));
    let app = app_with_reader(pool.clone(), TestClock::at(NOW), fake.clone());

    let response = call(&app, draft_request(None, json!({ "text": "kèo nè" }))).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(fake.calls.lock().unwrap().is_empty());
    assert_eq!(reads_in_db(&pool).await, 0);
}

#[sqlx::test]
async fn refuses_empty_or_long_messages_without_reading_them(pool: PgPool) {
    let fake = reader(Some(tonight_at_ssa()));
    let app = app_with_reader(pool.clone(), TestClock::at(NOW), fake.clone());
    let session = sign_in(&app, "host").await;

    for text in ["   ".to_owned(), "á".repeat(501)] {
        let response = read(&app, &session, &text).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            json_body(response).await,
            json!({ "error": "invalid_text" })
        );
    }
    // 500 characters is still fine.
    assert_eq!(
        read(&app, &session, &"á".repeat(500)).await.status(),
        StatusCode::OK
    );

    let malformed = call(
        &app,
        draft_request(Some(&session), json!({ "message": "kèo" })),
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    assert_eq!(reads_in_db(&pool).await, 1);
}

#[sqlx::test]
async fn one_user_gets_ten_reads_per_day(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let fake = reader(Some(tonight_at_ssa()));
    let app = app_with_reader(pool, clock.clone(), fake.clone());
    let host = sign_in(&app, "host").await;
    let other = sign_in(&app, "other").await;

    for _ in 0..10 {
        assert_eq!(read(&app, &host, "kèo").await.status(), StatusCode::OK);
    }
    let limited = read(&app, &host, "kèo").await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        json_body(limited).await,
        json!({ "error": "draft_limit", "scope": "user" })
    );
    assert_eq!(fake.calls.lock().unwrap().len(), 10);
    // Someone else is not affected.
    assert_eq!(read(&app, &other, "kèo").await.status(), StatusCode::OK);

    // The window rolls: a day after the first read, the host can read again.
    clock.advance(Duration::hours(24) + Duration::seconds(1));
    assert_eq!(read(&app, &host, "kèo").await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn the_whole_service_gets_two_hundred_reads_per_day(pool: PgPool) {
    let app = app_with_reader(
        pool.clone(),
        TestClock::at(NOW),
        reader(Some(tonight_at_ssa())),
    );
    let session = sign_in(&app, "host").await;
    let others = insert_user(&pool).await;
    sqlx::query(
        "INSERT INTO draft_reads (user_id, created_at)
         SELECT $1, $2::timestamptz - interval '1 hour' FROM generate_series(1, 200)",
    )
    .bind(others)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();

    let limited = read(&app, &session, "kèo").await;

    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        json_body(limited).await,
        json!({ "error": "draft_limit", "scope": "service" })
    );
}

#[sqlx::test]
async fn old_reads_are_deleted(pool: PgPool) {
    let app = app_with_reader(
        pool.clone(),
        TestClock::at(NOW),
        reader(Some(tonight_at_ssa())),
    );
    let session = sign_in(&app, "host").await;
    let others = insert_user(&pool).await;
    sqlx::query("INSERT INTO draft_reads (user_id, created_at) VALUES ($1, $2::timestamptz - interval '25 hours')")
        .bind(others)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(read(&app, &session, "kèo").await.status(), StatusCode::OK);

    assert_eq!(reads_in_db(&pool).await, 1);
}

#[sqlx::test]
async fn a_failed_read_says_so_and_still_counts(pool: PgPool) {
    let app = app_with_reader(pool.clone(), TestClock::at(NOW), reader(None));
    let session = sign_in(&app, "host").await;

    let response = read(&app, &session, "kèo").await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        json_body(response).await,
        json!({ "error": "reader_failed" })
    );
    assert_eq!(reads_in_db(&pool).await, 1);
}

#[sqlx::test]
async fn without_a_reader_drafts_are_unavailable(pool: PgPool) {
    let app = app(pool.clone());
    let session = sign_in(&app, "host").await;

    let response = read(&app, &session, "kèo").await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(response).await,
        json!({ "error": "drafts_unavailable" })
    );
    assert_eq!(reads_in_db(&pool).await, 0);
}
