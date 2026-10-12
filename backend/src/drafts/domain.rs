//! Match drafts: what an AI reader made of an organiser's message, checked
//! against the match rules before the create form is filled with it.
//! No Axum, sqlx, HTTP or SDK types here.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};

use crate::auth::UserId;
use crate::matches::domain::{
    Format, Level, MAX_DAYS_AHEAD, MAX_DURATION_HOURS, MAX_TOTAL_FEE_VND, MatchType,
    SLOT_COUNT_RANGE, Venue,
};

/// Longest message accepted, in characters. Organisers' posts are a few lines.
pub const MAX_TEXT_CHARS: usize = 500;
/// Reads one signed-in user may request in 24 hours (confirmed by Long 2026-10-12).
pub const USER_READS_PER_DAY: i64 = 10;
/// Reads the whole service may request in 24 hours (confirmed by Long 2026-10-12).
pub const SERVICE_READS_PER_DAY: i64 = 200;

/// What the reader is asked to read.
pub struct ReadRequest<'a> {
    pub text: &'a str,
    /// Today in Ho Chi Minh City, so "tối nay" and "mai" resolve to dates.
    pub today: NaiveDate,
    pub venues: &'a [Venue],
}

/// The reader's answer before any checks. Every field may be missing or wrong.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawDraft {
    pub venue_id: Option<String>,
    pub date: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub format: Option<String>,
    pub match_type: Option<String>,
    pub level_min: Option<f64>,
    pub level_max: Option<f64>,
    /// Places the organiser still needs ("thiếu 3 người" → 3).
    pub slot_count: Option<i64>,
    pub price_per_player_vnd: Option<i64>,
    pub total_fee_vnd: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("reader request failed: {0}")]
    Failed(String),
}

pub type ReaderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RawDraft, ReaderError>> + Send + 'a>>;

/// Port: turns an organiser's message into draft fields (an AI model in production).
pub trait MatchTextReader: Send + Sync {
    fn read<'a>(&'a self, request: ReadRequest<'a>) -> ReaderFuture<'a>;
}

/// Draft fields that passed the match rules. Anything that did not is left out,
/// so the organiser fills it in by hand.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchDraft {
    pub venue_id: Option<String>,
    pub date: Option<NaiveDate>,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub format: Option<Format>,
    pub match_type: Option<MatchType>,
    pub level_min: Option<f64>,
    pub level_max: Option<f64>,
    pub slot_count: Option<i64>,
    pub total_fee_vnd: Option<i64>,
}

impl MatchDraft {
    /// Keeps only the fields that fit the match rules.
    pub fn checked(raw: RawDraft, today: NaiveDate, venues: &[Venue]) -> Self {
        let venue_id = raw
            .venue_id
            .filter(|id| venues.iter().any(|venue| venue.slug.as_str() == id));
        let date = raw
            .date
            .as_deref()
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
            .filter(|day| *day >= today && *day <= today + Duration::days(MAX_DAYS_AHEAD));
        let start_time = raw.start_time.as_deref().and_then(parse_time);
        let end_time = raw.end_time.as_deref().and_then(parse_time).filter(|end| {
            start_time.is_some_and(|start| {
                let length = *end - start;
                length > Duration::zero() && length <= Duration::hours(MAX_DURATION_HOURS)
            })
        });
        let level = |value: Option<f64>| value.and_then(Level::from_value).map(Level::as_f64);
        let (level_min, level_max) = match (level(raw.level_min), level(raw.level_max)) {
            (Some(min), Some(max)) if min > max => (None, None),
            levels => levels,
        };
        let slot_count = raw.slot_count.filter(|count| {
            i16::try_from(*count).is_ok_and(|count| SLOT_COUNT_RANGE.contains(&count))
        });
        // A message usually states the price per person; the match stores the
        // total for the places it offers (confirmed by Long 2026-10-12).
        let total_fee_vnd = raw
            .total_fee_vnd
            .or_else(|| {
                raw.price_per_player_vnd
                    .zip(slot_count)
                    .and_then(|(price, count)| price.checked_mul(count))
            })
            .filter(|fee| (0..=MAX_TOTAL_FEE_VND).contains(fee));
        Self {
            venue_id,
            date,
            start_time,
            end_time,
            format: raw.format.as_deref().and_then(Format::parse),
            match_type: raw.match_type.as_deref().and_then(MatchType::parse),
            level_min,
            level_max,
            slot_count,
            total_fee_vnd,
        }
    }
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}

/// Who asked for a read, for the daily limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitScope {
    User,
    Service,
}

#[derive(Debug, thiserror::Error)]
#[error("draft repository error: {0}")]
pub struct RepoError(pub String);

/// Port: counts reads so the daily limits hold across instances.
pub trait DraftRepository: Send + Sync {
    /// Records a read for `user` at `now` unless a limit is reached; the check and
    /// the insert are one statement.
    fn record_read(
        &self,
        user: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Result<(), LimitScope>, RepoError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matches::domain::VenueSlug;

    fn venues() -> Vec<Venue> {
        vec![Venue {
            slug: VenueSlug::parse("ssa-amitie").unwrap(),
            name: "SSA Sports Center".to_owned(),
            address: "28 Duyên Hải".to_owned(),
        }]
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2099, 10, 1).unwrap()
    }

    fn full() -> RawDraft {
        RawDraft {
            venue_id: Some("ssa-amitie".to_owned()),
            date: Some("2099-10-01".to_owned()),
            start_time: Some("19:00".to_owned()),
            end_time: Some("20:30".to_owned()),
            format: Some("seven_a_side".to_owned()),
            match_type: Some("casual".to_owned()),
            level_min: Some(2.0),
            level_max: Some(3.5),
            slot_count: Some(3),
            price_per_player_vnd: Some(60_000),
            total_fee_vnd: None,
        }
    }

    #[test]
    fn keeps_valid_fields_and_turns_the_price_per_person_into_a_total() {
        let draft = MatchDraft::checked(full(), today(), &venues());
        assert_eq!(draft.venue_id.as_deref(), Some("ssa-amitie"));
        assert_eq!(draft.date, Some(today()));
        assert_eq!(draft.start_time, NaiveTime::from_hms_opt(19, 0, 0));
        assert_eq!(draft.end_time, NaiveTime::from_hms_opt(20, 30, 0));
        assert_eq!(draft.format, Some(Format::SevenASide));
        assert_eq!(draft.match_type, Some(MatchType::Casual));
        assert_eq!((draft.level_min, draft.level_max), (Some(2.0), Some(3.5)));
        assert_eq!(draft.slot_count, Some(3));
        assert_eq!(draft.total_fee_vnd, Some(180_000));
    }

    #[test]
    fn a_stated_total_wins_over_the_price_per_person() {
        let raw = RawDraft {
            total_fee_vnd: Some(600_000),
            ..full()
        };
        assert_eq!(
            MatchDraft::checked(raw, today(), &venues()).total_fee_vnd,
            Some(600_000)
        );
    }

    #[test]
    fn drops_fields_that_break_the_rules() {
        let raw = RawDraft {
            venue_id: Some("unknown-pitch".to_owned()),
            date: Some("2099-09-30".to_owned()),
            start_time: Some("7pm".to_owned()),
            end_time: Some("20:30".to_owned()),
            format: Some("futsal".to_owned()),
            match_type: Some("ranked".to_owned()),
            level_min: Some(4.0),
            level_max: Some(2.0),
            slot_count: Some(31),
            price_per_player_vnd: Some(60_000),
            total_fee_vnd: None,
        };
        assert_eq!(
            MatchDraft::checked(raw, today(), &venues()),
            MatchDraft::default()
        );
    }

    #[test]
    fn drops_an_end_time_that_is_not_after_the_start_or_too_long() {
        for end in ["18:00", "19:00", "23:30"] {
            let raw = RawDraft {
                end_time: Some(end.to_owned()),
                ..full()
            };
            assert_eq!(
                MatchDraft::checked(raw, today(), &venues()).end_time,
                None,
                "{end}"
            );
        }
    }

    #[test]
    fn drops_dates_beyond_the_booking_window_and_fees_above_the_cap() {
        let raw = RawDraft {
            date: Some("2099-11-01".to_owned()),
            total_fee_vnd: Some(MAX_TOTAL_FEE_VND + 1),
            ..full()
        };
        let draft = MatchDraft::checked(raw, today(), &venues());
        assert_eq!(draft.date, None);
        assert_eq!(draft.total_fee_vnd, None);
    }

    #[test]
    fn levels_must_be_half_steps_between_one_and_five() {
        let raw = RawDraft {
            level_min: Some(0.5),
            level_max: Some(3.3),
            ..full()
        };
        let draft = MatchDraft::checked(raw, today(), &venues());
        assert_eq!((draft.level_min, draft.level_max), (None, None));
    }
}
