//! HTTP adapter: routes, response DTOs and error mapping for matches.

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;

use super::domain::{Format, Match, MatchType};
use super::repo::PgMatchRepository;
use super::service::{GetMatchError, get_public_match};

pub fn router(repo: PgMatchRepository) -> Router {
    Router::new()
        .route("/api/matches/{share_id}", get(get_match))
        .with_state(repo)
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
}

impl From<Match> for MatchView {
    fn from(m: Match) -> Self {
        Self {
            share_id: m.share_id.as_str().to_owned(),
            venue_name: m.venue_name,
            starts_at: m.starts_at,
            ends_at: m.ends_at,
            format: match m.format {
                Format::FiveASide => "five_a_side",
                Format::SevenASide => "seven_a_side",
                Format::ElevenASide => "eleven_a_side",
            },
            match_type: match m.match_type {
                MatchType::Casual => "casual",
                MatchType::Competitive => "competitive",
                MatchType::BeginnerFriendly => "beginner_friendly",
            },
            level_min: m.level_min.as_f64(),
            level_max: m.level_max.as_f64(),
            total_fee_vnd: m.total_fee_vnd,
            slot_count: m.slot_count,
        }
    }
}

/// Public match views may be cached briefly by browsers and CDNs: share links
/// arrive in bursts, and slot counts tolerate a few seconds of staleness.
const PUBLIC_MATCH_CACHE: &str = "public, max-age=30";

async fn get_match(
    State(repo): State<PgMatchRepository>,
    Path(share_id): Path<String>,
) -> Result<impl IntoResponse, GetMatchError> {
    let found = get_public_match(&repo, &share_id).await?;
    Ok((
        [(header::CACHE_CONTROL, PUBLIC_MATCH_CACHE)],
        Json(MatchView::from(found)),
    ))
}

impl IntoResponse for GetMatchError {
    fn into_response(self) -> Response {
        match self {
            GetMatchError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "match_not_found" })),
            )
                .into_response(),
            GetMatchError::Repo(err) => {
                tracing::error!(error = %err, "failed to load match");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "internal_error" })),
                )
                    .into_response()
            }
        }
    }
}
