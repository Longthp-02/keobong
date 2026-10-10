//! HTTP adapter for slots: the public roster, the user's own place with payment
//! instructions, join, leave, reporting a transfer, and the host's payment checks.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use chrono::{DateTime, Utc};

use super::domain::{
    ClaimRejected, GuestNames, HostAction, HostActionOutcome, HostParty, HostView, JoinRequest,
    LeaveRejected, MyPlace, PaymentStatus, ReportOutcome, Roster, Team,
};
use super::repo::PgSlotRepository;
use super::service::{self, JoinError, LeaveError};
use crate::auth::{AuthState, AuthenticatedUser};
use crate::clock::Clock;
use crate::payments::PgPayoutRepository;
use crate::payments::domain::PaymentInstructions;
use crate::payments::service::payment_instructions;

#[derive(Clone)]
struct SlotState {
    repo: PgSlotRepository,
    clock: Arc<dyn Clock>,
    auth: AuthState,
    payouts: PgPayoutRepository,
}

impl FromRef<SlotState> for AuthState {
    fn from_ref(state: &SlotState) -> Self {
        state.auth.clone()
    }
}

pub fn router(
    repo: PgSlotRepository,
    clock: Arc<dyn Clock>,
    auth: AuthState,
    payouts: PgPayoutRepository,
) -> Router {
    Router::new()
        .route(
            "/api/matches/{share_id}/slots",
            get(get_roster).post(post_join),
        )
        .route(
            "/api/matches/{share_id}/slots/mine",
            get(get_mine).delete(delete_mine),
        )
        .route(
            "/api/matches/{share_id}/slots/mine/report-payment",
            post(post_report_payment),
        )
        .route("/api/matches/{share_id}/payments", get(get_host_parties))
        .route(
            "/api/matches/{share_id}/payments/{code}/confirm",
            post(post_confirm),
        )
        .route(
            "/api/matches/{share_id}/payments/{code}/reject",
            post(post_reject),
        )
        .with_state(SlotState {
            repo,
            clock,
            auth,
            payouts,
        })
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
    cancelled: bool,
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
        Self {
            teams,
            cancelled: roster.cancelled,
        }
    }
}

/// Where and how much to transfer. Private: only for the player holding the place.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaymentView {
    bank_name: &'static str,
    account_number: String,
    account_name: String,
    amount_vnd: i64,
    memo: String,
    qr_payload: String,
}

impl From<PaymentInstructions> for PaymentView {
    fn from(p: PaymentInstructions) -> Self {
        Self {
            bank_name: p.bank_name,
            account_number: p.account_number,
            account_name: p.account_name,
            amount_vnd: p.amount_vnd,
            memo: p.memo,
            qr_payload: p.qr_payload,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JoinedView {
    joined: bool,
    team: &'static str,
    guests: Vec<String>,
    payment_code: i64,
    payment_status: &'static str,
    hold_expires_at: Option<DateTime<Utc>>,
    amount_vnd: i64,
    /// Present while a transfer is still expected.
    payment: Option<PaymentView>,
}

/// Loads the user's place and, if they still owe money, the host's transfer details.
async fn place_response(
    state: &SlotState,
    user: crate::auth::UserId,
    share_id: &str,
    status: StatusCode,
) -> Response {
    let place = match service::my_place(&state.repo, state.clock.as_ref(), user, share_id).await {
        Ok(Some(place)) => place,
        Ok(None) => return not_found(),
        Err(err) => return internal_error("failed to load own place", &err),
    };
    let Some(place) = place else {
        return ([(CACHE_CONTROL, PRIVATE)], Json(json!({ "joined": false }))).into_response();
    };
    let MyPlace {
        team,
        guests,
        payment_code,
        payment_status,
        hold_expires_at,
        amount_vnd,
        host,
        match_cancelled,
    } = place;
    let payment =
        if payment_status != PaymentStatus::Confirmed && amount_vnd > 0 && !match_cancelled {
            match payment_instructions(&state.payouts, host, amount_vnd, payment_code).await {
                Ok(instructions) => instructions.map(PaymentView::from),
                Err(err) => return internal_error("failed to load payout account", &err),
            }
        } else {
            None
        };
    let view = JoinedView {
        joined: true,
        team: team.as_str(),
        guests,
        payment_code,
        payment_status: payment_status.as_str(),
        hold_expires_at,
        amount_vnd,
        payment,
    };
    (status, [(CACHE_CONTROL, PRIVATE)], Json(view)).into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostPartyView {
    payment_code: i64,
    team: &'static str,
    holder_name: Option<String>,
    guests: Vec<String>,
    amount_vnd: i64,
    payment_status: &'static str,
    hold_expires_at: Option<DateTime<Utc>>,
}

impl From<HostParty> for HostPartyView {
    fn from(p: HostParty) -> Self {
        Self {
            payment_code: p.payment_code,
            team: p.team.as_str(),
            holder_name: p.holder_name,
            guests: p.guests,
            amount_vnd: p.amount_vnd,
            payment_status: p.payment_status.as_str(),
            hold_expires_at: p.hold_expires_at,
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
    match service::roster(&state.repo, state.clock.as_ref(), &share_id).await {
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
    place_response(&state, user.id, &share_id, StatusCode::OK).await
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
        Ok(()) => place_response(&state, user.id, &share_id, StatusCode::CREATED).await,
        Err(JoinError::MatchNotFound) => not_found(),
        Err(JoinError::Rejected(reason)) => error(
            StatusCode::CONFLICT,
            match reason {
                ClaimRejected::MatchCancelled => "match_cancelled",
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
        Err(LeaveError::Rejected(LeaveRejected::MatchCancelled)) => {
            error(StatusCode::CONFLICT, "match_cancelled")
        }
        Err(LeaveError::Rejected(LeaveRejected::MatchStarted)) => {
            error(StatusCode::CONFLICT, "match_started")
        }
        Err(LeaveError::Repo(err)) => internal_error("failed to leave match", &err),
    }
}

async fn post_report_payment(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<String>,
) -> Response {
    match service::report_payment(&state.repo, state.clock.as_ref(), user.id, &share_id).await {
        Ok(ReportOutcome::Reported | ReportOutcome::AlreadyDone) => {
            place_response(&state, user.id, &share_id, StatusCode::OK).await
        }
        Ok(ReportOutcome::NotJoined) => error(StatusCode::CONFLICT, "not_joined"),
        Ok(ReportOutcome::MatchCancelled) => error(StatusCode::CONFLICT, "match_cancelled"),
        Ok(ReportOutcome::MatchNotFound) => not_found(),
        Err(err) => internal_error("failed to report payment", &err),
    }
}

async fn get_host_parties(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<String>,
) -> Response {
    match service::host_parties(&state.repo, state.clock.as_ref(), user.id, &share_id).await {
        Ok(HostView::Parties(parties)) => {
            let parties: Vec<HostPartyView> = parties.into_iter().map(Into::into).collect();
            (
                [(CACHE_CONTROL, PRIVATE)],
                Json(json!({ "parties": parties })),
            )
                .into_response()
        }
        Ok(HostView::NotHost) => error(StatusCode::FORBIDDEN, "not_host"),
        Ok(HostView::MatchNotFound) => not_found(),
        Err(err) => internal_error("failed to load payments", &err),
    }
}

async fn host_action_response(
    state: &SlotState,
    user: crate::auth::UserId,
    share_id: &str,
    code: &str,
    action: HostAction,
) -> Response {
    let Ok(code) = code.parse::<i64>() else {
        return error(StatusCode::NOT_FOUND, "party_not_found");
    };
    let result = service::host_action(
        &state.repo,
        state.clock.as_ref(),
        user,
        share_id,
        code,
        action,
    )
    .await;
    match result {
        Ok(HostActionOutcome::Done) => StatusCode::NO_CONTENT.into_response(),
        Ok(HostActionOutcome::NotHost) => error(StatusCode::FORBIDDEN, "not_host"),
        Ok(HostActionOutcome::MatchNotFound) => not_found(),
        Ok(HostActionOutcome::PartyNotFound) => error(StatusCode::NOT_FOUND, "party_not_found"),
        Ok(HostActionOutcome::AlreadyConfirmed) => error(StatusCode::CONFLICT, "already_confirmed"),
        Ok(HostActionOutcome::NotReported) => error(StatusCode::CONFLICT, "not_reported"),
        Ok(HostActionOutcome::MatchCancelled) => error(StatusCode::CONFLICT, "match_cancelled"),
        Ok(HostActionOutcome::MatchStarted) => error(StatusCode::CONFLICT, "match_started"),
        Err(err) => internal_error("failed to update payment", &err),
    }
}

async fn post_confirm(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path((share_id, code)): Path<(String, String)>,
) -> Response {
    host_action_response(&state, user.id, &share_id, &code, HostAction::Confirm).await
}

async fn post_reject(
    State(state): State<SlotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path((share_id, code)): Path<(String, String)>,
) -> Response {
    host_action_response(&state, user.id, &share_id, &code, HostAction::Reject).await
}
