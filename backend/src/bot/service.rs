//! Use cases of the Zalo bot: answer one chat message, and link a Zalo account
//! from the web. Matches are created through `matches::service::create_match`,
//! messages are read through `drafts::service::read_draft` (same daily limits).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Datelike, Duration, Utc};
use sha2::{Digest, Sha256};

use super::domain::{
    BotDraft, BotRepository, COPY, ChatKind, Command, DEFAULT_LEVEL_MAX_TENTHS,
    DEFAULT_LEVEL_MIN_TENTHS, DEFAULT_MATCH_TYPE, DRAFT_MINUTES, Incoming, LINK_CODE_MINUTES,
    entry, fill, parse_command,
};
use crate::auth::UserId;
use crate::clock::Clock;
use crate::drafts::domain::{DraftRepository, LimitScope, MatchDraft, MatchTextReader};
use crate::drafts::service::{ReadDraftError, read_draft};
use crate::matches;
use crate::matches::domain::{
    Field, Level, MatchRepository, NewMatch, NewMatchInput, local_instant, local_time,
    price_per_player_vnd,
};
use crate::matches::service::{CreateMatchError, create_match};
use crate::payments::domain::PayoutRepository;
use crate::payments::service::has_payout_account;
use crate::random::random_bytes;

/// What one message needs: storage, the other features' ports and settings.
pub struct BotContext<'a, B, D, M, P> {
    pub bot: &'a B,
    pub drafts: &'a D,
    pub matches: &'a M,
    pub payouts: &'a P,
    pub clock: &'a dyn Clock,
    pub reader: Option<&'a dyn MatchTextReader>,
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
    B: BotRepository,
    D: DraftRepository,
    M: MatchRepository,
    P: PayoutRepository,
{
    let Some(command) = parse_command(&message.text, message.chat_kind, ctx.bot_name) else {
        return Ok(None);
    };
    let now = ctx.clock.now();
    if !ctx
        .bot
        .record_update(&message.message_id, now)
        .await
        .map_err(fail)?
    {
        return Ok(None);
    }
    // In groups every command must tag the bot, so the examples do too.
    let mention = match message.chat_kind {
        ChatKind::Group => format!("@{} ", ctx.bot_name),
        ChatKind::Private => String::new(),
    };
    let reply = match command {
        Command::Help => fill(&COPY.help, &[("mention", &mention)]),
        Command::Link => link(ctx, message, now).await?,
        Command::Cancel => match ctx
            .bot
            .take_draft(&message.sender_id, &message.chat_id, now)
            .await
            .map_err(fail)?
        {
            Some(_) => COPY.cancelled.clone(),
            None => COPY.no_draft.clone(),
        },
        Command::Confirm => confirm(ctx, message).await?,
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
    // host (and be paid for) this sender's matches.
    if message.chat_kind == ChatKind::Group {
        return Ok(COPY.link_in_group.clone());
    }
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
    B: BotRepository,
    D: DraftRepository,
    M: MatchRepository,
    P: PayoutRepository,
{
    let Some(user) = ctx
        .bot
        .linked_user(&message.sender_id)
        .await
        .map_err(fail)?
    else {
        return Ok(COPY.not_linked.clone());
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
    let input = match complete(&draft) {
        Ok(input) => input,
        Err(missing) => {
            let fields = missing
                .iter()
                .map(|field| entry(&COPY.fields, field))
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
    let venues = matches::service::venues(ctx.matches).await.map_err(fail)?;
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
    let price = price_per_player_vnd(draft.total_fee_vnd, draft.slot_count);
    fill(
        &COPY.summary,
        &[
            ("day", &day),
            ("start", &start.format("%H:%M").to_string()),
            ("end", &end.format("%H:%M").to_string()),
            ("venue", venue_name),
            ("format", entry(&COPY.formats, draft.format.as_str())),
            ("slots", &draft.slot_count.to_string()),
            ("price", &format_vnd(price)),
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
    format!("{sign}{grouped}đ")
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
    let input = NewMatchInput {
        venue: draft.venue_slug,
        starts_at: draft.starts_at,
        ends_at: draft.ends_at,
        format: draft.format,
        match_type: draft.match_type,
        level_min: Level::from_tenths(draft.level_min_tenths).map_or(1.0, Level::as_f64),
        level_max: Level::from_tenths(draft.level_max_tenths).map_or(5.0, Level::as_f64),
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
