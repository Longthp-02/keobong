//! Acceptance tests for the bank list and payout accounts.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, ORIGIN};
use axum::http::{Request, StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

fn put_payout(session: &str, body: Value) -> Request<Body> {
    Request::put("/api/me/payout")
        .header(COOKIE, format!("daghep_session={session}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn account() -> Value {
    json!({ "bankBin": "970436", "accountNumber": "0123456789", "accountName": "PHAM TRINH HOANG LONG" })
}

#[sqlx::test]
async fn the_bank_list_is_public_and_cacheable(pool: PgPool) {
    let response = call(&app(pool), get("/api/banks")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "public, max-age=86400");
    let body = json_body(response).await;
    assert!(
        body["banks"]
            .as_array()
            .unwrap()
            .contains(&json!({ "bin": "970436", "name": "Vietcombank" }))
    );
}

#[sqlx::test]
async fn a_user_saves_and_reads_their_payout_account(pool: PgPool) {
    let app = app(pool);
    let session = sign_in(&app, "host").await;

    let missing = call(
        &app,
        get_with_cookie("/api/me/payout", &format!("daghep_session={session}")),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let saved = call(&app, put_payout(&session, account())).await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(saved.headers()["cache-control"], "private, no-store");
    assert_eq!(json_body(saved).await, account());

    let mut changed = account();
    changed["accountNumber"] = json!("9999999999");
    call(&app, put_payout(&session, changed.clone())).await;
    let read = call(
        &app,
        get_with_cookie("/api/me/payout", &format!("daghep_session={session}")),
    )
    .await;
    assert_eq!(json_body(read).await, changed);
}

#[sqlx::test]
async fn payout_accounts_are_private_to_their_owner(pool: PgPool) {
    let app = app(pool);
    let host = sign_in(&app, "host").await;
    call(&app, put_payout(&host, account())).await;
    let other = sign_in(&app, "other").await;

    let anonymous = call(&app, get("/api/me/payout")).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let others = call(
        &app,
        get_with_cookie("/api/me/payout", &format!("daghep_session={other}")),
    )
    .await;
    assert_eq!(others.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn invalid_payout_accounts_are_rejected_with_the_field(pool: PgPool) {
    let app = app(pool);
    let session = sign_in(&app, "host").await;

    for (field, value) in [
        ("bankBin", json!("123456")),
        ("accountNumber", json!("12-34")),
        ("accountName", json!("Phạm Long")),
    ] {
        let mut body = account();
        body[field] = value;

        let response = call(&app, put_payout(&session, body)).await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field}"
        );
        assert_eq!(
            json_body(response).await,
            json!({ "error": "invalid_payout_account", "field": field })
        );
    }
}

#[sqlx::test]
async fn saving_a_payout_account_needs_sign_in_and_the_same_site(pool: PgPool) {
    let app = app(pool);
    let session = sign_in(&app, "host").await;

    let anonymous = Request::put("/api/me/payout")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(account().to_string()))
        .unwrap();
    assert_eq!(
        call(&app, anonymous).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let mut cross_site = put_payout(&session, account());
    cross_site
        .headers_mut()
        .insert(ORIGIN, "https://evil.example".parse().unwrap());
    assert_eq!(call(&app, cross_site).await.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn a_bank_no_longer_listed_asks_the_host_to_update_instead_of_failing(pool: PgPool) {
    let app = app(pool.clone());
    let session = sign_in(&app, "host").await;
    call(&app, put_payout(&session, account())).await;
    // As if the bank were later removed from the supported list.
    sqlx::query("UPDATE payout_accounts SET bank_bin = '999999'")
        .execute(&pool)
        .await
        .unwrap();

    let read = call(
        &app,
        get_with_cookie("/api/me/payout", &format!("daghep_session={session}")),
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(json_body(read).await["bankBin"], json!("999999"));

    let create = Request::post("/api/matches")
        .header(COOKIE, format!("daghep_session={session}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
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
            .to_string(),
        ))
        .unwrap();
    let response = call(&app, create).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["field"], json!("payout"));
}
