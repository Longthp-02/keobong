//! Match domain types and the repository port. No Axum, sqlx or wire types here.

use std::future::Future;

use chrono::{DateTime, Utc};

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

impl ShareId {
    /// Random 10-character id (60 bits of entropy) from the OS random source.
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; SHARE_ID_LEN];
        getrandom::fill(&mut bytes)?;
        let id = bytes
            .iter()
            .map(|b| char::from(SHARE_ID_ALPHABET[usize::from(b & 63)]))
            .collect();
        Ok(Self(id))
    }
}

/// Total fee split across all slots, rounded up to the next 1,000 VND; the host keeps the remainder.
pub fn price_per_player_vnd(total_fee_vnd: i64, slot_count: i16) -> i64 {
    let divisor = i64::from(slot_count.max(1)) * 1_000;
    (total_fee_vnd + divisor - 1) / divisor * 1_000
}

pub const SLOT_COUNT_RANGE: std::ops::RangeInclusive<i16> = 2..=30;
pub const VENUE_NAME_MAX_CHARS: usize = 120;

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
    pub slot_count: Option<i16>,
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
        if venue_len == 0 || venue_len > VENUE_NAME_MAX_CHARS {
            return Err(Field::VenueName);
        }
        if input.starts_at <= now {
            return Err(Field::StartsAt);
        }
        if input.ends_at <= input.starts_at {
            return Err(Field::EndsAt);
        }
        let level_min = Level::from_value(input.level_min).ok_or(Field::LevelMin)?;
        let level_max = Level::from_value(input.level_max)
            .filter(|max| *max >= level_min)
            .ok_or(Field::LevelMax)?;
        if input.total_fee_vnd < 0 {
            return Err(Field::TotalFeeVnd);
        }
        let slot_count = input
            .slot_count
            .unwrap_or_else(|| input.format.default_slot_count());
        if !SLOT_COUNT_RANGE.contains(&slot_count) {
            return Err(Field::SlotCount);
        }
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
    ) -> impl Future<Output = Result<(), InsertError>> + Send;
}

/// Time source, injected so rules like "must start in the future" are testable.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

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
        let cases: [Case; 8] = [
            (|i| i.venue_name = " ".into(), Field::VenueName),
            (|i| i.venue_name = "x".repeat(121), Field::VenueName),
            (
                |i| i.starts_at = "2026-10-03T00:00:00Z".parse().unwrap(),
                Field::StartsAt,
            ),
            (|i| i.ends_at = i.starts_at, Field::EndsAt),
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
}
