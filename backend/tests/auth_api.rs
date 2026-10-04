//! Acceptance tests for Google sign-in, sessions and sign-out through the HTTP API.

mod common;

use axum::body::Body;
use axum::http::header::{COOKIE, ORIGIN};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use common::*;
use serde_json::json;
use sqlx::PgPool;

fn logout(cookie: &str, origin: &str) -> Request<Body> {
    Request::post("/api/auth/logout")
        .header(COOKIE, format!("daghep_session={cookie}"))
        .header(ORIGIN, origin)
        .body(Body::empty())
        .unwrap()
}

async fn me_status(app: &axum::Router, session: &str) -> StatusCode {
    call(
        app,
        get_with_cookie("/api/me", &format!("daghep_session={session}")),
    )
    .await
    .status()
}

#[sqlx::test]
async fn start_redirects_to_google_and_binds_the_state_to_the_browser(pool: PgPool) {
    let app = app(pool);

    let response = call(&app, get("/api/auth/google/start?next=/create")).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let cookie = set_cookie(&response, "daghep_oauth_state").expect("state cookie");
    let state = cookie_value(&cookie);
    assert_eq!(
        location(&response),
        format!("{FAKE_AUTH_URL}?state={state}")
    );
    assert_eq!(state.len(), 43);
    assert!(cookie.contains("; Path=/api/auth/google;"), "{cookie}");
    assert!(cookie.contains("; Max-Age=600;"), "{cookie}");
    assert!(cookie.contains("; HttpOnly"), "{cookie}");
    assert!(cookie.contains("; SameSite=Lax"), "{cookie}");
    assert!(
        !cookie.contains("Secure"),
        "plain-http dev origin: {cookie}"
    );
}

#[sqlx::test]
async fn sign_in_is_unavailable_without_google_configuration(pool: PgPool) {
    let app = app_without_google(pool);

    let response = call(&app, get("/api/auth/google/start")).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(response).await,
        json!({ "error": "sign_in_unavailable" })
    );
}

#[sqlx::test]
async fn completing_sign_in_opens_a_session_and_returns_to_the_page(pool: PgPool) {
    let app = app(pool.clone());
    let state = start_sign_in(&app, "/create").await;

    let response = finish_sign_in(&app, &state, "ok:google-123").await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), format!("{FRONTEND}/create"));
    let session = set_cookie(&response, "daghep_session").expect("session cookie");
    assert!(session.contains("; Path=/;"), "{session}");
    assert!(session.contains("; Max-Age=2592000;"), "{session}");
    assert!(session.contains("; HttpOnly"), "{session}");
    assert!(session.contains("; SameSite=Lax"), "{session}");
    let cleared = set_cookie(&response, "daghep_oauth_state").expect("state cookie cleared");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");

    let token = cookie_value(&session);
    let me = call(
        &app,
        get_with_cookie("/api/me", &format!("daghep_session={token}")),
    )
    .await;
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(me.headers()["cache-control"], "private, no-store");
    assert_eq!(
        json_body(me).await,
        json!({
            "displayName": "Player google-123",
            "avatarUrl": "https://lh3.googleusercontent.com/a/google-123"
        })
    );
}

#[sqlx::test]
async fn only_a_hash_of_the_session_token_is_stored(pool: PgPool) {
    let app = app(pool.clone());

    let token = sign_in(&app, "google-123").await;

    let (by_hash, by_raw): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE token_hash = sha256(convert_to($1, 'UTF8'))),
                count(*) FILTER (WHERE token_hash = convert_to($1, 'UTF8'))
         FROM sessions",
    )
    .bind(&token)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((by_hash, by_raw), (1, 0));
}

#[sqlx::test]
async fn signing_in_again_reuses_the_account(pool: PgPool) {
    let app = app(pool.clone());

    sign_in(&app, "google-123").await;
    sign_in(&app, "google-123").await;
    sign_in(&app, "google-456").await;

    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 2);
}

#[sqlx::test]
async fn sign_in_fails_without_the_browsers_state_cookie(pool: PgPool) {
    let app = app(pool.clone());
    let state = start_sign_in(&app, "/").await;

    // An attacker's callback link opened in the victim's browser (login CSRF).
    let response = call(
        &app,
        get(&format!(
            "/api/auth/google/callback?code=ok:attacker&state={state}"
        )),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), format!("{FRONTEND}/login-failed"));
    assert!(set_cookie(&response, "daghep_session").is_none());
}

#[sqlx::test]
async fn failed_sign_ins_never_open_a_session(pool: PgPool) {
    let app = app(pool.clone());

    for code in ["rejected", "bad-nonce"] {
        let state = start_sign_in(&app, "/").await;

        let response = finish_sign_in(&app, &state, code).await;

        assert_eq!(
            location(&response),
            format!("{FRONTEND}/login-failed"),
            "{code}"
        );
        assert!(set_cookie(&response, "daghep_session").is_none(), "{code}");
    }
    let cancelled = call(
        &app,
        get("/api/auth/google/callback?error=access_denied&state=x"),
    )
    .await;
    assert_eq!(location(&cancelled), format!("{FRONTEND}/login-failed"));

    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}

#[sqlx::test]
async fn a_sign_in_attempt_can_be_used_only_once(pool: PgPool) {
    let app = app(pool);
    let state = start_sign_in(&app, "/").await;
    finish_sign_in(&app, &state, "ok:google-123").await;

    let replay = finish_sign_in(&app, &state, "ok:google-123").await;

    assert_eq!(location(&replay), format!("{FRONTEND}/login-failed"));
    assert!(set_cookie(&replay, "daghep_session").is_none());
}

#[sqlx::test]
async fn a_sign_in_attempt_expires_after_ten_minutes(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);
    let state = start_sign_in(&app, "/").await;

    clock.advance(Duration::minutes(11));
    let response = finish_sign_in(&app, &state, "ok:google-123").await;

    assert_eq!(location(&response), format!("{FRONTEND}/login-failed"));
}

#[sqlx::test]
async fn return_paths_outside_the_site_are_ignored(pool: PgPool) {
    let app = app(pool);

    for next in ["//evil.example", "https://evil.example", "/%5Cevil.example"] {
        let state = start_sign_in(&app, next).await;
        let response = finish_sign_in(&app, &state, "ok:google-123").await;

        assert_eq!(location(&response), format!("{FRONTEND}/"), "{next}");
    }
}

#[sqlx::test]
async fn me_requires_a_valid_unexpired_session(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool, clock.clone(), FRONTEND);

    let anonymous = call(&app, get("/api/me")).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(anonymous).await,
        json!({ "error": "unauthenticated" })
    );
    assert_eq!(me_status(&app, "garbage").await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        me_status(&app, &"A".repeat(43)).await,
        StatusCode::UNAUTHORIZED
    );

    let session = sign_in(&app, "google-123").await;
    assert_eq!(me_status(&app, &session).await, StatusCode::OK);

    clock.advance(Duration::days(30) + Duration::seconds(1));
    assert_eq!(me_status(&app, &session).await, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn signing_out_ends_the_session(pool: PgPool) {
    let app = app(pool);
    let session = sign_in(&app, "google-123").await;

    let response = call(&app, logout(&session, FRONTEND)).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), format!("{FRONTEND}/"));
    let cleared = set_cookie(&response, "daghep_session").expect("session cookie cleared");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert_eq!(me_status(&app, &session).await, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn cross_site_sign_out_is_refused(pool: PgPool) {
    let app = app(pool);
    let session = sign_in(&app, "google-123").await;

    let response = call(&app, logout(&session, "https://evil.example")).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json_body(response).await,
        json!({ "error": "forbidden_origin" })
    );
    assert_eq!(me_status(&app, &session).await, StatusCode::OK);
}

#[sqlx::test]
async fn cookies_are_secure_when_the_site_uses_https(pool: PgPool) {
    let app = app_with(pool, TestClock::at(NOW), "https://daghep.vn");
    let state = start_sign_in(&app, "/").await;

    let response = finish_sign_in(&app, &state, "ok:google-123").await;

    assert_eq!(location(&response), "https://daghep.vn/");
    for cookie in set_cookies(&response) {
        assert!(cookie.ends_with("; Secure"), "{cookie}");
    }
}

#[sqlx::test]
async fn concurrent_first_sign_ins_create_one_account(pool: PgPool) {
    use daghep_api::auth::PgAuthRepository;
    use daghep_api::auth::domain::{AuthRepository, NewProfile, Provider, ProviderIdentity};

    let repo = PgAuthRepository::new(pool.clone());
    let identity = ProviderIdentity {
        subject: "google-123".to_owned(),
        email: None,
        name: Some("Long".to_owned()),
        picture: None,
        nonce: None,
    };
    let profile = NewProfile::from_identity(&identity);

    let (a, b) = tokio::join!(
        repo.find_or_create_user(Provider::Google, &identity, &profile),
        repo.find_or_create_user(Provider::Google, &identity, &profile),
    );

    assert_eq!(a.unwrap(), b.unwrap());
    let (users, identities): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM user_identities)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((users, identities), (1, 1));
}

#[sqlx::test]
async fn signing_in_removes_expired_sessions(pool: PgPool) {
    let clock = TestClock::at(NOW);
    let app = app_with(pool.clone(), clock.clone(), FRONTEND);
    sign_in(&app, "google-123").await;

    clock.advance(Duration::days(31));
    sign_in(&app, "google-456").await;

    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 1);
}

#[sqlx::test]
async fn signing_in_again_in_the_same_browser_ends_the_previous_session(pool: PgPool) {
    let app = app(pool);
    let old = sign_in(&app, "google-123").await;
    let state = start_sign_in(&app, "/").await;

    let response = call(
        &app,
        get_with_cookie(
            &format!("/api/auth/google/callback?code=ok:google-456&state={state}"),
            &format!("daghep_oauth_state={state}; daghep_session={old}"),
        ),
    )
    .await;

    assert!(set_cookie(&response, "daghep_session").is_some());
    assert_eq!(me_status(&app, &old).await, StatusCode::UNAUTHORIZED);
}
