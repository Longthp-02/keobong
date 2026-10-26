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

use super::domain::{
    COPY, ChatKind, ChatReply, FindFuture, Incoming, MatchFinder, OpenMatchLine, fill,
};
use super::groups::{GroupRepository, JoinOutcome, Membership};
use super::maps::MapsLocator;
use super::polls::{AnswerOutcome, MAX_POLL_DAYS, Poll, PollReader, PollRepository, PublicGroup};
use super::reminders::ReminderReader;
use super::repo::PgBotRepository;
use super::service::{
    BotContext, handle_message, redeem_link, request_join, run_due_polls, run_due_reminders,
    run_due_series,
};
use crate::auth::{AuthRejection, AuthState, AuthenticatedUser, UserId};
use crate::clock::Clock;
use crate::config::Secret;
use crate::discovery::service::open_matches;
use crate::drafts::PgDraftRepository;
use crate::drafts::domain::MatchTextReader;
use crate::matches::PgMatchRepository;
use crate::matches::domain::ShareId;
use crate::matches::service::ListRequest;
use crate::payments::PgPayoutRepository;
use crate::slots::PgSlotRepository;

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
    /// Reads Google Maps links for new venues; `None` refuses them.
    pub maps: Option<Arc<dyn MapsLocator>>,
    /// Đá Ghép accounts that approve venues (`ADMIN_USER_IDS`).
    pub admins: Vec<UserId>,
    /// `None` disables the scheduler endpoint (404); reminders then never post.
    pub cron_secret: Option<Secret>,
}

#[derive(Clone)]
pub struct BotState {
    pub bot: PgBotRepository,
    pub drafts: PgDraftRepository,
    pub matches: PgMatchRepository,
    pub payouts: PgPayoutRepository,
    pub slots: PgSlotRepository,
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
        .route("/api/groups", get(get_groups))
        .route("/api/groups/{public_id}", get(get_group))
        .route("/api/groups/{public_id}/join", post(join_group))
        .route("/api/polls/{share_id}/answers", put(answer_poll))
        .with_state(state)
}

/// The public match list, as the bot reads it for players.
struct OpenMatchFinder {
    matches: PgMatchRepository,
    slots: PgSlotRepository,
    clock: Arc<dyn Clock>,
}

impl MatchFinder for OpenMatchFinder {
    fn find(&self, limit: usize) -> FindFuture<'_> {
        Box::pin(async move {
            let request = ListRequest {
                day: None,
                match_type: None,
                near: None,
                after: None,
            };
            let page = open_matches(&self.matches, &self.slots, self.clock.as_ref(), request)
                .await
                .map_err(|error| error.to_string())?;
            Ok(page
                .matches
                .into_iter()
                .take(limit)
                .map(|open| {
                    let found = open.listed.found;
                    OpenMatchLine {
                        share_id: found.share_id.as_str().to_owned(),
                        price_per_player_vnd: found.price_per_player_vnd(),
                        venue_name: found.venue_name,
                        starts_at: found.starts_at,
                        ends_at: found.ends_at,
                        format: found.format,
                        places_left: open.places_left,
                    }
                })
                .collect())
        })
    }
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
    let unsupported = match result["event_name"].as_str() {
        Some("message.text.received") => false,
        // Zalo sends a link with a preview this way; it may carry no text.
        Some("message.unsupported.received") => true,
        _ => return None,
    };
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
    if unsupported {
        // Which fields such a message has, one level deep, never their values,
        // to learn its shape; and whether it came from a group.
        let fields: Vec<String> = message
            .as_object()
            .map(|object| {
                object
                    .iter()
                    .map(|(key, value)| {
                        match value
                            .as_object()
                            .or_else(|| value.as_array()?.first()?.as_object())
                        {
                            Some(inner) => format!(
                                "{key}{{{}}}",
                                inner
                                    .keys()
                                    .map(String::as_str)
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                            None => key.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let chat_type = message["chat"]["chat_type"].as_str().unwrap_or("missing");
        tracing::info!(fields = ?fields, chat_type, "unsupported Zalo message");
    }
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
        text: if unsupported {
            text(&message["text"])
                .or_else(|| text(&message["caption"]))
                .unwrap_or_default()
        } else {
            text(&message["text"])?
        },
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
    let finder = OpenMatchFinder {
        matches: state.matches.clone(),
        slots: state.slots.clone(),
        clock: state.clock.clone(),
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
        maps: zalo.maps.as_deref(),
        admins: &zalo.admins,
        finder: Some(&finder),
        chat: Some(zalo.reply.as_ref()),
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
        &zalo.bot_name,
    )
    .await;
    let weekly = run_due_series(
        &state.bot,
        state.clock.as_ref(),
        zalo.reply.as_ref(),
        &state.frontend_origin,
        &zalo.bot_name,
    )
    .await;
    match reminders.and_then(|reminders| {
        polls.and_then(|polls| weekly.map(|weekly| (reminders, polls, weekly)))
    }) {
        Ok((sent, nudged, weekly)) => {
            Json(json!({ "sent": sent, "nudged": nudged, "weeklyPolls": weekly })).into_response()
        }
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

/// Public weekly groups for the home page. No member names; cached briefly.
async fn get_groups(State(state): State<BotState>) -> Response {
    let groups = match state.bot.public_groups(state.clock.now(), None).await {
        Ok(groups) => groups,
        Err(error) => {
            tracing::error!(error = %error, "failed to load public groups");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal_error" })),
            )
                .into_response();
        }
    };
    let body: Vec<Value> = groups.iter().map(group_json).collect();
    (
        [(CACHE_CONTROL, "public, max-age=60")],
        Json(json!({ "groups": body })),
    )
        .into_response()
}

/// A public group as the web shows it: no member names.
fn group_json(group: &PublicGroup) -> Value {
    let day = |day: &chrono::NaiveDate| day.format("%Y-%m-%d").to_string();
    json!({
        "id": group.public_id,
        "title": group.title,
        "sport": group.group.sport.map(|sport| sport.as_str()),
        "format": group.group.format.map(|format| format.as_str()),
        "venue": group.venue_name.as_ref().map(|name| json!({
            "name": name,
            "address": group.venue_address,
        })),
        // Sunday = 0 … Saturday = 6.
        "weekdays": (0..7).filter(|day| group.weekdays & (1 << day) != 0).collect::<Vec<u8>>(),
        "playTime": group.play_time.map(|time| time.format("%H:%M").to_string()),
        "week": group.week.as_ref().map(|(yes, players)| json!({
            "players": players,
            "yes": yes.iter().map(|(d, n)| json!({ "day": day(d), "yes": n })).collect::<Vec<_>>(),
        })),
    })
}

fn membership_json(membership: &Membership) -> Value {
    match membership {
        Membership::Outside => json!({ "status": "none" }),
        Membership::Pending => json!({ "status": "pending" }),
        Membership::Declined { ask_again_at } => json!({
            "status": "declined",
            "askAgainAt": ask_again_at.to_rfc3339(),
        }),
        Membership::Member { group_link, poll } => json!({
            "status": "member",
            "groupLink": group_link,
            "pollPath": poll.as_ref().map(|poll| format!("/p/{}", poll.as_str())),
        }),
    }
}

/// Public ids are 12 lower-case hex characters; anything else is unknown.
fn valid_public_id(id: &str) -> bool {
    id.len() == 12
        && id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// One public group, and where the signed-in viewer stands with it.
async fn get_group(
    State(state): State<BotState>,
    Path(public_id): Path<String>,
    viewer: Result<AuthenticatedUser, AuthRejection>,
) -> Response {
    let internal = || {
        private(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": "internal_error" }),
        )
    };
    if !valid_public_id(&public_id) {
        return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" }));
    }
    let viewer = match viewer {
        Ok(AuthenticatedUser(user)) => Some(user.id),
        Err(AuthRejection::Unauthenticated) => None,
        Err(AuthRejection::Internal) => return internal(),
    };
    let now = state.clock.now();
    let group = match state.bot.public_groups(now, Some(&public_id)).await {
        Ok(groups) => groups.into_iter().next(),
        Err(error) => {
            tracing::error!(error = %error, "failed to load a public group");
            return internal();
        }
    };
    let Some(group) = group else {
        return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" }));
    };
    let membership = match viewer {
        Some(user) => match state.bot.membership(&public_id, user, now).await {
            Ok(membership) => membership,
            Err(error) => {
                tracing::error!(error = %error, "failed to load a group membership");
                return internal();
            }
        },
        None => None,
    };
    let mut body = group_json(&group);
    body["membership"] = membership.as_ref().map_or(Value::Null, membership_json);
    private(StatusCode::OK, body)
}

/// Asks to join a public group as the signed-in player.
async fn join_group(
    State(state): State<BotState>,
    Path(public_id): Path<String>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> Response {
    let internal = || {
        private(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": "internal_error" }),
        )
    };
    if !valid_public_id(&public_id) {
        return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" }));
    }
    let now = state.clock.now();
    let chat = state.zalo.as_ref().map(|zalo| zalo.reply.as_ref());
    let outcome = match request_join(&state.bot, chat, &public_id, user.id, now).await {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::error!(error = %error, "failed to save a join request");
            return internal();
        }
    };
    match outcome {
        JoinOutcome::Requested(_) | JoinOutcome::AlreadyPending | JoinOutcome::AlreadyMember => {}
        JoinOutcome::TooSoon { ask_again_at } => {
            return private(
                StatusCode::CONFLICT,
                json!({ "error": "too_soon", "askAgainAt": ask_again_at.to_rfc3339() }),
            );
        }
        JoinOutcome::PlayerLimit => {
            return private(
                StatusCode::TOO_MANY_REQUESTS,
                json!({ "error": "too_many_requests" }),
            );
        }
        JoinOutcome::OrganiserLimit => {
            return private(StatusCode::CONFLICT, json!({ "error": "group_busy" }));
        }
        JoinOutcome::UnknownGroup => {
            return private(StatusCode::NOT_FOUND, json!({ "error": "not_found" }));
        }
    }
    match state.bot.membership(&public_id, user.id, now).await {
        Ok(Some(membership)) => private(
            StatusCode::OK,
            json!({ "membership": membership_json(&membership) }),
        ),
        Ok(None) => private(StatusCode::NOT_FOUND, json!({ "error": "not_found" })),
        Err(error) => {
            tracing::error!(error = %error, "failed to load a group membership");
            internal()
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
        "playTime": poll.plan.play_time.map(|time| time.format("%H:%M").to_string()),
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

/// The poll behind a share id; `Err` is a storage error, already logged.
async fn load_poll(state: &BotState, share_id: &str) -> Result<Option<Poll>, ()> {
    let Some(share_id) = ShareId::parse(share_id) else {
        return Ok(None);
    };
    state.bot.poll(&share_id).await.map_err(|error| {
        tracing::error!(error = %error, "failed to load a poll");
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
        Err(()) => {
            return private(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal_error" }),
            );
        }
    };
    // A failed session lookup must not show a signed-in player as signed out.
    let viewer = match viewer {
        Ok(AuthenticatedUser(user)) => Some(user.id),
        Err(AuthRejection::Unauthenticated) => None,
        Err(AuthRejection::Internal) => {
            return private(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal_error" }),
            );
        }
    };
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
        // A message Zalo cannot show as text (a link with a preview): its text
        // when it has one, otherwise empty, so the bot can say what to do.
        let mut link = body["result"].clone();
        link["event_name"] = json!("message.unsupported.received");
        assert_eq!(incoming(&link), incoming(&body));
        link["message"].as_object_mut().unwrap().remove("text");
        assert_eq!(
            incoming(&link).map(|message| message.text),
            Some(String::new())
        );
        let not_ok = json!({ "ok": false, "result": bare });
        assert_eq!(incoming(&not_ok), None);
        assert_eq!(incoming(&json!(["not", "an", "object"])), None);
        let mut odd_chat = body;
        odd_chat["result"]["message"]["chat"]["chat_type"] = json!("CHANNEL");
        assert_eq!(incoming(&odd_chat), None);
    }
}
