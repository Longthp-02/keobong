//! HTTP adapter: the Zalo webhook and the web page's account linking.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::domain::{COPY, ChatKind, ChatReply, Incoming};
use super::repo::PgBotRepository;
use super::service::{BotContext, handle_message, redeem_link};
use crate::auth::{AuthState, AuthenticatedUser};
use crate::clock::Clock;
use crate::config::Secret;
use crate::drafts::PgDraftRepository;
use crate::drafts::domain::MatchTextReader;
use crate::matches::PgMatchRepository;
use crate::payments::PgPayoutRepository;

/// Header Zalo sends with every webhook call, carrying our secret.
const SECRET_HEADER: &str = "x-bot-api-secret-token";

/// What the bot needs from Zalo; without it the bot routes answer 404.
pub struct ZaloSettings {
    pub reply: Arc<dyn ChatReply>,
    pub webhook_secret: Secret,
    pub bot_name: String,
}

#[derive(Clone)]
pub struct BotState {
    pub bot: PgBotRepository,
    pub drafts: PgDraftRepository,
    pub matches: PgMatchRepository,
    pub payouts: PgPayoutRepository,
    pub clock: Arc<dyn Clock>,
    pub reader: Option<Arc<dyn MatchTextReader>>,
    pub zalo: Option<Arc<ZaloSettings>>,
    pub frontend_origin: String,
    pub auth: AuthState,
}

impl FromRef<BotState> for AuthState {
    fn from_ref(state: &BotState) -> Self {
        state.auth.clone()
    }
}

pub fn router(state: BotState) -> Router {
    Router::new()
        .route("/api/bot/zalo/webhook", post(webhook))
        .route("/api/bot/link", post(link))
        .with_state(state)
}

/// Compares secrets without leaking how much of them matched.
fn same_secret(given: &[u8], expected: &str) -> bool {
    let given = Sha256::digest(given);
    let expected = Sha256::digest(expected.as_bytes());
    given
        .iter()
        .zip(expected.iter())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

/// Reads a text message out of a webhook body; anything else is ignored.
fn incoming(body: &Value) -> Option<Incoming> {
    let result = &body["result"];
    if result["event_name"] != "message.text.received" {
        return None;
    }
    let message = &result["message"];
    if message["from"]["is_bot"] == true {
        return None;
    }
    let text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let chat_kind = match message["chat"]["chat_type"].as_str()? {
        "PRIVATE" => ChatKind::Private,
        "GROUP" => ChatKind::Group,
        _ => return None,
    };
    Some(Incoming {
        message_id: text(&message["message_id"])?,
        chat_id: text(&message["chat"]["id"]).filter(|id| id.len() <= 64)?,
        chat_kind,
        sender_id: text(&message["from"]["id"]).filter(|id| id.len() <= 64)?,
        text: text(&message["text"])?,
    })
}

async fn webhook(
    State(state): State<BotState>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    let Some(zalo) = state.zalo.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let authentic = headers
        .get(SECRET_HEADER)
        .is_some_and(|value| same_secret(value.as_bytes(), zalo.webhook_secret.expose()));
    if !authentic {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" }))).into_response();
    }
    // Zalo gets 200 for anything we could not use, so it does not retry it.
    let Some(message) = body.as_ref().and_then(|Json(body)| incoming(body)) else {
        return Json(json!({ "ok": true })).into_response();
    };
    let ctx = BotContext {
        bot: &state.bot,
        drafts: &state.drafts,
        matches: &state.matches,
        payouts: &state.payouts,
        clock: state.clock.as_ref(),
        reader: state.reader.as_deref(),
        frontend_origin: &state.frontend_origin,
        bot_name: &zalo.bot_name,
    };
    match handle_message(&ctx, &message).await {
        Ok(Some(reply)) => {
            if let Err(error) = zalo.reply.send(&message.chat_id, &reply).await {
                tracing::error!(error = %error, "bot reply failed");
            }
        }
        Ok(None) => {}
        // The message text is never logged.
        Err(error) => tracing::error!(error = %error, "bot could not handle a message"),
    }
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct LinkRequest {
    code: String,
}

async fn link(
    State(state): State<BotState>,
    AuthenticatedUser(user): AuthenticatedUser,
    body: Result<Json<LinkRequest>, JsonRejection>,
) -> Response {
    let respond = |status: StatusCode, body: Value| {
        (status, [(CACHE_CONTROL, "private, no-store")], Json(body)).into_response()
    };
    let Some(zalo) = state.zalo.clone() else {
        return respond(StatusCode::NOT_FOUND, json!({ "error": "bot_unavailable" }));
    };
    let Ok(Json(request)) = body else {
        return respond(
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_request" }),
        );
    };
    match redeem_link(&state.bot, state.clock.as_ref(), user.id, &request.code).await {
        Ok(Some(zalo_user_id)) => {
            // A private chat's id is the user's id; this confirms on the phone too.
            if let Err(error) = zalo.reply.send(&zalo_user_id, &COPY.linked).await {
                tracing::warn!(error = %error, "could not confirm a Zalo link in chat");
            }
            respond(StatusCode::OK, json!({ "linked": true }))
        }
        Ok(None) => respond(StatusCode::GONE, json!({ "error": "invalid_code" })),
        Err(error) => {
            tracing::error!(error = %error, "failed to link a Zalo account");
            respond(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal_error" }),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_must_match_exactly() {
        assert!(same_secret(b"s3cret-value", "s3cret-value"));
        assert!(!same_secret(b"s3cret-valu", "s3cret-value"));
        assert!(!same_secret(b"", "s3cret-value"));
    }

    #[test]
    fn reads_text_messages_and_ignores_the_rest() {
        let body = json!({ "ok": true, "result": {
            "event_name": "message.text.received",
            "message": {
                "chat": { "id": "zgr-1", "chat_type": "GROUP" },
                "text": "@Bot DaGhep test", "message_id": "m1", "date": 1,
                "from": { "id": "u1", "is_bot": false, "display_name": "Long" }
            }
        } });
        assert_eq!(
            incoming(&body),
            Some(Incoming {
                message_id: "m1".to_owned(),
                chat_id: "zgr-1".to_owned(),
                chat_kind: ChatKind::Group,
                sender_id: "u1".to_owned(),
                text: "@Bot DaGhep test".to_owned(),
            })
        );
        let mut sticker = body.clone();
        sticker["result"]["event_name"] = json!("message.sticker.received");
        assert_eq!(incoming(&sticker), None);
        let mut from_bot = body.clone();
        from_bot["result"]["message"]["from"]["is_bot"] = json!(true);
        assert_eq!(incoming(&from_bot), None);
        let mut odd_chat = body;
        odd_chat["result"]["message"]["chat"]["chat_type"] = json!("CHANNEL");
        assert_eq!(incoming(&odd_chat), None);
    }
}
