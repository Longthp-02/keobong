//! sqlx/PostgreSQL adapter for the `MatchRepository` port.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::auth::UserId;

use super::domain::{
    CancelOutcome, Format, InsertError, Level, ListedMatch, Match, MatchRepository, MatchType,
    NewMatch, RepoError, ShareId, UpcomingQuery, Venue, VenueSlug, decide_cancel,
};

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
    venue_address: Option<String>,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    format: String,
    match_type: String,
    level_min_tenths: i16,
    level_max_tenths: i16,
    total_fee_vnd: i64,
    slot_count: i16,
    cancelled_at: Option<DateTime<Utc>>,
}

impl TryFrom<MatchRow> for Match {
    type Error = RepoError;

    fn try_from(row: MatchRow) -> Result<Self, Self::Error> {
        let corrupt = |what: &str| RepoError::Corrupt(format!("{what} for {}", row.share_id));
        Ok(Match {
            share_id: ShareId::parse(&row.share_id).ok_or_else(|| corrupt("share_id"))?,
            format: Format::parse(&row.format).ok_or_else(|| corrupt("format"))?,
            match_type: MatchType::parse(&row.match_type).ok_or_else(|| corrupt("match_type"))?,
            level_min: Level::from_tenths(row.level_min_tenths)
                .ok_or_else(|| corrupt("level_min"))?,
            level_max: Level::from_tenths(row.level_max_tenths)
                .ok_or_else(|| corrupt("level_max"))?,
            venue_name: row.venue_name,
            venue_address: row.venue_address,
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            total_fee_vnd: row.total_fee_vnd,
            slot_count: row.slot_count,
            cancelled_at: row.cancelled_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct VenueRow {
    slug: String,
    name: String,
    address: String,
}

impl TryFrom<VenueRow> for Venue {
    type Error = RepoError;

    fn try_from(row: VenueRow) -> Result<Self, Self::Error> {
        Ok(Venue {
            slug: VenueSlug::parse(&row.slug)
                .ok_or_else(|| RepoError::Corrupt(format!("venue slug {}", row.slug)))?,
            name: row.name,
            address: row.address,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ListedRow {
    #[sqlx(flatten)]
    found: MatchRow,
    distance_m: Option<f64>,
}

/// Columns of [`MatchRow`], for `matches m LEFT JOIN venues v`.
const MATCH_COLUMNS: &str = "m.share_id, m.venue_name, v.address AS venue_address, m.starts_at,
    m.ends_at, m.format, m.match_type, m.level_min_tenths, m.level_max_tenths, m.total_fee_vnd,
    m.slot_count, m.cancelled_at";

impl MatchRepository for PgMatchRepository {
    async fn find_by_share_id(&self, id: &ShareId) -> Result<Option<Match>, RepoError> {
        let row = sqlx::query_as::<_, MatchRow>(&format!(
            "SELECT {MATCH_COLUMNS}
             FROM matches m LEFT JOIN venues v ON v.id = m.venue_id
             WHERE m.share_id = $1"
        ))
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Unavailable(e.to_string()))?;

        row.map(Match::try_from).transpose()
    }

    async fn insert(
        &self,
        share_id: &ShareId,
        new: &NewMatch,
        host: UserId,
    ) -> Result<Venue, InsertError> {
        // The venue must be offered; its name and location are copied in the same statement.
        let result = sqlx::query_as::<_, VenueRow>(
            "WITH venue AS (
                 SELECT id, slug, name, address, location FROM venues
                 WHERE slug = $2 AND active
             ), inserted AS (
                 INSERT INTO matches
                     (share_id, venue_id, venue_name, location, starts_at, ends_at, format,
                      match_type, level_min_tenths, level_max_tenths, total_fee_vnd, slot_count,
                      host_user_id)
                 SELECT $1, venue.id, venue.name, venue.location, $3, $4, $5, $6, $7, $8, $9,
                        $10, $11
                 FROM venue
                 RETURNING 1
             )
             SELECT venue.slug, venue.name, venue.address FROM venue, inserted",
        )
        .bind(share_id.as_str())
        .bind(new.venue.as_str())
        .bind(new.starts_at)
        .bind(new.ends_at)
        .bind(new.format.as_str())
        .bind(new.match_type.as_str())
        .bind(new.level_min.tenths())
        .bind(new.level_max.tenths())
        .bind(new.total_fee_vnd)
        .bind(new.slot_count)
        .bind(host.0)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some(venue)) => Ok(Venue::try_from(venue)?),
            Ok(None) => Err(InsertError::UnknownVenue),
            Err(sqlx::Error::Database(db)) if db.constraint() == Some("matches_share_id_key") => {
                Err(InsertError::DuplicateShareId)
            }
            Err(err) => Err(RepoError::Unavailable(err.to_string()).into()),
        }
    }

    async fn active_venues(&self) -> Result<Vec<Venue>, RepoError> {
        let rows = sqlx::query_as::<_, VenueRow>(
            "SELECT slug, name, address FROM venues WHERE active ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Unavailable(e.to_string()))?;
        rows.into_iter().map(Venue::try_from).collect()
    }

    async fn upcoming(&self, query: &UpcomingQuery) -> Result<Vec<ListedMatch>, RepoError> {
        let (lat, lng) = query.near.map(|p| (p.lat, p.lng)).unzip();
        let (after_start, after_id) = query
            .after
            .as_ref()
            .map(|c| (c.starts_at, c.share_id.as_str().to_owned()))
            .unzip();
        // Served by matches_open_by_start_idx: not cancelled, ordered by (starts_at, share_id).
        let rows = sqlx::query_as::<_, ListedRow>(&format!(
            "SELECT {MATCH_COLUMNS},
                    CASE WHEN $5::float8 IS NULL OR m.location IS NULL THEN NULL
                         ELSE ST_Distance(m.location,
                                  ST_SetSRID(ST_MakePoint($6::float8, $5::float8), 4326)::geography)
                    END AS distance_m
             FROM matches m LEFT JOIN venues v ON v.id = m.venue_id
             WHERE m.cancelled_at IS NULL
               AND m.starts_at >= $1 AND m.starts_at > $2 AND m.starts_at < $3
               AND ($4::text IS NULL OR m.match_type = $4)
               AND ($7::timestamptz IS NULL OR (m.starts_at, m.share_id) > ($7, $8::text))
             ORDER BY m.starts_at, m.share_id
             LIMIT $9"
        ))
        .bind(query.from)
        .bind(query.now)
        .bind(query.until)
        .bind(query.match_type.map(MatchType::as_str))
        .bind(lat)
        .bind(lng)
        .bind(after_start)
        .bind(after_id)
        .bind(query.limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Unavailable(e.to_string()))?;
        rows.into_iter()
            .map(|row| {
                Ok(ListedMatch {
                    found: Match::try_from(row.found)?,
                    distance_m: row.distance_m,
                })
            })
            .collect()
    }

    async fn cancel(
        &self,
        share_id: &ShareId,
        caller: UserId,
        now: DateTime<Utc>,
    ) -> Result<CancelOutcome, RepoError> {
        let unavailable = |e: sqlx::Error| RepoError::Unavailable(e.to_string());
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        // Same lock as slot claims, so no one joins while the match is being cancelled.
        let row: Option<(i64, DateTime<Utc>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT host_user_id, starts_at, cancelled_at FROM matches
             WHERE share_id = $1 FOR NO KEY UPDATE",
        )
        .bind(share_id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?;
        let Some((host, starts_at, cancelled_at)) = row else {
            return Ok(CancelOutcome::MatchNotFound);
        };
        let outcome = decide_cancel(caller, UserId(host), starts_at, cancelled_at, now);
        if outcome == CancelOutcome::Cancelled {
            sqlx::query("UPDATE matches SET cancelled_at = $2 WHERE share_id = $1")
                .bind(share_id.as_str())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
            tx.commit().await.map_err(unavailable)?;
        }
        Ok(outcome)
    }
}
