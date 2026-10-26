//! Join requests for public weekly groups (`docs/public-groups-plan.md`, step 2):
//! a player outside the Zalo group asks to join from the home page, and the
//! organiser approves or declines through the bot. Pure rules and the port;
//! no Axum, sqlx, HTTP or SDK types.

use std::future::Future;

use chrono::{DateTime, Duration, Utc};

use crate::auth::UserId;
use crate::matches::domain::ShareId;

/// Pending requests one player may have at a time (approved by Long 2026-10-26).
pub const MAX_PENDING_PER_PLAYER: i64 = 5;
/// Pending requests waiting for one organiser across their groups.
pub const MAX_PENDING_PER_ORGANISER: i64 = 50;
/// How long a declined player waits before asking the same group again.
pub const ASK_AGAIN_DAYS: i64 = 7;
/// Days a request waits for the organiser; after that it lapses and the player
/// may ask again. TODO: verify with Long (proposed 2026-10-26).
pub const PENDING_DAYS: i64 = 14;

/// Where a viewer stands with a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Membership {
    /// Not a member and no request.
    Outside,
    Pending,
    /// Declined; may ask again from `ask_again_at`.
    Declined {
        ask_again_at: DateTime<Utc>,
    },
    /// The organiser, someone seen in the group's chat, or an approved player.
    Member {
        group_link: Option<String>,
        /// This week's open poll, if any.
        poll: Option<ShareId>,
    },
}

/// What asking to join found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinOutcome {
    /// A new pending request, for the organiser to decide.
    Requested(JoinNotice),
    AlreadyPending,
    AlreadyMember,
    TooSoon {
        ask_again_at: DateTime<Utc>,
    },
    PlayerLimit,
    OrganiserLimit,
    UnknownGroup,
}

/// What the organiser is told about a new request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinNotice {
    pub request_id: i64,
    pub organiser: UserId,
    pub title: String,
    pub player_name: Option<String>,
}

/// A request waiting for the organiser, as the bot lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingRequest {
    pub id: i64,
    pub player_name: Option<String>,
    pub title: String,
}

/// What deciding a request found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideOutcome {
    Decided(Decision),
    /// Unknown, not this organiser's, or already decided.
    NotFound,
}

/// A decided request, for telling the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub player: UserId,
    pub player_name: Option<String>,
    pub title: String,
    pub approved: bool,
    pub group_link: Option<String>,
    pub public_id: String,
}

/// When a declined player may ask again.
pub fn ask_again_at(decided_at: DateTime<Utc>) -> DateTime<Utc> {
    decided_at + Duration::days(ASK_AGAIN_DAYS)
}

/// A Zalo group invite link from the organiser's message
/// ("link nhóm: https://zalo.me/g/abc123"), or `None` when there is none.
pub fn find_group_link(text: &str) -> Option<&str> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | ',' | '.')))
        // A copied link may carry a query ("?src=share").
        .map(|word| word.split(['?', '#']).next().unwrap_or(word))
        .find(|word| {
            word.strip_prefix("https://zalo.me/g/").is_some_and(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code.chars().all(|c| c.is_ascii_alphanumeric())
            })
        })
}

#[derive(Debug, thiserror::Error)]
#[error("group repository error: {0}")]
pub struct GroupRepoError(pub String);

/// Port: join requests and group links in Postgres.
pub trait GroupRepository: Send + Sync {
    /// Where `user` stands with the public group `public_id`; `None` when there
    /// is no such public group.
    fn membership(
        &self,
        public_id: &str,
        user: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<Membership>, GroupRepoError>> + Send;

    /// Records a request to join, within the limits.
    fn request_join(
        &self,
        public_id: &str,
        user: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<JoinOutcome, GroupRepoError>> + Send;

    /// Approves or declines a pending request to one of `organiser`'s groups.
    fn decide_join(
        &self,
        organiser: UserId,
        request_id: i64,
        approve: bool,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<DecideOutcome, GroupRepoError>> + Send;

    /// Requests waiting for `organiser`, oldest first.
    fn waiting_requests(
        &self,
        organiser: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<WaitingRequest>, GroupRepoError>> + Send;

    /// Sets the Zalo link of `organiser`'s groups in this chat; returns how many.
    fn set_group_link(
        &self,
        chat_id: &str,
        organiser: UserId,
        link: &str,
    ) -> impl Future<Output = Result<u64, GroupRepoError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_zalo_group_link() {
        assert_eq!(
            find_group_link("link nhóm: https://zalo.me/g/abc123XYZ."),
            Some("https://zalo.me/g/abc123XYZ")
        );
        assert_eq!(
            find_group_link("https://zalo.me/g/abc123?src=share"),
            Some("https://zalo.me/g/abc123")
        );
        for text in [
            "link nhóm: http://zalo.me/g/abc",
            "link nhóm: https://zalo.me/g/",
            "link nhóm: https://zalo.me/g/abc/def",
            "link nhóm: https://zalo.me.evil.com/g/abc",
            "link nhóm: https://zalo.me/abc",
            "link nhóm",
        ] {
            assert_eq!(find_group_link(text), None, "{text}");
        }
    }

    #[test]
    fn a_declined_player_may_ask_again_after_seven_days() {
        let decided = DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(ask_again_at(decided), decided + Duration::days(7));
    }
}
