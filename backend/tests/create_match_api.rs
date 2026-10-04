//! Acceptance tests for creating a match through the HTTP API.
//! The clock is fixed a few days before the 2099 matches used here, so the
//! "starts in the future, at most 30 days ahead" rules hold.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn send(pool: PgPool, request: Request<Body>) -> (StatusCode, Value) {
    send_at(common::NOW, pool, request).await
}

/// Sends the request as a signed-in host, with "now" fixed at `now`.
async fn send_at(now: &str, pool: PgPool, mut request: Request<Body>) -> (StatusCode, Value) {
    let app = common::app_with(pool, common::TestClock::at(now), common::FRONTEND);
    let session = common::sign_in(&app, "host-1").await;
    request.headers_mut().insert(
        header::COOKIE,
        format!("daghep_session={session}").parse().unwrap(),
    );
    let response = common::call(&app, request).await;
    let status = response.status();
    (status, common::json_body(response).await)
}

fn post_json(body: Value) -> Request<Body> {
    Request::post("/api/matches")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn valid_request() -> Value {
    json!({
        "venueName": "SSA Sports Center",
        "startsAt": "2099-10-10T11:30:00Z",
        "endsAt": "2099-10-10T13:00:00Z",
        "format": "seven_a_side",
        "matchType": "casual",
        "levelMin": 2.5,
        "levelMax": 3.5,
        "totalFeeVnd": 900000
    })
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

#[sqlx::test]
async fn creating_a_match_returns_its_public_view_with_defaults(pool: PgPool) {
    let (status, body) = send(pool, post_json(valid_request())).await;

    assert_eq!(status, StatusCode::CREATED);
    let share_id = body["shareId"].as_str().expect("shareId");
    assert!(
        (8..=16).contains(&share_id.len()),
        "unexpected share id {share_id}"
    );
    let mut expected = json!({
        "venueName": "SSA Sports Center",
        "startsAt": "2099-10-10T11:30:00Z",
        "endsAt": "2099-10-10T13:00:00Z",
        "format": "seven_a_side",
        "matchType": "casual",
        "levelMin": 2.5,
        "levelMax": 3.5,
        "totalFeeVnd": 900000,
        "slotCount": 18,
        "pricePerPlayerVnd": 50000
    });
    expected["shareId"] = json!(share_id);
    assert_eq!(body, expected);
}

#[sqlx::test]
async fn created_match_can_be_read_back_by_its_share_id(pool: PgPool) {
    let (_, created) = send(pool.clone(), post_json(valid_request())).await;
    let share_id = created["shareId"].as_str().unwrap();

    let (status, fetched) = send(
        pool,
        Request::get(format!("/api/matches/{share_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched, created);
}

#[sqlx::test]
async fn default_slot_count_depends_on_format(pool: PgPool) {
    for (format, expected) in [
        ("five_a_side", 14),
        ("seven_a_side", 18),
        ("eleven_a_side", 28),
    ] {
        let request = with(valid_request(), "format", json!(format));
        let (status, body) = send(pool.clone(), post_json(request)).await;

        assert_eq!(status, StatusCode::CREATED, "{format}");
        assert_eq!(body["slotCount"], json!(expected), "{format}");
    }
}

#[sqlx::test]
async fn host_can_choose_the_slot_count_and_price_rounds_up(pool: PgPool) {
    let request = with(valid_request(), "slotCount", json!(14));

    let (status, body) = send(pool, post_json(request)).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["slotCount"], json!(14));
    // 900,000 / 14 = 64,285.7 -> rounded up to the next 1,000.
    assert_eq!(body["pricePerPlayerVnd"], json!(65000));
}

#[sqlx::test]
async fn invalid_fields_are_rejected_with_the_offending_field(pool: PgPool) {
    let cases = [
        ("venueName", json!("   ")),
        ("startsAt", json!("2001-01-01T10:00:00Z")),
        ("startsAt", json!("2099-11-15T11:30:00Z")),
        ("endsAt", json!("2099-10-10T16:00:00Z")),
        ("endsAt", json!("2099-10-10T17:30:00Z")),
        ("endsAt", json!("2099-10-10T11:00:00Z")),
        ("format", json!("futsal")),
        ("matchType", json!("tournament")),
        ("levelMin", json!(2.3)),
        ("levelMax", json!(2.0)),
        ("totalFeeVnd", json!(-1)),
        ("totalFeeVnd", json!(i64::MAX)),
        ("venueName", json!("SSA\u{0}Center")),
        ("slotCount", json!(31)),
        ("slotCount", json!(40000)),
    ];
    for (field, value) in cases {
        let request = with(valid_request(), field, value.clone());

        let (status, body) = send(pool.clone(), post_json(request)).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}={value}");
        assert_eq!(
            body,
            json!({ "error": "invalid_match", "field": field }),
            "{field}={value}"
        );
    }
}

#[sqlx::test]
async fn malformed_json_is_a_bad_request(pool: PgPool) {
    let request = Request::post("/api/matches")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))
        .unwrap();

    let (status, body) = send(pool, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, json!({ "error": "invalid_request" }));
}

#[sqlx::test]
async fn invalid_requests_store_nothing(pool: PgPool) {
    let request = with(valid_request(), "levelMin", json!(9.0));
    send(pool.clone(), post_json(request)).await;

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM matches")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn repository_reports_a_taken_share_id_as_duplicate(pool: PgPool) {
    use daghep_api::matches::PgMatchRepository;
    use daghep_api::matches::domain::{
        Format, InsertError, MatchRepository, MatchType, NewMatch, NewMatchInput, ShareId,
    };

    let host = daghep_api::auth::UserId(common::insert_user(&pool).await);
    let repo = PgMatchRepository::new(pool);
    let new = NewMatch::validate(
        NewMatchInput {
            venue_name: "SSA Sports Center".to_owned(),
            starts_at: "2099-10-10T11:30:00Z".parse().unwrap(),
            ends_at: "2099-10-10T13:00:00Z".parse().unwrap(),
            format: Format::SevenASide,
            match_type: MatchType::Casual,
            level_min: 2.5,
            level_max: 3.5,
            total_fee_vnd: 900_000,
            slot_count: None,
        },
        "2099-10-01T00:00:00Z".parse().unwrap(),
    )
    .unwrap();
    let id = ShareId::parse("k7Qm2xPa").unwrap();
    repo.insert(&id, &new, host).await.unwrap();

    let second = repo.insert(&id, &new, host).await;

    assert!(matches!(second, Err(InsertError::DuplicateShareId)));
}

#[sqlx::test]
async fn match_must_start_after_now(pool: PgPool) {
    let (status, body) = send_at("2099-10-10T11:30:00Z", pool, post_json(valid_request())).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body,
        json!({ "error": "invalid_match", "field": "startsAt" })
    );
}

#[sqlx::test]
async fn match_cannot_run_past_midnight_in_ho_chi_minh_city(pool: PgPool) {
    // 22:00-00:00 local time: only 2 hours, but it ends on the next day.
    let request = with(valid_request(), "startsAt", json!("2099-10-10T15:00:00Z"));
    let request = with(request, "endsAt", json!("2099-10-10T17:00:00Z"));

    let (status, body) = send(pool, post_json(request)).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, json!({ "error": "invalid_match", "field": "endsAt" }));
}

#[sqlx::test]
async fn creating_a_match_requires_sign_in(pool: PgPool) {
    let app = common::app(pool.clone());

    let response = common::call(&app, post_json(valid_request())).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        common::json_body(response).await,
        json!({ "error": "unauthenticated" })
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM matches")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn the_signed_in_user_becomes_the_host(pool: PgPool) {
    let (status, created) = send(pool.clone(), post_json(valid_request())).await;
    assert_eq!(status, StatusCode::CREATED);

    let host: Option<String> = sqlx::query_scalar(
        "SELECT i.subject FROM matches m
         JOIN user_identities i ON i.user_id = m.host_user_id
         WHERE m.share_id = $1",
    )
    .bind(created["shareId"].as_str().unwrap())
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(host.as_deref(), Some("host-1"));
}

#[sqlx::test]
async fn cross_site_match_creation_is_refused(pool: PgPool) {
    let mut request = post_json(valid_request());
    request
        .headers_mut()
        .insert(header::ORIGIN, "https://evil.example".parse().unwrap());

    let (status, body) = send(pool.clone(), request).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, json!({ "error": "forbidden_origin" }));
}

#[sqlx::test]
async fn oversized_request_bodies_are_refused(pool: PgPool) {
    let request = with(valid_request(), "venueName", json!("x".repeat(20_000)));

    let (status, body) = send(pool, post_json(request)).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body, json!({ "error": "payload_too_large" }));
}
