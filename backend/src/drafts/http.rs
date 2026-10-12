//! HTTP adapter: `POST /api/match-drafts` reads a pasted message into form fields.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::domain::{LimitScope, MatchDraft, MatchTextReader};
use super::repo::PgDraftRepository;
use super::service::{ReadDraftError, read_draft};
use crate::auth::{AuthState, AuthenticatedUser};
use crate::clock::Clock;
use crate::matches::PgMatchRepository;

#[derive(Clone)]
struct DraftState {
    drafts: PgDraftRepository,
    matches: PgMatchRepository,
    clock: Arc<dyn Clock>,
    reader: Option<Arc<dyn MatchTextReader>>,
    auth: AuthState,
}

impl FromRef<DraftState> for AuthState {
    fn from_ref(state: &DraftState) -> Self {
        state.auth.clone()
    }
}

pub fn router(
    drafts: PgDraftRepository,
    matches: PgMatchRepository,
    clock: Arc<dyn Clock>,
    reader: Option<Arc<dyn MatchTextReader>>,
    auth: AuthState,
) -> Router {
    Router::new()
        .route("/api/match-drafts", post(post_draft))
        .with_state(DraftState {
            drafts,
            matches,
            clock,
            reader,
            auth,
        })
}

#[derive(Deserialize)]
struct DraftRequest {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DraftView {
    venue_id: Option<String>,
    /// `YYYY-MM-DD`, in Ho Chi Minh City.
    date: Option<String>,
    /// `HH:MM`, in Ho Chi Minh City.
    start_time: Option<String>,
    end_time: Option<String>,
    format: Option<&'static str>,
    match_type: Option<&'static str>,
    level_min: Option<f64>,
    level_max: Option<f64>,
    slot_count: Option<i64>,
    total_fee_vnd: Option<i64>,
}

impl From<MatchDraft> for DraftView {
    fn from(draft: MatchDraft) -> Self {
        Self {
            venue_id: draft.venue_id,
            date: draft.date.map(|day| day.format("%Y-%m-%d").to_string()),
            start_time: draft
                .start_time
                .map(|time| time.format("%H:%M").to_string()),
            end_time: draft.end_time.map(|time| time.format("%H:%M").to_string()),
            format: draft.format.map(|format| format.as_str()),
            match_type: draft.match_type.map(|kind| kind.as_str()),
            level_min: draft.level_min,
            level_max: draft.level_max,
            slot_count: draft.slot_count,
            total_fee_vnd: draft.total_fee_vnd,
        }
    }
}

const PRIVATE: &str = "private, no-store";

async fn post_draft(
    State(state): State<DraftState>,
    AuthenticatedUser(user): AuthenticatedUser,
    body: Result<Json<DraftRequest>, JsonRejection>,
) -> Response {
    let error = |status: StatusCode, body: serde_json::Value| {
        (status, [(CACHE_CONTROL, PRIVATE)], Json(body)).into_response()
    };
    let Ok(Json(request)) = body else {
        return error(
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_request" }),
        );
    };
    let result = read_draft(
        &state.drafts,
        &state.matches,
        state.clock.as_ref(),
        state.reader.as_deref(),
        user.id,
        &request.text,
    )
    .await;
    match result {
        Ok(draft) => (
            StatusCode::OK,
            [(CACHE_CONTROL, PRIVATE)],
            Json(json!({ "draft": DraftView::from(draft) })),
        )
            .into_response(),
        Err(ReadDraftError::InvalidText) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "error": "invalid_text" }),
        ),
        Err(ReadDraftError::Limited(scope)) => error(
            StatusCode::TOO_MANY_REQUESTS,
            json!({
                "error": "draft_limit",
                "scope": match scope { LimitScope::User => "user", LimitScope::Service => "service" },
            }),
        ),
        Err(ReadDraftError::Unavailable) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({ "error": "drafts_unavailable" }),
        ),
        Err(ReadDraftError::Reader(err)) => {
            // The message itself is never logged.
            tracing::warn!(error = %err, "match draft reader failed");
            error(StatusCode::BAD_GATEWAY, json!({ "error": "reader_failed" }))
        }
        Err(ReadDraftError::Storage(err)) => {
            tracing::error!(error = %err, "failed to record a match draft read");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal_error" }),
            )
        }
    }
}
