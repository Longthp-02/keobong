//! Zalo bot domain: commands read from chat messages, reply copy and ports.
//! No Axum, sqlx, HTTP or SDK types here.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use serde::Deserialize;

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
    Some(if is(&commands.help) {
        Command::Help
    } else if is(&commands.link) {
        Command::Link
    } else if is(&commands.confirm) {
        Command::Confirm
    } else if is(&commands.cancel) {
        Command::Cancel
    } else {
        Command::Describe(rest.to_owned())
    })
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
    pub commands: Commands,
}

/// Words that mean a command (lower case, without trailing punctuation).
#[derive(Debug, Deserialize)]
pub struct Commands {
    pub help: Vec<String>,
    pub link: Vec<String>,
    pub confirm: Vec<String>,
    pub cancel: Vec<String>,
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
            parse_command("@Bot DaGhep", ChatKind::Group, BOT),
            Some(Command::Help)
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
