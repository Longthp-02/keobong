//! Acceptance tests for taking and releasing places through the HTTP API.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, ORIGIN};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

/// Creates a match as a fresh host and returns its share id.
async fn create_match(app: &Router, slot_count: u32) -> String {
    let host = sign_in(app, "host").await;
    add_payout(app, &host).await;
    let request = Request::post("/api/matches")
        .header(CONTENT_TYPE, "application/json")
        .header(COOKIE, format!("daghep_session={host}"))
        .body(Body::from(
            json!({
                "venueName": "SSA Sports Center",
                "startsAt": "2099-10-10T11:30:00Z",
                "endsAt": "2099-10-10T13:00:00Z",
                "format": "seven_a_side",
                "matchType": "casual",
                "levelMin": 2.5,
                "levelMax": 3.5,
                "totalFeeVnd": 900000,
                "slotCount": slot_count
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

fn join(share_id: &str, session: &str, body: Value) -> Request<Body> {
    Request::post(format!("/api/matches/{share_id}/slots"))
        .header(CONTENT_TYPE, "application/json")
        .header(COOKIE, format!("daghep_session={session}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn leave(share_id: &str, session: &str) -> Request<Body> {
    Request::delete(format!("/api/matches/{share_id}/slots/mine"))
        .header(COOKIE, format!("daghep_session={session}"))
        .body(Body::empty())
        .unwrap()
}

async fn roster(app: &Router, share_id: &str) -> Value {
    let response = call(app, get(&format!("/api/matches/{share_id}/slots"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn mine(app: &Router, share_id: &str, session: &str) -> (StatusCode, Value) {
    let response = call(
        app,
        get_with_cookie(
            &format!("/api/matches/{share_id}/slots/mine"),
            &format!("daghep_session={session}"),
        ),
    )
    .await;
    (response.status(), json_body(response).await)
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = call(app, request).await;
    (response.status(), json_body(response).await)
}

#[sqlx::test]
async fn a_new_match_has_two_empty_teams_splitting_the_places(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 15).await;

    let response = call(&app, get(&format!("/api/matches/{share_id}/slots"))).await;

    assert_eq!(response.headers()["cache-control"], "public, max-age=30");
    assert_eq!(
        json_body(response).await,
        json!({ "teams": [
            { "team": "a", "capacity": 8, "players": [] },
            { "team": "b", "capacity": 7, "players": [] }
        ], "cancelled": false })
    );
}

#[sqlx::test]
async fn a_player_takes_a_place_with_named_guests(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;

    let (status, body) = send(
        &app,
        join(
            &share_id,
            &player,
            json!({ "team": "b", "guests": [" An ", "Bình"] }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        (&body["joined"], &body["team"], &body["guests"]),
        (&json!(true), &json!("b"), &json!(["An", "Bình"]))
    );
    assert_eq!(
        roster(&app, &share_id).await["teams"][1],
        json!({ "team": "b", "capacity": 9, "players": [
            { "name": "Player p1", "avatarUrl": "https://lh3.googleusercontent.com/a/p1",
              "isGuest": false, "guestOf": null },
            { "name": "An", "avatarUrl": null, "isGuest": true, "guestOf": "Player p1" },
            { "name": "Bình", "avatarUrl": null, "isGuest": true, "guestOf": "Player p1" }
        ]})
    );
    let (status, own) = mine(&app, &share_id, &player).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&own["joined"], &own["team"], &own["guests"]),
        (&json!(true), &json!("b"), &json!(["An", "Bình"]))
    );
}

#[sqlx::test]
async fn own_place_is_private_and_requires_sign_in(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;

    let (status, body) = mine(&app, &share_id, &player).await;
    assert_eq!((status, body), (StatusCode::OK, json!({ "joined": false })));

    let anonymous = call(&app, get(&format!("/api/matches/{share_id}/slots/mine"))).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let response = call(
        &app,
        get_with_cookie(
            &format!("/api/matches/{share_id}/slots/mine"),
            &format!("daghep_session={player}"),
        ),
    )
    .await;
    assert_eq!(response.headers()["cache-control"], "private, no-store");
}

#[sqlx::test]
async fn joining_requires_sign_in(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let request = Request::post(format!("/api/matches/{share_id}/slots"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "team": "a" }).to_string()))
        .unwrap();

    let (status, body) = send(&app, request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "error": "unauthenticated" }));
}

#[sqlx::test]
async fn a_player_cannot_join_twice(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;
    send(&app, join(&share_id, &player, json!({ "team": "a" }))).await;

    for team in ["a", "b"] {
        let (status, body) = send(&app, join(&share_id, &player, json!({ "team": team }))).await;

        assert_eq!(status, StatusCode::CONFLICT, "{team}");
        assert_eq!(body, json!({ "error": "already_joined" }), "{team}");
    }
}

#[sqlx::test]
async fn a_full_team_or_too_big_a_party_is_refused(pool: PgPool) {
    let app = app(pool);
    // 5 places: team A 3, team B 2.
    let share_id = create_match(&app, 5).await;
    let p1 = sign_in(&app, "p1").await;
    let p2 = sign_in(&app, "p2").await;
    let p3 = sign_in(&app, "p3").await;

    let (status, _) = send(
        &app,
        join(&share_id, &p1, json!({ "team": "a", "guests": ["An"] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Team A has 1 place left: a party of 2 does not fit, and nobody is added.
    let (status, body) = send(
        &app,
        join(&share_id, &p2, json!({ "team": "a", "guests": ["Bình"] })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "team_full" }))
    );
    assert_eq!(
        roster(&app, &share_id).await["teams"][0]["players"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let (status, _) = send(&app, join(&share_id, &p2, json!({ "team": "a" }))).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = send(&app, join(&share_id, &p3, json!({ "team": "a" }))).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "team_full" }))
    );
    let (status, _) = send(&app, join(&share_id, &p3, json!({ "team": "b" }))).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[sqlx::test]
async fn invalid_join_requests_are_rejected(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;

    let cases = [
        (json!({ "team": "c" }), "team"),
        (json!({ "team": "a", "guests": ["A", "B", "C"] }), "guests"),
        (json!({ "team": "a", "guests": ["  "] }), "guests"),
        (json!({ "team": "a", "guests": ["x".repeat(41)] }), "guests"),
    ];
    for (body, field) in cases {
        let (status, response) = send(&app, join(&share_id, &player, body.clone())).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(
            response,
            json!({ "error": "invalid_slot_request", "field": field }),
            "{body}"
        );
    }
    let (status, _) = send(&app, join(&share_id, &player, json!({ "guests": [] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn unknown_matches_are_not_found(pool: PgPool) {
    let app = app(pool);
    let player = sign_in(&app, "p1").await;

    for share_id in ["doesNotExist", "bad!id"] {
        let roster = call(&app, get(&format!("/api/matches/{share_id}/slots"))).await;
        assert_eq!(roster.status(), StatusCode::NOT_FOUND, "{share_id}");
        let (status, _) = send(&app, join(share_id, &player, json!({ "team": "a" }))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{share_id}");
        let (status, _) = mine(&app, share_id, &player).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{share_id}");
        let (status, _) = send(&app, leave(share_id, &player)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{share_id}");
    }
}

#[sqlx::test]
async fn leaving_frees_the_place_and_the_guests(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;
    send(
        &app,
        join(&share_id, &player, json!({ "team": "a", "guests": ["An"] })),
    )
    .await;

    let (status, _) = send(&app, leave(&share_id, &player)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        roster(&app, &share_id).await["teams"][0]["players"],
        json!([])
    );
    assert_eq!(
        mine(&app, &share_id, &player).await.1,
        json!({ "joined": false })
    );
    // Leaving again is harmless, and the player can come back.
    assert_eq!(
        send(&app, leave(&share_id, &player)).await.0,
        StatusCode::NO_CONTENT
    );
    let (status, _) = send(&app, join(&share_id, &player, json!({ "team": "b" }))).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[sqlx::test]
async fn places_are_fixed_once_the_match_starts(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let share_id = create_match(&app, 18).await;
    let p1 = sign_in(&app, "p1").await;
    let p2 = sign_in(&app, "p2").await;
    send(&app, join(&share_id, &p1, json!({ "team": "a" }))).await;

    // Kickoff is 2099-10-10T11:30Z; NOW is 2099-10-01T00:00Z.
    clock.advance(Duration::days(9) + Duration::minutes(690));

    let (status, body) = send(&app, join(&share_id, &p2, json!({ "team": "a" }))).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "match_started" }))
    );
    let (status, body) = send(&app, leave(&share_id, &p1)).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "match_started" }))
    );

    // One second before kickoff, leaving was still possible.
    clock.advance(-Duration::seconds(1));
    let (status, _) = send(&app, leave(&share_id, &p1)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn oversized_request_bodies_are_refused(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;
    let huge: Vec<String> = (0..10_000).map(|i| format!("guest-{i}")).collect();

    let (status, _) = send(
        &app,
        join(&share_id, &player, json!({ "team": "a", "guests": huge })),
    )
    .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[sqlx::test]
async fn concurrent_joins_never_overbook_a_team(pool: PgPool) {
    let app = app(pool.clone());
    // 4 places: 2 per team.
    let share_id = create_match(&app, 4).await;
    let mut sessions = Vec::new();
    for i in 0..4 {
        sessions.push(sign_in(&app, &format!("racer-{i}")).await);
    }
    // Four racers plus this gate use all 5 connections of the sqlx test pool;
    // more racers would wait for a connection until the pool times out.
    // Make the race certain: while this lock is held, every join can read and
    // count places but no join can insert. Without the claim's lock on the
    // match, they would all count "0 taken" and then all insert.
    let mut gate = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE slots IN SHARE MODE")
        .execute(&mut *gate)
        .await
        .unwrap();

    let tasks: Vec<_> = sessions
        .into_iter()
        .map(|session| {
            let app = app.clone();
            let request = join(&share_id, &session, json!({ "team": "a" }));
            tokio::spawn(async move { call(&app, request).await.status() })
        })
        .collect();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    gate.commit().await.unwrap();
    let mut statuses = Vec::new();
    for task in tasks {
        statuses.push(task.await.unwrap());
    }

    let created = statuses
        .iter()
        .filter(|s| **s == StatusCode::CREATED)
        .count();
    let full = statuses
        .iter()
        .filter(|s| **s == StatusCode::CONFLICT)
        .count();
    assert_eq!((created, full), (2, 2), "{statuses:?}");
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM slots WHERE team = 'a' AND released_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active, 2);
}

#[sqlx::test]
async fn cross_site_joins_are_refused(pool: PgPool) {
    let app = app(pool);
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;
    let mut request = join(&share_id, &player, json!({ "team": "a" }));
    request
        .headers_mut()
        .insert(ORIGIN, "https://evil.example".parse().unwrap());

    let (status, _) = send(&app, request).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn the_database_allows_one_own_place_per_player(pool: PgPool) {
    let app = app(pool.clone());
    let share_id = create_match(&app, 18).await;
    let user = insert_user(&pool).await;
    let insert = "INSERT INTO slots (match_id, team, holder_user_id, claimed_at, payment_status)
                  SELECT id, 'a', $2, now(), 'confirmed' FROM matches WHERE share_id = $1";

    sqlx::query(insert)
        .bind(&share_id)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let second = sqlx::query(insert)
        .bind(&share_id)
        .bind(user)
        .execute(&pool)
        .await;

    assert!(
        second.is_err(),
        "a second own place must violate the unique index"
    );
}

#[sqlx::test]
async fn joins_wait_for_the_match_lock(pool: PgPool) {
    let app = app(pool.clone());
    let share_id = create_match(&app, 18).await;
    let player = sign_in(&app, "p1").await;
    // NO KEY UPDATE conflicts with the claim's own NO KEY UPDATE but not with
    // the KEY SHARE lock a slot insert's foreign key takes, so only the
    // claim's lock can make the join wait.
    let mut other = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM matches WHERE share_id = $1 FOR NO KEY UPDATE")
        .bind(&share_id)
        .execute(&mut *other)
        .await
        .unwrap();

    let request = join(&share_id, &player, json!({ "team": "a" }));
    let pending = {
        let app = app.clone();
        tokio::spawn(async move { call(&app, request).await.status() })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !pending.is_finished(),
        "a join must wait while another transaction holds the match"
    );

    other.rollback().await.unwrap();
    assert_eq!(pending.await.unwrap(), StatusCode::CREATED);
}
