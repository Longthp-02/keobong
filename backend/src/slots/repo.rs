//! sqlx/PostgreSQL adapter for the `SlotRepository` port.
//!
//! Claiming must check capacity and write in one atomic step, so this adapter
//! reads the `matches` row itself (id, slot count, kickoff, fee, host) and
//! locks it with `FOR NO KEY UPDATE`: concurrent writes for the same match
//! queue up instead of overbooking, while inserts elsewhere that reference the
//! match (their foreign keys take `KEY SHARE`) are not blocked. The count after
//! the lock must see the previous claim's rows, so these transactions run at
//! READ COMMITTED explicitly, whatever the database default is. This is the
//! only place slots read the matches table (see docs/architecture.md).
//!
//! Unpaid holds expire without a background job: a place whose
//! `hold_expires_at` has passed counts as released everywhere, and every
//! locked write first records those expiries as `released_at`, which keeps the
//! one-own-place unique index correct.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use super::domain::{
    ClaimContext, ClaimOutcome, HostAction, HostActionOutcome, HostParty, HostView, JoinRequest,
    MyPlace, PaymentStatus, ReleaseOutcome, RepoError, ReportOutcome, Roster, RosterEntry,
    SlotRepository, Team, check_claim, check_leave, initial_payment, party_amount_vnd,
};
use crate::auth::UserId;
use crate::matches::domain::{ShareId, price_per_player_vnd};

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
struct MatchFacts {
    id: i64,
    slot_count: i16,
    starts_at: DateTime<Utc>,
    total_fee_vnd: i64,
    host_user_id: i64,
    cancelled_at: Option<DateTime<Utc>>,
}

impl MatchFacts {
    fn price_per_player_vnd(&self) -> i64 {
        price_per_player_vnd(self.total_fee_vnd, self.slot_count)
    }

    /// The time holds are measured against. Cancelling freezes every place as
    /// it was, so nobody drops off the host's list while refunds are sorted out.
    fn hold_clock(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.cancelled_at
            .map_or(now, |cancelled| cancelled.min(now))
    }
}

const MATCH_FACTS: &str =
    "SELECT id, slot_count, starts_at, total_fee_vnd, host_user_id, cancelled_at
     FROM matches WHERE share_id = $1";

/// Starts a READ COMMITTED transaction, locks the match and records expired holds.
async fn lock_match(
    tx: &mut Transaction<'_, Postgres>,
    share_id: &ShareId,
    now: DateTime<Utc>,
) -> Result<Option<MatchFacts>, RepoError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut **tx)
        .await
        .map_err(unavailable)?;
    let facts: Option<MatchFacts> = sqlx::query_as(&format!("{MATCH_FACTS} FOR NO KEY UPDATE"))
        .bind(share_id.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unavailable)?;
    if let Some(facts) = &facts {
        sqlx::query(
            "UPDATE slots SET released_at = hold_expires_at
             WHERE match_id = $1 AND released_at IS NULL AND hold_expires_at <= $2",
        )
        .bind(facts.id)
        .bind(facts.hold_clock(now))
        .execute(&mut **tx)
        .await
        .map_err(unavailable)?;
    }
    Ok(facts)
}

async fn match_facts(pool: &PgPool, share_id: &ShareId) -> Result<Option<MatchFacts>, RepoError> {
    sqlx::query_as(MATCH_FACTS)
        .bind(share_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(unavailable)
}

fn parse_team(raw: &str) -> Result<Team, RepoError> {
    Team::parse(raw).ok_or_else(|| RepoError::Corrupt(format!("unknown team {raw:?}")))
}

fn parse_status(raw: &str) -> Result<PaymentStatus, RepoError> {
    PaymentStatus::parse(raw)
        .ok_or_else(|| RepoError::Corrupt(format!("unknown payment status {raw:?}")))
}

#[derive(sqlx::FromRow)]
struct RosterRow {
    team: String,
    guest_name: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(sqlx::FromRow)]
struct PartyRow {
    id: i64,
    team: String,
    display_name: Option<String>,
    payment_status: String,
    hold_expires_at: Option<DateTime<Utc>>,
    guests: Vec<String>,
}

/// Active own places with their guests, for one holder (`$3`) or all holders (`$3` NULL).
const PARTIES: &str = "
    SELECT o.id, o.team, u.display_name, o.payment_status, o.hold_expires_at,
           coalesce(array_agg(g.guest_name ORDER BY g.id) FILTER (WHERE g.id IS NOT NULL),
                    '{}') AS guests
    FROM slots o
    JOIN users u ON u.id = o.holder_user_id
    LEFT JOIN slots g
           ON g.match_id = o.match_id AND g.holder_user_id = o.holder_user_id
          AND g.guest_name IS NOT NULL AND g.released_at IS NULL
    WHERE o.match_id = $1 AND o.guest_name IS NULL AND o.released_at IS NULL
      AND (o.hold_expires_at IS NULL OR o.hold_expires_at > $2)
      AND ($3::bigint IS NULL OR o.holder_user_id = $3)
    GROUP BY o.id, u.display_name
    ORDER BY o.claimed_at, o.id";

impl SlotRepository for PgSlotRepository {
    async fn claim(
        &self,
        share_id: &ShareId,
        holder: UserId,
        request: &JoinRequest,
        now: DateTime<Utc>,
    ) -> Result<ClaimOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let Some(facts) = lock_match(&mut tx, share_id, now).await? else {
            return Ok(ClaimOutcome::MatchNotFound);
        };
        let (taken_in_team, already_joined): (i64, bool) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE team = $2),
                    coalesce(bool_or(holder_user_id = $3 AND guest_name IS NULL), false)
             FROM slots
             WHERE match_id = $1 AND released_at IS NULL",
        )
        .bind(facts.id)
        .bind(request.team.as_str())
        .bind(holder.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
        let ctx = ClaimContext {
            now,
            starts_at: facts.starts_at,
            slot_count: facts.slot_count,
            taken_in_team,
            already_joined,
            cancelled: facts.cancelled_at.is_some(),
        };
        if let Err(reason) = check_claim(&ctx, request) {
            return Ok(ClaimOutcome::Rejected(reason));
        }
        let (status, hold_expires_at) = initial_payment(
            facts.price_per_player_vnd(),
            facts.host_user_id == holder.0,
            now,
        );
        // The holder's own place (NULL guest name) followed by each guest.
        let guest_names: Vec<Option<&str>> = std::iter::once(None)
            .chain(request.guests.names().iter().map(|n| Some(n.as_str())))
            .collect();
        sqlx::query(
            "INSERT INTO slots (match_id, team, holder_user_id, guest_name, claimed_at,
                                payment_status, hold_expires_at)
             SELECT $1, $2, $3, guest_name, $4, $6, $7 FROM unnest($5::text[]) AS guest_name",
        )
        .bind(facts.id)
        .bind(request.team.as_str())
        .bind(holder.0)
        .bind(now)
        .bind(&guest_names)
        .bind(status.as_str())
        .bind(hold_expires_at)
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
        let Some(facts) = lock_match(&mut tx, share_id, now).await? else {
            return Ok(ReleaseOutcome::MatchNotFound);
        };
        if let Err(reason) = check_leave(now, facts.starts_at, facts.cancelled_at.is_some()) {
            return Ok(ReleaseOutcome::Rejected(reason));
        }
        let released = sqlx::query(
            // GREATEST: app instances' clocks may differ slightly.
            "UPDATE slots SET released_at = GREATEST(claimed_at, $3)
             WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL",
        )
        .bind(facts.id)
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

    async fn taken_places(
        &self,
        match_ids: &[ShareId],
        now: DateTime<Utc>,
    ) -> Result<std::collections::HashMap<String, i64>, RepoError> {
        let ids: Vec<&str> = match_ids.iter().map(ShareId::as_str).collect();
        // Same rule as the roster: holds are measured against the cancel time if any.
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT m.share_id, count(s.id)
             FROM matches m
             LEFT JOIN slots s ON s.match_id = m.id AND s.released_at IS NULL
                  AND (s.hold_expires_at IS NULL
                       OR s.hold_expires_at > LEAST($2, COALESCE(m.cancelled_at, $2)))
             WHERE m.share_id = ANY($1)
             GROUP BY m.share_id",
        )
        .bind(&ids)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(rows.into_iter().collect())
    }

    async fn roster(
        &self,
        share_id: &ShareId,
        now: DateTime<Utc>,
    ) -> Result<Option<Roster>, RepoError> {
        let Some(facts) = match_facts(&self.pool, share_id).await? else {
            return Ok(None);
        };
        let rows: Vec<RosterRow> = sqlx::query_as(
            "SELECT s.team, s.guest_name, u.display_name, u.avatar_url
             FROM slots s JOIN users u ON u.id = s.holder_user_id
             WHERE s.match_id = $1 AND s.released_at IS NULL
               AND (s.hold_expires_at IS NULL OR s.hold_expires_at > $2)
             ORDER BY s.claimed_at, s.id",
        )
        .bind(facts.id)
        .bind(facts.hold_clock(now))
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
            slot_count: facts.slot_count,
            cancelled: facts.cancelled_at.is_some(),
            entries,
        }))
    }

    async fn my_place(
        &self,
        share_id: &ShareId,
        holder: UserId,
        now: DateTime<Utc>,
    ) -> Result<Option<Option<MyPlace>>, RepoError> {
        let Some(facts) = match_facts(&self.pool, share_id).await? else {
            return Ok(None);
        };
        let party: Option<PartyRow> = sqlx::query_as(PARTIES)
            .bind(facts.id)
            .bind(facts.hold_clock(now))
            .bind(Some(holder.0))
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        let Some(party) = party else {
            return Ok(Some(None));
        };
        let party_size = 1 + party.guests.len() as i64;
        Ok(Some(Some(MyPlace {
            team: parse_team(&party.team)?,
            payment_code: party.id,
            payment_status: parse_status(&party.payment_status)?,
            hold_expires_at: party.hold_expires_at,
            amount_vnd: party_amount_vnd(facts.price_per_player_vnd(), party_size),
            host: UserId(facts.host_user_id),
            match_cancelled: facts.cancelled_at.is_some(),
            guests: party.guests,
        })))
    }

    async fn report_payment(
        &self,
        share_id: &ShareId,
        holder: UserId,
        now: DateTime<Utc>,
    ) -> Result<ReportOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let Some(facts) = lock_match(&mut tx, share_id, now).await? else {
            return Ok(ReportOutcome::MatchNotFound);
        };
        if facts.cancelled_at.is_some() {
            return Ok(ReportOutcome::MatchCancelled);
        }
        let reported = sqlx::query(
            "UPDATE slots SET payment_status = 'payment_reported', hold_expires_at = NULL
             WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL
               AND payment_status = 'awaiting_payment'",
        )
        .bind(facts.id)
        .bind(holder.0)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?
        .rows_affected();
        let outcome = if reported > 0 {
            ReportOutcome::Reported
        } else {
            let joined: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM slots
                                WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL)",
            )
            .bind(facts.id)
            .bind(holder.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(unavailable)?;
            if joined {
                ReportOutcome::AlreadyDone
            } else {
                ReportOutcome::NotJoined
            }
        };
        tx.commit().await.map_err(unavailable)?;
        Ok(outcome)
    }

    async fn host_parties(
        &self,
        share_id: &ShareId,
        caller: UserId,
        now: DateTime<Utc>,
    ) -> Result<HostView, RepoError> {
        let Some(facts) = match_facts(&self.pool, share_id).await? else {
            return Ok(HostView::MatchNotFound);
        };
        if facts.host_user_id != caller.0 {
            return Ok(HostView::NotHost);
        }
        let rows: Vec<PartyRow> = sqlx::query_as(PARTIES)
            .bind(facts.id)
            .bind(facts.hold_clock(now))
            .bind(None::<i64>)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        let price = facts.price_per_player_vnd();
        let parties = rows
            .into_iter()
            .map(|row| {
                Ok(HostParty {
                    payment_code: row.id,
                    team: parse_team(&row.team)?,
                    holder_name: row.display_name,
                    amount_vnd: party_amount_vnd(price, 1 + row.guests.len() as i64),
                    payment_status: parse_status(&row.payment_status)?,
                    hold_expires_at: row.hold_expires_at,
                    guests: row.guests,
                })
            })
            .collect::<Result<_, RepoError>>()?;
        Ok(HostView::Parties(parties))
    }

    async fn host_action(
        &self,
        share_id: &ShareId,
        caller: UserId,
        payment_code: i64,
        action: HostAction,
        now: DateTime<Utc>,
    ) -> Result<HostActionOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let Some(facts) = lock_match(&mut tx, share_id, now).await? else {
            return Ok(HostActionOutcome::MatchNotFound);
        };
        if facts.host_user_id != caller.0 {
            return Ok(HostActionOutcome::NotHost);
        }
        if facts.cancelled_at.is_some() {
            return Ok(HostActionOutcome::MatchCancelled);
        }
        let party: Option<(i64, String)> = sqlx::query_as(
            "SELECT holder_user_id, payment_status FROM slots
             WHERE id = $1 AND match_id = $2 AND guest_name IS NULL AND released_at IS NULL",
        )
        .bind(payment_code)
        .bind(facts.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?;
        let Some((holder, status)) = party else {
            return Ok(HostActionOutcome::PartyNotFound);
        };
        let update = match action {
            HostAction::Confirm => {
                // $3 (now) is bound for both statements; confirming does not need it.
                "UPDATE slots SET payment_status = 'confirmed', hold_expires_at = NULL
                 WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL
                   AND $3::timestamptz IS NOT NULL"
            }
            HostAction::Reject => {
                match parse_status(&status)? {
                    PaymentStatus::Confirmed => return Ok(HostActionOutcome::AlreadyConfirmed),
                    PaymentStatus::AwaitingPayment => return Ok(HostActionOutcome::NotReported),
                    PaymentStatus::PaymentReported => {}
                }
                if now >= facts.starts_at {
                    return Ok(HostActionOutcome::MatchStarted);
                }
                "UPDATE slots SET released_at = GREATEST(claimed_at, $3)
                 WHERE match_id = $1 AND holder_user_id = $2 AND released_at IS NULL"
            }
        };
        sqlx::query(update)
            .bind(facts.id)
            .bind(holder)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(HostActionOutcome::Done)
    }
}
