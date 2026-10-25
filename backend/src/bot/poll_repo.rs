//! Postgres adapter for attendance polls.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

use super::polls::{
    AnswerOutcome, MAX_OPEN_POLLS_PER_CHAT, Member, PendingPoll, Poll, PollAnswer, PollPlan,
    PollRepoError, PollRepository,
};
use super::repo::PgBotRepository;
use crate::auth::UserId;
use crate::matches::domain::ShareId;

fn poll_error(error: sqlx::Error) -> PollRepoError {
    PollRepoError(error.to_string())
}

/// Polls processed per scheduler run; later ones wait for the next run.
const NUDGES_PER_RUN: i64 = 20;

type PollRow = (
    i64,
    String,
    String,
    String,
    Vec<NaiveDate>,
    NaiveTime,
    DateTime<Utc>,
);

fn poll_from_row(row: PollRow) -> Result<Poll, PollRepoError> {
    let (id, share_id, chat_id, title, days, nudge_time, closes_at) = row;
    let share_id = ShareId::parse(&share_id)
        .ok_or_else(|| PollRepoError(format!("stored poll {id} has an invalid share id")))?;
    Ok(Poll {
        id,
        share_id,
        chat_id,
        plan: PollPlan {
            title,
            days,
            nudge_time,
        },
        closes_at,
    })
}

type PendingPollRow = (i64, String, Vec<NaiveDate>, NaiveTime, DateTime<Utc>);

type DueRow = (
    i64,
    String,
    String,
    String,
    Vec<NaiveDate>,
    NaiveTime,
    DateTime<Utc>,
    DateTime<Utc>,
);

const POLL_COLUMNS: &str = "id, share_id, chat_id, title, days, nudge_time, closes_at";

impl PollRepository for PgBotRepository {
    async fn save_pending_poll(&self, pending: &PendingPoll) -> Result<(), PollRepoError> {
        let mut tx = self.pool().begin().await.map_err(poll_error)?;
        // One pending item per sender and chat: a poll replaces a match draft or
        // a reminder.
        for table in ["bot_drafts", "bot_pending_reminders"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE zalo_user_id = $1 AND chat_id = $2"
            ))
            .bind(&pending.zalo_user_id)
            .bind(&pending.chat_id)
            .execute(&mut *tx)
            .await
            .map_err(poll_error)?;
        }
        sqlx::query(
            "INSERT INTO bot_pending_polls (zalo_user_id, chat_id, user_id, title, days,
                 nudge_time, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (zalo_user_id, chat_id) DO UPDATE SET
                 user_id = EXCLUDED.user_id, title = EXCLUDED.title, days = EXCLUDED.days,
                 nudge_time = EXCLUDED.nudge_time, expires_at = EXCLUDED.expires_at",
        )
        .bind(&pending.zalo_user_id)
        .bind(&pending.chat_id)
        .bind(pending.user.0)
        .bind(&pending.plan.title)
        .bind(&pending.plan.days)
        .bind(pending.plan.nudge_time)
        .bind(pending.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(poll_error)?;
        tx.commit().await.map_err(poll_error)
    }

    async fn take_pending_poll(
        &self,
        zalo_user_id: &str,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<PendingPoll>, PollRepoError> {
        let row: Option<PendingPollRow> = sqlx::query_as(
            "DELETE FROM bot_pending_polls WHERE zalo_user_id = $1 AND chat_id = $2
                 RETURNING user_id, title, days, nudge_time, expires_at",
        )
        .bind(zalo_user_id)
        .bind(chat_id)
        .fetch_optional(self.pool())
        .await
        .map_err(poll_error)?;
        Ok(row
            .filter(|row| row.4 > now)
            .map(|(user, title, days, nudge_time, expires_at)| PendingPoll {
                zalo_user_id: zalo_user_id.to_owned(),
                chat_id: chat_id.to_owned(),
                user: UserId(user),
                plan: PollPlan {
                    title,
                    days,
                    nudge_time,
                },
                expires_at,
            }))
    }

    async fn add_poll(
        &self,
        chat_id: &str,
        user: UserId,
        plan: &PollPlan,
        share_id: &ShareId,
        now: DateTime<Utc>,
    ) -> Result<Option<Poll>, PollRepoError> {
        let mut tx = self.pool().begin().await.map_err(poll_error)?;
        // Serialises adds per chat so two "ok"s cannot both pass the count.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('bot_polls:' || $1, 0))")
            .bind(chat_id)
            .execute(&mut *tx)
            .await
            .map_err(poll_error)?;
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bot_polls WHERE chat_id = $1 AND closes_at > $2",
        )
        .bind(chat_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(poll_error)?;
        if open >= MAX_OPEN_POLLS_PER_CHAT {
            return Ok(None);
        }
        let row: PollRow = sqlx::query_as(&format!(
            "INSERT INTO bot_polls (share_id, chat_id, user_id, title, days, nudge_time,
                 next_nudge_at, closes_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING {POLL_COLUMNS}"
        ))
        .bind(share_id.as_str())
        .bind(chat_id)
        .bind(user.0)
        .bind(&plan.title)
        .bind(&plan.days)
        .bind(plan.nudge_time)
        .bind(plan.next_nudge(now))
        .bind(plan.closes_at())
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(poll_error)?;
        tx.commit().await.map_err(poll_error)?;
        poll_from_row(row).map(Some)
    }

    async fn poll(&self, share_id: &ShareId) -> Result<Option<Poll>, PollRepoError> {
        let row: Option<PollRow> = sqlx::query_as(&format!(
            "SELECT {POLL_COLUMNS} FROM bot_polls WHERE share_id = $1"
        ))
        .bind(share_id.as_str())
        .fetch_optional(self.pool())
        .await
        .map_err(poll_error)?;
        row.map(poll_from_row).transpose()
    }

    async fn answers(&self, poll_id: i64) -> Result<Vec<PollAnswer>, PollRepoError> {
        let rows: Vec<(i64, Option<String>, NaiveDate, bool)> = sqlx::query_as(
            "SELECT a.user_id, u.display_name, a.day, a.attending
             FROM bot_poll_answers a JOIN users u ON u.id = a.user_id
             WHERE a.poll_id = $1
             ORDER BY (SELECT min(b.answered_at) FROM bot_poll_answers b
                       WHERE b.poll_id = a.poll_id AND b.user_id = a.user_id), a.user_id, a.day",
        )
        .bind(poll_id)
        .fetch_all(self.pool())
        .await
        .map_err(poll_error)?;
        Ok(rows
            .into_iter()
            .map(|(user, name, day, attending)| PollAnswer {
                user: UserId(user),
                name,
                day,
                attending,
            })
            .collect())
    }

    async fn answer(
        &self,
        share_id: &ShareId,
        user: UserId,
        answers: &[(NaiveDate, bool)],
        now: DateTime<Utc>,
    ) -> Result<AnswerOutcome, PollRepoError> {
        let mut tx = self.pool().begin().await.map_err(poll_error)?;
        let poll: Option<(i64, Vec<NaiveDate>, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, days, closes_at FROM bot_polls WHERE share_id = $1 FOR SHARE",
        )
        .bind(share_id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(poll_error)?;
        let Some((poll_id, days, closes_at)) = poll else {
            return Ok(AnswerOutcome::UnknownPoll);
        };
        if closes_at <= now {
            return Ok(AnswerOutcome::Closed);
        }
        if answers.iter().any(|(day, _)| !days.contains(day)) {
            return Ok(AnswerOutcome::UnknownDay);
        }
        for (day, attending) in answers {
            sqlx::query(
                "INSERT INTO bot_poll_answers (poll_id, user_id, day, attending, answered_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (poll_id, user_id, day) DO UPDATE SET
                     attending = EXCLUDED.attending, answered_at = EXCLUDED.answered_at",
            )
            .bind(poll_id)
            .bind(user.0)
            .bind(day)
            .bind(attending)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(poll_error)?;
        }
        tx.commit().await.map_err(poll_error)?;
        Ok(AnswerOutcome::Saved)
    }

    async fn note_member(
        &self,
        chat_id: &str,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<(), PollRepoError> {
        sqlx::query(
            "INSERT INTO bot_chat_members (chat_id, user_id, last_seen_at) VALUES ($1, $2, $3)
             ON CONFLICT (chat_id, user_id) DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at",
        )
        .bind(chat_id)
        .bind(user.0)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(poll_error)?;
        Ok(())
    }

    async fn members(&self, chat_id: &str) -> Result<Vec<Member>, PollRepoError> {
        let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT u.id, u.display_name FROM users u
             WHERE u.id IN (
                 SELECT user_id FROM bot_chat_members WHERE chat_id = $1
                 UNION
                 SELECT a.user_id FROM bot_poll_answers a
                 JOIN bot_polls p ON p.id = a.poll_id WHERE p.chat_id = $1
             )
             ORDER BY u.id",
        )
        .bind(chat_id)
        .fetch_all(self.pool())
        .await
        .map_err(poll_error)?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| Member {
                user: UserId(id),
                name,
            })
            .collect())
    }

    async fn claim_due_nudges(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<(Poll, DateTime<Utc>)>, PollRepoError> {
        let mut tx = self.pool().begin().await.map_err(poll_error)?;
        // SKIP LOCKED lets an overlapping run take other polls, never these.
        let rows: Vec<DueRow> = sqlx::query_as(&format!(
            "SELECT {POLL_COLUMNS}, next_nudge_at FROM bot_polls
                 WHERE next_nudge_at <= $1
                 ORDER BY next_nudge_at LIMIT $2 FOR UPDATE SKIP LOCKED"
        ))
        .bind(now)
        .bind(NUDGES_PER_RUN)
        .fetch_all(&mut *tx)
        .await
        .map_err(poll_error)?;
        let mut due = Vec::with_capacity(rows.len());
        for (id, share_id, chat_id, title, days, nudge_time, closes_at, due_at) in rows {
            let poll = poll_from_row((id, share_id, chat_id, title, days, nudge_time, closes_at))?;
            sqlx::query("UPDATE bot_polls SET next_nudge_at = $2 WHERE id = $1")
                .bind(poll.id)
                .bind(poll.plan.next_nudge(now))
                .execute(&mut *tx)
                .await
                .map_err(poll_error)?;
            due.push((poll, due_at));
        }
        tx.commit().await.map_err(poll_error)?;
        Ok(due)
    }
}
