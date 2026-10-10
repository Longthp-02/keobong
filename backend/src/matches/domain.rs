//! Match domain types and the repository port. No Axum, sqlx or wire types here.

use std::future::Future;

use chrono::{DateTime, Duration, FixedOffset, Utc};

use crate::auth::UserId;
use crate::text::is_disallowed_in_names;

/// Public, unguessable identifier used in share links. Internal ids never leave the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareId(String);

impl ShareId {
    /// Accepts 8–16 characters from `[A-Za-z0-9_-]`, matching the database constraint.
    pub fn parse(raw: &str) -> Option<Self> {
        let valid_len = (8..=16).contains(&raw.len());
        let valid_chars = raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        (valid_len && valid_chars).then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    FiveASide,
    SevenASide,
    ElevenASide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    Casual,
    Competitive,
    BeginnerFriendly,
}

/// Player level stored in tenths: 1.0 (beginner) .. 5.0 (semi-pro), in steps of 0.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Level(i16);

impl Level {
    pub fn from_tenths(tenths: i16) -> Option<Self> {
        ((10..=50).contains(&tenths) && tenths % 5 == 0).then_some(Self(tenths))
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / 10.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub share_id: ShareId,
    pub venue_name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub format: Format,
    pub match_type: MatchType,
    pub level_min: Level,
    pub level_max: Level,
    pub total_fee_vnd: i64,
    pub slot_count: i16,
    /// Set when the host cancelled the match.
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl Format {
    /// Default slots include substitutes so players can rotate (confirmed by Long).
    pub fn default_slot_count(self) -> i16 {
        match self {
            Format::FiveASide => 14,
            Format::SevenASide => 18,
            Format::ElevenASide => 28,
        }
    }
}

impl Level {
    /// Accepts 1.0..=5.0 in steps of 0.5; rejects anything that is not an exact half step.
    pub fn from_value(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let tenths = (value * 10.0).round();
        if (value * 10.0 - tenths).abs() > 1e-9 || !(10.0..=50.0).contains(&tenths) {
            return None;
        }
        // In range 10..=50, so the cast cannot truncate.
        Self::from_tenths(tenths as i16)
    }
}

/// URL-safe alphabet of exactly 64 symbols, so `byte & 63` picks uniformly.
const SHARE_ID_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
const SHARE_ID_LEN: usize = 10;

pub use crate::random::RandomnessError;

impl ShareId {
    /// Random 10-character id (60 bits of entropy) from the OS random source.
    pub fn generate() -> Result<Self, RandomnessError> {
        let bytes = crate::random::random_bytes::<SHARE_ID_LEN>()?;
        let id = bytes
            .iter()
            .map(|b| char::from(SHARE_ID_ALPHABET[usize::from(b & 63)]))
            .collect();
        Ok(Self(id))
    }
}

/// Total fee split across all slots, rounded up to the next 1,000 VND; the host keeps the remainder.
pub fn price_per_player_vnd(total_fee_vnd: i64, slot_count: i16) -> i64 {
    // i128 cannot overflow here; saturate in case a stored fee ever exceeds the guard.
    let divisor = i128::from(slot_count.max(1)) * 1_000;
    let rounded = (i128::from(total_fee_vnd) + divisor - 1) / divisor * 1_000;
    i64::try_from(rounded).unwrap_or(i64::MAX)
}

// SLOT_COUNT_RANGE, MAX_TOTAL_FEE_VND and MAX_DURATION_HOURS are mirrored in
// frontend/lib/pricing.ts for the form; change both sides together.
pub const SLOT_COUNT_RANGE: std::ops::RangeInclusive<i16> = 2..=30;
pub const VENUE_NAME_MAX_CHARS: usize = 120;
/// Largest total fee (confirmed by Long 2026-10-04); also enforced by a DB constraint.
pub const MAX_TOTAL_FEE_VND: i64 = 100_000_000;
/// How far ahead a match can be created.
pub const MAX_DAYS_AHEAD: i64 = 30;
/// Longest allowed match.
pub const MAX_DURATION_HOURS: i64 = 4;
/// Matches are local to Ho Chi Minh City (UTC+7, no daylight saving time).
const LOCAL_OFFSET: FixedOffset = match FixedOffset::east_opt(7 * 3600) {
    Some(offset) => offset,
    None => panic!("UTC+7 is a valid offset"),
};

/// A match starts and ends on the same local calendar day.
fn same_local_day(start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    start.with_timezone(&LOCAL_OFFSET).date_naive() == end.with_timezone(&LOCAL_OFFSET).date_naive()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    VenueName,
    StartsAt,
    EndsAt,
    Format,
    MatchType,
    LevelMin,
    LevelMax,
    TotalFeeVnd,
    SlotCount,
    /// A paid match needs the host's payout account so players know where to pay.
    Payout,
}

#[derive(Debug, Clone)]
pub struct NewMatchInput {
    pub venue_name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub format: Format,
    pub match_type: MatchType,
    pub level_min: f64,
    pub level_max: f64,
    pub total_fee_vnd: i64,
    pub slot_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewMatch {
    pub venue_name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub format: Format,
    pub match_type: MatchType,
    pub level_min: Level,
    pub level_max: Level,
    pub total_fee_vnd: i64,
    pub slot_count: i16,
}

impl NewMatch {
    /// Checks every rule in field order and returns the first offending field.
    pub fn validate(input: NewMatchInput, now: DateTime<Utc>) -> Result<Self, Field> {
        let venue_name = input.venue_name.trim().to_owned();
        let venue_len = venue_name.chars().count();
        if venue_len == 0
            || venue_len > VENUE_NAME_MAX_CHARS
            || venue_name.chars().any(is_disallowed_in_names)
        {
            return Err(Field::VenueName);
        }
        if input.starts_at <= now || input.starts_at > now + Duration::days(MAX_DAYS_AHEAD) {
            return Err(Field::StartsAt);
        }
        if input.ends_at <= input.starts_at
            || input.ends_at - input.starts_at > Duration::hours(MAX_DURATION_HOURS)
            || !same_local_day(input.starts_at, input.ends_at)
        {
            return Err(Field::EndsAt);
        }
        let level_min = Level::from_value(input.level_min).ok_or(Field::LevelMin)?;
        let level_max = Level::from_value(input.level_max)
            .filter(|max| *max >= level_min)
            .ok_or(Field::LevelMax)?;
        if !(0..=MAX_TOTAL_FEE_VND).contains(&input.total_fee_vnd) {
            return Err(Field::TotalFeeVnd);
        }
        let slot_count = match input.slot_count {
            None => input.format.default_slot_count(),
            Some(n) => i16::try_from(n)
                .ok()
                .filter(|n| SLOT_COUNT_RANGE.contains(n))
                .ok_or(Field::SlotCount)?,
        };
        Ok(Self {
            venue_name,
            starts_at: input.starts_at,
            ends_at: input.ends_at,
            format: input.format,
            match_type: input.match_type,
            level_min,
            level_max,
            total_fee_vnd: input.total_fee_vnd,
            slot_count,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("storage unavailable: {0}")]
    Unavailable(String),
    #[error("stored match is invalid: {0}")]
    Corrupt(String),
}

impl Match {
    pub fn from_new(share_id: ShareId, new: NewMatch) -> Self {
        Self {
            share_id,
            venue_name: new.venue_name,
            starts_at: new.starts_at,
            ends_at: new.ends_at,
            format: new.format,
            match_type: new.match_type,
            level_min: new.level_min,
            level_max: new.level_max,
            total_fee_vnd: new.total_fee_vnd,
            slot_count: new.slot_count,
            cancelled_at: None,
        }
    }

    pub fn price_per_player_vnd(&self) -> i64 {
        price_per_player_vnd(self.total_fee_vnd, self.slot_count)
    }
}

impl Level {
    pub fn tenths(self) -> i16 {
        self.0
    }
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::FiveASide => "five_a_side",
            Format::SevenASide => "seven_a_side",
            Format::ElevenASide => "eleven_a_side",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Format::FiveASide, Format::SevenASide, Format::ElevenASide]
            .into_iter()
            .find(|f| f.as_str() == value)
    }
}

impl MatchType {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchType::Casual => "casual",
            MatchType::Competitive => "competitive",
            MatchType::BeginnerFriendly => "beginner_friendly",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            MatchType::Casual,
            MatchType::Competitive,
            MatchType::BeginnerFriendly,
        ]
        .into_iter()
        .find(|t| t.as_str() == value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InsertError {
    #[error("share id already taken")]
    DuplicateShareId,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Port for storing and reading matches. Adapters live in `repo.rs`.
pub trait MatchRepository: Send + Sync {
    fn find_by_share_id(
        &self,
        id: &ShareId,
    ) -> impl Future<Output = Result<Option<Match>, RepoError>> + Send;

    fn insert(
        &self,
        share_id: &ShareId,
        new: &NewMatch,
        host: UserId,
    ) -> impl Future<Output = Result<(), InsertError>> + Send;

    /// Cancels the match if [`decide_cancel`] allows it, atomically.
    fn cancel(
        &self,
        share_id: &ShareId,
        caller: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<CancelOutcome, RepoError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    Cancelled,
    /// Already cancelled earlier; cancelling again changes nothing.
    AlreadyCancelled,
    MatchNotFound,
    NotHost,
    /// Cancelling closes at kickoff.
    MatchStarted,
}

/// Who may cancel and when: only the host, and only before kickoff.
pub fn decide_cancel(
    caller: UserId,
    host: UserId,
    starts_at: DateTime<Utc>,
    cancelled_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> CancelOutcome {
    if caller != host {
        CancelOutcome::NotHost
    } else if cancelled_at.is_some() {
        CancelOutcome::AlreadyCancelled
    } else if now >= starts_at {
        CancelOutcome::MatchStarted
    } else {
        CancelOutcome::Cancelled
    }
}

pub use crate::clock::{Clock, SystemClock};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_slot_count_includes_substitutes() {
        assert_eq!(Format::FiveASide.default_slot_count(), 14);
        assert_eq!(Format::SevenASide.default_slot_count(), 18);
        assert_eq!(Format::ElevenASide.default_slot_count(), 28);
    }

    #[test]
    fn price_per_player_never_overflows() {
        // Saturates instead of overflowing; validation keeps real fees far below this.
        assert_eq!(price_per_player_vnd(i64::MAX, 1), i64::MAX);
        assert_eq!(price_per_player_vnd(100_000_000, 1), 100_000_000);
    }

    #[test]
    fn venue_name_rejects_control_and_bidi_characters() {
        for bad in ["SSA\u{0}x", "SSA\nCenter", "SSA\u{202E}x", "SSA\u{2066}x"] {
            let mut i = input();
            i.venue_name = bad.to_owned();
            assert_eq!(
                NewMatch::validate(i, now()).unwrap_err(),
                Field::VenueName,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn boundaries_are_inclusive() {
        let mut i = input();
        i.venue_name = "Đ".repeat(120);
        i.slot_count = Some(2);
        i.total_fee_vnd = MAX_TOTAL_FEE_VND;
        assert!(NewMatch::validate(i, now()).is_ok());

        let mut i = input();
        i.slot_count = Some(30);
        assert!(NewMatch::validate(i, now()).is_ok());

        let mut i = input();
        i.total_fee_vnd = MAX_TOTAL_FEE_VND + 1;
        assert_eq!(
            NewMatch::validate(i, now()).unwrap_err(),
            Field::TotalFeeVnd
        );

        let mut i = input();
        i.slot_count = Some(40_000);
        assert_eq!(NewMatch::validate(i, now()).unwrap_err(), Field::SlotCount);

        let mut i = input();
        i.starts_at = now();
        assert_eq!(NewMatch::validate(i, now()).unwrap_err(), Field::StartsAt);
    }

    #[test]
    fn price_per_player_rounds_up_to_the_next_thousand() {
        assert_eq!(price_per_player_vnd(900_000, 18), 50_000);
        assert_eq!(price_per_player_vnd(900_000, 14), 65_000);
        assert_eq!(price_per_player_vnd(500_000, 10), 50_000);
        assert_eq!(price_per_player_vnd(1, 10), 1_000);
        assert_eq!(price_per_player_vnd(0, 10), 0);
    }

    #[test]
    fn level_from_value_accepts_only_half_steps() {
        assert_eq!(Level::from_value(2.5).map(Level::as_f64), Some(2.5));
        assert!(Level::from_value(2.3).is_none());
        assert!(Level::from_value(5.5).is_none());
        assert!(Level::from_value(f64::NAN).is_none());
    }

    fn now() -> DateTime<Utc> {
        "2026-10-04T00:00:00Z".parse().unwrap()
    }

    fn input() -> NewMatchInput {
        NewMatchInput {
            venue_name: "  SSA Sports Center ".to_owned(),
            starts_at: "2026-10-10T11:30:00Z".parse().unwrap(),
            ends_at: "2026-10-10T13:00:00Z".parse().unwrap(),
            format: Format::SevenASide,
            match_type: MatchType::Casual,
            level_min: 2.5,
            level_max: 3.5,
            total_fee_vnd: 900_000,
            slot_count: None,
        }
    }

    #[test]
    fn valid_input_is_trimmed_and_gets_default_slots() {
        let new = NewMatch::validate(input(), now()).unwrap();

        assert_eq!(new.venue_name, "SSA Sports Center");
        assert_eq!(new.slot_count, 18);
    }

    #[test]
    fn invalid_input_reports_the_first_bad_field() {
        type Case = (fn(&mut NewMatchInput), Field);
        let cases: [Case; 12] = [
            (|i| i.venue_name = " ".into(), Field::VenueName),
            (|i| i.venue_name = "x".repeat(121), Field::VenueName),
            (
                |i| i.starts_at = "2026-10-03T00:00:00Z".parse().unwrap(),
                Field::StartsAt,
            ),
            (|i| i.ends_at = i.starts_at, Field::EndsAt),
            // More than 30 days ahead of `now` (2026-10-04T00:00Z).
            (
                |i| {
                    i.starts_at = "2026-11-03T00:00:01Z".parse().unwrap();
                    i.ends_at = "2026-11-03T01:00:00Z".parse().unwrap();
                },
                Field::StartsAt,
            ),
            // Longer than 4 hours.
            (
                |i| i.ends_at = "2026-10-10T15:31:00Z".parse().unwrap(),
                Field::EndsAt,
            ),
            // 22:30-00:30 in Ho Chi Minh City crosses midnight.
            (
                |i| {
                    i.starts_at = "2026-10-10T15:30:00Z".parse().unwrap();
                    i.ends_at = "2026-10-10T17:30:00Z".parse().unwrap();
                },
                Field::EndsAt,
            ),
            // Ending at exactly midnight still belongs to the next day.
            (
                |i| {
                    i.starts_at = "2026-10-10T15:00:00Z".parse().unwrap();
                    i.ends_at = "2026-10-10T17:00:00Z".parse().unwrap();
                },
                Field::EndsAt,
            ),
            (|i| i.level_min = 1.2, Field::LevelMin),
            (|i| i.level_max = 2.0, Field::LevelMax),
            (|i| i.total_fee_vnd = -1, Field::TotalFeeVnd),
            (|i| i.slot_count = Some(1), Field::SlotCount),
        ];
        for (mutate, field) in cases {
            let mut bad = input();
            mutate(&mut bad);

            assert_eq!(NewMatch::validate(bad, now()).unwrap_err(), field);
        }
    }

    #[test]
    fn limits_are_inclusive() {
        let mut edge = input();
        // Exactly 30 days ahead, exactly 4 hours, ending 23:59 local time.
        edge.starts_at = "2026-11-03T12:59:00Z".parse().unwrap();
        edge.ends_at = "2026-11-03T16:59:00Z".parse().unwrap();
        let now: DateTime<Utc> = "2026-10-04T12:59:00Z".parse().unwrap();

        assert!(NewMatch::validate(edge, now).is_ok());
    }

    #[test]
    fn generated_share_ids_are_valid_and_distinct() {
        let a = ShareId::generate().unwrap();
        let b = ShareId::generate().unwrap();

        assert!(ShareId::parse(a.as_str()).is_some());
        assert_ne!(a, b);
    }

    #[test]
    fn share_id_accepts_url_safe_ids_of_8_to_16_chars() {
        assert!(ShareId::parse("k7Qm2xPa").is_some());
        assert!(ShareId::parse("abc_DEF-1234567x").is_some());
    }

    #[test]
    fn share_id_rejects_bad_length_or_characters() {
        assert!(ShareId::parse("short").is_none());
        assert!(ShareId::parse("waytoolongshareid1").is_none());
        assert!(ShareId::parse("bad!id12").is_none());
        assert!(ShareId::parse("has space").is_none());
        assert!(ShareId::parse("").is_none());
    }

    #[test]
    fn level_only_accepts_half_steps() {
        assert_eq!(Level::from_tenths(25).map(Level::as_f64), Some(2.5));
        assert_eq!(Level::from_tenths(30).map(Level::as_f64), Some(3.0));
        assert!(Level::from_tenths(23).is_none());
        assert!(Level::from_tenths(41).is_none());
    }

    #[test]
    fn level_is_bounded_between_one_and_five() {
        assert_eq!(Level::from_tenths(10).map(Level::as_f64), Some(1.0));
        assert_eq!(Level::from_tenths(35).map(Level::as_f64), Some(3.5));
        assert!(Level::from_tenths(9).is_none());
        assert!(Level::from_tenths(51).is_none());
    }

    #[test]
    fn only_the_host_cancels_and_only_before_kickoff() {
        let starts_at: DateTime<Utc> = "2099-10-10T11:30:00Z".parse().unwrap();
        let before = starts_at - Duration::seconds(1);
        let (host, other) = (UserId(1), UserId(2));

        assert_eq!(
            decide_cancel(host, host, starts_at, None, before),
            CancelOutcome::Cancelled
        );
        assert_eq!(
            decide_cancel(other, host, starts_at, None, before),
            CancelOutcome::NotHost
        );
        assert_eq!(
            decide_cancel(host, host, starts_at, None, starts_at),
            CancelOutcome::MatchStarted
        );
        assert_eq!(
            decide_cancel(host, host, starts_at, Some(before), starts_at),
            CancelOutcome::AlreadyCancelled
        );
    }
}
