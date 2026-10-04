//! Acceptance tests for creating a match through the HTTP API.
//! Matches are created far in the future so the "must start in the future" rule holds.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

async fn send(pool: PgPool, request: Request<Body>) -> (StatusCode, Value) {
    let response = daghep_api::app(pool).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
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
        "2026-10-04T00:00:00Z".parse().unwrap(),
    )
    .unwrap();
    let id = ShareId::parse("k7Qm2xPa").unwrap();
    repo.insert(&id, &new).await.unwrap();

    let second = repo.insert(&id, &new).await;

    assert!(matches!(second, Err(InsertError::DuplicateShareId)));
}

#[sqlx::test]
async fn match_must_start_after_now(pool: PgPool) {
    use std::sync::Arc;

    struct FixedClock;
    impl daghep_api::matches::domain::Clock for FixedClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            "2099-10-10T11:30:00Z".parse().unwrap()
        }
    }

    let response = daghep_api::app_with_clock(pool, Arc::new(FixedClock))
        .oneshot(post_json(valid_request()))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body,
        json!({ "error": "invalid_match", "field": "startsAt" })
    );
}
