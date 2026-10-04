//! Acceptance tests for the walking skeleton: health check and reading a
//! public match by its share id. Each test gets a fresh database with all
//! migrations applied (`#[sqlx::test]`), so tests never share state.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(pool: PgPool, uri: &str) -> (StatusCode, Value) {
    let response = daghep_api::app(pool)
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

async fn insert_match(pool: &PgPool, share_id: &str) {
    sqlx::query(
        "INSERT INTO matches
            (share_id, venue_name, location, starts_at, ends_at, format, match_type,
             level_min_tenths, level_max_tenths, total_fee_vnd, slot_count)
         VALUES
            ($1, 'SSA Sports Center',
             ST_SetSRID(ST_MakePoint(106.7388602, 10.8069529), 4326)::geography,
             '2026-10-10T11:30:00Z', '2026-10-10T13:00:00Z',
             'seven_a_side', 'casual', 25, 35, 900000, 14)",
    )
    .bind(share_id)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn health_returns_ok(pool: PgPool) {
    let (status, body) = get(pool, "/health").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[sqlx::test]
async fn get_match_by_share_id_returns_public_view(pool: PgPool) {
    insert_match(&pool, "k7Qm2xPa").await;

    let (status, body) = get(pool, "/api/matches/k7Qm2xPa").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "shareId": "k7Qm2xPa",
            "venueName": "SSA Sports Center",
            "startsAt": "2026-10-10T11:30:00Z",
            "endsAt": "2026-10-10T13:00:00Z",
            "format": "seven_a_side",
            "matchType": "casual",
            "levelMin": 2.5,
            "levelMax": 3.5,
            "totalFeeVnd": 900000,
            "slotCount": 14,
            "pricePerPlayerVnd": 65000
        })
    );
}

#[sqlx::test]
async fn public_match_view_never_exposes_internal_id(pool: PgPool) {
    insert_match(&pool, "k7Qm2xPa").await;

    let (status, body) = get(pool, "/api/matches/k7Qm2xPa").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object(), "expected a JSON object, got {body}");
    assert!(body.get("id").is_none(), "internal id leaked: {body}");
}

#[sqlx::test]
async fn unknown_share_id_returns_not_found(pool: PgPool) {
    let (status, body) = get(pool, "/api/matches/doesNotExist").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "error": "match_not_found" }));
}

#[sqlx::test]
async fn malformed_share_id_returns_not_found(pool: PgPool) {
    // Same response as an unknown id, so callers cannot probe the id format.
    // That storage is not queried is covered by a unit test in `service.rs`.
    let (status, body) = get(pool, "/api/matches/bad!id").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "error": "match_not_found" }));
}

#[sqlx::test]
async fn public_match_view_is_cacheable_briefly(pool: PgPool) {
    insert_match(&pool, "k7Qm2xPa").await;

    let response = daghep_api::app(pool)
        .oneshot(
            Request::get("/api/matches/k7Qm2xPa")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("public, max-age=30")
    );
}

#[sqlx::test]
async fn not_found_response_is_not_cached(pool: PgPool) {
    let response = daghep_api::app(pool)
        .oneshot(
            Request::get("/api/matches/doesNotExist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.headers().get("cache-control").is_none());
}

#[sqlx::test]
async fn database_rejects_levels_outside_half_steps(pool: PgPool) {
    let result = sqlx::query(
        "INSERT INTO matches
            (share_id, venue_name, starts_at, ends_at, format, match_type,
             level_min_tenths, level_max_tenths, total_fee_vnd, slot_count)
         VALUES ('k7Qm2xPb', 'SSA Sports Center', '2026-10-10T11:30:00Z',
                 '2026-10-10T13:00:00Z', 'seven_a_side', 'casual', 23, 35, 900000, 14)",
    )
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "level 2.3 must be rejected by the database"
    );
}
