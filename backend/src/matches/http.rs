//! HTTP adapter: routes, request/response DTOs and error mapping for matches.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{AuthState, AuthenticatedUser};
use crate::payments::PgPayoutRepository;
use crate::payments::service::has_payout_account;

use super::domain::{Clock, Field, Format, Match, MatchType, NewMatchInput};
use super::repo::PgMatchRepository;
use super::service::{CreateMatchError, GetMatchError, create_match, get_public_match};

#[derive(Clone)]
struct MatchState {
    repo: PgMatchRepository,
    clock: Arc<dyn Clock>,
    auth: AuthState,
    payouts: PgPayoutRepository,
}

impl FromRef<MatchState> for AuthState {
    fn from_ref(state: &MatchState) -> Self {
        state.auth.clone()
    }
}

pub fn router(
    repo: PgMatchRepository,
    clock: Arc<dyn Clock>,
    auth: AuthState,
    payouts: PgPayoutRepository,
) -> Router {
    Router::new()
        .route("/api/matches", post(post_match))
        .route("/api/matches/{share_id}", get(get_match))
        .with_state(MatchState {
            repo,
            clock,
            auth,
            payouts,
        })
}

/// Public view of a match. Deliberately excludes internal ids and any host
/// payment details (those are only for slot holders).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchView {
    share_id: String,
    venue_name: String,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    format: &'static str,
    match_type: &'static str,
    level_min: f64,
    level_max: f64,
    total_fee_vnd: i64,
    slot_count: i16,
    price_per_player_vnd: i64,
}

impl From<Match> for MatchView {
    fn from(m: Match) -> Self {
        let price_per_player_vnd = m.price_per_player_vnd();
        Self {
            share_id: m.share_id.as_str().to_owned(),
            venue_name: m.venue_name,
            starts_at: m.starts_at,
            ends_at: m.ends_at,
            format: m.format.as_str(),
            match_type: m.match_type.as_str(),
            level_min: m.level_min.as_f64(),
            level_max: m.level_max.as_f64(),
            total_fee_vnd: m.total_fee_vnd,
            slot_count: m.slot_count,
            price_per_player_vnd,
        }
    }
}

/// Request body for creating a match. Enum fields arrive as strings so an
/// unknown value is reported as that field, not as malformed JSON.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateMatchRequest {
    venue_name: String,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    format: String,
    match_type: String,
    level_min: f64,
    level_max: f64,
    total_fee_vnd: i64,
    slot_count: Option<i64>,
}

impl CreateMatchRequest {
    fn into_input(self) -> Result<NewMatchInput, Field> {
        Ok(NewMatchInput {
            format: Format::parse(&self.format).ok_or(Field::Format)?,
            match_type: MatchType::parse(&self.match_type).ok_or(Field::MatchType)?,
            venue_name: self.venue_name,
            starts_at: self.starts_at,
            ends_at: self.ends_at,
            level_min: self.level_min,
            level_max: self.level_max,
            total_fee_vnd: self.total_fee_vnd,
            slot_count: self.slot_count,
        })
    }
}

/// Public match views may be cached briefly by browsers and CDNs: share links
/// arrive in bursts, and slot counts tolerate a few seconds of staleness.
const PUBLIC_MATCH_CACHE: &str = "public, max-age=30";

async fn get_match(
    State(state): State<MatchState>,
    Path(share_id): Path<String>,
) -> Result<impl IntoResponse, GetMatchError> {
    let found = get_public_match(&state.repo, &share_id).await?;
    Ok((
        [(header::CACHE_CONTROL, PUBLIC_MATCH_CACHE)],
        Json(MatchView::from(found)),
    ))
}

async fn post_match(
    State(state): State<MatchState>,
    AuthenticatedUser(host): AuthenticatedUser,
    body: Result<Json<CreateMatchRequest>, JsonRejection>,
) -> Result<impl IntoResponse, CreateMatchError> {
    // Malformed JSON, wrong types or missing fields are a bad request; well-formed
    // values that break a rule are reported per field (422).
    let Json(request) = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            CreateMatchError::PayloadTooLarge
        } else {
            CreateMatchError::MalformedRequest
        }
    })?;
    let input = request.into_input().map_err(CreateMatchError::Invalid)?;
    let host_has_payout = has_payout_account(&state.payouts, host.id)
        .await
        .map_err(|err| CreateMatchError::Payouts(err.to_string()))?;
    let created = create_match(
        &state.repo,
        state.clock.as_ref(),
        host.id,
        host_has_payout,
        input,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(MatchView::from(created))))
}

fn field_name(field: Field) -> &'static str {
    match field {
        Field::VenueName => "venueName",
        Field::StartsAt => "startsAt",
        Field::EndsAt => "endsAt",
        Field::Format => "format",
        Field::MatchType => "matchType",
        Field::LevelMin => "levelMin",
        Field::LevelMax => "levelMax",
        Field::TotalFeeVnd => "totalFeeVnd",
        Field::SlotCount => "slotCount",
        Field::Payout => "payout",
    }
}

fn internal_error(context: &str, err: &dyn std::fmt::Display) -> Response {
    tracing::error!(error = %err, "{context}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error" })),
    )
        .into_response()
}

impl IntoResponse for GetMatchError {
    fn into_response(self) -> Response {
        match self {
            GetMatchError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "match_not_found" })),
            )
                .into_response(),
            GetMatchError::Repo(err) => internal_error("failed to load match", &err),
        }
    }
}

impl IntoResponse for CreateMatchError {
    fn into_response(self) -> Response {
        match self {
            CreateMatchError::MalformedRequest => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_request" })),
            )
                .into_response(),
            CreateMatchError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({ "error": "payload_too_large" })),
            )
                .into_response(),
            CreateMatchError::Invalid(field) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "invalid_match", "field": field_name(field) })),
            )
                .into_response(),
            other => internal_error("failed to create match", &other),
        }
    }
}
