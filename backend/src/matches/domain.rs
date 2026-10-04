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

/// Player level stored in tenths: 1.0 (beginner) .. 5.0 (semi-pro).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Level(i16);

impl Level {
    pub fn from_tenths(tenths: i16) -> Option<Self> {
        (10..=50).contains(&tenths).then_some(Self(tenths))
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

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("storage unavailable: {0}")]
    Unavailable(String),
    #[error("stored match is invalid: {0}")]
    Corrupt(String),
}

/// Port for reading matches. Adapters live in `repo.rs`.
pub trait MatchRepository: Send + Sync {
    fn find_by_share_id(
        &self,
        id: &ShareId,
    ) -> impl Future<Output = Result<Option<Match>, RepoError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn level_is_bounded_between_one_and_five() {
        assert_eq!(Level::from_tenths(10).map(Level::as_f64), Some(1.0));
        assert_eq!(Level::from_tenths(35).map(Level::as_f64), Some(3.5));
        assert!(Level::from_tenths(9).is_none());
        assert!(Level::from_tenths(51).is_none());
    }
}
