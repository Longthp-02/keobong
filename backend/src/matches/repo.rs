//! sqlx/PostgreSQL adapter for the `MatchRepository` port.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::auth::UserId;

use super::domain::{
    CancelOutcome, Format, InsertError, Level, Match, MatchRepository, MatchType, NewMatch,
    RepoError, ShareId, decide_cancel,
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
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            total_fee_vnd: row.total_fee_vnd,
            slot_count: row.slot_count,
            cancelled_at: row.cancelled_at,
        })
    }
}

impl MatchRepository for PgMatchRepository {
    async fn find_by_share_id(&self, id: &ShareId) -> Result<Option<Match>, RepoError> {
        let row = sqlx::query_as::<_, MatchRow>(
            "SELECT share_id, venue_name, starts_at, ends_at, format, match_type,
                    level_min_tenths, level_max_tenths, total_fee_vnd, slot_count, cancelled_at
             FROM matches
             WHERE share_id = $1",
        )
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
    ) -> Result<(), InsertError> {
        let result = sqlx::query(
            "INSERT INTO matches
                (share_id, venue_name, starts_at, ends_at, format, match_type,
                 level_min_tenths, level_max_tenths, total_fee_vnd, slot_count, host_user_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(share_id.as_str())
        .bind(&new.venue_name)
        .bind(new.starts_at)
        .bind(new.ends_at)
        .bind(new.format.as_str())
        .bind(new.match_type.as_str())
        .bind(new.level_min.tenths())
        .bind(new.level_max.tenths())
        .bind(new.total_fee_vnd)
        .bind(new.slot_count)
        .bind(host.0)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db)) if db.constraint() == Some("matches_share_id_key") => {
                Err(InsertError::DuplicateShareId)
            }
            Err(err) => Err(RepoError::Unavailable(err.to_string()).into()),
        }
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
