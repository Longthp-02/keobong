//! HTTP adapter: the Zalo webhook and the web page's account linking.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::Path;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::domain::{COPY, ChatKind, ChatReply, Incoming, fill};
use super::polls::{AnswerOutcome, MAX_POLL_DAYS, Poll, PollReader, PollRepository};
use super::reminders::ReminderReader;
use super::repo::PgBotRepository;
use super::service::{BotContext, handle_message, redeem_link, run_due_polls, run_due_reminders};
use crate::auth::{AuthRejection, AuthState, AuthenticatedUser};
use crate::clock::Clock;
use crate::config::Secret;
use crate::drafts::PgDraftRepository;
use crate::drafts::domain::MatchTextReader;
use crate::matches::PgMatchRepository;
use crate::matches::domain::ShareId;
use crate::payments::PgPayoutRepository;

/// Header Zalo sends with every webhook call, carrying our secret.
const SECRET_HEADER: &str = "x-bot-api-secret-token";
/// Header the scheduler (Cloud Scheduler) sends to run due reminders.
const CRON_HEADER: &str = "x-cron-secret";

/// What the bot needs from Zalo; without it the bot routes answer 404.
pub struct ZaloSettings {
    pub reply: Arc<dyn ChatReply>,
    pub webhook_secret: Secret,
    pub bot_name: String,
    /// `None` makes the bot answer reminder requests with "resting".
    pub reminder_reader: Option<Arc<dyn ReminderReader>>,
    /// `None` makes the bot answer poll requests with "resting".
    pub poll_reader: Option<Arc<dyn PollReader>>,
    /// `None` disables the scheduler endpoint (404); reminders then never post.
    pub cron_secret: Option<Secret>,
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
        .route("/api/bot/reminders/run", post(run_reminders))
        .route("/api/polls/{share_id}", get(get_poll))
        .route("/api/polls/{share_id}/answers", put(answer_poll))
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
/// Zalo's webhook sends the update itself; `getUpdates` wraps it in
/// `{"ok": true, "result": ...}`. Both are accepted.
fn incoming(body: &Value) -> Option<Incoming> {
    let result = match body.get("result") {
        Some(result) if body["ok"] == true => result,
        _ => body,
    };
    if result["event_name"] != "message.text.received" {
        return None;
    }
    let message = &result["message"];
    if message["from"]["is_bot"] == true {
        return None;
    }
    let text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    // Ids longer than the database allows would fail later without a reply.
    let id = |value: &Value, max: usize| {
        let id = text(value)?;
        if id.len() > max {
            tracing::warn!(
                length = id.len(),
                "ignoring a Zalo update with an over-long id"
            );
            return None;
        }
        Some(id)
    };
    let chat_kind = match message["chat"]["chat_type"].as_str()? {
        "PRIVATE" => ChatKind::Private,
        "GROUP" => ChatKind::Group,
        _ => return None,
    };
    Some(Incoming {
        message_id: id(&message["message_id"], 128)?,
        chat_id: id(&message["chat"]["id"], 64)?,
        chat_kind,
        sender_id: id(&message["from"]["id"], 64)?,
        text: text(&message["text"])?,
    })
}

async fn webhook(State(state): State<BotState>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(zalo) = state.zalo.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let authentic = headers
        .get(SECRET_HEADER)
        .is_some_and(|value| same_secret(value.as_bytes(), zalo.webhook_secret.expose()));
    if !authentic {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" }))).into_response();
    }
    // Parsed only after the secret check. Zalo gets 200 for anything we could
    // not use, so it does not retry it.
    let Some(message) = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|body| incoming(&body))
    else {
        tracing::info!(
            event = body_event(&body),
            "ignoring a Zalo webhook body the bot does not answer"
        );
        return Json(json!({ "ok": true })).into_response();
    };
    let ctx = BotContext {
        bot: &state.bot,
        drafts: &state.drafts,
        matches: &state.matches,
        payouts: &state.payouts,
        clock: state.clock.as_ref(),
        reader: state.reader.as_deref(),
        reminder_reader: zalo.reminder_reader.as_deref(),
        poll_reader: zalo.poll_reader.as_deref(),
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
        Err(error) => {
            tracing::error!(error = %error, "bot could not handle a message");
            if let Err(error) = zalo.reply.send(&message.chat_id, &COPY.failed).await {
                tracing::error!(error = %error, "bot reply failed");
            }
        }
    }
    Json(json!({ "ok": true })).into_response()
}

/// Called every few minutes by the scheduler; posts the reminders that are due.
async fn run_reminders(State(state): State<BotState>, headers: HeaderMap) -> Response {
    let Some((zalo, secret)) = state
        .zalo
        .as_ref()
        .and_then(|zalo| zalo.cron_secret.as_ref().map(|secret| (zalo, secret)))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let authentic = headers
        .get(CRON_HEADER)
        .is_some_and(|value| same_secret(value.as_bytes(), secret.expose()));
    if !authentic {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" }))).into_response();
    }
    let reminders = run_due_reminders(&state.bot, state.clock.as_ref(), zalo.reply.as_ref()).await;
    let polls = run_due_polls(
        &state.bot,
        state.clock.as_ref(),
        zalo.reply.as_ref(),
        &state.frontend_origin,
    )
    .await;
    match reminders.and_then(|reminders| polls.map(|polls| (reminders, polls))) {
        Ok((sent, nudged)) => Json(json!({ "sent": sent, "nudged": nudged })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "could not run reminders");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal_error" })),
            )
                .into_response()
        }
    }
}

fn private(status: StatusCode, body: Value) -> Response {
    (status, [(CACHE_CONTROL, "private, no-store")], Json(body)).into_response()
}

/// A poll as the web page shows it: everyone's answers by name, the yes-counts
/// and, when signed in, the viewer's own answers.
async fn poll_view(
    state: &BotState,
    poll: &Poll,
    viewer: Option<crate::auth::UserId>,
) -> Result<Value, String> {
    let answers = state
        .bot
        .answers(poll.id)
        .await
        .map_err(|e| e.to_string())?;
    let day = |day: &chrono::NaiveDate| day.format("%Y-%m-%d").to_string();
    let mut players: Vec<(
        crate::auth::UserId,
        Option<String>,
        serde_json::Map<String, Value>,
    )> = Vec::new();
    for answer in &answers {
        let index = match players.iter().position(|(user, _, _)| *user == answer.user) {
            Some(index) => index,
            None => {
                players.push((answer.user, answer.name.clone(), serde_json::Map::new()));
                players.len() - 1
            }
        };
        players[index]
            .2
            .insert(day(&answer.day), Value::Bool(answer.attending));
    }
    let mine = viewer.map(|viewer| {
        players
            .iter()
            .find(|(user, _, _)| *user == viewer)
            .map_or_else(
                || Value::Object(serde_json::Map::new()),
                |(_, _, days)| Value::Object(days.clone()),
            )
    });
    let counts: Vec<Value> = poll
        .plan
        .days
        .iter()
        .map(|d| {
            let yes = answers
                .iter()
                .filter(|answer| answer.day == *d && answer.attending)
                .count();
            json!({ "day": day(d), "yes": yes })
        })
        .collect();
    Ok(json!({
        "title": poll.plan.title,
        "days": poll.plan.days.iter().map(day).collect::<Vec<_>>(),
        "closesAt": poll.closes_at.to_rfc3339(),
        "closed": poll.closes_at <= state.clock.now(),
        "counts": counts,
        "players": players
            .into_iter()
            .map(|(_, name, days)| json!({ "name": name, "answers": days }))
            .collect::<Vec<_>>(),
        "mine": mine,
    }))
}

async fn load_poll(state: &BotState, share_id: &str) -> Result<Option<Poll>, Response> {
    let Some(share_id) = ShareId::parse(share_id) else {
        return Ok(None);
    };
    state.bot.poll(&share_id).await.map_err(|error| {
        tracing::error!(error = %error, "failed to load a poll");
        private(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": "internal_error" }),
        )
    })
}

async fn get_poll(
    State(state): State<BotState>,
    Path(share_id): Path<String>,
    viewer: Result<AuthenticatedUser, AuthRejection>,
) -> Response {
    let poll = match load_poll(&state, &share_id).await {
        Ok(Some(poll)) => poll,
        Ok(None) => return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" })),
        Err(response) => return response,
    };
    let viewer = viewer.ok().map(|AuthenticatedUser(user)| user.id);
    match poll_view(&state, &poll, viewer).await {
        Ok(view) => private(StatusCode::OK, view),
        Err(error) => {
            tracing::error!(error = %error, "failed to load poll answers");
            private(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal_error" }),
            )
        }
    }
}

#[derive(Deserialize)]
struct AnswerItem {
    day: chrono::NaiveDate,
    attending: bool,
}

#[derive(Deserialize)]
struct AnswerRequest {
    answers: Vec<AnswerItem>,
}

async fn answer_poll(
    State(state): State<BotState>,
    Path(share_id): Path<String>,
    AuthenticatedUser(user): AuthenticatedUser,
    body: Result<Json<AnswerRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return private(
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_request" }),
        );
    };
    if request.answers.is_empty() || request.answers.len() > MAX_POLL_DAYS {
        return private(
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_request" }),
        );
    }
    let Some(share) = ShareId::parse(&share_id) else {
        return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" }));
    };
    let answers: Vec<_> = request
        .answers
        .iter()
        .map(|item| (item.day, item.attending))
        .collect();
    let outcome = state
        .bot
        .answer(&share, user.id, &answers, state.clock.now())
        .await;
    match outcome {
        Ok(AnswerOutcome::Saved) => {}
        Ok(AnswerOutcome::UnknownPoll) => {
            return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" }));
        }
        Ok(AnswerOutcome::Closed) => {
            return private(StatusCode::CONFLICT, json!({ "error": "poll_closed" }));
        }
        Ok(AnswerOutcome::UnknownDay) => {
            return private(StatusCode::BAD_REQUEST, json!({ "error": "invalid_day" }));
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to save poll answers");
            return private(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal_error" }),
            );
        }
    }
    get_poll(State(state), Path(share_id), Ok(AuthenticatedUser(user))).await
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
            let name = user
                .display_name
                .clone()
                .unwrap_or_else(|| COPY.anonymous_name.clone());
            let confirmation = fill(&COPY.linked, &[("name", &name)]);
            if let Err(error) = zalo.reply.send(&zalo_user_id, &confirmation).await {
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

/// The event name of a webhook body, for logs; never the message itself.
fn body_event(body: &[u8]) -> String {
    let Ok(body) = serde_json::from_slice::<Value>(body) else {
        return "invalid_json".to_owned();
    };
    let update = match body.get("result") {
        Some(result) if body["ok"] == true => result,
        _ => &body,
    };
    let event = update["event_name"].as_str().unwrap_or("missing");
    event.chars().take(64).collect()
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
        let bare = body["result"].clone();
        assert_eq!(incoming(&bare), incoming(&body));
        assert!(incoming(&bare).is_some());
        let not_ok = json!({ "ok": false, "result": bare });
        assert_eq!(incoming(&not_ok), None);
        assert_eq!(incoming(&json!(["not", "an", "object"])), None);
        let mut odd_chat = body;
        odd_chat["result"]["message"]["chat"]["chat_type"] = json!("CHANNEL");
        assert_eq!(incoming(&odd_chat), None);
    }
}
