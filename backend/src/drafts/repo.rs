//! Postgres adapter for the daily read limits.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use super::domain::{
    DraftRepository, LimitScope, RepoError, SERVICE_READS_PER_DAY, USER_READS_PER_DAY,
};
use crate::auth::UserId;

#[derive(Clone)]
pub struct PgDraftRepository {
    pool: PgPool,
}

impl PgDraftRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl DraftRepository for PgDraftRepository {
    async fn record_read(
        &self,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<Result<(), LimitScope>, RepoError> {
        let since = now - Duration::hours(24);
        // One statement: count both windows and insert only when both are open.
        // Two concurrent requests can both pass the check, so a limit may be
        // exceeded by a read or two; it bounds cost, it is not exact.
        let row: (i64, i64, bool) = sqlx::query_as(
            "WITH expired AS (
                 -- Reads only matter for 24 hours; older rows are not kept.
                 DELETE FROM draft_reads WHERE created_at <= $2
             ),
             counts AS (
                 SELECT count(*) FILTER (WHERE user_id = $1) AS user_reads,
                        count(*) AS service_reads
                 FROM draft_reads
                 WHERE created_at > $2
             ),
             inserted AS (
                 INSERT INTO draft_reads (user_id, created_at)
                 SELECT $1, $3 FROM counts
                 WHERE user_reads < $4 AND service_reads < $5
                 RETURNING 1
             )
             SELECT user_reads, service_reads, EXISTS (SELECT 1 FROM inserted)
             FROM counts",
        )
        .bind(user.0)
        .bind(since)
        .bind(now)
        .bind(USER_READS_PER_DAY)
        .bind(SERVICE_READS_PER_DAY)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| RepoError(error.to_string()))?;
        Ok(match row {
            (_, _, true) => Ok(()),
            (user_reads, _, false) if user_reads >= USER_READS_PER_DAY => Err(LimitScope::User),
            _ => Err(LimitScope::Service),
        })
    }
}
