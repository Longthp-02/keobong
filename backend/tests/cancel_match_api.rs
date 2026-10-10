//! Acceptance tests for a host cancelling a match.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, ORIGIN};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

struct Match {
    share_id: String,
    host: String,
}

/// A paid match 9 days after `NOW` (kickoff 2099-10-10T11:30Z).
async fn create_match(app: &Router) -> Match {
    let host = sign_in(app, "host").await;
    add_payout(app, &host).await;
    let request = Request::post("/api/matches")
        .header(CONTENT_TYPE, "application/json")
        .header(COOKIE, format!("daghep_session={host}"))
        .body(Body::from(
            json!({
                "venueId": "ssa-amitie",
                "startsAt": "2099-10-10T11:30:00Z",
                "endsAt": "2099-10-10T13:00:00Z",
                "format": "seven_a_side",
                "matchType": "casual",
                "levelMin": 2.5,
                "levelMax": 3.5,
                "totalFeeVnd": 900000
            })
            .to_string(),
        ))
        .unwrap();
    let response = call(app, request).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let share_id = json_body(response).await["shareId"]
        .as_str()
        .unwrap()
        .to_owned();
    Match { share_id, host }
}

fn post(uri: &str, session: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::post(uri).header(CONTENT_TYPE, "application/json");
    if let Some(session) = session {
        builder = builder.header(COOKIE, format!("daghep_session={session}"));
    }
    builder
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = call(app, request).await;
    (response.status(), json_body(response).await)
}

async fn cancel(app: &Router, m: &Match, session: Option<&str>) -> (StatusCode, Value) {
    send(
        app,
        post(
            &format!("/api/matches/{}/cancel", m.share_id),
            session,
            None,
        ),
    )
    .await
}

async fn public_view(app: &Router, m: &Match) -> Value {
    json_body(call(app, get(&format!("/api/matches/{}", m.share_id))).await).await
}

async fn join(app: &Router, m: &Match, session: &str) -> (StatusCode, Value) {
    let uri = format!("/api/matches/{}/slots", m.share_id);
    send(app, post(&uri, Some(session), Some(json!({ "team": "a" })))).await
}

#[sqlx::test]
async fn the_host_cancels_and_everyone_sees_it(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app).await;
    assert_eq!(public_view(&app, &m).await["cancelledAt"], Value::Null);

    assert_eq!(
        cancel(&app, &m, Some(&m.host)).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(public_view(&app, &m).await["cancelledAt"], json!(NOW));
    let roster =
        json_body(call(&app, get(&format!("/api/matches/{}/slots", m.share_id))).await).await;
    assert_eq!(roster["cancelled"], json!(true));
}

#[sqlx::test]
async fn cancelling_again_changes_nothing(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app).await;
    cancel(&app, &m, Some(&m.host)).await;

    clock.advance(Duration::hours(1));
    assert_eq!(
        cancel(&app, &m, Some(&m.host)).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(public_view(&app, &m).await["cancelledAt"], json!(NOW));
}

#[sqlx::test]
async fn only_the_host_can_cancel(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app).await;
    let player = sign_in(&app, "p1").await;

    assert_eq!(
        cancel(&app, &m, Some(&player)).await,
        (StatusCode::FORBIDDEN, json!({ "error": "not_host" }))
    );
    assert_eq!(cancel(&app, &m, None).await.0, StatusCode::UNAUTHORIZED);
    let unknown = send(
        &app,
        post("/api/matches/doesNotExist/cancel", Some(&m.host), None),
    )
    .await;
    assert_eq!(
        unknown,
        (StatusCode::NOT_FOUND, json!({ "error": "match_not_found" }))
    );

    let mut cross_site = post(
        &format!("/api/matches/{}/cancel", m.share_id),
        Some(&m.host),
        None,
    );
    cross_site
        .headers_mut()
        .insert(ORIGIN, "https://evil.example".parse().unwrap());
    assert_eq!(call(&app, cross_site).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(public_view(&app, &m).await["cancelledAt"], Value::Null);
}

#[sqlx::test]
async fn a_match_cannot_be_cancelled_after_kickoff(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app).await;

    clock.advance(Duration::days(9) + Duration::minutes(690));

    assert_eq!(
        cancel(&app, &m, Some(&m.host)).await,
        (StatusCode::CONFLICT, json!({ "error": "match_started" }))
    );
}

#[sqlx::test]
async fn a_cancelled_match_takes_no_places_and_no_payments(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app).await;
    let p1 = sign_in(&app, "p1").await;
    let p2 = sign_in(&app, "p2").await;
    join(&app, &m, &p1).await;
    let mine_uri = format!("/api/matches/{}/slots/mine", m.share_id);
    let code = json_body(
        call(
            &app,
            get_with_cookie(&mine_uri, &format!("daghep_session={p1}")),
        )
        .await,
    )
    .await["paymentCode"]
        .clone();

    cancel(&app, &m, Some(&m.host)).await;

    let cancelled = (StatusCode::CONFLICT, json!({ "error": "match_cancelled" }));
    assert_eq!(join(&app, &m, &p2).await, cancelled);
    let report = format!("/api/matches/{}/slots/mine/report-payment", m.share_id);
    assert_eq!(send(&app, post(&report, Some(&p1), None)).await, cancelled);
    for action in ["confirm", "reject"] {
        let uri = format!("/api/matches/{}/payments/{code}/{action}", m.share_id);
        assert_eq!(
            send(&app, post(&uri, Some(&m.host), None)).await,
            cancelled,
            "{action}"
        );
    }
    // Nobody is asked to pay for a cancelled match.
    let own = json_body(
        call(
            &app,
            get_with_cookie(&mine_uri, &format!("daghep_session={p1}")),
        )
        .await,
    )
    .await;
    assert_eq!(
        (&own["joined"], &own["payment"]),
        (&json!(true), &Value::Null)
    );
    // The host still sees who paid, to arrange refunds.
    let list = json_body(
        call(
            &app,
            get_with_cookie(
                &format!("/api/matches/{}/payments", m.share_id),
                &format!("daghep_session={}", m.host),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(list["parties"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn cancelling_freezes_places_so_the_host_can_refund_everyone(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1).await;
    cancel(&app, &m, Some(&m.host)).await;

    // Past the 30-minute hold: the party must not disappear from the refund list.
    clock.advance(Duration::minutes(31));

    let list_uri = format!("/api/matches/{}/payments", m.share_id);
    let list = json_body(
        call(
            &app,
            get_with_cookie(&list_uri, &format!("daghep_session={}", m.host)),
        )
        .await,
    )
    .await;
    assert_eq!(list["parties"].as_array().unwrap().len(), 1);
    let mine_uri = format!("/api/matches/{}/slots/mine", m.share_id);
    let own = json_body(
        call(
            &app,
            get_with_cookie(&mine_uri, &format!("daghep_session={p1}")),
        )
        .await,
    )
    .await;
    assert_eq!(own["joined"], json!(true));
    let roster =
        json_body(call(&app, get(&format!("/api/matches/{}/slots", m.share_id))).await).await;
    assert_eq!(roster["teams"][0]["players"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn nobody_leaves_a_cancelled_match(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1).await;
    cancel(&app, &m, Some(&m.host)).await;

    let leave = Request::delete(format!("/api/matches/{}/slots/mine", m.share_id))
        .header(COOKIE, format!("daghep_session={p1}"))
        .body(Body::empty())
        .unwrap();

    assert_eq!(
        send(&app, leave).await,
        (StatusCode::CONFLICT, json!({ "error": "match_cancelled" }))
    );
}
