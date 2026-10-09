//! HTTP adapter: the bank list and the signed-in user's own payout account.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::domain::{BANKS, PayoutAccount, PayoutField};
use super::repo::PgPayoutRepository;
use super::service::{self, SavePayoutError};
use crate::auth::{AuthState, AuthenticatedUser};
use crate::clock::Clock;

#[derive(Clone)]
struct PaymentsState {
    repo: PgPayoutRepository,
    clock: Arc<dyn Clock>,
    auth: AuthState,
}

impl FromRef<PaymentsState> for AuthState {
    fn from_ref(state: &PaymentsState) -> Self {
        state.auth.clone()
    }
}

pub fn router(repo: PgPayoutRepository, clock: Arc<dyn Clock>, auth: AuthState) -> Router {
    Router::new()
        .route("/api/banks", get(get_banks))
        .route("/api/me/payout", get(get_payout).put(put_payout))
        .with_state(PaymentsState { repo, clock, auth })
}

const PRIVATE: &str = "private, no-store";

#[derive(Serialize)]
struct BankView {
    bin: &'static str,
    name: &'static str,
}

async fn get_banks() -> Response {
    let banks: Vec<BankView> = BANKS
        .iter()
        .map(|(bin, name)| BankView { bin, name })
        .collect();
    (
        [(CACHE_CONTROL, "public, max-age=86400")],
        Json(json!({ "banks": banks })),
    )
        .into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PayoutView {
    bank_bin: String,
    account_number: String,
    account_name: String,
}

impl From<PayoutAccount> for PayoutView {
    fn from(account: PayoutAccount) -> Self {
        Self {
            bank_bin: account.bank_bin,
            account_number: account.account_number,
            account_name: account.account_name,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayoutBody {
    bank_bin: String,
    account_number: String,
    account_name: String,
}

fn error(status: StatusCode, body: serde_json::Value) -> Response {
    (status, [(CACHE_CONTROL, PRIVATE)], Json(body)).into_response()
}

fn internal_error(context: &str, err: &dyn std::fmt::Display) -> Response {
    tracing::error!(error = %err, "{context}");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": "internal_error" }),
    )
}

async fn get_payout(
    State(state): State<PaymentsState>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> Response {
    match service::payout_account(&state.repo, user.id).await {
        Ok(Some(account)) => {
            ([(CACHE_CONTROL, PRIVATE)], Json(PayoutView::from(account))).into_response()
        }
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_payout_account" }),
        ),
        Err(err) => internal_error("failed to load payout account", &err),
    }
}

async fn put_payout(
    State(state): State<PaymentsState>,
    AuthenticatedUser(user): AuthenticatedUser,
    body: Result<Json<PayoutBody>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({ "error": "payload_too_large" }),
            );
        }
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                json!({ "error": "invalid_request" }),
            );
        }
    };
    let result = service::save_payout_account(
        &state.repo,
        state.clock.as_ref(),
        user.id,
        &body.bank_bin,
        &body.account_number,
        &body.account_name,
    )
    .await;
    match result {
        Ok(account) => {
            ([(CACHE_CONTROL, PRIVATE)], Json(PayoutView::from(account))).into_response()
        }
        Err(SavePayoutError::Invalid(field)) => {
            let field = match field {
                PayoutField::BankBin => "bankBin",
                PayoutField::AccountNumber => "accountNumber",
                PayoutField::AccountName => "accountName",
            };
            error(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "error": "invalid_payout_account", "field": field }),
            )
        }
        Err(SavePayoutError::Repo(err)) => internal_error("failed to save payout account", &err),
    }
}
