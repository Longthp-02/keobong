//! HTTP adapter for the match list: `GET /api/matches`.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::clock::Clock;
use crate::matches::domain::{GeoPoint, ListCursor, MatchType};
use crate::matches::service::{ListError, ListRequest};
use crate::matches::{MatchView, PgMatchRepository};
use crate::slots::PgSlotRepository;

use super::service::{OpenMatchesError, open_matches};

#[derive(Clone)]
struct DiscoveryState {
    matches: PgMatchRepository,
    slots: PgSlotRepository,
    clock: Arc<dyn Clock>,
}

pub fn router(
    matches: PgMatchRepository,
    slots: PgSlotRepository,
    clock: Arc<dyn Clock>,
) -> Router {
    Router::new()
        .route("/api/matches", get(get_open_matches))
        .with_state(DiscoveryState {
            matches,
            slots,
            clock,
        })
}

/// Query parameters arrive as text so a bad value is reported as its field.
#[derive(Deserialize)]
struct ListParams {
    date: Option<String>,
    #[serde(rename = "type")]
    match_type: Option<String>,
    lat: Option<String>,
    lng: Option<String>,
    cursor: Option<String>,
}

fn parse(params: ListParams) -> Result<ListRequest, &'static str> {
    let day = params
        .date
        .map(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(|_| "date"))
        .transpose()?;
    let match_type = params
        .match_type
        .map(|t| MatchType::parse(&t).ok_or("type"))
        .transpose()?;
    let near = match (params.lat, params.lng) {
        (None, None) => None,
        (Some(lat), Some(lng)) => {
            let lat = lat.parse().map_err(|_| "near")?;
            let lng = lng.parse().map_err(|_| "near")?;
            Some(GeoPoint::new(lat, lng).ok_or("near")?)
        }
        _ => return Err("near"),
    };
    let after = params
        .cursor
        .map(|c| ListCursor::decode(&c).ok_or("cursor"))
        .transpose()?;
    Ok(ListRequest {
        day,
        match_type,
        near,
        after,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenMatchView {
    #[serde(flatten)]
    view: MatchView,
    places_left: i64,
    /// Whole metres; `null` unless the visitor shared where they are.
    distance_m: Option<i64>,
}

/// Same window as the match page: lists tolerate a few seconds of staleness.
/// The visitor's position is part of the URL, so caches keep it per position.
const LIST_CACHE: &str = "public, max-age=30";

fn invalid(field: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": "invalid_query", "field": field })),
    )
        .into_response()
}

async fn get_open_matches(
    State(state): State<DiscoveryState>,
    Query(params): Query<ListParams>,
) -> Response {
    let request = match parse(params) {
        Ok(request) => request,
        Err(field) => return invalid(field),
    };
    match open_matches(&state.matches, &state.slots, state.clock.as_ref(), request).await {
        Ok(page) => {
            let matches: Vec<OpenMatchView> = page
                .matches
                .into_iter()
                .map(|open| OpenMatchView {
                    // Distances are at most half the Earth's circumference, far inside i64.
                    distance_m: open.listed.distance_m.map(|d| d.round() as i64),
                    view: MatchView::from(open.listed.found),
                    places_left: open.places_left,
                })
                .collect();
            (
                [(header::CACHE_CONTROL, LIST_CACHE)],
                Json(json!({ "matches": matches, "nextCursor": page.next })),
            )
                .into_response()
        }
        Err(OpenMatchesError::List(ListError::DayOutOfRange)) => invalid("date"),
        Err(err) => {
            tracing::error!(error = %err, "failed to list matches");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal_error" })),
            )
                .into_response()
        }
    }
}
