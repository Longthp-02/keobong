//! sqlx/PostgreSQL adapter for the `SlotRepository` port.
//!
//! Claiming must check capacity and write in one atomic step, so this adapter
//! reads the `matches` row itself (id, slot count, kickoff) and locks it with
//! `FOR NO KEY UPDATE`: concurrent joins and leaves for the same match queue up
//! instead of overbooking, while inserts elsewhere that reference the match
//! (their foreign keys take `KEY SHARE`) are not blocked. The count after the
//! lock must see the previous claim's rows, so these transactions run at READ
//! COMMITTED explicitly, whatever the database default is. This is the only
//! place slots read the matches table (see docs/architecture.md).

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use super::domain::{
    ClaimContext, ClaimOutcome, JoinRequest, MyPlace, ReleaseOutcome, RepoError, Roster,
    RosterEntry, SlotRepository, Team, check_claim, check_leave,
};
use crate::auth::UserId;
use crate::matches::domain::ShareId;

#[derive(Clone)]
pub struct PgSlotRepository {
    pool: PgPool,
}

impl PgSlotRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn unavailable(err: sqlx::Error) -> RepoError {
    RepoError::Unavailable(err.to_string())
}

#[derive(sqlx::FromRow)]
struct LockedMatch {
    id: i64,
    slot_count: i16,
    starts_at: DateTime<Utc>,
}

async fn lock_match(
    tx: &mut Transaction<'_, Postgres>,
    share_id: &ShareId,
) -> Result<Option<LockedMatch>, RepoError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut **tx)
        .await
        .map_err(unavailable)?;
    sqlx::query_as(
        "SELECT id, slot_count, starts_at FROM matches WHERE share_id = $1 FOR NO KEY UPDATE",
    )
    .bind(share_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(unavailable)
}

async fn match_id(pool: &PgPool, share_id: &ShareId) -> Result<Option<(i64, i16)>, RepoError> {
    sqlx::query_as("SELECT id, slot_count FROM matches WHERE share_id = $1")
        .bind(share_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(unavailable)
}

fn parse_team(raw: &str) -> Result<Team, RepoError> {
    Team::parse(raw).ok_or_else(|| RepoError::Corrupt(format!("unknown team {raw:?}")))
}

#[derive(sqlx::FromRow)]
struct RosterRow {
    team: String,
    guest_name: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
}

impl SlotRepository for PgSlotRepository {
    async fn claim(
        &self,
        share_id: &ShareId,
        holder: UserId,
        request: &JoinRequest,
        now: DateTime<Utc>,
    ) -> Result<ClaimOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let Some(locked) = lock_match(&mut tx, share_id).await? else {
            return Ok(ClaimOutcome::MatchNotFound);
        };
        let (taken_in_team, already_joined): (i64, bool) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE team = $2),
                    coalesce(bool_or(holder_user_id = $3 AND guest_name IS NULL), false)
             FROM slots
             WHERE match_id = $1 AND released_at IS NULL",
        )
        .bind(locked.id)
        .bind(request.team.as_str())
        .bind(holder.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
        let ctx = ClaimContext {
            now,
            starts_at: locked.starts_at,
            slot_count: locked.slot_count,
            taken_in_team,
            already_joined,
        };
        if let Err(reason) = check_claim(&ctx, request) {
            return Ok(ClaimOutcome::Rejected(reason));
        }
        // The holder's own place (NULL guest name) followed by each guest.
        let guest_names: Vec<Option<&str>> = std::iter::once(None)
            .chain(request.guests.names().iter().map(|n| Some(n.as_str())))
            .collect();
        sqlx::query(
            "INSERT INTO slots (match_id, team, holder_user_id, guest_name, claimed_at)
             SELECT $1, $2, $3, guest_name, $4 FROM unnest($5::text[]) AS guest_name",
        )
        .bind(locked.id)
        .bind(request.team.as_str())
        .bind(holder.0)
        .bind(now)
        .bind(&guest_names)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(ClaimOutcome::Claimed)
    }

    async fn release(
        &self,
        share_id: &ShareId,
        holder: UserId,
        now: DateTime<Utc>,
    ) -> Result<ReleaseOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let Some(locked) = lock_match(&mut tx, share_id).await? else {
            return Ok(ReleaseOutcome::MatchNotFound);
        };
        if let Err(reason) = check_leave(now, locked.starts_at) {
            return Ok(ReleaseOutcome::Rejected(reason));
        }
        let released = sqlx::query(
            // GREATEST: app instances' clocks may differ slightly.
            "UPDATE slots SET released_at = GREATEST(claimed_at, $3)
             WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL",
        )
        .bind(locked.id)
        .bind(holder.0)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?
        .rows_affected();
        tx.commit().await.map_err(unavailable)?;
        Ok(if released == 0 {
            ReleaseOutcome::NotJoined
        } else {
            ReleaseOutcome::Released
        })
    }

    async fn roster(&self, share_id: &ShareId) -> Result<Option<Roster>, RepoError> {
        let Some((id, slot_count)) = match_id(&self.pool, share_id).await? else {
            return Ok(None);
        };
        let rows: Vec<RosterRow> = sqlx::query_as(
            "SELECT s.team, s.guest_name, u.display_name, u.avatar_url
             FROM slots s JOIN users u ON u.id = s.holder_user_id
             WHERE s.match_id = $1 AND s.released_at IS NULL
             ORDER BY s.claimed_at, s.id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        let entries = rows
            .into_iter()
            .map(|row| {
                let team = parse_team(&row.team)?;
                Ok(match row.guest_name {
                    Some(guest) => RosterEntry {
                        team,
                        name: Some(guest),
                        avatar_url: None,
                        is_guest: true,
                        guest_of: row.display_name,
                    },
                    None => RosterEntry {
                        team,
                        name: row.display_name,
                        avatar_url: row.avatar_url,
                        is_guest: false,
                        guest_of: None,
                    },
                })
            })
            .collect::<Result<_, RepoError>>()?;
        Ok(Some(Roster {
            slot_count,
            entries,
        }))
    }

    async fn my_place(
        &self,
        share_id: &ShareId,
        holder: UserId,
    ) -> Result<Option<Option<MyPlace>>, RepoError> {
        let Some((id, _)) = match_id(&self.pool, share_id).await? else {
            return Ok(None);
        };
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT team, guest_name FROM slots
             WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL
             ORDER BY id",
        )
        .bind(id)
        .bind(holder.0)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        let Some((team, _)) = rows.first() else {
            return Ok(Some(None));
        };
        Ok(Some(Some(MyPlace {
            team: parse_team(team)?,
            guests: rows.iter().filter_map(|(_, guest)| guest.clone()).collect(),
        })))
    }
}
