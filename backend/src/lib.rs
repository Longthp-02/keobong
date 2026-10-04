//! Daghep API. `main.rs` only wires configuration; everything testable lives here.

pub mod config;
pub mod matches;

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

/// Embedded migrations, run by `daghep-api migrate` (not on every cold start).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Builds the stateless HTTP application. All state lives in Postgres.
pub fn app(pool: PgPool) -> Router {
    app_with_clock(pool, Arc::new(matches::domain::SystemClock))
}

/// Same as [`app`] with an injected clock (used by tests that need a fixed "now").
pub fn app_with_clock(pool: PgPool, clock: Arc<dyn matches::domain::Clock>) -> Router {
    Router::new()
        .route("/health", get(health))
        .merge(matches::router(
            matches::PgMatchRepository::new(pool),
            clock,
        ))
        .layer(TraceLayer::new_for_http())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}
