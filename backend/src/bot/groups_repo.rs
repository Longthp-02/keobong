//! Postgres adapter for join requests and group links.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgConnection;

use super::groups::{
    DecideOutcome, Decision, GroupRepoError, GroupRepository, JoinNotice, JoinOutcome,
    MAX_PENDING_PER_ORGANISER, MAX_PENDING_PER_PLAYER, Membership, PENDING_DAYS, WaitingRequest,
    ask_again_at,
};
use super::polls::MEMBER_DAYS;
use super::repo::PgBotRepository;
use crate::auth::UserId;
use crate::matches::domain::ShareId;

fn group_error(error: sqlx::Error) -> GroupRepoError {
    GroupRepoError(error.to_string())
}

/// A public group and where one user stands with it.
struct Standing {
    series_id: i64,
    organiser: UserId,
    title: String,
    membership: Membership,
}

/// Player, their name, the group's title, Zalo link and public id.
type DecidedRow = (i64, Option<String>, String, Option<String>, String);

type StandingRow = (
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
    Option<DateTime<Utc>>,
);

/// Members are the organiser, approved players, and linked people seen tagging
/// the bot in the group's chat in the last `MEMBER_DAYS`. Answering a poll on
/// the web does not count: its link may have been forwarded. A pending request
/// older than `PENDING_DAYS` has lapsed.
async fn standing(
    conn: &mut PgConnection,
    public_id: &str,
    user: UserId,
    now: DateTime<Utc>,
) -> Result<Option<Standing>, GroupRepoError> {
    let row: Option<StandingRow> = sqlx::query_as(
        "SELECT s.id, s.user_id, s.title, s.zalo_group_link,
                (SELECT p.share_id FROM bot_polls p
                 WHERE p.series_id = s.id AND p.closes_at > $3
                 ORDER BY p.closes_at LIMIT 1),
                EXISTS (SELECT 1 FROM bot_chat_members m
                        WHERE m.chat_id = s.chat_id AND m.user_id = $2 AND m.last_seen_at > $4),
                CASE WHEN r.status = 'pending' AND r.requested_at <= $5 THEN NULL ELSE r.status END,
                r.decided_at
         FROM bot_poll_series s
         LEFT JOIN bot_join_requests r ON r.series_id = s.id AND r.user_id = $2
         WHERE s.public_id = $1 AND s.public",
    )
    .bind(public_id)
    .bind(user.0)
    .bind(now)
    .bind(now - Duration::days(MEMBER_DAYS))
    .bind(now - Duration::days(PENDING_DAYS))
    .fetch_optional(&mut *conn)
    .await
    .map_err(group_error)?;
    let Some((series_id, organiser, title, group_link, poll, seen, status, decided_at)) = row
    else {
        return Ok(None);
    };
    let membership = if organiser == user.0 || seen || status.as_deref() == Some("approved") {
        Membership::Member {
            group_link,
            poll: poll.as_deref().and_then(ShareId::parse),
        }
    } else {
        match (status.as_deref(), decided_at) {
            (Some("pending"), _) => Membership::Pending,
            (Some("rejected"), Some(decided_at)) => Membership::Declined {
                ask_again_at: ask_again_at(decided_at),
            },
            _ => Membership::Outside,
        }
    };
    Ok(Some(Standing {
        series_id,
        organiser: UserId(organiser),
        title,
        membership,
    }))
}

impl GroupRepository for PgBotRepository {
    async fn membership(
        &self,
        public_id: &str,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<Option<Membership>, GroupRepoError> {
        let mut conn = self.pool().acquire().await.map_err(group_error)?;
        Ok(standing(&mut conn, public_id, user, now)
            .await?
            .map(|standing| standing.membership))
    }

    async fn request_join(
        &self,
        public_id: &str,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<JoinOutcome, GroupRepoError> {
        let mut tx = self.pool().begin().await.map_err(group_error)?;
        // Locks first, each in its own statement, so the standing read after
        // them sees what a request that held them before has committed. The
        // group row lock serialises requests to one group; the organiser and
        // player locks serialise the limit counts.
        let organiser: Option<i64> = sqlx::query_scalar(
            "SELECT user_id FROM bot_poll_series WHERE public_id = $1 AND public FOR UPDATE",
        )
        .bind(public_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(group_error)?;
        let Some(organiser) = organiser else {
            return Ok(JoinOutcome::UnknownGroup);
        };
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('bot_join:organiser:' || $1, 0))",
        )
        .bind(organiser.to_string())
        .execute(&mut *tx)
        .await
        .map_err(group_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('bot_join:player:' || $1, 0))")
            .bind(user.0.to_string())
            .execute(&mut *tx)
            .await
            .map_err(group_error)?;
        let Some(found) = standing(&mut tx, public_id, user, now).await? else {
            return Ok(JoinOutcome::UnknownGroup);
        };
        match found.membership {
            Membership::Member { .. } => return Ok(JoinOutcome::AlreadyMember),
            Membership::Pending => return Ok(JoinOutcome::AlreadyPending),
            Membership::Declined { ask_again_at } if now < ask_again_at => {
                return Ok(JoinOutcome::TooSoon { ask_again_at });
            }
            Membership::Declined { .. } | Membership::Outside => {}
        }
        let player_pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bot_join_requests
             WHERE user_id = $1 AND status = 'pending' AND requested_at > $2",
        )
        .bind(user.0)
        .bind(now - Duration::days(PENDING_DAYS))
        .fetch_one(&mut *tx)
        .await
        .map_err(group_error)?;
        if player_pending >= MAX_PENDING_PER_PLAYER {
            return Ok(JoinOutcome::PlayerLimit);
        }
        let organiser_pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bot_join_requests r JOIN bot_poll_series s ON s.id = r.series_id
             WHERE s.user_id = $1 AND r.status = 'pending' AND r.requested_at > $2",
        )
        .bind(found.organiser.0)
        .bind(now - Duration::days(PENDING_DAYS))
        .fetch_one(&mut *tx)
        .await
        .map_err(group_error)?;
        if organiser_pending >= MAX_PENDING_PER_ORGANISER {
            return Ok(JoinOutcome::OrganiserLimit);
        }
        // Only a declined or lapsed request is renewed; a live one stays as it is.
        let request_id: Option<i64> = sqlx::query_scalar(
            "INSERT INTO bot_join_requests (series_id, user_id, status, requested_at)
             VALUES ($1, $2, 'pending', $3)
             ON CONFLICT (series_id, user_id) DO UPDATE SET
                 status = 'pending', requested_at = EXCLUDED.requested_at, decided_at = NULL
             WHERE bot_join_requests.status = 'rejected' OR bot_join_requests.requested_at <= $4
             RETURNING id",
        )
        .bind(found.series_id)
        .bind(user.0)
        .bind(now)
        .bind(now - Duration::days(PENDING_DAYS))
        .fetch_optional(&mut *tx)
        .await
        .map_err(group_error)?;
        let Some(request_id) = request_id else {
            return Ok(JoinOutcome::AlreadyPending);
        };
        let player_name: Option<String> =
            sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
                .bind(user.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(group_error)?
                .flatten();
        tx.commit().await.map_err(group_error)?;
        Ok(JoinOutcome::Requested(JoinNotice {
            request_id,
            organiser: found.organiser,
            title: found.title,
            player_name,
        }))
    }

    async fn decide_join(
        &self,
        organiser: UserId,
        request_id: i64,
        approve: bool,
        now: DateTime<Utc>,
    ) -> Result<DecideOutcome, GroupRepoError> {
        let row: Option<DecidedRow> = sqlx::query_as(
            "WITH decided AS (
                 UPDATE bot_join_requests r SET status = $3, decided_at = $4
                 FROM bot_poll_series s
                 WHERE r.id = $1 AND r.series_id = s.id AND s.user_id = $2 AND r.status = 'pending'
                     AND r.requested_at > $5
                 RETURNING r.user_id, s.title, s.zalo_group_link, s.public_id
             )
             SELECT d.user_id, u.display_name, d.title, d.zalo_group_link, d.public_id
             FROM decided d JOIN users u ON u.id = d.user_id",
        )
        .bind(request_id)
        .bind(organiser.0)
        .bind(if approve { "approved" } else { "rejected" })
        .bind(now)
        .bind(now - Duration::days(PENDING_DAYS))
        .fetch_optional(self.pool())
        .await
        .map_err(group_error)?;
        Ok(match row {
            Some((player, player_name, title, group_link, public_id)) => {
                DecideOutcome::Decided(Decision {
                    player: UserId(player),
                    player_name,
                    title,
                    approved: approve,
                    group_link,
                    public_id,
                })
            }
            None => DecideOutcome::NotFound,
        })
    }

    async fn waiting_requests(
        &self,
        organiser: UserId,
        now: DateTime<Utc>,
    ) -> Result<Vec<WaitingRequest>, GroupRepoError> {
        let rows: Vec<(i64, Option<String>, String)> = sqlx::query_as(
            "SELECT r.id, u.display_name, s.title
             FROM bot_join_requests r
             JOIN bot_poll_series s ON s.id = r.series_id
             JOIN users u ON u.id = r.user_id
             WHERE s.user_id = $1 AND r.status = 'pending' AND r.requested_at > $2
             ORDER BY r.requested_at, r.id
             LIMIT 20",
        )
        .bind(organiser.0)
        .bind(now - Duration::days(PENDING_DAYS))
        .fetch_all(self.pool())
        .await
        .map_err(group_error)?;
        Ok(rows
            .into_iter()
            .map(|(id, player_name, title)| WaitingRequest {
                id,
                player_name,
                title,
            })
            .collect())
    }

    async fn set_group_link(
        &self,
        chat_id: &str,
        organiser: UserId,
        link: &str,
    ) -> Result<u64, GroupRepoError> {
        let result = sqlx::query(
            "UPDATE bot_poll_series SET zalo_group_link = $3 WHERE chat_id = $1 AND user_id = $2",
        )
        .bind(chat_id)
        .bind(organiser.0)
        .bind(link)
        .execute(self.pool())
        .await
        .map_err(group_error)?;
        Ok(result.rows_affected())
    }
}
