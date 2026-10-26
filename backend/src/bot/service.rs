//! Use cases of the Zalo bot: answer one chat message, and link a Zalo account
//! from the web. Matches are created through `matches::service::create_match`,
//! messages are read through `drafts::service::read_draft` (same daily limits).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Datelike, Duration, Utc};
use sha2::{Digest, Sha256};

use super::answers::{AnswerIntent, ResolveError, read_answer};
use super::domain::{
    BotDraft, BotRepository, COPY, ChatKind, ChatReply, Command, DEFAULT_LEVEL_MAX_TENTHS,
    DEFAULT_LEVEL_MIN_TENTHS, DEFAULT_MATCH_TYPE, DRAFT_MINUTES, Incoming, LINK_CODE_MINUTES,
    MatchFinder, UpdateOutcome, entry, fill, mentions_find_phrase, parse_command,
};
use super::groups::{DecideOutcome, GroupRepository, JoinOutcome, find_group_link};
use super::maps::{MapsLocator, find_maps_link};
use super::polls::{
    AnswerOutcome, MAX_NAMES_IN_NUDGE, MAX_SERIES_LATE_HOURS, MEMBER_DAYS, PendingPoll, Poll,
    PollAnswer, PollGroup, PollPlan, PollReader, PollRepository, PollRequest, PollSeries, RawPoll,
    first_week, tally, weekday_bits,
};
use super::reminders::{
    EVERY_DAY, MAX_LATE_MINUTES, PendingReminder, ReminderReader, ReminderRequest, Repeat, Schedule,
};
use crate::auth::UserId;
use crate::clock::Clock;
use crate::drafts::domain::MAX_TEXT_CHARS;
use crate::drafts::domain::{DraftRepository, LimitScope, MatchDraft, MatchTextReader};
use crate::drafts::service::{ReadDraftError, read_draft, use_read};
use crate::matches;
use crate::matches::domain::{
    Field, Level, MatchRepository, NewMatch, NewMatchInput, ShareId, local_date, local_instant,
    local_time, price_per_player_vnd,
};
use crate::matches::service::{CreateMatchError, create_match};
use crate::matches::venues::{
    DecideError, GameFormat, GroupVenue, MAX_VENUE_NAME_CHARS, Sport, VenueRepository, VenueStatus,
    VenueSubmission, in_vietnam, new_slug,
};
use crate::payments::domain::PayoutRepository;
use crate::payments::service::has_payout_account;
use crate::random::random_bytes;
use crate::text::clean_name;

/// What one message needs: storage, the other features' ports and settings.
pub struct BotContext<'a, B, D, M, P> {
    pub bot: &'a B,
    pub drafts: &'a D,
    pub matches: &'a M,
    pub payouts: &'a P,
    pub clock: &'a dyn Clock,
    pub reader: Option<&'a dyn MatchTextReader>,
    pub reminder_reader: Option<&'a dyn ReminderReader>,
    pub poll_reader: Option<&'a dyn PollReader>,
    /// Lists open matches for players; `None` points them to the website.
    pub finder: Option<&'a dyn MatchFinder>,
    /// Sends private messages (a link code to someone who tried the bot in a
    /// group); `None` only replies in the chat.
    pub chat: Option<&'a dyn ChatReply>,
    /// Reads a Google Maps link's place; `None` refuses new venues.
    pub maps: Option<&'a dyn MapsLocator>,
    /// Đá Ghép accounts that approve venues.
    pub admins: &'a [UserId],
    /// The web app's origin, for links in replies.
    pub frontend_origin: &'a str,
    /// The bot's display name, as tagged in groups ("Bot DaGhep").
    pub bot_name: &'a str,
}

#[derive(Debug, thiserror::Error)]
#[error("bot failed: {0}")]
pub struct BotError(pub String);

fn fail(error: impl std::fmt::Display) -> BotError {
    BotError(error.to_string())
}

/// Answers one message. `None` means no reply: a repeated delivery, or a group
/// message that does not tag the bot.
pub async fn handle_message<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
) -> Result<Option<String>, BotError>
where
    B: BotRepository + PollRepository + VenueRepository + GroupRepository,
    D: DraftRepository,
    M: MatchRepository,
    P: PayoutRepository,
{
    // A message Zalo could not pass on as text (a link with a preview, a voice
    // note): the bot says it only reads text. Zalo may pass such group messages
    // on without a tag, so a group gets this at most once an hour.
    if message.text.trim().is_empty() {
        let now = ctx.clock.now();
        let outcome = ctx
            .bot
            .record_update(&message.message_id, &message.sender_id, now)
            .await
            .map_err(fail)?;
        if outcome != UpdateOutcome::New {
            return Ok(None);
        }
        let mention = match message.chat_kind {
            ChatKind::Group => {
                let hour = local_time(now).format("%Y%m%d%H");
                let hint_id = format!("hint:{}:{hour}", message.chat_id);
                let first = ctx
                    .bot
                    .record_update(&hint_id, &message.chat_id, now)
                    .await
                    .map_err(fail)?;
                if first != UpdateOutcome::New {
                    return Ok(None);
                }
                format!("@{} ", ctx.bot_name)
            }
            ChatKind::Private => String::new(),
        };
        return Ok(Some(fill(
            &COPY.unsupported_message,
            &[("mention", &mention)],
        )));
    }
    let Some(command) = parse_command(&message.text, message.chat_kind, ctx.bot_name) else {
        return Ok(None);
    };
    let now = ctx.clock.now();
    let outcome = ctx
        .bot
        .record_update(&message.message_id, &message.sender_id, now)
        .await
        .map_err(fail)?;
    if outcome != UpdateOutcome::New {
        return Ok(None);
    }
    // A linked sender seen in a group joins its roster, so nudges can name them.
    if message.chat_kind == ChatKind::Group
        && let Some(user) = ctx
            .bot
            .linked_user(&message.sender_id)
            .await
            .map_err(fail)?
    {
        // Bookkeeping for nudges only: a failure must not block the command.
        if let Err(error) = PollRepository::note_member(ctx.bot, &message.chat_id, user, now).await
        {
            tracing::warn!(error = %error, "could not note a chat member");
        }
    }
    // In groups every command must tag the bot, so the examples do too.
    let mention = match message.chat_kind {
        ChatKind::Group => format!("@{} ", ctx.bot_name),
        ChatKind::Private => String::new(),
    };
    let reply = match command {
        Command::Help => fill(&COPY.help, &[("mention", &mention), ("bot", ctx.bot_name)]),
        Command::Link => link(ctx, message, now).await?,
        Command::Cancel => {
            let draft = ctx
                .bot
                .take_draft(&message.sender_id, &message.chat_id, now)
                .await
                .map_err(fail)?;
            let reminder = ctx
                .bot
                .take_pending_reminder(&message.sender_id, &message.chat_id, now)
                .await
                .map_err(fail)?;
            let poll = ctx
                .bot
                .take_pending_poll(&message.sender_id, &message.chat_id, now)
                .await
                .map_err(fail)?;
            if draft.is_some() || reminder.is_some() || poll.is_some() {
                COPY.cancelled.clone()
            } else {
                COPY.no_draft.clone()
            }
        }
        Command::Confirm => match ctx
            .bot
            .take_pending_reminder(&message.sender_id, &message.chat_id, now)
            .await
            .map_err(fail)?
        {
            Some(pending) => save_reminder(ctx, message, pending, &mention, now).await?,
            None => match ctx
                .bot
                .take_pending_poll(&message.sender_id, &message.chat_id, now)
                .await
                .map_err(fail)?
            {
                Some(pending) => save_poll(ctx, message, pending, &mention, now).await?,
                None => confirm(ctx, message).await?,
            },
        },
        Command::Poll(text) => poll(ctx, message, &text, &mention, now).await?,
        Command::StopPoll(number) => stop_poll_series(ctx, message, number, &mention).await?,
        Command::Remind(text) => remind(ctx, message, &text, &mention, now).await?,
        Command::ListReminders => list_reminders(ctx, message, &mention).await?,
        Command::StopReminder(number) => stop_reminder(ctx, message, number, &mention).await?,
        Command::FindMatches => find_matches(ctx).await,
        Command::AddVenue(text) => add_venue(ctx, message, &text, &mention, now).await?,
        Command::DecideVenue { id, approve } => decide_venue(ctx, message, id, approve).await?,
        Command::PendingVenues => pending_venues(ctx, message).await?,
        Command::PublishPoll { number, public } => {
            publish_poll(ctx, message, number, public, &mention).await?
        }
        Command::DecideJoin { id, approve } => decide_join(ctx, message, id, approve, now).await?,
        Command::SetGroupLink(text) => set_group_link(ctx, message, &text, &mention).await?,
        Command::Describe(text) => describe(ctx, message, &text, &mention, now).await?,
    };
    Ok(Some(reply))
}

async fn link<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository,
{
    // A link posted in a group could be opened by someone else, who would then
    // host (and be paid for) this sender's matches: it only goes privately.
    if message.chat_kind == ChatKind::Group {
        return Ok(if send_link_privately(ctx, message, now).await? {
            fill(&COPY.link_sent_privately, &[("bot", ctx.bot_name)])
        } else {
            fill(&COPY.link_in_group, &[("bot", ctx.bot_name)])
        });
    }
    link_text(ctx, message, now).await
}

/// A new link code for the sender, as the private message that carries it.
async fn link_text<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository,
{
    let code = URL_SAFE_NO_PAD.encode(random_bytes::<32>().map_err(fail)?);
    ctx.bot
        .save_link_code(
            hash_code(&code),
            &message.sender_id,
            now + Duration::minutes(LINK_CODE_MINUTES),
            now,
        )
        .await
        .map_err(fail)?;
    let url = format!("{}/bot/link?code={code}", ctx.frontend_origin);
    Ok(fill(&COPY.link_private, &[("url", &url)]))
}

/// Sends the sender a link code in their private chat with the bot (a Zalo
/// user's private chat id is their user id). Returns whether Zalo took it; it
/// may refuse a user who never wrote to the bot.
async fn send_link_privately<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    now: DateTime<Utc>,
) -> Result<bool, BotError>
where
    B: BotRepository,
{
    let Some(chat) = ctx.chat else {
        return Ok(false);
    };
    let text = link_text(ctx, message, now).await?;
    match chat.send(&message.sender_id, &text).await {
        Ok(()) => Ok(true),
        Err(error) => {
            tracing::info!(error = %error, "could not send a link code privately");
            Ok(false)
        }
    }
}

/// The reply to an organiser's request from someone not linked yet: in a group
/// the bot tries to send them the link privately.
async fn unlinked<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository,
{
    if message.chat_kind == ChatKind::Group && send_link_privately(ctx, message, now).await? {
        return Ok(fill(&COPY.link_sent_privately, &[("bot", ctx.bot_name)]));
    }
    Ok(fill(&COPY.not_linked, &[("bot", ctx.bot_name)]))
}

pub fn hash_code(code: &str) -> [u8; 32] {
    Sha256::digest(code.as_bytes()).into()
}

async fn describe<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    text: &str,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository + PollRepository,
    D: DraftRepository,
    M: MatchRepository,
    P: PayoutRepository,
{
    // A player answering the chat's poll ("T7 có, CN không"): no AI read.
    if let Some(intent) = read_answer(text)
        && let Some(reply) = answer_in_chat(ctx, message, &intent, mention, now).await?
    {
        return Ok(reply);
    }
    let Some(user) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        // Someone who is not an organiser asking for a match ("có kèo 19h không")
        // gets the open matches rather than the linking steps.
        if mentions_find_phrase(text) {
            // It may be a new organiser's post too: say how to post matches.
            return Ok(format!(
                "{}\n{}",
                find_matches(ctx).await,
                fill(&COPY.organiser_hint, &[("bot", ctx.bot_name)])
            ));
        }
        return unlinked(ctx, message, now).await;
    };
    let create_url = format!("{}/create", ctx.frontend_origin);
    let draft = match read_draft(ctx.drafts, ctx.matches, ctx.clock, ctx.reader, user, text).await {
        Ok(draft) => draft,
        Err(ReadDraftError::InvalidText) => return Ok(COPY.too_long.clone()),
        Err(ReadDraftError::Limited(LimitScope::User)) => {
            return Ok(fill(&COPY.limit_user, &[("url", &create_url)]));
        }
        Err(ReadDraftError::Limited(LimitScope::Service)) => {
            return Ok(fill(&COPY.limit_service, &[("url", &create_url)]));
        }
        Err(ReadDraftError::Unavailable) => {
            return Ok(fill(&COPY.unavailable, &[("url", &create_url)]));
        }
        Err(ReadDraftError::Reader(error)) => {
            tracing::warn!(error = %error, "bot could not read a match message");
            return Ok(COPY.failed.clone());
        }
        Err(ReadDraftError::Storage(error)) => return Err(BotError(error)),
    };
    if draft == MatchDraft::default() {
        // Next to an open poll, chatter is most likely a player unsure how to answer.
        let polls = ctx
            .bot
            .open_polls(&message.chat_id, now)
            .await
            .map_err(fail)?;
        if let Some(poll) = polls.first() {
            return Ok(fill(
                &COPY.answer_hint,
                &[
                    ("mention", mention),
                    ("example", &answer_example(&poll.plan)),
                    ("url", &poll_url(ctx.frontend_origin, poll)),
                ],
            ));
        }
        return Ok(fill(&COPY.not_a_match, &[("mention", mention)]));
    }
    let venues = matches::service::venues(ctx.matches).await.map_err(fail)?;
    let input = match complete(&draft) {
        Ok(input) => input,
        Err(missing) => {
            let venue_names = venues
                .iter()
                .map(|venue| venue.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let fields = missing
                .iter()
                .map(|field| fill(entry(&COPY.fields, field), &[("venues", &venue_names)]))
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(fill(
                &COPY.missing,
                &[("fields", &fields), ("mention", mention)],
            ));
        }
    };
    let new = match NewMatch::validate(input.clone(), now) {
        Ok(new) => new,
        Err(field) => return Ok(invalid(field)),
    };
    let has_payout = has_payout_account(ctx.payouts, user).await.map_err(fail)?;
    if new.total_fee_vnd > 0 && !has_payout {
        let url = format!("{}/account/payout", ctx.frontend_origin);
        return Ok(fill(&COPY.payout_needed, &[("url", &url)]));
    }
    let venue_name = venues
        .iter()
        .find(|venue| venue.slug.as_str() == input.venue)
        .map_or(input.venue.as_str(), |venue| venue.name.as_str());
    let pending = BotDraft {
        zalo_user_id: message.sender_id.clone(),
        chat_id: message.chat_id.clone(),
        user,
        venue_slug: input.venue.clone(),
        starts_at: new.starts_at,
        ends_at: new.ends_at,
        format: new.format,
        match_type: new.match_type,
        level_min_tenths: new.level_min.tenths(),
        level_max_tenths: new.level_max.tenths(),
        slot_count: new.slot_count,
        total_fee_vnd: new.total_fee_vnd,
        expires_at: now + Duration::minutes(DRAFT_MINUTES),
    };
    ctx.bot.save_draft(&pending).await.map_err(fail)?;
    Ok(format!(
        "{}\n{}",
        summary(&pending, venue_name),
        fill(&COPY.confirm_hint, &[("mention", mention)])
    ))
}

/// Turns a checked draft into create input, or lists what the message left out.
fn complete(draft: &MatchDraft) -> Result<NewMatchInput, Vec<&'static str>> {
    let mut missing = Vec::new();
    if draft.venue_id.is_none() {
        missing.push("venue");
    }
    if draft.date.is_none() {
        missing.push("date");
    }
    if draft.start_time.is_none() {
        missing.push("startTime");
    }
    if draft.end_time.is_none() {
        missing.push("endTime");
    }
    if draft.format.is_none() {
        missing.push("format");
    }
    if draft.slot_count.is_none() {
        missing.push("slotCount");
    }
    if draft.total_fee_vnd.is_none() {
        missing.push("fee");
    }
    let (
        Some(venue),
        Some(date),
        Some(start),
        Some(end),
        Some(format),
        Some(slot_count),
        Some(total_fee_vnd),
    ) = (
        draft.venue_id.clone(),
        draft.date,
        draft.start_time,
        draft.end_time,
        draft.format,
        draft.slot_count,
        draft.total_fee_vnd,
    )
    else {
        return Err(missing);
    };
    // One stated level keeps the range valid by moving the other end to meet it.
    let default_min = f64::from(DEFAULT_LEVEL_MIN_TENTHS) / 10.0;
    let default_max = f64::from(DEFAULT_LEVEL_MAX_TENTHS) / 10.0;
    let (level_min, level_max) = match (draft.level_min, draft.level_max) {
        (Some(min), Some(max)) => (min, max),
        (Some(min), None) => (min, default_max.max(min)),
        (None, Some(max)) => (default_min.min(max), max),
        (None, None) => (default_min, default_max),
    };
    Ok(NewMatchInput {
        venue,
        starts_at: local_instant(date, start),
        ends_at: local_instant(date, end),
        format,
        match_type: draft.match_type.unwrap_or(DEFAULT_MATCH_TYPE),
        level_min,
        level_max,
        total_fee_vnd,
        slot_count: Some(slot_count),
    })
}

fn invalid(field: Field) -> String {
    let key = match field {
        Field::Venue => "venueId",
        Field::StartsAt => "startsAt",
        Field::EndsAt => "endsAt",
        Field::SlotCount => "slotCount",
        Field::TotalFeeVnd => "totalFeeVnd",
        _ => "other",
    };
    fill(&COPY.invalid, &[("reason", entry(&COPY.reasons, key))])
}

fn summary(draft: &BotDraft, venue_name: &str) -> String {
    let start = local_time(draft.starts_at);
    let end = local_time(draft.ends_at);
    let weekday = COPY
        .weekdays
        .get(start.weekday().num_days_from_sunday() as usize)
        .map_or("", String::as_str);
    let day = format!("{weekday} {}", start.format("%d/%m"));
    let price = if draft.total_fee_vnd == 0 {
        COPY.free.clone()
    } else {
        let amount = format_vnd(price_per_player_vnd(draft.total_fee_vnd, draft.slot_count));
        fill(&COPY.per_player, &[("amount", &amount)])
    };
    fill(
        &COPY.summary,
        &[
            ("day", &day),
            ("start", &start.format("%H:%M").to_string()),
            ("end", &end.format("%H:%M").to_string()),
            ("venue", venue_name),
            ("format", entry(&COPY.formats, draft.format.as_str())),
            ("slots", &draft.slot_count.to_string()),
            ("price", &price),
        ],
    )
}

/// 60000 → "60.000đ", as Vietnamese readers write prices.
pub fn format_vnd(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(digit);
    }
    let sign = if amount < 0 { "-" } else { "" };
    fill(&COPY.currency, &[("amount", &format!("{sign}{grouped}"))])
}

async fn confirm<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
) -> Result<String, BotError>
where
    B: BotRepository,
    M: MatchRepository,
    P: PayoutRepository,
{
    let now = ctx.clock.now();
    let Some(draft) = ctx
        .bot
        .take_draft(&message.sender_id, &message.chat_id, now)
        .await
        .map_err(fail)?
    else {
        return Ok(COPY.no_draft.clone());
    };
    // The draft was read for whoever this Zalo account was linked to then.
    let linked = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?;
    if linked != Some(draft.user) {
        return Ok(fill(&COPY.not_linked, &[("bot", ctx.bot_name)]));
    }
    let level = |tenths: i16| {
        Level::from_tenths(tenths)
            .map(Level::as_f64)
            .ok_or_else(|| BotError(format!("stored draft has an invalid level {tenths}")))
    };
    let input = NewMatchInput {
        venue: draft.venue_slug,
        starts_at: draft.starts_at,
        ends_at: draft.ends_at,
        format: draft.format,
        match_type: draft.match_type,
        level_min: level(draft.level_min_tenths)?,
        level_max: level(draft.level_max_tenths)?,
        total_fee_vnd: draft.total_fee_vnd,
        slot_count: Some(i64::from(draft.slot_count)),
    };
    let has_payout = has_payout_account(ctx.payouts, draft.user)
        .await
        .map_err(fail)?;
    match create_match(ctx.matches, ctx.clock, draft.user, has_payout, input).await {
        Ok(created) => {
            let url = format!("{}/m/{}", ctx.frontend_origin, created.share_id.as_str());
            Ok(fill(&COPY.created, &[("url", &url)]))
        }
        Err(CreateMatchError::Invalid(Field::Payout)) => {
            let url = format!("{}/account/payout", ctx.frontend_origin);
            Ok(fill(&COPY.payout_needed, &[("url", &url)]))
        }
        Err(CreateMatchError::Invalid(field)) => Ok(invalid(field)),
        Err(error) => Err(fail(error)),
    }
}

async fn remind<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    text: &str,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository,
    D: DraftRepository,
{
    let Some(user) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        return unlinked(ctx, message, now).await;
    };
    let text = text.trim();
    if text.chars().count() > MAX_TEXT_CHARS {
        return Ok(COPY.too_long.clone());
    }
    let create_url = format!("{}/create", ctx.frontend_origin);
    let Some(reader) = ctx.reminder_reader else {
        return Ok(fill(&COPY.unavailable, &[("url", &create_url)]));
    };
    if let Some(limited) = use_ai_read(ctx, user, now, &create_url).await? {
        return Ok(limited);
    }
    let raw = match reader
        .read(ReminderRequest {
            text,
            today: local_date(now),
        })
        .await
    {
        Ok(raw) => raw,
        Err(error) => {
            tracing::warn!(error = %error, "bot could not read a reminder");
            return Ok(COPY.failed.clone());
        }
    };
    let plan = match raw.checked() {
        Ok(plan) => plan,
        Err(gaps) => {
            let fields = gaps
                .iter()
                .map(|gap| entry(&COPY.reminder_gaps, gap.key()))
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(fill(
                &COPY.reminder_missing,
                &[("fields", &fields), ("mention", mention)],
            ));
        }
    };
    let pending = PendingReminder {
        zalo_user_id: message.sender_id.clone(),
        chat_id: message.chat_id.clone(),
        user,
        plan,
        expires_at: now + Duration::minutes(DRAFT_MINUTES),
    };
    ctx.bot
        .save_pending_reminder(&pending)
        .await
        .map_err(fail)?;
    let (when, time) = describe_schedule(&pending.plan.schedule);
    Ok(format!(
        "{}\n{}",
        fill(
            &COPY.reminder_summary,
            &[
                ("when", &when),
                ("time", &time),
                ("text", &pending.plan.text)
            ]
        ),
        fill(&COPY.reminder_confirm_hint, &[("mention", mention)])
    ))
}

async fn save_reminder<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    pending: PendingReminder,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository,
{
    // Read under whoever this Zalo account was linked to then.
    let linked = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?;
    if linked != Some(pending.user) {
        return Ok(fill(&COPY.not_linked, &[("bot", ctx.bot_name)]));
    }
    let next = pending.plan.schedule.next_run(now);
    let saved = ctx
        .bot
        .add_reminder(&pending.chat_id, pending.user, &pending.plan, next, now)
        .await
        .map_err(fail)?;
    if !saved {
        return Ok(fill(&COPY.reminder_limit, &[("mention", mention)]));
    }
    Ok(fill(
        &COPY.reminder_saved,
        &[("next", &moment(next)), ("mention", mention)],
    ))
}

async fn list_reminders<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    mention: &str,
) -> Result<String, BotError>
where
    B: BotRepository,
{
    let reminders = ctx.bot.reminders(&message.chat_id).await.map_err(fail)?;
    if reminders.is_empty() {
        return Ok(fill(&COPY.no_reminders, &[("mention", mention)]));
    }
    let items = reminders
        .iter()
        .enumerate()
        .map(|(index, reminder)| {
            let (when, time) = describe_schedule(&reminder.plan.schedule);
            fill(
                &COPY.reminder_list_item,
                &[
                    ("n", &(index + 1).to_string()),
                    ("when", &when),
                    ("time", &time),
                    ("text", &reminder.plan.text),
                ],
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(fill(
        &COPY.reminder_list,
        &[("items", &items), ("mention", mention)],
    ))
}

async fn stop_reminder<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    number: Option<i64>,
    mention: &str,
) -> Result<String, BotError>
where
    B: BotRepository,
{
    let Some(number) = number else {
        return list_reminders(ctx, message, mention).await;
    };
    // Only linked members may stop a group's reminders.
    if ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
        .is_none()
    {
        return Ok(fill(&COPY.not_linked, &[("bot", ctx.bot_name)]));
    }
    let reminders = ctx.bot.reminders(&message.chat_id).await.map_err(fail)?;
    let n = number.to_string();
    let target = usize::try_from(number)
        .ok()
        .and_then(|number| number.checked_sub(1))
        .and_then(|index| reminders.get(index));
    let Some(target) = target else {
        return Ok(fill(
            &COPY.reminder_not_found,
            &[("n", &n), ("mention", mention)],
        ));
    };
    if ctx
        .bot
        .delete_reminder(&message.chat_id, target.id)
        .await
        .map_err(fail)?
    {
        Ok(fill(&COPY.reminder_stopped, &[("n", &n)]))
    } else {
        Ok(fill(
            &COPY.reminder_not_found,
            &[("n", &n), ("mention", mention)],
        ))
    }
}

async fn poll<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    text: &str,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository + PollRepository + VenueRepository,
    D: DraftRepository,
{
    let Some(user) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        return unlinked(ctx, message, now).await;
    };
    let text = text.trim();
    if text.chars().count() > MAX_TEXT_CHARS {
        return Ok(COPY.too_long.clone());
    }
    let create_url = format!("{}/create", ctx.frontend_origin);
    let Some(reader) = ctx.poll_reader else {
        return Ok(fill(&COPY.unavailable, &[("url", &create_url)]));
    };
    if let Some(limited) = use_ai_read(ctx, user, now, &create_url).await? {
        return Ok(limited);
    }
    let venues = ctx
        .bot
        .venues_with_status(VenueStatus::Approved)
        .await
        .map_err(fail)?;
    let today = local_date(now);
    let raw = match reader
        .read(PollRequest {
            text,
            today,
            venues: &venues,
        })
        .await
    {
        Ok(raw) => raw,
        Err(error) => {
            tracing::warn!(error = %error, "bot could not read a poll");
            return Ok(COPY.failed.clone());
        }
    };
    // The reader must say weekly and the request must too, so a poll never
    // repeats unless someone asked for it.
    let weekly = raw.weekly && asks_weekly(text);
    let group = poll_group(&raw, &venues, weekly && asks_public(text));
    let plan = match raw.checked(today) {
        Ok(plan) => plan,
        Err(gaps) => {
            let fields = gaps
                .iter()
                .map(|gap| entry(&COPY.poll_gaps, gap.key()))
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(fill(
                &COPY.poll_missing,
                &[("fields", &fields), ("mention", mention)],
            ));
        }
    };
    // Pending keeps the reader's days, whose weekdays a weekly series repeats;
    // the summary shows this week's first poll.
    let pending = PendingPoll {
        zalo_user_id: message.sender_id.clone(),
        chat_id: message.chat_id.clone(),
        user,
        plan,
        weekly,
        group,
        expires_at: now + Duration::minutes(DRAFT_MINUTES),
    };
    ctx.bot.save_pending_poll(&pending).await.map_err(fail)?;
    let first = if weekly {
        weekly_first_plan(pending.plan.clone(), now)
    } else {
        pending.plan.clone()
    };
    let plan = &first;
    let last_day = plan
        .days
        .last()
        .map(|day| day_label(*day))
        .unwrap_or_default();
    let nudge_time = plan.nudge_time.format("%H:%M").to_string();
    let mut reply = fill(
        &COPY.poll_summary,
        &[
            ("title", &plan.title),
            ("days", &days_line(plan)),
            ("time", &nudge_time),
            ("lastDay", &last_day),
        ],
    );
    if let Some(line) = group_line(&pending.group, &venues) {
        reply.push('\n');
        reply.push_str(&line);
    }
    if weekly {
        reply.push('\n');
        reply.push_str(&fill(
            &COPY.poll_weekly_summary,
            &[("time", &nudge_time), ("mention", mention)],
        ));
    }
    reply.push('\n');
    reply.push_str(&fill(&COPY.reminder_confirm_hint, &[("mention", mention)]));
    Ok(reply)
}

async fn save_poll<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    pending: PendingPoll,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository + PollRepository,
{
    let linked = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?;
    if linked != Some(pending.user) {
        return Ok(fill(&COPY.not_linked, &[("bot", ctx.bot_name)]));
    }
    let weekdays = weekday_bits(&pending.plan.days);
    let plan = if pending.weekly {
        weekly_first_plan(pending.plan.clone(), now)
    } else {
        pending.plan.clone()
    };
    let share_id = ShareId::generate().map_err(fail)?;
    let Some(poll) = ctx
        .bot
        .add_poll(&pending.chat_id, pending.user, &plan, &share_id, None, now)
        .await
        .map_err(fail)?
    else {
        return Ok(COPY.poll_limit.clone());
    };
    let mut reply = posted_text(ctx.frontend_origin, &poll, mention);
    if pending.weekly {
        let series = PollSeries {
            id: 0,
            chat_id: poll.chat_id.clone(),
            user: pending.user,
            title: poll.plan.title.clone(),
            weekdays,
            play_time: poll.plan.play_time,
            nudge_time: poll.plan.nudge_time,
            next_post_at: first_week(weekdays, poll.plan.nudge_time, now).1,
            group: pending.group,
        };
        let saved = ctx
            .bot
            .add_series(&series, poll.id, now)
            .await
            .map_err(fail)?
            .is_some();
        let note = if saved {
            fill(
                &COPY.poll_weekly_saved,
                &[
                    ("time", &series.nudge_time.format("%H:%M").to_string()),
                    ("mention", mention),
                ],
            )
        } else {
            fill(&COPY.series_limit, &[("mention", mention)])
        };
        reply.push('\n');
        reply.push_str(&note);
    }
    Ok(reply)
}

/// What the reader said the group plays and where. A venue counts only when it
/// is approved; a format implies its sport.
fn poll_group(raw: &RawPoll, venues: &[GroupVenue], public: bool) -> PollGroup {
    let format = raw.format.as_deref().and_then(GameFormat::parse);
    let sport = raw
        .sport
        .as_deref()
        .and_then(Sport::parse)
        .or(format.map(GameFormat::sport));
    // A format of another sport is dropped rather than mixed.
    let format = format.filter(|format| sport.is_none_or(|sport| format.sport() == sport));
    let venue_id = raw
        .venue_id
        .as_deref()
        .and_then(|slug| venues.iter().find(|venue| venue.slug == slug))
        .map(|venue| venue.id);
    PollGroup {
        sport,
        format,
        venue_id,
        public,
    }
}

/// "🏀 Bóng rổ · 3x3 · 📍 Sân Lakeview · 🌐 Hiện trên trang chủ daghep.vn".
fn group_line(group: &PollGroup, venues: &[GroupVenue]) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(sport) = group.sport {
        parts.push(entry(&COPY.sports, sport.as_str()).to_owned());
    }
    if let Some(format) = group.format {
        parts.push(entry(&COPY.formats, format.as_str()).to_owned());
    }
    if let Some(venue) = group
        .venue_id
        .and_then(|id| venues.iter().find(|venue| venue.id == id))
    {
        parts.push(format!("📍 {}", venue.name));
    }
    if group.public {
        parts.push(COPY.group_public.clone());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Whether the request itself asks for the home page ("công khai").
fn asks_public(text: &str) -> bool {
    let text = text.to_lowercase();
    COPY.commands
        .public
        .iter()
        .any(|word| text.contains(word.as_str()))
}

fn is_admin<B, D, M, P>(ctx: &BotContext<'_, B, D, M, P>, user: Option<UserId>) -> bool {
    user.is_some_and(|user| ctx.admins.contains(&user))
}

/// The words of `text`, lowercased, for matching phrases.
fn lower_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

/// Removes every occurrence of `phrase` (whole words, any case) from `words`.
fn drop_phrase(words: &mut Vec<&str>, phrase: &str) {
    let phrase = lower_words(phrase);
    if phrase.is_empty() {
        return;
    }
    let mut index = 0;
    while index + phrase.len() <= words.len() {
        let window: Vec<String> = words[index..index + phrase.len()]
            .iter()
            .map(|word| word.to_lowercase())
            .collect();
        if window == phrase {
            words.drain(index..index + phrase.len());
        } else {
            index += 1;
        }
    }
}

/// "Sân Lakeview" from "thêm sân Lakeview bóng rổ https://…", or the name in
/// the Maps link when the message has none.
fn venue_name(text: &str, link: &str, label: Option<&str>) -> Option<String> {
    let mut words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| *word != link)
        .collect();
    let commands = &COPY.commands;
    for lead in &commands.add_venue {
        let lead_words = lower_words(lead);
        let starts = words.len() >= lead_words.len()
            && words[..lead_words.len()]
                .iter()
                .map(|word| word.to_lowercase())
                .eq(lead_words.iter().cloned());
        if starts {
            words.drain(..lead_words.len());
            break;
        }
    }
    for phrase in commands
        .venue_football
        .iter()
        .chain(&commands.venue_basketball)
    {
        drop_phrase(&mut words, phrase);
    }
    let typed = clean_name(&words.join(" "), MAX_VENUE_NAME_CHARS);
    let name = typed.or_else(|| label.and_then(|label| clean_name(label, MAX_VENUE_NAME_CHARS)))?;
    let has_prefix = COPY
        .venue_prefixes
        .iter()
        .any(|prefix| name.to_lowercase().starts_with(&prefix.to_lowercase()));
    Some(if has_prefix {
        name
    } else {
        fill(&COPY.venue_named, &[("name", &name)])
    })
}

/// Sports a venue request names ("bóng rổ", "bóng đá").
fn venue_sports(text: &str) -> Vec<Sport> {
    let words = lower_words(text);
    let has = |phrases: &[String]| {
        phrases.iter().any(|phrase| {
            let phrase = lower_words(phrase);
            !phrase.is_empty()
                && words
                    .windows(phrase.len())
                    .any(|window| window == phrase.as_slice())
        })
    };
    let mut sports = Vec::new();
    if has(&COPY.commands.venue_football) {
        sports.push(Sport::Football);
    }
    if has(&COPY.commands.venue_basketball) {
        sports.push(Sport::Basketball);
    }
    sports
}

fn sports_text(sports: &[Sport]) -> String {
    sports
        .iter()
        .map(|sport| entry(&COPY.sports, sport.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn add_venue<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    text: &str,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository + VenueRepository,
{
    let Some(user) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        return unlinked(ctx, message, now).await;
    };
    let Some(link) = find_maps_link(text) else {
        return Ok(fill(&COPY.venue_needs_link, &[("mention", mention)]));
    };
    let sports = venue_sports(text);
    if sports.is_empty() {
        return Ok(COPY.venue_needs_sport.clone());
    }
    let Some(maps) = ctx.maps else {
        return Ok(COPY.venue_bad_link.clone());
    };
    let place = match maps.locate(link).await {
        Ok(place) => place,
        Err(error) => {
            tracing::info!(error = %error, "could not read a Maps link");
            return Ok(COPY.venue_bad_link.clone());
        }
    };
    if !in_vietnam(place.lat, place.lng) {
        return Ok(COPY.venue_outside.clone());
    }
    let Some(name) = venue_name(text, link, place.label.as_deref()) else {
        return Ok(fill(&COPY.venue_needs_link, &[("mention", mention)]));
    };
    let address = place
        .label
        .as_deref()
        .and_then(|label| clean_name(label, 200))
        .unwrap_or_else(|| name.clone());
    let submission = VenueSubmission {
        name: name.clone(),
        address,
        lat: place.lat,
        lng: place.lng,
        sports: sports.clone(),
        maps_url: link.chars().take(500).collect(),
        submitted_by: user,
    };
    let approved = ctx
        .bot
        .venues_with_status(VenueStatus::Approved)
        .await
        .map_err(fail)?;
    if approved
        .iter()
        .any(|venue| venue.name.to_lowercase() == name.to_lowercase())
    {
        return Ok(fill(&COPY.venue_exists, &[("name", &name)]));
    }
    let slug = new_slug(&name).map_err(fail)?;
    let id = ctx
        .bot
        .submit_venue(&submission, &slug)
        .await
        .map_err(fail)?;
    let notice = fill(
        &COPY.venue_admin_notice,
        &[
            ("id", &id.to_string()),
            ("name", &name),
            ("sports", &sports_text(&sports)),
            ("lat", &format!("{:.6}", place.lat)),
            ("lng", &format!("{:.6}", place.lng)),
        ],
    );
    // The venue is saved: a failed notice must not turn into a "failed" reply.
    if let Err(error) = notify_admins(ctx, &notice).await {
        tracing::warn!(error = %error, "could not notify admins about a venue");
    }
    Ok(fill(&COPY.venue_submitted, &[("name", &name)]))
}

/// Sends a private message to every admin linked to Zalo.
async fn notify_admins<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    text: &str,
) -> Result<(), BotError>
where
    B: BotRepository,
{
    let Some(chat) = ctx.chat else {
        return Ok(());
    };
    for admin in ctx.admins {
        if let Some(zalo_id) = ctx.bot.zalo_id_of(*admin).await.map_err(fail)?
            && let Err(error) = chat.send(&zalo_id, text).await
        {
            tracing::warn!(error = %error, "could not notify an admin");
        }
    }
    Ok(())
}

async fn decide_venue<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    id: i64,
    approve: bool,
) -> Result<String, BotError>
where
    B: BotRepository + VenueRepository,
{
    let sender = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?;
    if !is_admin(ctx, sender) {
        return Ok(COPY.admin_only.clone());
    }
    let status = if approve {
        VenueStatus::Approved
    } else {
        VenueStatus::Rejected
    };
    let decided = match ctx.bot.decide_venue(id, status).await {
        Ok(decided) => decided,
        Err(DecideError::Duplicate) => {
            return Ok(fill(&COPY.venue_duplicate, &[("id", &id.to_string())]));
        }
        Err(DecideError::Repo(error)) => return Err(fail(error)),
    };
    let Some((name, submitter)) = decided else {
        return Ok(fill(&COPY.venue_not_pending, &[("id", &id.to_string())]));
    };
    if let (Some(submitter), Some(chat)) = (submitter, ctx.chat) {
        // An admin approving their own venue already reads the reply.
        if let Some(zalo_id) = ctx
            .bot
            .zalo_id_of(submitter)
            .await
            .map_err(fail)?
            .filter(|zalo_id| *zalo_id != message.sender_id)
        {
            let template = if approve {
                &COPY.venue_approved
            } else {
                &COPY.venue_rejected
            };
            if let Err(error) = chat
                .send(&zalo_id, &fill(template, &[("name", &name)]))
                .await
            {
                tracing::warn!(error = %error, "could not tell an organiser about their venue");
            }
        }
    }
    let template = if approve {
        &COPY.venue_approved_admin
    } else {
        &COPY.venue_rejected_admin
    };
    Ok(fill(template, &[("id", &id.to_string()), ("name", &name)]))
}

async fn pending_venues<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
) -> Result<String, BotError>
where
    B: BotRepository + VenueRepository,
{
    let sender = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?;
    if !is_admin(ctx, sender) {
        return Ok(COPY.admin_only.clone());
    }
    let pending = ctx
        .bot
        .venues_with_status(VenueStatus::Pending)
        .await
        .map_err(fail)?;
    if pending.is_empty() {
        return Ok(COPY.no_pending_venues.clone());
    }
    let items = pending
        .iter()
        .map(|venue| {
            fill(
                &COPY.pending_venue_item,
                &[
                    ("id", &venue.id.to_string()),
                    ("name", &venue.name),
                    ("sports", &sports_text(&venue.sports)),
                    ("lat", &format!("{:.6}", venue.lat)),
                    ("lng", &format!("{:.6}", venue.lng)),
                ],
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(fill(&COPY.pending_venues_list, &[("items", &items)]))
}

async fn publish_poll<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    number: Option<i64>,
    public: bool,
    mention: &str,
) -> Result<String, BotError>
where
    B: BotRepository + PollRepository,
{
    let Some(number) = number else {
        let list = stop_poll_series(ctx, message, None, mention).await?;
        return Ok(format!(
            "{list}\n{}",
            fill(&COPY.publish_hint, &[("mention", mention)])
        ));
    };
    if ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
        .is_none()
    {
        return unlinked(ctx, message, ctx.clock.now()).await;
    }
    let all = ctx.bot.series(&message.chat_id).await.map_err(fail)?;
    let n = number.to_string();
    let target = usize::try_from(number)
        .ok()
        .and_then(|number| number.checked_sub(1))
        .and_then(|index| all.get(index));
    let updated = match target {
        Some(series) => ctx
            .bot
            .set_series_public(&message.chat_id, series.id, public)
            .await
            .map_err(fail)?,
        None => false,
    };
    Ok(if !updated {
        fill(&COPY.series_not_found, &[("n", &n), ("mention", mention)])
    } else if public {
        fill(&COPY.series_published, &[("n", &n)])
    } else {
        fill(&COPY.series_hidden, &[("n", &n)])
    })
}

/// A weekly series' first poll: the asked-for weekdays still ahead this week.
fn weekly_first_plan(plan: PollPlan, now: DateTime<Utc>) -> PollPlan {
    let (days, _) = first_week(weekday_bits(&plan.days), plan.nudge_time, now);
    PollPlan { days, ..plan }
}

/// Whether the request itself says the poll repeats every week.
fn asks_weekly(text: &str) -> bool {
    let text = text.to_lowercase();
    COPY.commands
        .weekly
        .iter()
        .any(|words| text.contains(words.as_str()))
}

fn posted_text(frontend_origin: &str, poll: &Poll, mention: &str) -> String {
    fill(
        &COPY.poll_posted,
        &[
            ("title", &poll.plan.title),
            ("days", &days_line(&poll.plan)),
            ("mention", mention),
            ("example", &answer_example(&poll.plan)),
            ("url", &poll_url(frontend_origin, poll)),
        ],
    )
}

async fn stop_poll_series<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    number: Option<i64>,
    mention: &str,
) -> Result<String, BotError>
where
    B: BotRepository + PollRepository,
{
    let all = ctx.bot.series(&message.chat_id).await.map_err(fail)?;
    let Some(number) = number else {
        if all.is_empty() {
            return Ok(fill(&COPY.no_series, &[("mention", mention)]));
        }
        let items = all
            .iter()
            .enumerate()
            .map(|(index, series)| {
                let mut when = describe_schedule(&Schedule {
                    repeat: Repeat::Weekdays(series.weekdays),
                    time: series.play_time.unwrap_or(series.nudge_time),
                })
                .0;
                if let Some(time) = series.play_time {
                    when.push_str(&fill(
                        &COPY.poll_play_suffix,
                        &[("time", &time.format("%H:%M").to_string())],
                    ));
                }
                fill(
                    &COPY.series_list_item,
                    &[
                        ("n", &(index + 1).to_string()),
                        ("when", &when),
                        ("title", &series.title),
                    ],
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(fill(
            &COPY.series_list,
            &[("items", &items), ("mention", mention)],
        ));
    };
    // Only linked members may stop a group's weekly polls.
    if ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
        .is_none()
    {
        return Ok(fill(&COPY.not_linked, &[("bot", ctx.bot_name)]));
    }
    let n = number.to_string();
    let target = usize::try_from(number)
        .ok()
        .and_then(|number| number.checked_sub(1))
        .and_then(|index| all.get(index));
    let stopped = match target {
        Some(series) => ctx
            .bot
            .delete_series(&message.chat_id, series.id)
            .await
            .map_err(fail)?,
        None => false,
    };
    Ok(if stopped {
        fill(&COPY.series_stopped, &[("n", &n)])
    } else {
        fill(&COPY.series_not_found, &[("n", &n), ("mention", mention)])
    })
}

/// Posts each weekly series' new poll that is due. Returns how many were posted.
pub async fn run_due_series<B: PollRepository>(
    bot: &B,
    clock: &dyn Clock,
    chat: &dyn ChatReply,
    frontend_origin: &str,
    bot_name: &str,
) -> Result<usize, BotError> {
    let now = clock.now();
    let due = bot.claim_due_series(now).await.map_err(fail)?;
    let mut posted = 0;
    for series in due {
        if now - series.next_post_at > Duration::hours(MAX_SERIES_LATE_HOURS) {
            tracing::info!(
                series = series.id,
                "skipping a weekly poll that is too late"
            );
            continue;
        }
        let plan = series.plan_for_week(series.next_post_at, now);
        if plan.days.is_empty() {
            continue;
        }
        let share_id = ShareId::generate().map_err(fail)?;
        let poll = match bot
            .add_poll(
                &series.chat_id,
                series.user,
                &plan,
                &share_id,
                Some(series.id),
                now,
            )
            .await
        {
            Ok(Some(poll)) => poll,
            Ok(None) => {
                tracing::info!(
                    series = series.id,
                    "weekly poll skipped: the chat has 3 open polls"
                );
                continue;
            }
            Err(error) => {
                tracing::error!(error = %error, series = series.id, "could not make a weekly poll");
                continue;
            }
        };
        let delivered = match chat
            .send(
                &poll.chat_id,
                &posted_text(frontend_origin, &poll, &format!("@{bot_name} ")),
            )
            .await
        {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(error = %error, series = series.id, "could not post a weekly poll");
                false
            }
        };
        posted += usize::from(delivered);
        if let Err(error) = bot.record_series_post(series.id, delivered).await {
            tracing::error!(error = %error, series = series.id, "could not record a weekly post");
        }
    }
    Ok(posted)
}

/// Uses up one AI read; returns the reply to send when a daily limit is reached.
async fn use_ai_read<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    user: UserId,
    now: DateTime<Utc>,
    create_url: &str,
) -> Result<Option<String>, BotError>
where
    D: DraftRepository,
{
    match use_read(ctx.drafts, user, now).await {
        Ok(()) => Ok(None),
        Err(ReadDraftError::Limited(LimitScope::User)) => {
            Ok(Some(fill(&COPY.limit_user, &[("url", create_url)])))
        }
        Err(ReadDraftError::Limited(LimitScope::Service)) => {
            Ok(Some(fill(&COPY.limit_service, &[("url", create_url)])))
        }
        Err(error) => Err(fail(error)),
    }
}

/// How to answer a poll in the chat, from its first days ("Thứ 7 có, Chủ nhật không").
fn answer_example(plan: &PollPlan) -> String {
    plan.days
        .iter()
        .take(2)
        .zip([&COPY.answer_yes, &COPY.answer_no])
        .map(|(day, answer)| {
            let weekday = COPY
                .weekdays
                .get(day.weekday().num_days_from_sunday() as usize)
                .map_or("", String::as_str);
            format!("{weekday} {answer}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Saves a poll answer typed in the chat. `None` when the chat has no open poll,
/// so the message is read as usual.
async fn answer_in_chat<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    intent: &AnswerIntent,
    mention: &str,
    now: DateTime<Utc>,
) -> Result<Option<String>, BotError>
where
    B: BotRepository + PollRepository,
{
    let polls = ctx
        .bot
        .open_polls(&message.chat_id, now)
        .await
        .map_err(fail)?;
    let Some(newest) = polls.first() else {
        return Ok(None);
    };
    let today = local_date(now);
    // An answer goes to a poll only when it clearly fits one: with no day named,
    // only when the chat has one poll; with days, only the one poll they fit.
    let fits: Vec<(&Poll, Vec<(chrono::NaiveDate, bool)>)> = match intent {
        AnswerIntent::NoDay(_) if polls.len() > 1 => Vec::new(),
        _ => polls
            .iter()
            .filter_map(|poll| {
                intent
                    .resolve(&poll.plan.days, today)
                    .ok()
                    .map(|answers| (poll, answers))
            })
            .collect(),
    };
    let (poll, answers) = match fits.as_slice() {
        [(poll, answers)] => (*poll, answers.clone()),
        [] if polls.len() == 1 => {
            let error = intent
                .resolve(&newest.plan.days, today)
                .err()
                .unwrap_or(ResolveError::WhichDay);
            return Ok(Some(match error {
                ResolveError::NotInPoll => not_in_poll(newest, mention),
                ResolveError::WhichDay => fill(
                    &COPY.answer_which_day,
                    &[
                        ("mention", mention),
                        ("example", &answer_example(&newest.plan)),
                    ],
                ),
            }));
        }
        _ => {
            let links = polls
                .iter()
                .map(|poll| {
                    fill(
                        &COPY.answer_poll_link,
                        &[
                            ("title", &poll.plan.title),
                            ("url", &poll_url(ctx.frontend_origin, poll)),
                        ],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            // The named day is in several polls: only the links help.
            let template = if fits.len() > 1 {
                &COPY.answer_which_poll_links
            } else {
                &COPY.answer_which_poll
            };
            return Ok(Some(fill(
                template,
                &[("n", &polls.len().to_string()), ("links", &links)],
            )));
        }
    };
    let url = poll_url(ctx.frontend_origin, poll);
    let Some(user) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        if message.chat_kind == ChatKind::Group && send_link_privately(ctx, message, now).await? {
            return Ok(Some(fill(&COPY.answer_link_sent, &[("url", &url)])));
        }
        return Ok(Some(fill(
            &COPY.answer_needs_link,
            &[("bot", ctx.bot_name), ("url", &url)],
        )));
    };
    match ctx
        .bot
        .answer(&poll.share_id, user, &answers, now)
        .await
        .map_err(fail)?
    {
        AnswerOutcome::Saved => {}
        AnswerOutcome::UnknownPoll | AnswerOutcome::Closed => {
            return Ok(Some(COPY.answer_closed.clone()));
        }
        AnswerOutcome::UnknownDay => return Ok(Some(not_in_poll(poll, mention))),
    }
    let all = ctx.bot.answers(poll.id).await.map_err(fail)?;
    let mine: Vec<&PollAnswer> = all.iter().filter(|answer| answer.user == user).collect();
    let name = mine
        .iter()
        .find_map(|answer| answer.name.as_deref())
        .unwrap_or(&COPY.player_name);
    let said = answers
        .iter()
        .map(|(day, attending)| {
            let answer = if *attending {
                &COPY.answer_yes
            } else {
                &COPY.answer_no
            };
            fill(
                &COPY.answer_day,
                &[("day", &day_label(*day)), ("answer", answer)],
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let counts = poll
        .plan
        .days
        .iter()
        .map(|day| {
            let yes = all
                .iter()
                .filter(|answer| answer.day == *day && answer.attending)
                .count();
            fill(
                &COPY.answer_count,
                &[("day", &day_label(*day)), ("n", &yes.to_string())],
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    let unanswered: Vec<String> = poll
        .plan
        .days
        .iter()
        .filter(|day| !mine.iter().any(|answer| answer.day == **day))
        .map(|day| day_label(*day))
        .collect();
    let missing = if unanswered.is_empty() {
        String::new()
    } else {
        fill(
            &COPY.answer_missing_days,
            &[("days", &unanswered.join(", "))],
        )
    };
    Ok(Some(fill(
        &COPY.answer_saved,
        &[
            ("name", name),
            ("answers", &said),
            ("counts", &counts),
            ("missing", &missing),
        ],
    )))
}

fn not_in_poll(poll: &Poll, mention: &str) -> String {
    let days = poll
        .plan
        .days
        .iter()
        .map(|day| day_label(*day))
        .collect::<Vec<_>>()
        .join(", ");
    fill(
        &COPY.answer_not_in_poll,
        &[
            ("title", &poll.plan.title),
            ("days", &days),
            ("mention", mention),
            ("example", &answer_example(&poll.plan)),
        ],
    )
}

fn poll_url(frontend_origin: &str, poll: &Poll) -> String {
    format!("{frontend_origin}/p/{}", poll.share_id.as_str())
}

/// "Thứ 7 03/10".
fn day_label(day: chrono::NaiveDate) -> String {
    let weekday = COPY
        .weekdays
        .get(day.weekday().num_days_from_sunday() as usize)
        .map_or("", String::as_str);
    format!("{weekday} {}", day.format("%d/%m"))
}

/// "Thứ 7 03/10 · Chủ nhật 04/10 · chơi lúc 18:00".
fn days_line(plan: &PollPlan) -> String {
    let days = plan
        .days
        .iter()
        .map(|day| day_label(*day))
        .collect::<Vec<_>>()
        .join(" · ");
    match plan.play_time {
        Some(time) => fill(
            &COPY.poll_days_with_time,
            &[("days", &days), ("time", &time.format("%H:%M").to_string())],
        ),
        None => days,
    }
}

/// Posts each poll's daily nudge that is due: yes-counts per day and who has
/// not answered. Returns how many were delivered.
pub async fn run_due_polls<B: PollRepository>(
    bot: &B,
    clock: &dyn Clock,
    chat: &dyn ChatReply,
    frontend_origin: &str,
    bot_name: &str,
) -> Result<usize, BotError> {
    let now = clock.now();
    let due = bot.claim_due_nudges(now).await.map_err(fail)?;
    let mut delivered = 0;
    for (poll, due_at) in due {
        if now - due_at > Duration::minutes(MAX_LATE_MINUTES) {
            tracing::info!(poll = poll.id, "skipping a poll nudge that is too late");
            continue;
        }
        let text = match nudge_text(bot, &poll, frontend_origin, bot_name, now).await {
            Ok(text) => text,
            Err(error) => {
                tracing::error!(error = %error, poll = poll.id, "could not build a poll nudge");
                continue;
            }
        };
        match chat.send(&poll.chat_id, &text).await {
            Ok(()) => delivered += 1,
            Err(error) => {
                tracing::warn!(error = %error, poll = poll.id, "could not post a poll nudge");
            }
        }
    }
    Ok(delivered)
}

async fn nudge_text<B: PollRepository>(
    bot: &B,
    poll: &Poll,
    frontend_origin: &str,
    bot_name: &str,
    now: DateTime<Utc>,
) -> Result<String, BotError> {
    let answers = bot.answers(poll.id).await.map_err(fail)?;
    let since = now - Duration::days(MEMBER_DAYS);
    let members = bot.members(&poll.chat_id, since).await.map_err(fail)?;
    let tally = tally(&poll.plan, &answers, &members);
    let counts = tally
        .yes_per_day
        .iter()
        .map(|(day, yes)| {
            fill(
                &COPY.poll_count,
                &[("day", &day_label(*day)), ("n", &yes.to_string())],
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let missing = if tally.missing.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = tally
            .missing
            .iter()
            .map(|member| member.name.as_deref().unwrap_or(&COPY.player_name))
            .collect();
        let shown = names
            .iter()
            .take(MAX_NAMES_IN_NUDGE)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let names = if names.len() > MAX_NAMES_IN_NUDGE {
            fill(
                &COPY.poll_more_names,
                &[
                    ("names", &shown),
                    ("n", &(names.len() - MAX_NAMES_IN_NUDGE).to_string()),
                ],
            )
        } else {
            shown
        };
        fill(&COPY.poll_missing_names, &[("names", &names)])
    };
    let play = poll.plan.play_time.map_or_else(String::new, |time| {
        fill(
            &COPY.poll_play_suffix,
            &[("time", &time.format("%H:%M").to_string())],
        )
    });
    Ok(fill(
        &COPY.poll_nudge,
        &[
            ("title", &poll.plan.title),
            ("play", &play),
            ("counts", &counts),
            ("missing", &missing),
            ("mention", &format!("@{bot_name} ")),
            ("example", &answer_example(&poll.plan)),
            ("url", &poll_url(frontend_origin, poll)),
        ],
    ))
}

/// Posts every reminder that is due; returns how many were delivered.
pub async fn run_due_reminders<B: BotRepository>(
    bot: &B,
    clock: &dyn Clock,
    chat: &dyn ChatReply,
) -> Result<usize, BotError> {
    let now = clock.now();
    let due = bot.claim_due_reminders(now).await.map_err(fail)?;
    let mut delivered = 0;
    for reminder in due {
        // After an outage, "ai đá tối nay?" at 3 am helps nobody: skip it; the
        // claim already moved it to its next time.
        if now - reminder.next_run_at > Duration::minutes(MAX_LATE_MINUTES) {
            tracing::info!(
                reminder = reminder.id,
                "skipping a reminder that is too late"
            );
            continue;
        }
        let text = fill(&COPY.reminder_post, &[("text", &reminder.plan.text)]);
        let sent = match chat.send(&reminder.chat_id, &text).await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(error = %error, reminder = reminder.id, "could not post a reminder");
                false
            }
        };
        delivered += usize::from(sent);
        // One failed write must not stop the rest of the batch from posting.
        if let Err(error) = bot.record_reminder_send(reminder.id, sent).await {
            tracing::error!(error = %error, reminder = reminder.id, "could not record a reminder send");
        }
    }
    Ok(delivered)
}

/// "mỗi Thứ 7, Chủ nhật" / "mỗi ngày" / "ngày 1 hằng tháng", and "08:00".
fn describe_schedule(schedule: &Schedule) -> (String, String) {
    let when = match schedule.repeat {
        Repeat::Weekdays(EVERY_DAY) => COPY.every_day.clone(),
        Repeat::Weekdays(days) => {
            // Monday first, Sunday last, as people say them.
            let names = [1, 2, 3, 4, 5, 6, 0]
                .into_iter()
                .filter(|day| days & (1 << day) != 0)
                .filter_map(|day| COPY.weekdays.get(day))
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            fill(&COPY.on_weekdays, &[("days", &names)])
        }
        Repeat::MonthDay(day) => fill(&COPY.month_day, &[("day", &day.to_string())]),
    };
    (when, schedule.time.format("%H:%M").to_string())
}

/// "Thứ 7 03/10 08:00" in Ho Chi Minh City time.
fn moment(at: DateTime<Utc>) -> String {
    let local = local_time(at);
    let weekday = COPY
        .weekdays
        .get(local.weekday().num_days_from_sunday() as usize)
        .map_or("", String::as_str);
    format!("{weekday} {}", local.format("%d/%m %H:%M"))
}

/// Matches listed to a player in one reply.
const MATCHES_IN_REPLY: usize = 5;

/// The soonest open matches, or where to look when there are none.
async fn find_matches<B, D, M, P>(ctx: &BotContext<'_, B, D, M, P>) -> String {
    let none = fill(&COPY.no_open_matches, &[("bot", ctx.bot_name)]);
    let Some(finder) = ctx.finder else {
        return none;
    };
    let found = match finder.find(MATCHES_IN_REPLY).await {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(error = %error, "bot could not list open matches");
            return COPY.open_matches_unavailable.clone();
        }
    };
    if found.is_empty() {
        return none;
    }
    let items = found
        .iter()
        .map(|line| {
            let start = local_time(line.starts_at);
            let price = if line.price_per_player_vnd == 0 {
                COPY.free.clone()
            } else {
                fill(
                    &COPY.per_player,
                    &[("amount", &format_vnd(line.price_per_player_vnd))],
                )
            };
            fill(
                &COPY.open_match_line,
                &[
                    ("day", &day_label(start.date_naive())),
                    ("start", &start.format("%H:%M").to_string()),
                    ("end", &local_time(line.ends_at).format("%H:%M").to_string()),
                    ("venue", &line.venue_name),
                    ("format", entry(&COPY.formats, line.format.as_str())),
                    ("places", &line.places_left.to_string()),
                    ("price", &price),
                    (
                        "url",
                        &format!("{}/m/{}", ctx.frontend_origin, line.share_id),
                    ),
                ],
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fill(&COPY.open_matches, &[("items", &items)])
}

/// Links the Zalo account behind a link code to `user`. Returns the Zalo id to
/// confirm to, or `None` when the code is unknown, used or expired.
pub async fn redeem_link<B: BotRepository>(
    bot: &B,
    clock: &dyn Clock,
    user: UserId,
    code: &str,
) -> Result<Option<String>, BotError> {
    if code.is_empty() || code.len() > 100 {
        return Ok(None);
    }
    bot.redeem_link_code(hash_code(code), user, clock.now())
        .await
        .map_err(fail)
}

/// The player's name as the organiser sees it.
fn name_or_player(name: Option<&str>) -> &str {
    name.unwrap_or(&COPY.player_name)
}

/// Asks to join a public group from the web. The organiser is told privately
/// when Zalo lets the bot reach them; the request waits on the web either way.
pub async fn request_join<B>(
    bot: &B,
    chat: Option<&dyn ChatReply>,
    public_id: &str,
    user: UserId,
    now: DateTime<Utc>,
) -> Result<JoinOutcome, BotError>
where
    B: BotRepository + GroupRepository,
{
    let outcome = bot.request_join(public_id, user, now).await.map_err(fail)?;
    if let JoinOutcome::Requested(notice) = &outcome
        && let Some(chat) = chat
    {
        match bot.zalo_id_of(notice.organiser).await {
            Ok(Some(zalo_id)) => {
                let text = fill(
                    &COPY.join_request_notice,
                    &[
                        ("name", name_or_player(notice.player_name.as_deref())),
                        ("title", &notice.title),
                        ("id", &notice.request_id.to_string()),
                    ],
                );
                if let Err(error) = chat.send(&zalo_id, &text).await {
                    tracing::warn!(error = %error, "could not tell an organiser about a join request");
                }
            }
            Ok(None) => {
                tracing::info!("join request for an organiser without a linked Zalo account")
            }
            Err(error) => {
                tracing::warn!(error = %error, "could not find an organiser's Zalo account")
            }
        }
    }
    Ok(outcome)
}

async fn decide_join<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    id: i64,
    approve: bool,
    now: DateTime<Utc>,
) -> Result<String, BotError>
where
    B: BotRepository + GroupRepository,
{
    let Some(organiser) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        return unlinked(ctx, message, now).await;
    };
    let decision = match ctx
        .bot
        .decide_join(organiser, id, approve, now)
        .await
        .map_err(fail)?
    {
        DecideOutcome::Decided(decision) => decision,
        DecideOutcome::NotFound => {
            return Ok(fill(&COPY.join_not_found, &[("id", &id.to_string())]));
        }
    };
    let name = name_or_player(decision.player_name.as_deref());
    // The player hears it privately when their Zalo account is linked; the web
    // page shows it either way.
    if let Some(chat) = ctx.chat
        && let Some(zalo_id) = ctx.bot.zalo_id_of(decision.player).await.map_err(fail)?
    {
        let url = format!("{}/g/{}", ctx.frontend_origin, decision.public_id);
        let text = if decision.approved {
            fill(
                &COPY.join_approved_player,
                &[("title", &decision.title), ("url", &url)],
            )
        } else {
            fill(&COPY.join_declined_player, &[("title", &decision.title)])
        };
        if let Err(error) = chat.send(&zalo_id, &text).await {
            tracing::warn!(error = %error, "could not tell a player about their join request");
        }
    }
    Ok(if decision.approved {
        let mut reply = fill(
            &COPY.join_approved_organiser,
            &[("name", name), ("title", &decision.title)],
        );
        if decision.group_link.is_none() {
            reply.push_str(&fill(&COPY.join_needs_group_link, &[("bot", ctx.bot_name)]));
        }
        reply
    } else {
        fill(
            &COPY.join_declined_organiser,
            &[("name", name), ("title", &decision.title)],
        )
    })
}

async fn set_group_link<B, D, M, P>(
    ctx: &BotContext<'_, B, D, M, P>,
    message: &Incoming,
    text: &str,
    mention: &str,
) -> Result<String, BotError>
where
    B: BotRepository + GroupRepository,
{
    let Some(link) = find_group_link(text) else {
        return Ok(fill(&COPY.group_link_missing, &[("mention", mention)]));
    };
    let Some(organiser) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        return unlinked(ctx, message, ctx.clock.now()).await;
    };
    let updated = ctx
        .bot
        .set_group_link(&message.chat_id, organiser, link)
        .await
        .map_err(fail)?;
    Ok(if updated > 0 {
        COPY.group_link_saved.clone()
    } else {
        COPY.group_link_no_series.clone()
    })
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime};

    use super::*;
    use crate::matches::domain::Format;

    #[test]
    fn formats_prices_the_vietnamese_way() {
        assert_eq!(format_vnd(0), "0đ");
        assert_eq!(format_vnd(60_000), "60.000đ");
        assert_eq!(format_vnd(1_200_000), "1.200.000đ");
    }

    fn full() -> MatchDraft {
        MatchDraft {
            venue_id: Some("ssa-amitie".to_owned()),
            date: NaiveDate::from_ymd_opt(2099, 10, 1),
            start_time: NaiveTime::from_hms_opt(19, 0, 0),
            end_time: NaiveTime::from_hms_opt(20, 30, 0),
            format: Some(Format::SevenASide),
            match_type: None,
            level_min: None,
            level_max: None,
            slot_count: Some(3),
            total_fee_vnd: Some(180_000),
        }
    }

    #[test]
    fn a_complete_draft_gets_the_defaults_for_what_was_not_said() {
        let input = complete(&full()).unwrap();
        assert_eq!(input.match_type, DEFAULT_MATCH_TYPE);
        assert_eq!((input.level_min, input.level_max), (1.0, 5.0));
        assert_eq!(input.starts_at.to_rfc3339(), "2099-10-01T12:00:00+00:00");
        assert_eq!(input.ends_at.to_rfc3339(), "2099-10-01T13:30:00+00:00");
        assert_eq!(input.slot_count, Some(3));
    }

    #[test]
    fn lists_every_missing_field_in_order() {
        let draft = MatchDraft {
            end_time: None,
            format: None,
            total_fee_vnd: None,
            ..full()
        };
        assert_eq!(
            complete(&draft).unwrap_err(),
            vec!["endTime", "format", "fee"]
        );
    }

    #[test]
    fn one_stated_level_keeps_the_range_valid() {
        let only_min = MatchDraft {
            level_min: Some(3.5),
            ..full()
        };
        let input = complete(&only_min).unwrap();
        assert_eq!((input.level_min, input.level_max), (3.5, 5.0));
        let only_max = MatchDraft {
            level_max: Some(2.0),
            ..full()
        };
        let input = complete(&only_max).unwrap();
        assert_eq!((input.level_min, input.level_max), (1.0, 2.0));
    }
}
