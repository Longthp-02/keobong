//! HTTP adapter: sign-in redirects, cookies, `/api/me`, sign-out, and the
//! `AuthenticatedUser` extractor other features use to require a session.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts, Query, State};
use axum::http::header::{CACHE_CONTROL, COOKIE, LOCATION, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::domain::{IdentityProvider, LOGIN_ATTEMPT_TTL_MINUTES, SESSION_TTL_DAYS, User};
use super::repo::PgAuthRepository;
use super::service::{self, FinishLoginError};
use crate::clock::Clock;

const SESSION_COOKIE: &str = "daghep_session";
const STATE_COOKIE: &str = "daghep_oauth_state";
/// The state cookie is only sent to the Google sign-in endpoints.
const STATE_COOKIE_PATH: &str = "/api/auth/google";
/// Frontend page shown when a sign-in cannot be completed.
const LOGIN_FAILED_PATH: &str = "/login-failed";

/// Everything the auth endpoints and the extractor need. Cheap to clone.
#[derive(Clone)]
pub struct AuthState {
    repo: PgAuthRepository,
    clock: Arc<dyn Clock>,
    google: Option<Arc<dyn IdentityProvider>>,
    frontend_origin: Arc<str>,
    secure_cookies: bool,
}

impl AuthState {
    pub fn new(
        repo: PgAuthRepository,
        clock: Arc<dyn Clock>,
        google: Option<Arc<dyn IdentityProvider>>,
        frontend_origin: &str,
    ) -> Self {
        Self {
            repo,
            clock,
            google,
            frontend_origin: Arc::from(frontend_origin),
            secure_cookies: frontend_origin.starts_with("https://"),
        }
    }

    fn redirect_to_frontend(&self, path: &str) -> Response {
        redirect(&format!("{}{path}", self.frontend_origin))
    }

    fn cookie(&self, name: &str, value: &str, path: &str, max_age_secs: i64) -> String {
        let secure = if self.secure_cookies { "; Secure" } else { "" };
        format!(
            "{name}={value}; Path={path}; Max-Age={max_age_secs}; HttpOnly; SameSite=Lax{secure}"
        )
    }
}

pub fn router(state: AuthState) -> Router {
    Router::new()
        .route("/api/auth/google/start", get(start_google))
        .route("/api/auth/google/callback", get(google_callback))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
        .with_state(state)
}

fn redirect(location: &str) -> Response {
    match HeaderValue::from_str(location) {
        Ok(value) => (StatusCode::SEE_OTHER, [(LOCATION, value)]).into_response(),
        Err(_) => internal_error("redirect target is not a valid header", &location),
    }
}

fn with_cookies(mut response: Response, cookies: &[String]) -> Response {
    for cookie in cookies {
        match HeaderValue::from_str(cookie) {
            Ok(value) => {
                response.headers_mut().append(SET_COOKIE, value);
            }
            Err(_) => return internal_error("cookie is not a valid header", &"<cookie>"),
        }
    }
    response
}

/// Reads one cookie. If the browser sends the name twice, the first wins.
fn read_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn internal_error(context: &str, err: &dyn std::fmt::Display) -> Response {
    tracing::error!(error = %err, "{context}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error" })),
    )
        .into_response()
}

fn not_configured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "sign_in_unavailable" })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct StartQuery {
    next: Option<String>,
}

async fn start_google(State(state): State<AuthState>, Query(query): Query<StartQuery>) -> Response {
    let Some(google) = state.google.as_deref() else {
        return not_configured();
    };
    match service::start_login(
        &state.repo,
        google,
        state.clock.as_ref(),
        query.next.as_deref(),
    )
    .await
    {
        Ok(start) => {
            let max_age = Duration::minutes(LOGIN_ATTEMPT_TTL_MINUTES).num_seconds();
            let cookie = state.cookie(STATE_COOKIE, &start.state, STATE_COOKIE_PATH, max_age);
            with_cookies(redirect(&start.redirect_url), &[cookie])
        }
        Err(err) => internal_error("failed to start sign-in", &err),
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn google_callback(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(google) = state.google.as_deref() else {
        return not_configured();
    };
    let clear_state = state.cookie(STATE_COOKIE, "", STATE_COOKIE_PATH, 0);
    let failed = |reason: &str| {
        tracing::warn!(reason, "sign-in failed");
        with_cookies(
            state.redirect_to_frontend(LOGIN_FAILED_PATH),
            std::slice::from_ref(&clear_state),
        )
    };
    if let Some(error) = query.error.as_deref() {
        // The user cancelled or Google refused; the value is a short error code.
        return failed(&format!("provider returned {error:.64}"));
    }
    let (Some(code), Some(oauth_state)) = (query.code.as_deref(), query.state.as_deref()) else {
        return failed("missing code or state");
    };
    let result = service::finish_login(
        &state.repo,
        google,
        state.clock.as_ref(),
        code,
        oauth_state,
        read_cookie(&headers, STATE_COOKIE),
    )
    .await;
    match result {
        Ok(done) => {
            let max_age = Duration::days(SESSION_TTL_DAYS).num_seconds();
            let session = state.cookie(SESSION_COOKIE, done.session.as_str(), "/", max_age);
            with_cookies(
                state.redirect_to_frontend(done.return_to.as_str()),
                &[session, clear_state],
            )
        }
        Err(err @ (FinishLoginError::Randomness(_) | FinishLoginError::Repo(_))) => {
            internal_error("failed to complete sign-in", &err)
        }
        Err(err) => failed(&err.to_string()),
    }
}

async fn logout(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    if let Err(err) = service::sign_out(&state.repo, read_cookie(&headers, SESSION_COOKIE)).await {
        return internal_error("failed to sign out", &err);
    }
    let clear = state.cookie(SESSION_COOKIE, "", "/", 0);
    with_cookies(state.redirect_to_frontend("/"), &[clear])
}

/// The signed-in user's own view. Private: never cached by shared caches.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeView {
    display_name: Option<String>,
    avatar_url: Option<String>,
}

async fn me(user: AuthenticatedUser) -> Response {
    let view = MeView {
        display_name: user.0.display_name,
        avatar_url: user.0.avatar_url,
    };
    ([(CACHE_CONTROL, "private, no-store")], Json(view)).into_response()
}

/// Extractor for endpoints that require a signed-in user. Answers 401 otherwise.
pub struct AuthenticatedUser(pub User);

pub enum AuthRejection {
    Unauthenticated,
    Internal,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        match self {
            AuthRejection::Unauthenticated => (
                StatusCode::UNAUTHORIZED,
                [(CACHE_CONTROL, "private, no-store")],
                Json(json!({ "error": "unauthenticated" })),
            )
                .into_response(),
            AuthRejection::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal_error" })),
            )
                .into_response(),
        }
    }
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AuthState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        let cookie = read_cookie(&parts.headers, SESSION_COOKIE);
        match service::current_user(&auth.repo, auth.clock.as_ref(), cookie).await {
            Ok(Some(user)) => Ok(Self(user)),
            Ok(None) => Err(AuthRejection::Unauthenticated),
            Err(err) => {
                tracing::error!(error = %err, "failed to load session");
                Err(AuthRejection::Internal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_cookie_finds_the_named_cookie_across_headers() {
        let mut headers = HeaderMap::new();
        headers.append(COOKIE, HeaderValue::from_static("a=1; daghep_session=abc"));
        headers.append(
            COOKIE,
            HeaderValue::from_static("daghep_session=second; b=2"),
        );

        assert_eq!(read_cookie(&headers, "daghep_session"), Some("abc"));
        assert_eq!(read_cookie(&headers, "b"), Some("2"));
        assert_eq!(read_cookie(&headers, "missing"), None);
        assert_eq!(read_cookie(&headers, "daghep"), None);
    }
}
