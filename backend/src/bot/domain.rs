//! Zalo bot domain: commands read from chat messages, reply copy and ports.
//! No Axum, sqlx, HTTP or SDK types here.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::reminders::{PendingReminder, Reminder, ReminderPlan};
use crate::auth::UserId;
use crate::matches::domain::{Format, MatchType};

/// How long a link code works.
pub const LINK_CODE_MINUTES: i64 = 15;
/// How long a draft waits for "ok".
pub const DRAFT_MINUTES: i64 = 30;
/// Messages one sender gets answered per hour; later ones are ignored, so a
/// group member tagging the bot again and again cannot use up Zalo's quota.
pub const REPLIES_PER_SENDER_PER_HOUR: i64 = 20;

/// What recording a delivered message found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    New,
    /// Zalo delivered this message before.
    Duplicate,
    /// The sender reached the hourly reply limit.
    SenderLimited,
}
/// Defaults for what a message does not say (confirmed by Long 2026-10-13).
pub const DEFAULT_MATCH_TYPE: MatchType = MatchType::Casual;
pub const DEFAULT_LEVEL_MIN_TENTHS: i16 = 10;
pub const DEFAULT_LEVEL_MAX_TENTHS: i16 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    Private,
    Group,
}

/// A text message the bot received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub message_id: String,
    pub chat_id: String,
    pub chat_kind: ChatKind,
    pub sender_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Link,
    Confirm,
    Cancel,
    /// A reminder to post on a schedule, in the sender's own words.
    Remind(String),
    /// An attendance poll, in the sender's own words.
    Poll(String),
    /// Stop the weekly poll with this number in the list; `None` shows the list.
    StopPoll(Option<i64>),
    ListReminders,
    /// Stop the reminder with this number in the list; `None` shows the list.
    StopReminder(Option<i64>),
    /// A player looking for a match to join ("tối nay có kèo nào không").
    FindMatches,
    /// A venue from a Google Maps link ("thêm sân Lakeview https://maps…").
    AddVenue(String),
    /// An admin's decision on a pending venue ("duyệt sân 12").
    DecideVenue {
        id: i64,
        approve: bool,
    },
    /// An admin asks which venues wait for a decision.
    PendingVenues,
    /// Show or hide a weekly poll on the home page ("công khai bảng 1").
    PublishPoll {
        number: Option<i64>,
        public: bool,
    },
    /// A match described in the organiser's own words.
    Describe(String),
}

/// Reads what the sender asks for. In a group the message starts with the bot's
/// tag (Zalo only delivers tagged group messages); privately the tag is optional.
/// Returns `None` for a group message that does not tag the bot.
pub fn parse_command(text: &str, kind: ChatKind, bot_name: &str) -> Option<Command> {
    let text = text.trim();
    let tag = format!("@{bot_name}");
    let rest = match strip_prefix_ignore_case(text, &tag) {
        Some(rest) => rest,
        None if kind == ChatKind::Group => return None,
        None => text,
    };
    let rest = rest.trim_start_matches([',', ':', '.', '!', '-']).trim();
    let word = rest.trim_end_matches(['.', '!', '?', ' ']).to_lowercase();
    let commands = &COPY.commands;
    let is = |words: &[String]| words.contains(&word);
    let stop_after = |prefixes: &[String]| {
        prefixes
            .iter()
            .find_map(|prefix| word.strip_prefix(prefix.as_str()))
            .filter(|after| after.is_empty() || after.starts_with(' '))
            .map(str::to_owned)
    };
    // "tắt nhắc 2", "tắt bảng số 2", "tắt nhắc #2": the first number only.
    let number = |after: &str| -> Option<i64> {
        let digits: String = after
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    let stop = stop_after(&commands.stop_reminder);
    let stop_poll = stop_after(&commands.stop_poll);
    let approve_venue = stop_after(&commands.approve_venue);
    let reject_venue = stop_after(&commands.reject_venue);
    let publish = stop_after(&commands.publish_poll);
    let hide = stop_after(&commands.hide_poll);
    Some(if is(&commands.help) {
        Command::Help
    } else if is(&commands.link) {
        Command::Link
    } else if is(&commands.confirm) {
        Command::Confirm
    } else if is(&commands.cancel) {
        Command::Cancel
    } else if let Some(after) = stop {
        Command::StopReminder(number(&after))
    } else if let Some(after) = stop_poll {
        Command::StopPoll(number(&after))
    } else if let Some(id) = approve_venue.as_deref().and_then(number) {
        Command::DecideVenue { id, approve: true }
    } else if let Some(id) = reject_venue.as_deref().and_then(number) {
        Command::DecideVenue { id, approve: false }
    } else if is(&commands.pending_venues) {
        Command::PendingVenues
    } else if let Some(number) = publish.as_deref().and_then(only_number) {
        Command::PublishPoll {
            number,
            public: true,
        }
    } else if let Some(number) = hide.as_deref().and_then(only_number) {
        Command::PublishPoll {
            number,
            public: false,
        }
    } else if starts_with_words(&word, &commands.add_venue) {
        Command::AddVenue(rest.to_owned())
    } else if is(&commands.list_reminders) {
        Command::ListReminders
    } else if starts_with_words(&word, &commands.poll) {
        Command::Poll(rest.to_owned())
    } else if asks_for_reminder(&word) {
        Command::Remind(rest.to_owned())
    } else if asks_for_matches(&word) {
        Command::FindMatches
    } else {
        Command::Describe(rest.to_owned())
    })
}

/// A toggle's argument: nothing (show the list) or just a number ("1", "số 1",
/// "#1"). Anything else is not a toggle ("công khai bảng điểm danh T7…").
fn only_number(after: &str) -> Option<Option<i64>> {
    let rest = after
        .trim()
        .trim_start_matches('#')
        .trim_start_matches("số")
        .trim_start_matches("so ")
        .trim();
    if rest.is_empty() {
        Some(None)
    } else {
        rest.parse().ok().map(Some)
    }
}

/// Whether the text's first words are one of `phrases` ("tạo bảng điểm danh …").
fn starts_with_words(word: &str, phrases: &[String]) -> bool {
    let tokens: Vec<&str> = word
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    phrases.iter().any(|phrase| {
        let phrase: Vec<&str> = phrase.split_whitespace().collect();
        tokens.starts_with(&phrase)
    })
}

/// A reminder request starts with "nhắc", possibly after "đặt lịch" or "đặt"
/// ("đặt lịch nhắc …"). A match post that mentions "nhắc" later ("…, nhắc ae
/// mang áo trắng") stays a match.
fn asks_for_reminder(word: &str) -> bool {
    let tokens: Vec<&str> = word
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    let commands = &COPY.commands;
    let lead = commands
        .remind_lead
        .iter()
        .map(|lead| lead.split_whitespace().collect::<Vec<_>>())
        .find(|lead| tokens.starts_with(lead))
        .map_or(0, |lead| lead.len());
    tokens
        .get(lead)
        .is_some_and(|first| commands.remind.iter().any(|remind| remind == first))
}

/// Whether a message asks for open matches: it uses a "find" phrase and, since
/// organisers' match posts carry times and counts, has no digits.
pub fn asks_for_matches(text: &str) -> bool {
    !text.chars().any(|c| c.is_ascii_digit()) && mentions_find_phrase(text)
}

/// Whether the text holds a "find a match" phrase as whole words anywhere.
pub fn mentions_find_phrase(text: &str) -> bool {
    let text = text.to_lowercase();
    let tokens: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    COPY.commands.find_matches.iter().any(|phrase| {
        let phrase: Vec<&str> = phrase.split_whitespace().collect();
        tokens
            .windows(phrase.len())
            .any(|window| window == phrase.as_slice())
    })
}

/// One open match, as the bot lists it to a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMatchLine {
    pub share_id: String,
    pub venue_name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub format: Format,
    pub places_left: i64,
    pub price_per_player_vnd: i64,
}

pub type FindFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<OpenMatchLine>, String>> + Send + 'a>>;

/// Port: the soonest upcoming matches with open places (the public match list).
pub trait MatchFinder: Send + Sync {
    fn find(&self, limit: usize) -> FindFuture<'_>;
}

/// Strips the tag when the text starts with it as a whole word, ignoring case.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    let rest = &text[prefix.len()..];
    let whole_word = rest.is_empty()
        || rest
            .starts_with(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '.' | '!' | '-'));
    (whole_word && head.to_lowercase() == prefix.to_lowercase()).then_some(rest)
}

/// A match waiting for the sender's "ok"; every field already passed the match rules.
#[derive(Debug, Clone, PartialEq)]
pub struct BotDraft {
    pub zalo_user_id: String,
    pub chat_id: String,
    pub user: UserId,
    pub venue_slug: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub format: Format,
    pub match_type: MatchType,
    pub level_min_tenths: i16,
    pub level_max_tenths: i16,
    pub slot_count: i16,
    pub total_fee_vnd: i64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
#[error("bot repository error: {0}")]
pub struct RepoError(pub String);

/// Port: everything the bot keeps in Postgres.
pub trait BotRepository: Send + Sync {
    /// Records a message from `sender`, unless it was seen before or the sender
    /// reached the hourly limit. Also forgets old messages.
    fn record_update(
        &self,
        message_id: &str,
        sender_id: &str,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<UpdateOutcome, RepoError>> + Send;

    fn linked_user(
        &self,
        zalo_user_id: &str,
    ) -> impl Future<Output = Result<Option<UserId>, RepoError>> + Send;

    /// The Zalo account linked to `user`, for private messages to them.
    fn zalo_id_of(
        &self,
        user: UserId,
    ) -> impl Future<Output = Result<Option<String>, RepoError>> + Send;

    fn save_link_code(
        &self,
        code_hash: [u8; 32],
        zalo_user_id: &str,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;

    /// Uses up a link code and links its Zalo account to `user`; returns the Zalo
    /// id, or `None` when the code is unknown, used or expired.
    fn redeem_link_code(
        &self,
        code_hash: [u8; 32],
        user: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<String>, RepoError>> + Send;

    /// Replaces the sender's pending draft in that chat.
    fn save_draft(&self, draft: &BotDraft) -> impl Future<Output = Result<(), RepoError>> + Send;

    /// Removes and returns the sender's unexpired draft in that chat.
    fn take_draft(
        &self,
        zalo_user_id: &str,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<BotDraft>, RepoError>> + Send;

    /// Replaces whatever the sender has pending in that chat (a match draft or a
    /// reminder) with this reminder.
    fn save_pending_reminder(
        &self,
        pending: &PendingReminder,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;

    /// Removes and returns the sender's unexpired pending reminder in that chat.
    fn take_pending_reminder(
        &self,
        zalo_user_id: &str,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<PendingReminder>, RepoError>> + Send;

    /// Saves a reminder unless its chat already has `MAX_REMINDERS_PER_CHAT`;
    /// returns whether it was saved.
    fn add_reminder(
        &self,
        chat_id: &str,
        user: UserId,
        plan: &ReminderPlan,
        next_run_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, RepoError>> + Send;

    /// The chat's reminders, oldest first (their list numbers).
    fn reminders(
        &self,
        chat_id: &str,
    ) -> impl Future<Output = Result<Vec<Reminder>, RepoError>> + Send;

    fn delete_reminder(
        &self,
        chat_id: &str,
        id: i64,
    ) -> impl Future<Output = Result<bool, RepoError>> + Send;

    /// Takes up to `REMINDERS_PER_RUN` reminders due at `now` and moves each to
    /// its next run before returning them, so a reminder is posted at most once
    /// even when runs overlap.
    fn claim_due_reminders(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Reminder>, RepoError>> + Send;

    /// Records a send result; a reminder that failed `MAX_SEND_FAILURES` times
    /// in a row is deleted.
    fn record_reminder_send(
        &self,
        id: i64,
        delivered: bool,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;
}

#[derive(Debug, thiserror::Error)]
#[error("chat reply failed: {0}")]
pub struct SendError(pub String);

pub type SendFuture<'a> = Pin<Box<dyn Future<Output = Result<(), SendError>> + Send + 'a>>;

/// Port: sends a text message to a Zalo chat.
pub trait ChatReply: Send + Sync {
    fn send<'a>(&'a self, chat_id: &'a str, text: &'a str) -> SendFuture<'a>;
}

/// The bot's replies, kept in `backend/messages/vi.json` (end-user copy).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotCopy {
    pub help: String,
    pub link_in_group: String,
    pub link_private: String,
    pub linked: String,
    pub not_linked: String,
    pub missing: String,
    pub fields: HashMap<String, String>,
    pub summary: String,
    pub confirm_hint: String,
    pub no_draft: String,
    pub cancelled: String,
    pub created: String,
    pub payout_needed: String,
    pub invalid: String,
    pub reasons: HashMap<String, String>,
    pub limit_user: String,
    pub limit_service: String,
    pub unavailable: String,
    pub failed: String,
    pub too_long: String,
    pub formats: HashMap<String, String>,
    pub weekdays: Vec<String>,
    pub per_player: String,
    pub free: String,
    pub currency: String,
    pub anonymous_name: String,
    pub not_a_match: String,
    pub reminder_summary: String,
    pub reminder_confirm_hint: String,
    pub reminder_missing: String,
    pub reminder_gaps: HashMap<String, String>,
    pub reminder_saved: String,
    pub reminder_limit: String,
    pub reminder_list: String,
    pub reminder_list_item: String,
    pub no_reminders: String,
    pub reminder_stopped: String,
    pub reminder_not_found: String,
    pub reminder_post: String,
    pub poll_summary: String,
    pub poll_missing: String,
    pub poll_gaps: HashMap<String, String>,
    pub poll_posted: String,
    pub poll_limit: String,
    pub poll_nudge: String,
    pub poll_count: String,
    pub poll_days_with_time: String,
    pub poll_play_suffix: String,
    pub poll_weekly_summary: String,
    pub poll_weekly_saved: String,
    pub series_limit: String,
    pub series_list: String,
    pub series_list_item: String,
    pub no_series: String,
    pub series_stopped: String,
    pub series_not_found: String,
    pub open_matches: String,
    pub sports: HashMap<String, String>,
    pub group_public: String,
    pub venue_needs_link: String,
    pub venue_needs_sport: String,
    pub venue_bad_link: String,
    pub venue_outside: String,
    pub venue_submitted: String,
    pub venue_admin_notice: String,
    pub venue_approved: String,
    pub venue_rejected: String,
    pub venue_approved_admin: String,
    pub venue_rejected_admin: String,
    pub venue_duplicate: String,
    pub venue_exists: String,
    pub venue_named: String,
    /// Words a venue name may already start with ("Sân").
    pub venue_prefixes: Vec<String>,
    pub venue_not_pending: String,
    pub admin_only: String,
    pub no_pending_venues: String,
    pub pending_venues_list: String,
    pub pending_venue_item: String,
    pub series_published: String,
    pub series_hidden: String,
    pub publish_hint: String,
    pub link_sent_privately: String,
    pub organiser_hint: String,
    pub open_matches_unavailable: String,
    pub open_match_line: String,
    pub no_open_matches: String,
    pub poll_missing_names: String,
    pub poll_more_names: String,
    pub player_name: String,
    pub every_day: String,
    pub on_weekdays: String,
    pub month_day: String,
    pub commands: Commands,
}

/// Words that mean a command (lower case, without trailing punctuation).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commands {
    pub help: Vec<String>,
    pub link: Vec<String>,
    pub confirm: Vec<String>,
    pub cancel: Vec<String>,
    /// A message whose first word is one of these asks for a reminder.
    pub remind: Vec<String>,
    /// Words that may come before it ("đặt lịch", "đặt"), longest first.
    pub remind_lead: Vec<String>,
    pub list_reminders: Vec<String>,
    /// Prefixes; a number may follow ("tắt nhắc 2").
    pub stop_reminder: Vec<String>,
    /// First words of a poll request ("tạo bảng điểm danh …").
    pub poll: Vec<String>,
    /// Prefixes; a number may follow ("tắt bảng 1").
    pub stop_poll: Vec<String>,
    /// Words that ask for a poll every week ("hằng tuần").
    pub weekly: Vec<String>,
    /// Phrases of a player looking for a match ("kèo nào", "muốn đá").
    pub find_matches: Vec<String>,
    /// First words of a venue request ("thêm sân").
    pub add_venue: Vec<String>,
    /// Admin prefixes followed by a venue id ("duyệt sân 12").
    pub approve_venue: Vec<String>,
    pub reject_venue: Vec<String>,
    pub pending_venues: Vec<String>,
    /// Prefixes followed by a weekly poll's number ("công khai bảng 1").
    pub publish_poll: Vec<String>,
    pub hide_poll: Vec<String>,
    /// Words that ask for a weekly poll to be public when it is made.
    pub public: Vec<String>,
    /// Sport words in a venue request.
    pub venue_football: Vec<String>,
    pub venue_basketball: Vec<String>,
}

const COPY_JSON: &str = include_str!("../../messages/vi.json");

/// Parses the copy file; `main` calls it at startup so a broken file stops the
/// server there instead of on the first message.
pub fn check_copy() -> Result<(), serde_json::Error> {
    serde_json::from_str::<BotCopy>(COPY_JSON).map(|_| ())
}

pub static COPY: LazyLock<BotCopy> = LazyLock::new(|| {
    serde_json::from_str(COPY_JSON)
        .unwrap_or_else(|error| panic!("backend/messages/vi.json is invalid: {error}"))
});

/// Replaces `{name}` placeholders.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

/// Text for a map entry that the copy file must have (checked by a test).
pub fn entry<'a>(map: &'a HashMap<String, String>, key: &'a str) -> &'a str {
    map.get(key).map_or(key, String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: &str = "Bot DaGhep";

    #[test]
    fn group_messages_must_tag_the_bot() {
        assert_eq!(parse_command("tối nay 19h", ChatKind::Group, BOT), None);
        assert_eq!(
            parse_command("@Bot DaGhep tối nay 19h SSA", ChatKind::Group, BOT),
            Some(Command::Describe("tối nay 19h SSA".to_owned()))
        );
        assert_eq!(
            parse_command("@bot daghep OK", ChatKind::Group, BOT),
            Some(Command::Confirm)
        );
    }

    #[test]
    fn private_messages_may_skip_the_tag() {
        assert_eq!(
            parse_command("Liên kết", ChatKind::Private, BOT),
            Some(Command::Link)
        );
        assert_eq!(
            parse_command("@Bot DaGhep liên kết", ChatKind::Private, BOT),
            Some(Command::Link)
        );
        assert_eq!(
            parse_command("  ", ChatKind::Private, BOT),
            Some(Command::Help)
        );
    }

    #[test]
    fn understands_the_short_commands() {
        for (text, command) in [
            ("ok", Command::Confirm),
            ("oke!", Command::Confirm),
            ("đồng ý", Command::Confirm),
            ("huỷ", Command::Cancel),
            ("hủy", Command::Cancel),
            ("hi", Command::Help),
            ("hướng dẫn", Command::Help),
        ] {
            assert_eq!(
                parse_command(text, ChatKind::Private, BOT),
                Some(command),
                "{text}"
            );
        }
        assert_eq!(
            parse_command("ok tối nay 19h", ChatKind::Private, BOT),
            Some(Command::Describe("ok tối nay 19h".to_owned()))
        );
        assert_eq!(
            parse_command("chốt", ChatKind::Private, BOT),
            Some(Command::Confirm)
        );
        assert_eq!(
            parse_command("ok nhé", ChatKind::Private, BOT),
            Some(Command::Confirm)
        );
    }

    #[test]
    fn the_tag_must_be_a_whole_word() {
        assert_eq!(parse_command("@Bot DaGhepok", ChatKind::Group, BOT), None);
        assert_eq!(
            parse_command("@Bot DaGhep, ok", ChatKind::Group, BOT),
            Some(Command::Confirm)
        );
        assert_eq!(
            parse_command("@Bot DaGhep: tối nay 19h", ChatKind::Group, BOT),
            Some(Command::Describe("tối nay 19h".to_owned()))
        );
        assert_eq!(
            parse_command("@Bot DaGhep", ChatKind::Group, BOT),
            Some(Command::Help)
        );
    }

    #[test]
    fn understands_reminder_commands() {
        assert_eq!(
            parse_command(
                "@Bot DaGhep nhắc mỗi thứ 5 20h: ai đá không?",
                ChatKind::Group,
                BOT
            ),
            Some(Command::Remind(
                "nhắc mỗi thứ 5 20h: ai đá không?".to_owned()
            ))
        );
        assert_eq!(
            parse_command(
                "@Bot DaGhep đặt lịch nhắc hẹn mỗi tuần t7 cn",
                ChatKind::Group,
                BOT
            ),
            Some(Command::Remind(
                "đặt lịch nhắc hẹn mỗi tuần t7 cn".to_owned()
            ))
        );
        for (text, number) in [
            ("tắt nhắc", None),
            ("tắt nhắc 2", Some(2)),
            ("Tắt nhắc số 3.", Some(3)),
            ("xoá nhắc #1", Some(1)),
        ] {
            assert_eq!(
                parse_command(text, ChatKind::Private, BOT),
                Some(Command::StopReminder(number)),
                "{text}"
            );
        }
        assert_eq!(
            parse_command("xem nhắc", ChatKind::Private, BOT),
            Some(Command::ListReminders)
        );
        assert_eq!(
            parse_command("@Bot DaGhep tắt bảng", ChatKind::Group, BOT),
            Some(Command::StopPoll(None))
        );
        assert_eq!(
            parse_command("dừng bảng số 2", ChatKind::Private, BOT),
            Some(Command::StopPoll(Some(2)))
        );
        assert_eq!(
            parse_command(
                "@Bot DaGhep tạo bảng điểm danh T7, CN, nhắc lúc 20h",
                ChatKind::Group,
                BOT
            ),
            Some(Command::Poll(
                "tạo bảng điểm danh T7, CN, nhắc lúc 20h".to_owned()
            ))
        );
        assert_eq!(
            parse_command(
                "@Bot DaGhep thêm sân Lakeview https://maps.app.goo.gl/x",
                ChatKind::Group,
                BOT
            ),
            Some(Command::AddVenue(
                "thêm sân Lakeview https://maps.app.goo.gl/x".to_owned()
            ))
        );
        assert_eq!(
            parse_command("duyệt sân 12", ChatKind::Private, BOT),
            Some(Command::DecideVenue {
                id: 12,
                approve: true
            })
        );
        assert_eq!(
            parse_command("từ chối sân 3", ChatKind::Private, BOT),
            Some(Command::DecideVenue {
                id: 3,
                approve: false
            })
        );
        assert_eq!(
            parse_command("sân chờ duyệt", ChatKind::Private, BOT),
            Some(Command::PendingVenues)
        );
        assert_eq!(
            parse_command("@Bot DaGhep công khai bảng 1", ChatKind::Group, BOT),
            Some(Command::PublishPoll {
                number: Some(1),
                public: true
            })
        );
        assert_eq!(
            parse_command("@Bot DaGhep ẩn bảng 2", ChatKind::Group, BOT),
            Some(Command::PublishPoll {
                number: Some(2),
                public: false
            })
        );
        // A poll request that starts with "công khai" is not a toggle.
        assert!(!matches!(
            parse_command(
                "@Bot DaGhep công khai bảng điểm danh T7, CN hằng tuần 18h",
                ChatKind::Group,
                BOT
            ),
            Some(Command::PublishPoll { .. })
        ));
        // Players looking for a match, in their own words.
        for text in [
            "tui muốn đá",
            "tối nay có kèo nào ko tui muốn đá ghép",
            "đá ghép",
            "tìm kèo",
        ] {
            assert_eq!(
                parse_command(&format!("@Bot DaGhep {text}"), ChatKind::Group, BOT),
                Some(Command::FindMatches),
                "{text}"
            );
        }
        // An organiser's post with times and counts is still a match post,
        // even with a "find" phrase in it.
        assert_eq!(
            parse_command(
                "@Bot DaGhep tối nay 19h có kèo thiếu 3",
                ChatKind::Group,
                BOT
            ),
            Some(Command::Describe("tối nay 19h có kèo thiếu 3".to_owned()))
        );
        // Poll and reminder requests win over "find" phrases.
        assert!(matches!(
            parse_command(
                "@Bot DaGhep nhắc mỗi thứ năm: ai đá không?",
                ChatKind::Group,
                BOT
            ),
            Some(Command::Remind(_))
        ));
        assert!(matches!(
            parse_command(
                "@Bot DaGhep tạo bảng điểm danh thứ bảy, muốn đá thì vào",
                ChatKind::Group,
                BOT
            ),
            Some(Command::Poll(_))
        ));
        // Whole words only: "muon dat san" (book a pitch) is not "muon da".
        assert!(!mentions_find_phrase("muon dat san"));
        assert!(mentions_find_phrase("ai muon da khong"));
        // A match post that mentions "nhắc" later is still a match.
        let post = "tối nay 19h SSA sân 7 thiếu 3, nhắc ae mang áo trắng";
        assert_eq!(
            parse_command(post, ChatKind::Private, BOT),
            Some(Command::Describe(post.to_owned()))
        );
        assert_eq!(
            parse_command("tắt nhắc 1, 2", ChatKind::Private, BOT),
            Some(Command::StopReminder(Some(1)))
        );
        // "huỷ" alone still cancels.
        assert_eq!(
            parse_command("huỷ", ChatKind::Private, BOT),
            Some(Command::Cancel)
        );
    }

    #[test]
    fn a_tag_with_a_multibyte_start_does_not_panic() {
        assert_eq!(parse_command("đá", ChatKind::Group, BOT), None);
    }

    #[test]
    fn the_copy_file_has_every_entry_the_bot_uses() {
        let copy = &*COPY;
        for key in [
            "venue",
            "date",
            "startTime",
            "endTime",
            "format",
            "slotCount",
            "fee",
        ] {
            assert!(copy.fields.contains_key(key), "fields.{key}");
        }
        for key in [
            "venueId",
            "startsAt",
            "endsAt",
            "slotCount",
            "totalFeeVnd",
            "other",
        ] {
            assert!(copy.reasons.contains_key(key), "reasons.{key}");
        }
        for gap in [
            super::super::polls::PollGap::Title,
            super::super::polls::PollGap::Days,
            super::super::polls::PollGap::PlayTime,
            super::super::polls::PollGap::NudgeTime,
        ] {
            assert!(
                copy.poll_gaps.contains_key(gap.key()),
                "pollGaps.{}",
                gap.key()
            );
        }
        for gap in [
            super::super::reminders::ReminderGap::Repeat,
            super::super::reminders::ReminderGap::Time,
            super::super::reminders::ReminderGap::Message,
        ] {
            assert!(
                copy.reminder_gaps.contains_key(gap.key()),
                "reminderGaps.{}",
                gap.key()
            );
        }
        for format in [Format::FiveASide, Format::SevenASide, Format::ElevenASide] {
            assert!(
                copy.formats.contains_key(format.as_str()),
                "{}",
                format.as_str()
            );
        }
        assert_eq!(copy.weekdays.len(), 7);
    }

    #[test]
    fn fill_replaces_placeholders() {
        assert_eq!(fill("{a} và {b}", &[("a", "1"), ("b", "2")]), "1 và 2");
    }
}
