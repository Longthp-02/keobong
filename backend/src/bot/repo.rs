//! Postgres adapter for the Zalo bot.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use chrono::NaiveTime;

use super::domain::{
    BotDraft, BotRepository, DRAFT_MINUTES, REPLIES_PER_SENDER_PER_HOUR, RepoError, UpdateOutcome,
};
use super::reminders::{
    MAX_REMINDERS_PER_CHAT, MAX_SEND_FAILURES, PendingReminder, REMINDERS_PER_RUN, Reminder,
    ReminderPlan, Schedule, repeat_columns, repeat_from_columns,
};
use crate::auth::UserId;
use crate::matches::domain::{Format, MatchType};

#[derive(Clone)]
pub struct PgBotRepository {
    pool: PgPool,
}

impl PgBotRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn repo_error(error: sqlx::Error) -> RepoError {
    RepoError(error.to_string())
}

/// Rebuilds a stored plan; a row the rules would not accept is an error.
fn plan_from_row(
    weekdays: i16,
    day_of_month: Option<i16>,
    time: NaiveTime,
    text: String,
) -> Result<ReminderPlan, RepoError> {
    let repeat = repeat_from_columns(weekdays, day_of_month).ok_or_else(|| {
        RepoError(format!(
            "stored reminder has an invalid repeat: {weekdays}, {day_of_month:?}"
        ))
    })?;
    Ok(ReminderPlan {
        schedule: Schedule { repeat, time },
        text,
    })
}

type PendingRow = (i64, i16, Option<i16>, NaiveTime, String, DateTime<Utc>);

type ReminderRow = (
    i64,
    String,
    i16,
    Option<i16>,
    NaiveTime,
    String,
    DateTime<Utc>,
);

fn reminder_from_row(row: ReminderRow) -> Result<Reminder, RepoError> {
    let (id, chat_id, weekdays, day_of_month, time, text, next_run_at) = row;
    Ok(Reminder {
        id,
        chat_id,
        plan: plan_from_row(weekdays, day_of_month, time, text)?,
        next_run_at,
    })
}

type DraftRow = (
    i64,
    String,
    DateTime<Utc>,
    DateTime<Utc>,
    String,
    String,
    i16,
    i16,
    i16,
    i64,
    DateTime<Utc>,
);

impl BotRepository for PgBotRepository {
    async fn record_update(
        &self,
        message_id: &str,
        sender_id: &str,
        now: DateTime<Utc>,
    ) -> Result<UpdateOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        sqlx::query("DELETE FROM bot_updates WHERE received_at < $1")
            .bind(now - Duration::days(2))
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        let seen: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM bot_updates WHERE message_id = $1)")
                .bind(message_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(repo_error)?;
        if seen {
            return Ok(UpdateOutcome::Duplicate);
        }
        let recent: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bot_updates WHERE sender_id = $1 AND received_at > $2",
        )
        .bind(sender_id)
        .bind(now - Duration::hours(1))
        .fetch_one(&mut *tx)
        .await
        .map_err(repo_error)?;
        if recent >= REPLIES_PER_SENDER_PER_HOUR {
            return Ok(UpdateOutcome::SenderLimited);
        }
        // A concurrent delivery of the same message may have won; then it is a duplicate.
        let inserted = sqlx::query(
            "INSERT INTO bot_updates (message_id, sender_id, received_at) VALUES ($1, $2, $3)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .bind(sender_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?
        .rows_affected()
            == 1;
        tx.commit().await.map_err(repo_error)?;
        Ok(if inserted {
            UpdateOutcome::New
        } else {
            UpdateOutcome::Duplicate
        })
    }

    async fn linked_user(&self, zalo_user_id: &str) -> Result<Option<UserId>, RepoError> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT user_id FROM zalo_links WHERE zalo_user_id = $1")
                .bind(zalo_user_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(repo_error)?;
        Ok(id.map(UserId))
    }

    async fn save_link_code(
        &self,
        code_hash: [u8; 32],
        zalo_user_id: &str,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        // Expired codes are useless; a new code also replaces the sender's older ones.
        sqlx::query("DELETE FROM zalo_link_codes WHERE expires_at <= $1 OR zalo_user_id = $2")
            .bind(now)
            .bind(zalo_user_id)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        sqlx::query(
            "INSERT INTO zalo_link_codes (code_hash, zalo_user_id, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(code_hash.as_slice())
        .bind(zalo_user_id)
        .bind(expires_at)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?;
        tx.commit().await.map_err(repo_error)
    }

    async fn redeem_link_code(
        &self,
        code_hash: [u8; 32],
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<Option<String>, RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        let zalo_user_id: Option<String> = sqlx::query_scalar(
            "DELETE FROM zalo_link_codes WHERE code_hash = $1 AND expires_at > $2
             RETURNING zalo_user_id",
        )
        .bind(code_hash.as_slice())
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(repo_error)?;
        let Some(zalo_user_id) = zalo_user_id else {
            return Ok(None);
        };
        // One Zalo account per user and the other way round: the newest link wins.
        sqlx::query("DELETE FROM zalo_links WHERE user_id = $1 OR zalo_user_id = $2")
            .bind(user.0)
            .bind(&zalo_user_id)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        sqlx::query(
            "INSERT INTO zalo_links (zalo_user_id, user_id, linked_at) VALUES ($1, $2, $3)",
        )
        .bind(&zalo_user_id)
        .bind(user.0)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?;
        // Drafts read under the previous link, by this Zalo account or for this
        // user, must not be created after the link changed.
        for table in ["bot_drafts", "bot_pending_reminders"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE zalo_user_id = $1 OR user_id = $2"
            ))
            .bind(&zalo_user_id)
            .bind(user.0)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        }
        tx.commit().await.map_err(repo_error)?;
        Ok(Some(zalo_user_id))
    }

    async fn save_draft(&self, draft: &BotDraft) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        // Drafts expire unused; clear old ones while writing a new one.
        sqlx::query("DELETE FROM bot_drafts WHERE expires_at <= $1")
            .bind(draft.expires_at - Duration::minutes(DRAFT_MINUTES))
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        // One pending item per sender and chat: a new match replaces a reminder.
        sqlx::query("DELETE FROM bot_pending_reminders WHERE zalo_user_id = $1 AND chat_id = $2")
            .bind(&draft.zalo_user_id)
            .bind(&draft.chat_id)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        sqlx::query(
            "INSERT INTO bot_drafts (zalo_user_id, chat_id, user_id, venue_slug, starts_at, ends_at,
                 format, match_type, level_min_tenths, level_max_tenths, slot_count, total_fee_vnd,
                 expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (zalo_user_id, chat_id) DO UPDATE SET
                 user_id = EXCLUDED.user_id, venue_slug = EXCLUDED.venue_slug,
                 starts_at = EXCLUDED.starts_at, ends_at = EXCLUDED.ends_at,
                 format = EXCLUDED.format, match_type = EXCLUDED.match_type,
                 level_min_tenths = EXCLUDED.level_min_tenths,
                 level_max_tenths = EXCLUDED.level_max_tenths,
                 slot_count = EXCLUDED.slot_count, total_fee_vnd = EXCLUDED.total_fee_vnd,
                 expires_at = EXCLUDED.expires_at",
        )
        .bind(&draft.zalo_user_id)
        .bind(&draft.chat_id)
        .bind(draft.user.0)
        .bind(&draft.venue_slug)
        .bind(draft.starts_at)
        .bind(draft.ends_at)
        .bind(draft.format.as_str())
        .bind(draft.match_type.as_str())
        .bind(draft.level_min_tenths)
        .bind(draft.level_max_tenths)
        .bind(draft.slot_count)
        .bind(draft.total_fee_vnd)
        .bind(draft.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?;
        tx.commit().await.map_err(repo_error)
    }

    async fn take_draft(
        &self,
        zalo_user_id: &str,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<BotDraft>, RepoError> {
        // Deleting it is what makes a repeated "ok" create the match only once.
        let row: Option<DraftRow> = sqlx::query_as(
            "DELETE FROM bot_drafts WHERE zalo_user_id = $1 AND chat_id = $2
             RETURNING user_id, venue_slug, starts_at, ends_at, format, match_type,
                 level_min_tenths, level_max_tenths, slot_count, total_fee_vnd, expires_at",
        )
        .bind(zalo_user_id)
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(repo_error)?;
        let Some((
            user,
            venue_slug,
            starts_at,
            ends_at,
            format,
            match_type,
            min,
            max,
            slots,
            fee,
            expires_at,
        )) = row
        else {
            return Ok(None);
        };
        if expires_at <= now {
            return Ok(None);
        }
        let (Some(format), Some(match_type)) =
            (Format::parse(&format), MatchType::parse(&match_type))
        else {
            return Err(RepoError(format!(
                "stored draft has an unknown format or match type: {format}, {match_type}"
            )));
        };
        Ok(Some(BotDraft {
            zalo_user_id: zalo_user_id.to_owned(),
            chat_id: chat_id.to_owned(),
            user: UserId(user),
            venue_slug,
            starts_at,
            ends_at,
            format,
            match_type,
            level_min_tenths: min,
            level_max_tenths: max,
            slot_count: slots,
            total_fee_vnd: fee,
            expires_at,
        }))
    }

    async fn save_pending_reminder(&self, pending: &PendingReminder) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        sqlx::query("DELETE FROM bot_pending_reminders WHERE expires_at <= $1")
            .bind(pending.expires_at - Duration::minutes(DRAFT_MINUTES))
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        // One pending item per sender and chat: a reminder replaces a match draft.
        sqlx::query("DELETE FROM bot_drafts WHERE zalo_user_id = $1 AND chat_id = $2")
            .bind(&pending.zalo_user_id)
            .bind(&pending.chat_id)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        let (weekdays, day_of_month) = repeat_columns(pending.plan.schedule.repeat);
        sqlx::query(
            "INSERT INTO bot_pending_reminders (zalo_user_id, chat_id, user_id, weekdays,
                 day_of_month, time_of_day, text, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (zalo_user_id, chat_id) DO UPDATE SET
                 user_id = EXCLUDED.user_id, weekdays = EXCLUDED.weekdays,
                 day_of_month = EXCLUDED.day_of_month, time_of_day = EXCLUDED.time_of_day,
                 text = EXCLUDED.text, expires_at = EXCLUDED.expires_at",
        )
        .bind(&pending.zalo_user_id)
        .bind(&pending.chat_id)
        .bind(pending.user.0)
        .bind(weekdays)
        .bind(day_of_month)
        .bind(pending.plan.schedule.time)
        .bind(&pending.plan.text)
        .bind(pending.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?;
        tx.commit().await.map_err(repo_error)
    }

    async fn take_pending_reminder(
        &self,
        zalo_user_id: &str,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<PendingReminder>, RepoError> {
        let row: Option<PendingRow> = sqlx::query_as(
            "DELETE FROM bot_pending_reminders WHERE zalo_user_id = $1 AND chat_id = $2
                 RETURNING user_id, weekdays, day_of_month, time_of_day, text, expires_at",
        )
        .bind(zalo_user_id)
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(repo_error)?;
        let Some((user, weekdays, day_of_month, time, text, expires_at)) = row else {
            return Ok(None);
        };
        if expires_at <= now {
            return Ok(None);
        }
        Ok(Some(PendingReminder {
            zalo_user_id: zalo_user_id.to_owned(),
            chat_id: chat_id.to_owned(),
            user: UserId(user),
            plan: plan_from_row(weekdays, day_of_month, time, text)?,
            expires_at,
        }))
    }

    async fn add_reminder(
        &self,
        chat_id: &str,
        user: UserId,
        plan: &ReminderPlan,
        next_run_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<bool, RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        // Serialises adds per chat so two "ok"s cannot both pass the count.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('bot_reminders:' || $1, 0))")
            .bind(chat_id)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM bot_reminders WHERE chat_id = $1")
                .bind(chat_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(repo_error)?;
        if count >= MAX_REMINDERS_PER_CHAT {
            return Ok(false);
        }
        let (weekdays, day_of_month) = repeat_columns(plan.schedule.repeat);
        sqlx::query(
            "INSERT INTO bot_reminders (chat_id, user_id, weekdays, day_of_month, time_of_day,
                 text, next_run_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(chat_id)
        .bind(user.0)
        .bind(weekdays)
        .bind(day_of_month)
        .bind(plan.schedule.time)
        .bind(&plan.text)
        .bind(next_run_at)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?;
        tx.commit().await.map_err(repo_error)?;
        Ok(true)
    }

    async fn reminders(&self, chat_id: &str) -> Result<Vec<Reminder>, RepoError> {
        let rows: Vec<ReminderRow> = sqlx::query_as(
            "SELECT id, chat_id, weekdays, day_of_month, time_of_day, text, next_run_at
             FROM bot_reminders WHERE chat_id = $1 ORDER BY id",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await
        .map_err(repo_error)?;
        rows.into_iter().map(reminder_from_row).collect()
    }

    async fn delete_reminder(&self, chat_id: &str, id: i64) -> Result<bool, RepoError> {
        let deleted = sqlx::query("DELETE FROM bot_reminders WHERE chat_id = $1 AND id = $2")
            .bind(chat_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(repo_error)?
            .rows_affected();
        Ok(deleted == 1)
    }

    async fn claim_due_reminders(&self, now: DateTime<Utc>) -> Result<Vec<Reminder>, RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        // SKIP LOCKED lets an overlapping run take other reminders, never these.
        let rows: Vec<ReminderRow> = sqlx::query_as(
            "SELECT id, chat_id, weekdays, day_of_month, time_of_day, text, next_run_at
             FROM bot_reminders WHERE next_run_at <= $1
             ORDER BY next_run_at LIMIT $2 FOR UPDATE SKIP LOCKED",
        )
        .bind(now)
        .bind(REMINDERS_PER_RUN)
        .fetch_all(&mut *tx)
        .await
        .map_err(repo_error)?;
        let mut due = Vec::with_capacity(rows.len());
        for row in rows {
            let reminder = reminder_from_row(row)?;
            // Posted once even if runs were missed: the next run is after now.
            sqlx::query("UPDATE bot_reminders SET next_run_at = $2 WHERE id = $1")
                .bind(reminder.id)
                .bind(reminder.plan.schedule.next_run(now))
                .execute(&mut *tx)
                .await
                .map_err(repo_error)?;
            due.push(reminder);
        }
        tx.commit().await.map_err(repo_error)?;
        Ok(due)
    }

    async fn record_reminder_send(&self, id: i64, delivered: bool) -> Result<(), RepoError> {
        if delivered {
            sqlx::query("UPDATE bot_reminders SET failures = 0 WHERE id = $1 AND failures > 0")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(repo_error)?;
            return Ok(());
        }
        // Postgres applies only one change per row per statement, so the count and
        // the delete are two statements.
        sqlx::query("UPDATE bot_reminders SET failures = failures + 1 WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(repo_error)?;
        sqlx::query("DELETE FROM bot_reminders WHERE id = $1 AND failures >= $2")
            .bind(id)
            .bind(MAX_SEND_FAILURES)
            .execute(&self.pool)
            .await
            .map_err(repo_error)?;
        Ok(())
    }
}
