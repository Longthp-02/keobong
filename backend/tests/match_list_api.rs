//! Acceptance tests for the public list of upcoming matches with open places.
//! "Now" is 2099-10-01T00:00Z, 07:00 on 1 October in Ho Chi Minh City, so the
//! seven listed days run from 1 to 7 October (local).

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

struct Host {
    session: String,
}

async fn host(app: &Router) -> Host {
    let session = sign_in(app, "host").await;
    add_payout(app, &session).await;
    Host { session }
}

/// Creates a free 90-minute match and returns its share id.
async fn create(
    app: &Router,
    host: &Host,
    venue: &str,
    starts_at: &str,
    match_type: &str,
    slots: u32,
) -> String {
    let start: chrono::DateTime<chrono::Utc> = starts_at.parse().unwrap();
    let request = Request::post("/api/matches")
        .header(CONTENT_TYPE, "application/json")
        .header(COOKIE, format!("daghep_session={}", host.session))
        .body(Body::from(
            json!({
                "venueId": venue,
                "startsAt": start,
                "endsAt": start + Duration::minutes(90),
                "format": "five_a_side",
                "matchType": match_type,
                "levelMin": 2.5,
                "levelMax": 3.5,
                "totalFeeVnd": 0,
                "slotCount": slots
            })
            .to_string(),
        ))
        .unwrap();
    let response = call(app, request).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["shareId"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn join(app: &Router, share_id: &str, subject: &str, team: &str, guests: Value) {
    let session = sign_in(app, subject).await;
    let request = Request::post(format!("/api/matches/{share_id}/slots"))
        .header(CONTENT_TYPE, "application/json")
        .header(COOKIE, format!("daghep_session={session}"))
        .body(Body::from(
            json!({ "team": team, "guests": guests }).to_string(),
        ))
        .unwrap();
    assert_eq!(call(app, request).await.status(), StatusCode::CREATED);
}

async fn list(app: &Router, query: &str) -> (StatusCode, Value) {
    let response = call(app, get(&format!("/api/matches{query}"))).await;
    let status = response.status();
    (status, json_body(response).await)
}

fn share_ids(body: &Value) -> Vec<String> {
    body["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .map(|m| m["shareId"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test]
async fn lists_upcoming_matches_in_kickoff_order_with_venue_and_open_places(pool: PgPool) {
    let app = app(pool);
    let host = host(&app).await;
    let later = create(
        &app,
        &host,
        "an-phu-nguyen-hoang",
        "2099-10-02T12:00:00Z",
        "casual",
        10,
    )
    .await;
    let earlier = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-02T11:00:00Z",
        "competitive",
        10,
    )
    .await;
    join(&app, &earlier, "player", "a", json!(["An", "Bình"])).await;

    let response = call(&app, get("/api/matches")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CACHE_CONTROL], "public, max-age=30");
    let body = json_body(response).await;

    assert_eq!(share_ids(&body), vec![earlier.clone(), later]);
    let first = &body["matches"][0];
    assert_eq!(first["venueName"], "SSA Sports Center (Amitie Thảo Điền)");
    assert_eq!(first["venueAddress"], "28 Duyên Hải, An Khánh");
    assert_eq!(first["matchType"], "competitive");
    // One player with two guests took three of ten places.
    assert_eq!(first["placesLeft"], 7);
    assert_eq!(first["distanceM"], Value::Null);
    assert_eq!(body["nextCursor"], Value::Null);
}

#[sqlx::test]
async fn leaves_out_full_cancelled_and_later_matches(pool: PgPool) {
    let app = app(pool);
    let host = host(&app).await;
    let open = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-03T11:00:00Z",
        "casual",
        10,
    )
    .await;
    let full = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-03T12:00:00Z",
        "casual",
        2,
    )
    .await;
    join(&app, &full, "p1", "a", json!([])).await;
    join(&app, &full, "p2", "b", json!([])).await;
    let cancelled = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-03T13:00:00Z",
        "casual",
        10,
    )
    .await;
    let cancel = Request::post(format!("/api/matches/{cancelled}/cancel"))
        .header(COOKIE, format!("daghep_session={}", host.session))
        .body(Body::empty())
        .unwrap();
    assert_eq!(call(&app, cancel).await.status(), StatusCode::NO_CONTENT);
    // 8 October, 10:00 local: past the seven listed days.
    create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-08T03:00:00Z",
        "casual",
        10,
    )
    .await;
    // 7 October, 21:00 local: the last listed day.
    let last_day = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-07T14:00:00Z",
        "casual",
        10,
    )
    .await;

    let (status, body) = list(&app, "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(share_ids(&body), vec![open, last_day]);
}

#[sqlx::test]
async fn leaves_out_matches_that_have_started(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let host = host(&app).await;
    let soon = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-01T02:00:00Z",
        "casual",
        10,
    )
    .await;
    let later = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-01T05:00:00Z",
        "casual",
        10,
    )
    .await;

    clock.advance(Duration::hours(2));
    let (_, body) = list(&app, "").await;

    assert_eq!(share_ids(&body), vec![later]);
    assert!(!share_ids(&body).contains(&soon));
}

#[sqlx::test]
async fn filters_by_local_day_and_match_type(pool: PgPool) {
    let app = app(pool);
    let host = host(&app).await;
    // 3 October 06:00 local is still 2 October in UTC.
    let early = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-02T23:00:00Z",
        "beginner_friendly",
        10,
    )
    .await;
    let evening = create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-03T12:00:00Z",
        "casual",
        10,
    )
    .await;
    create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-02T12:00:00Z",
        "casual",
        10,
    )
    .await;

    let (status, body) = list(&app, "?date=2099-10-03").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(share_ids(&body), vec![early.clone(), evening]);

    let (_, body) = list(&app, "?date=2099-10-03&type=beginner_friendly").await;
    assert_eq!(share_ids(&body), vec![early]);
}

#[sqlx::test]
async fn shows_the_distance_from_the_visitor(pool: PgPool) {
    let app = app(pool);
    let host = host(&app).await;
    create(
        &app,
        &host,
        "ssa-amitie",
        "2099-10-02T11:00:00Z",
        "casual",
        10,
    )
    .await;
    create(
        &app,
        &host,
        "khu-the-thao-an-phu",
        "2099-10-02T12:00:00Z",
        "casual",
        10,
    )
    .await;

    // Standing at SSA Sports Center.
    let (status, body) = list(&app, "?lat=10.806938&lng=106.738812").await;

    assert_eq!(status, StatusCode::OK);
    let here = body["matches"][0]["distanceM"].as_i64().expect("distance");
    let there = body["matches"][1]["distanceM"].as_i64().expect("distance");
    assert!(here < 20, "distance at the venue was {here} m");
    // About 0.0025° north and 0.0173° east: roughly 1.9 km.
    assert!(
        (1_800..2_000).contains(&there),
        "distance to An Phu was {there} m"
    );
}

#[sqlx::test]
async fn pages_with_a_cursor(pool: PgPool) {
    let app = app(pool);
    let host = host(&app).await;
    let mut created = Vec::new();
    for minute in 0..21 {
        let start = format!("2099-10-02T11:{minute:02}:00Z");
        created.push(create(&app, &host, "ssa-amitie", &start, "casual", 10).await);
    }

    let (_, first) = list(&app, "").await;
    assert_eq!(share_ids(&first), created[..20].to_vec());
    let cursor = first["nextCursor"]
        .as_str()
        .expect("next cursor")
        .to_owned();

    let (status, second) = list(&app, &format!("?cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(share_ids(&second), created[20..].to_vec());
    assert_eq!(second["nextCursor"], Value::Null);
}

#[sqlx::test]
async fn rejects_bad_query_parameters_by_field(pool: PgPool) {
    let app = app(pool);
    for (query, field) in [
        ("?date=2099-09-30", "date"),
        ("?date=2099-10-08", "date"),
        ("?date=tomorrow", "date"),
        ("?type=friendly", "type"),
        ("?lat=10.8", "near"),
        ("?lat=91&lng=106.7", "near"),
        ("?lat=abc&lng=106.7", "near"),
        ("?cursor=not-a-cursor", "cursor"),
    ] {
        let (status, body) = list(&app, query).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert_eq!(
            body,
            json!({ "error": "invalid_query", "field": field }),
            "{query}"
        );
    }
}
