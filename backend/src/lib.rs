//! Daghep API. `main.rs` only wires configuration; everything testable lives here.

pub mod auth;
pub mod bot;
pub mod clock;
pub mod config;
pub mod discovery;
pub mod drafts;
pub mod matches;
pub mod payments;
pub mod random;
pub mod slots;
pub mod text;

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::header::ORIGIN;
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

use crate::auth::domain::IdentityProvider;
use crate::clock::Clock;

const MAX_BODY_BYTES: usize = 16 * 1024;

/// Embedded migrations, run by `daghep-api migrate` (not on every cold start).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// External dependencies of the app. Tests swap the clock and the identity provider.
pub struct Deps {
    pub pool: PgPool,
    pub clock: Arc<dyn Clock>,
    /// `None` disables Google sign-in.
    pub google: Option<Arc<dyn IdentityProvider>>,
    /// The web app's origin, e.g. `https://daghep.vn`.
    pub frontend_origin: String,
    /// `None` disables reading pasted messages into match drafts.
    pub reader: Option<Arc<dyn drafts::domain::MatchTextReader>>,
    /// `None` disables the Zalo bot (its routes answer 404).
    pub zalo: Option<Arc<bot::ZaloSettings>>,
}

/// Builds the stateless HTTP application. All state lives in Postgres.
pub fn app(deps: Deps) -> Router {
    let auth_state = auth::AuthState::new(
        auth::PgAuthRepository::new(deps.pool.clone()),
        deps.clock.clone(),
        deps.google,
        &deps.frontend_origin,
    );
    Router::new()
        .route("/health", get(health))
        .merge(auth::router(auth_state.clone()))
        .merge(matches::router(
            matches::PgMatchRepository::new(deps.pool.clone()),
            deps.clock.clone(),
            auth_state.clone(),
            payments::PgPayoutRepository::new(deps.pool.clone()),
        ))
        .merge(payments::router(
            payments::PgPayoutRepository::new(deps.pool.clone()),
            deps.clock.clone(),
            auth_state.clone(),
        ))
        .merge(drafts::router(
            drafts::PgDraftRepository::new(deps.pool.clone()),
            matches::PgMatchRepository::new(deps.pool.clone()),
            deps.clock.clone(),
            deps.reader.clone(),
            auth_state.clone(),
        ))
        .merge(bot::router(bot::BotState {
            bot: bot::PgBotRepository::new(deps.pool.clone()),
            drafts: drafts::PgDraftRepository::new(deps.pool.clone()),
            matches: matches::PgMatchRepository::new(deps.pool.clone()),
            payouts: payments::PgPayoutRepository::new(deps.pool.clone()),
            clock: deps.clock.clone(),
            reader: deps.reader,
            zalo: deps.zalo,
            frontend_origin: deps.frontend_origin.clone(),
            auth: auth_state.clone(),
        }))
        .merge(discovery::router(
            matches::PgMatchRepository::new(deps.pool.clone()),
            slots::PgSlotRepository::new(deps.pool.clone()),
            deps.clock.clone(),
        ))
        .merge(slots::router(
            slots::PgSlotRepository::new(deps.pool.clone()),
            deps.clock,
            auth_state,
            payments::PgPayoutRepository::new(deps.pool),
        ))
        // Every request body is small JSON; refuse anything bigger before parsing.
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            Arc::<str>::from(deps.frontend_origin),
            same_origin_guard,
        ))
        .layer(TraceLayer::new_for_http())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Defense in depth against cross-site requests (on top of `SameSite=Lax`
/// cookies): a state-changing request that carries an `Origin` header must come
/// from the web app. Server-to-server calls without `Origin` pass.
async fn same_origin_guard(
    State(frontend_origin): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let foreign = request
        .headers()
        .get(ORIGIN)
        .is_some_and(|origin| origin.as_bytes() != frontend_origin.as_bytes());
    if !safe && foreign {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "forbidden_origin" })),
        )
            .into_response();
    }
    next.run(request).await
}
