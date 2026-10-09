//! Acceptance tests for paying the host: VietQR instructions, the 30-minute
//! hold, reporting a transfer, and the host's confirm/reject.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

struct Match {
    share_id: String,
    host: String,
}

/// Creates a match 9 days after `NOW`; 900,000 VND over 18 places is 50,000 VND each.
async fn create_match(app: &Router, total_fee_vnd: i64) -> Match {
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
                "totalFeeVnd": total_fee_vnd,
                "slotCount": 18
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

fn post(uri: &str, session: &str, body: Option<Value>) -> Request<Body> {
    Request::post(uri)
        .header(COOKIE, format!("daghep_session={session}"))
        .header(CONTENT_TYPE, "application/json")
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = call(app, request).await;
    (response.status(), json_body(response).await)
}

async fn join(app: &Router, m: &Match, session: &str, guests: Value) -> (StatusCode, Value) {
    let uri = format!("/api/matches/{}/slots", m.share_id);
    send(
        app,
        post(
            &uri,
            session,
            Some(json!({ "team": "a", "guests": guests })),
        ),
    )
    .await
}

async fn mine(app: &Router, m: &Match, session: &str) -> Value {
    let uri = format!("/api/matches/{}/slots/mine", m.share_id);
    let response = call(
        app,
        get_with_cookie(&uri, &format!("daghep_session={session}")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn report(app: &Router, m: &Match, session: &str) -> (StatusCode, Value) {
    let uri = format!("/api/matches/{}/slots/mine/report-payment", m.share_id);
    send(app, post(&uri, session, None)).await
}

async fn host_list(app: &Router, m: &Match, session: &str) -> (StatusCode, Value) {
    let uri = format!("/api/matches/{}/payments", m.share_id);
    let response = call(
        app,
        get_with_cookie(&uri, &format!("daghep_session={session}")),
    )
    .await;
    (response.status(), json_body(response).await)
}

async fn host_action(
    app: &Router,
    m: &Match,
    session: &str,
    code: &Value,
    action: &str,
) -> (StatusCode, Value) {
    // Numbers as digits, strings as they are (no JSON quotes in the URL).
    let code = match code {
        Value::String(raw) => raw.clone(),
        other => other.to_string(),
    };
    let uri = format!("/api/matches/{}/payments/{code}/{action}", m.share_id);
    send(app, post(&uri, session, None)).await
}

async fn roster_names(app: &Router, m: &Match) -> Vec<String> {
    let response = call(app, get(&format!("/api/matches/{}/slots", m.share_id))).await;
    let body = json_body(response).await;
    body["teams"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|team| team["players"].as_array().unwrap().clone())
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test]
async fn joining_a_paid_match_shows_vietqr_for_the_whole_party(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let player = sign_in(&app, "p1").await;

    let (status, body) = join(&app, &m, &player, json!(["An"])).await;

    assert_eq!(status, StatusCode::CREATED);
    let code = body["paymentCode"].as_i64().unwrap();
    let memo = format!("DAGHEP {code}");
    assert_eq!(body["paymentStatus"], json!("awaiting_payment"));
    assert_eq!(body["holdExpiresAt"], json!("2099-10-01T00:30:00Z"));
    assert_eq!(body["amountVnd"], json!(100_000));
    let payment = &body["payment"];
    assert_eq!(
        (
            &payment["bankName"],
            &payment["accountNumber"],
            &payment["accountName"]
        ),
        (&json!("ACB"), &json!("257678859"), &json!("PHAM LONG"))
    );
    assert_eq!(
        (&payment["amountVnd"], &payment["memo"]),
        (&json!(100_000), &json!(memo))
    );
    let qr = payment["qrPayload"].as_str().unwrap();
    assert!(qr.starts_with("000201010212"), "{qr}");
    assert!(qr.contains("5406100000"), "amount in the QR: {qr}");
    assert!(qr.contains(&memo), "memo in the QR: {qr}");
}

#[sqlx::test]
async fn bank_details_never_appear_in_public_views(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let player = sign_in(&app, "p1").await;
    join(&app, &m, &player, json!([])).await;

    for uri in [
        format!("/api/matches/{}", m.share_id),
        format!("/api/matches/{}/slots", m.share_id),
    ] {
        let response = call(&app, get(&uri)).await;
        let text = json_body(response).await.to_string();
        assert!(!text.contains("257678859"), "{uri}: {text}");
        assert!(!text.contains("PHAM LONG"), "{uri}: {text}");
    }
}

#[sqlx::test]
async fn an_unpaid_hold_is_released_after_thirty_minutes(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app, 900_000).await;
    let player = sign_in(&app, "p1").await;
    join(&app, &m, &player, json!(["An"])).await;

    clock.advance(Duration::minutes(29));
    assert_eq!(roster_names(&app, &m).await, ["Player p1", "An"]);

    clock.advance(Duration::minutes(1));
    assert!(roster_names(&app, &m).await.is_empty());
    assert_eq!(mine(&app, &m, &player).await, json!({ "joined": false }));
    // The released place can be taken again, also by the same player.
    let (status, _) = join(&app, &m, &player, json!([])).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[sqlx::test]
async fn reporting_a_transfer_stops_the_countdown(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app, 900_000).await;
    let player = sign_in(&app, "p1").await;
    join(&app, &m, &player, json!(["An"])).await;

    let (status, body) = report(&app, &m, &player).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["paymentStatus"], json!("payment_reported"));
    assert_eq!(body["holdExpiresAt"], Value::Null);
    assert!(
        body["payment"].is_object(),
        "details stay visible until the host confirms"
    );
    clock.advance(Duration::hours(3));
    assert_eq!(roster_names(&app, &m).await, ["Player p1", "An"]);
    // Reporting again is harmless.
    assert_eq!(report(&app, &m, &player).await.0, StatusCode::OK);
}

#[sqlx::test]
async fn reporting_needs_a_place(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let player = sign_in(&app, "p1").await;

    assert_eq!(
        report(&app, &m, &player).await,
        (StatusCode::CONFLICT, json!({ "error": "not_joined" }))
    );
}

#[sqlx::test]
async fn the_host_sees_every_party_and_confirms_payment(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    let p2 = sign_in(&app, "p2").await;
    join(&app, &m, &p1, json!(["An"])).await;
    join(&app, &m, &p2, json!([])).await;
    report(&app, &m, &p1).await;

    let (status, list) = host_list(&app, &m, &m.host).await;

    assert_eq!(status, StatusCode::OK);
    let parties = list["parties"].as_array().unwrap();
    assert_eq!(parties.len(), 2);
    assert_eq!(
        (
            &parties[0]["holderName"],
            &parties[0]["guests"],
            &parties[0]["amountVnd"],
            &parties[0]["paymentStatus"]
        ),
        (
            &json!("Player p1"),
            &json!(["An"]),
            &json!(100_000),
            &json!("payment_reported")
        )
    );
    assert_eq!(parties[1]["paymentStatus"], json!("awaiting_payment"));
    assert_eq!(parties[1]["holdExpiresAt"], json!("2099-10-01T00:30:00Z"));

    let code = parties[0]["paymentCode"].clone();
    assert_eq!(
        host_action(&app, &m, &m.host, &code, "confirm").await.0,
        StatusCode::NO_CONTENT
    );
    let own = mine(&app, &m, &p1).await;
    assert_eq!(own["paymentStatus"], json!("confirmed"));
    assert_eq!(own["payment"], Value::Null);
}

#[sqlx::test]
async fn the_host_can_report_a_missing_transfer_which_frees_the_places(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    let p2 = sign_in(&app, "p2").await;
    join(&app, &m, &p1, json!(["An"])).await;
    join(&app, &m, &p2, json!([])).await;
    report(&app, &m, &p1).await;
    let (_, list) = host_list(&app, &m, &m.host).await;
    let first = list["parties"][0]["paymentCode"].clone();
    let second = list["parties"][1]["paymentCode"].clone();

    assert_eq!(
        host_action(&app, &m, &m.host, &first, "reject").await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(roster_names(&app, &m).await, ["Player p2"]);

    host_action(&app, &m, &m.host, &second, "confirm").await;
    assert_eq!(
        host_action(&app, &m, &m.host, &second, "reject").await,
        (
            StatusCode::CONFLICT,
            json!({ "error": "already_confirmed" })
        )
    );
}

#[sqlx::test]
async fn only_the_host_manages_payments(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1, json!([])).await;
    let code = mine(&app, &m, &p1).await["paymentCode"].clone();

    assert_eq!(
        host_list(&app, &m, &p1).await,
        (StatusCode::FORBIDDEN, json!({ "error": "not_host" }))
    );
    for action in ["confirm", "reject"] {
        assert_eq!(
            host_action(&app, &m, &p1, &code, action).await,
            (StatusCode::FORBIDDEN, json!({ "error": "not_host" })),
            "{action}"
        );
    }
    let anonymous = call(&app, get(&format!("/api/matches/{}/payments", m.share_id))).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        mine(&app, &m, &p1).await["paymentStatus"],
        json!("awaiting_payment")
    );
}

#[sqlx::test]
async fn payment_codes_from_other_matches_are_not_found(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let other = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &other, &p1, json!([])).await;
    let foreign_code = mine(&app, &other, &p1).await["paymentCode"].clone();

    for code in [foreign_code, json!(999_999), json!("abc")] {
        assert_eq!(
            host_action(&app, &m, &m.host, &code, "confirm").await,
            (StatusCode::NOT_FOUND, json!({ "error": "party_not_found" })),
            "{code}"
        );
    }
}

#[sqlx::test]
async fn free_matches_and_the_hosts_own_party_need_no_transfer(pool: PgPool) {
    let app = app(pool);
    let free = create_match(&app, 0).await;
    let player = sign_in(&app, "p1").await;

    let (_, body) = join(&app, &free, &player, json!(["An"])).await;
    assert_eq!(
        (
            &body["paymentStatus"],
            &body["holdExpiresAt"],
            &body["amountVnd"],
            &body["payment"]
        ),
        (&json!("confirmed"), &Value::Null, &json!(0), &Value::Null)
    );

    let paid = create_match(&app, 900_000).await;
    let (_, body) = join(&app, &paid, &paid.host, json!([])).await;
    assert_eq!(
        (&body["paymentStatus"], &body["payment"]),
        (&json!("confirmed"), &Value::Null)
    );
}

#[sqlx::test]
async fn an_expired_hold_can_no_longer_be_reported_or_managed(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1, json!([])).await;
    let code = mine(&app, &m, &p1).await["paymentCode"].clone();

    clock.advance(Duration::minutes(30));

    assert_eq!(
        report(&app, &m, &p1).await,
        (StatusCode::CONFLICT, json!({ "error": "not_joined" }))
    );
    for action in ["confirm", "reject"] {
        assert_eq!(
            host_action(&app, &m, &m.host, &code, action).await,
            (StatusCode::NOT_FOUND, json!({ "error": "party_not_found" })),
            "{action}"
        );
    }
    assert_eq!(
        host_list(&app, &m, &m.host).await.1,
        json!({ "parties": [] })
    );
}

#[sqlx::test]
async fn a_place_freed_by_an_expired_hold_goes_to_the_next_player(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app, 900_000).await;
    // Fill team A (9 places): one party of 3 that will expire, six that pay.
    let late = sign_in(&app, "late").await;
    join(&app, &m, &late, json!(["G1", "G2"])).await;
    for i in 0..6 {
        let player = sign_in(&app, &format!("payer-{i}")).await;
        join(&app, &m, &player, json!([])).await;
        report(&app, &m, &player).await;
    }
    let next = sign_in(&app, "next").await;
    assert_eq!(
        join(&app, &m, &next, json!([])).await.0,
        StatusCode::CONFLICT
    );

    clock.advance(Duration::minutes(31));

    assert_eq!(
        join(&app, &m, &next, json!(["G3", "G4"])).await.0,
        StatusCode::CREATED
    );
}

#[sqlx::test]
async fn the_host_can_reject_a_reported_transfer(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1, json!([])).await;
    report(&app, &m, &p1).await;
    let code = mine(&app, &m, &p1).await["paymentCode"].clone();

    assert_eq!(
        host_action(&app, &m, &m.host, &code, "reject").await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(mine(&app, &m, &p1).await, json!({ "joined": false }));
}

#[sqlx::test]
async fn players_get_no_payment_details_when_the_hosts_bank_is_unsupported(pool: PgPool) {
    let app = app(pool.clone());
    let m = create_match(&app, 900_000).await;
    sqlx::query("UPDATE payout_accounts SET bank_bin = '999999'")
        .execute(&pool)
        .await
        .unwrap();
    let p1 = sign_in(&app, "p1").await;

    let (status, body) = join(&app, &m, &p1, json!([])).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["payment"], Value::Null);
}

#[sqlx::test]
async fn a_transfer_can_be_rejected_only_after_the_player_reports_it(pool: PgPool) {
    let app = app(pool);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1, json!([])).await;
    let code = mine(&app, &m, &p1).await["paymentCode"].clone();

    // Still inside the 30-minute hold: the countdown handles unpaid places.
    assert_eq!(
        host_action(&app, &m, &m.host, &code, "reject").await,
        (StatusCode::CONFLICT, json!({ "error": "not_reported" }))
    );
    assert_eq!(mine(&app, &m, &p1).await["joined"], json!(true));
}

#[sqlx::test]
async fn a_transfer_cannot_be_rejected_after_kickoff(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let m = create_match(&app, 900_000).await;
    let p1 = sign_in(&app, "p1").await;
    join(&app, &m, &p1, json!([])).await;
    report(&app, &m, &p1).await;
    let code = mine(&app, &m, &p1).await["paymentCode"].clone();

    // Kickoff is 2099-10-10T11:30Z; NOW is 2099-10-01T00:00Z.
    clock.advance(Duration::days(9) + Duration::minutes(690));

    assert_eq!(
        host_action(&app, &m, &m.host, &code, "reject").await,
        (StatusCode::CONFLICT, json!({ "error": "match_started" }))
    );
    // Confirming a late transfer is still possible.
    assert_eq!(
        host_action(&app, &m, &m.host, &code, "confirm").await.0,
        StatusCode::NO_CONTENT
    );
}
