//! HTTP adapter for slots: the public roster, the user's own place, join and leave.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::domain::{ClaimRejected, GuestNames, JoinRequest, LeaveRejected, MyPlace, Roster, Team};
use super::repo::PgSlotRepository;
use super::service::{self, JoinError, LeaveError};
use crate::auth::{AuthState, AuthenticatedUser};
use crate::clock::Clock;

#[derive(Clone)]
struct SlotState {
    repo: PgSlotRepository,
    clock: Arc<dyn Clock>,
    auth: AuthState,
}

impl FromRef<SlotState> for AuthState {
    fn from_ref(state: &SlotState) -> Self {
        state.auth.clone()
    }
}

pub fn router(repo: PgSlotRepository, clock: Arc<dyn Clock>, auth: AuthState) -> Router {
    Router::new()
        .route(
            "/api/matches/{share_id}/slots",
            get(get_roster).post(post_join),
        )
        .route(
            "/api/matches/{share_id}/slots/mine",
            get(get_mine).delete(delete_mine),
        )
        .with_state(SlotState { repo, clock, auth })
}

/// Same short shared-cache window as the public match view.
const PUBLIC_ROSTER_CACHE: &str = "public, max-age=30";
const PRIVATE: &str = "private, no-store";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerView {
    name: Option<String>,
    avatar_url: Option<String>,
    is_guest: bool,
    guest_of: Option<String>,
}

#[derive(Serialize)]
struct TeamView {
    team: &'static str,
    capacity: i64,
    players: Vec<PlayerView>,
}

#[derive(Serialize)]
struct RosterView {
    teams: Vec<TeamView>,
}

impl From<Roster> for RosterView {
    fn from(roster: Roster) -> Self {
        let teams = [Team::A, Team::B]
            .into_iter()
            .map(|team| TeamView {
                team: team.as_str(),
                capacity: team.capacity(roster.slot_count),
                players: roster
                    .entries
                    .iter()
                    .filter(|entry| entry.team == team)
                    .map(|entry| PlayerView {
                        name: entry.name.clone(),
                        avatar_url: entry.avatar_url.clone(),
                        is_guest: entry.is_guest,
                        guest_of: entry.guest_of.clone(),
                    })
                    .collect(),
            })
            .collect();
        Self { teams }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MyPlaceView {
    joined: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    team: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guests: Option<Vec<String>>,
}

impl From<Option<MyPlace>> for MyPlaceView {
    fn from(place: Option<MyPlace>) -> Self {
        match place {
            Some(place) => Self {
                joined: true,
                team: Some(place.team.as_str()),
                guests: Some(place.guests),
            },
            None => Self {
                joined: false,
                team: None,
                guests: None,
            },
        }
    }
}

#[derive(Deserialize)]
struct JoinBody {
    team: String,
    #[serde(default)]
    guests: Vec<String>,
}

fn error(status: StatusCode, code: &str) -> Response {
    (
        status,
        [(CACHE_CONTROL, PRIVATE)],
        Json(json!({ "error": code })),
    )
        .into_response()
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "match_not_found")
}

fn internal_error(context: &str, err: &dyn std::fmt::Display) -> Response {
    tracing::error!(error = %err, "{context}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

fn invalid(field: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        [(CACHE_CONTROL, PRIVATE)],
        Json(json!({ "error": "invalid_slot_request", "field": field })),
    )
        .into_response()
}

async fn get_roster(State(state): State<SlotState>, Path(share_id): Path<String>) -> Response {
    match service::roster(&state.repo, &share_id).await {
        Ok(Some(roster)) => (
            [(CACHE_CONTROL, PUBLIC_ROSTER_CACHE)],
            Json(RosterView::from(roster)),
        )
            .into_response(),
        Ok(None) => not_found(),
        Err(err) => internal_error("failed to load roster", &err),
    }
}

async fn get_mine(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<String>,
) -> Response {
    match service::my_place(&state.repo, user.id, &share_id).await {
        Ok(Some(place)) => {
            ([(CACHE_CONTROL, PRIVATE)], Json(MyPlaceView::from(place))).into_response()
        }
        Ok(None) => not_found(),
        Err(err) => internal_error("failed to load own place", &err),
    }
}

async fn post_join(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<String>,
    body: Result<Json<JoinBody>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let Some(team) = Team::parse(&body.team) else {
        return invalid("team");
    };
    let Some(guests) = GuestNames::parse(&body.guests) else {
        return invalid("guests");
    };
    let request = JoinRequest { team, guests };
    match service::join(
        &state.repo,
        state.clock.as_ref(),
        user.id,
        &share_id,
        &request,
    )
    .await
    {
        Ok(()) => {
            let place = MyPlace {
                team: request.team,
                guests: request.guests.names().to_vec(),
            };
            (
                StatusCode::CREATED,
                [(CACHE_CONTROL, PRIVATE)],
                Json(MyPlaceView::from(Some(place))),
            )
                .into_response()
        }
        Err(JoinError::MatchNotFound) => not_found(),
        Err(JoinError::Rejected(reason)) => error(
            StatusCode::CONFLICT,
            match reason {
                ClaimRejected::MatchStarted => "match_started",
                ClaimRejected::AlreadyJoined => "already_joined",
                ClaimRejected::TeamFull => "team_full",
            },
        ),
        Err(JoinError::Repo(err)) => internal_error("failed to join match", &err),
    }
}

async fn delete_mine(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<String>,
) -> Response {
    match service::leave(&state.repo, state.clock.as_ref(), user.id, &share_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(LeaveError::MatchNotFound) => not_found(),
        Err(LeaveError::Rejected(LeaveRejected::MatchStarted)) => {
            error(StatusCode::CONFLICT, "match_started")
        }
        Err(LeaveError::Repo(err)) => internal_error("failed to leave match", &err),
    }
}
