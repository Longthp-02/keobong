//! sqlx/PostgreSQL adapter for the `MatchRepository` port.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::domain::{Format, Level, Match, MatchRepository, MatchType, RepoError, ShareId};

#[derive(Clone)]
pub struct PgMatchRepository {
    pool: PgPool,
}

impl PgMatchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct MatchRow {
    share_id: String,
    venue_name: String,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    format: String,
    match_type: String,
    level_min_tenths: i16,
    level_max_tenths: i16,
    total_fee_vnd: i64,
    slot_count: i16,
}

impl TryFrom<MatchRow> for Match {
    type Error = RepoError;

    fn try_from(row: MatchRow) -> Result<Self, Self::Error> {
        let corrupt = |what: &str| RepoError::Corrupt(format!("{what} for {}", row.share_id));
        Ok(Match {
            share_id: ShareId::parse(&row.share_id).ok_or_else(|| corrupt("share_id"))?,
            format: parse_format(&row.format).ok_or_else(|| corrupt("format"))?,
            match_type: parse_match_type(&row.match_type).ok_or_else(|| corrupt("match_type"))?,
            level_min: Level::from_tenths(row.level_min_tenths)
                .ok_or_else(|| corrupt("level_min"))?,
            level_max: Level::from_tenths(row.level_max_tenths)
                .ok_or_else(|| corrupt("level_max"))?,
            venue_name: row.venue_name,
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            total_fee_vnd: row.total_fee_vnd,
            slot_count: row.slot_count,
        })
    }
}

fn parse_format(value: &str) -> Option<Format> {
    match value {
        "five_a_side" => Some(Format::FiveASide),
        "seven_a_side" => Some(Format::SevenASide),
        "eleven_a_side" => Some(Format::ElevenASide),
        _ => None,
    }
}

fn parse_match_type(value: &str) -> Option<MatchType> {
    match value {
        "casual" => Some(MatchType::Casual),
        "competitive" => Some(MatchType::Competitive),
        "beginner_friendly" => Some(MatchType::BeginnerFriendly),
        _ => None,
    }
}

impl MatchRepository for PgMatchRepository {
    async fn find_by_share_id(&self, id: &ShareId) -> Result<Option<Match>, RepoError> {
        let row = sqlx::query_as::<_, MatchRow>(
            "SELECT share_id, venue_name, starts_at, ends_at, format, match_type,
                    level_min_tenths, level_max_tenths, total_fee_vnd, slot_count
             FROM matches
             WHERE share_id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Unavailable(e.to_string()))?;

        row.map(Match::try_from).transpose()
    }
}
