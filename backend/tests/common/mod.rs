//! Shared harness for acceptance tests: the real router and database with a
//! fake Google provider and a clock tests can move.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::{COOKIE, LOCATION, SET_COOKIE};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Duration, Utc};
use daghep_api::Deps;
use daghep_api::auth::domain::{
    IdentityProvider, LoginAttempt, Provider, ProviderError, ProviderFuture, ProviderIdentity,
};
use daghep_api::clock::Clock;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

pub const FRONTEND: &str = "http://localhost:3000";
pub const FAKE_AUTH_URL: &str = "https://accounts.example/auth";

/// A clock that starts at a fixed instant and can be moved forward.
#[derive(Clone)]
pub struct TestClock(Arc<Mutex<DateTime<Utc>>>);

impl TestClock {
    pub fn at(now: &str) -> Self {
        Self(Arc::new(Mutex::new(now.parse().unwrap())))
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

/// Fake Google. The authorization code decides the outcome:
/// `ok:<subject>` signs in that account, `bad-nonce` returns a wrong nonce,
/// anything else is rejected like an expired code.
pub struct FakeGoogle;

impl IdentityProvider for FakeGoogle {
    fn provider(&self) -> Provider {
        Provider::Google
    }

    fn authorization_url(&self, attempt: &LoginAttempt) -> String {
        format!("{FAKE_AUTH_URL}?state={}", attempt.state)
    }

    fn exchange_code<'a>(&'a self, code: &'a str, attempt: &'a LoginAttempt) -> ProviderFuture<'a> {
        let result = if let Some(subject) = code.strip_prefix("ok:") {
            Ok(ProviderIdentity {
                subject: subject.to_owned(),
                email: Some(format!("{subject}@example.com")),
                name: Some(format!("Player {subject}")),
                picture: Some(format!("https://lh3.googleusercontent.com/a/{subject}")),
                nonce: Some(attempt.nonce.clone()),
            })
        } else if code == "bad-nonce" {
            Ok(ProviderIdentity {
                subject: "nonce-victim".to_owned(),
                email: None,
                name: None,
                picture: None,
                nonce: Some("not-the-nonce".to_owned()),
            })
        } else {
            Err(ProviderError::Rejected("invalid_grant".to_owned()))
        };
        Box::pin(async move { result })
    }
}

/// "Now" for tests: a few days before the 2099 matches they create.
pub const NOW: &str = "2099-10-01T00:00:00Z";

pub fn app(pool: PgPool) -> Router {
    app_with(pool, TestClock::at(NOW), FRONTEND)
}

pub fn app_with(pool: PgPool, clock: TestClock, frontend_origin: &str) -> Router {
    daghep_api::app(Deps {
        pool,
        clock: Arc::new(clock),
        google: Some(Arc::new(FakeGoogle)),
        frontend_origin: frontend_origin.to_owned(),
    })
}

pub fn app_without_google(pool: PgPool) -> Router {
    daghep_api::app(Deps {
        pool,
        clock: Arc::new(TestClock::at(NOW)),
        google: None,
        frontend_origin: FRONTEND.to_owned(),
    })
}

pub async fn call(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.unwrap()
}

pub async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

pub fn location(response: &Response) -> &str {
    response.headers()[LOCATION].to_str().unwrap()
}

/// All `Set-Cookie` header values of a response.
pub fn set_cookies(response: &Response) -> Vec<String> {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect()
}

/// The full `Set-Cookie` line for `name`, if the response sets it.
pub fn set_cookie(response: &Response, name: &str) -> Option<String> {
    set_cookies(response)
        .into_iter()
        .find(|c| c.starts_with(&format!("{name}=")))
}

/// The value part of a `Set-Cookie` line.
pub fn cookie_value(set_cookie_line: &str) -> String {
    let pair = set_cookie_line.split(';').next().unwrap();
    pair.split_once('=').unwrap().1.to_owned()
}

pub fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

pub fn get_with_cookie(uri: &str, cookie: &str) -> Request<Body> {
    Request::get(uri)
        .header(COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

/// Starts a sign-in and returns the `state` value the browser carries.
pub async fn start_sign_in(app: &Router, next: &str) -> String {
    let response = call(app, get(&format!("/api/auth/google/start?next={next}"))).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    cookie_value(&set_cookie(&response, "daghep_oauth_state").expect("state cookie"))
}

/// Sends the browser back from Google with `code`, as a browser holding `state` would.
pub async fn finish_sign_in(app: &Router, state: &str, code: &str) -> Response {
    call(
        app,
        get_with_cookie(
            &format!("/api/auth/google/callback?code={code}&state={state}"),
            &format!("daghep_oauth_state={state}"),
        ),
    )
    .await
}

/// Signs in the Google account `subject` and returns the session cookie value.
pub async fn sign_in(app: &Router, subject: &str) -> String {
    let state = start_sign_in(app, "/").await;
    let response = finish_sign_in(app, &state, &format!("ok:{subject}")).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    cookie_value(&set_cookie(&response, "daghep_session").expect("session cookie"))
}

/// Saves a payout account for the signed-in user (needed to host paid matches).
pub async fn add_payout(app: &Router, session: &str) {
    let request = Request::put("/api/me/payout")
        .header(COOKIE, format!("daghep_session={session}"))
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "bankBin": "970416",
                "accountNumber": "257678859",
                "accountName": "PHAM LONG"
            })
            .to_string(),
        ))
        .unwrap();
    let response = call(app, request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// Inserts a user directly (for fixtures that need a match host).
pub async fn insert_user(pool: &PgPool) -> i64 {
    sqlx::query_scalar("INSERT INTO users (display_name) VALUES ('Host') RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}
