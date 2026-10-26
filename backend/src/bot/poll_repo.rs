//! Postgres adapter for attendance polls.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

use super::polls::{
    AnswerOutcome, MAX_OPEN_POLLS_PER_CHAT, MAX_SERIES_FAILURES, MAX_SERIES_PER_CHAT, Member,
    PendingPoll, Poll, PollAnswer, PollGroup, PollPlan, PollRepoError, PollRepository, PollSeries,
    PublicGroup, post_at, week_posted_at,
};
use super::repo::PgBotRepository;
use crate::auth::UserId;
use crate::matches::domain::ShareId;
use crate::matches::venues::{GameFormat, Sport};

type PublicGroupRow = (
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    i16,
    Option<NaiveTime>,
    Option<String>,
    Option<String>,
    Option<Vec<NaiveDate>>,
    Option<Vec<i64>>,
    Option<i64>,
);

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
    Option<NaiveTime>,
    DateTime<Utc>,
);

fn poll_from_row(row: PollRow) -> Result<Poll, PollRepoError> {
    let (id, share_id, chat_id, title, days, nudge_time, play_time, closes_at) = row;
    let share_id = ShareId::parse(&share_id)
        .ok_or_else(|| PollRepoError(format!("stored poll {id} has an invalid share id")))?;
    Ok(Poll {
        id,
        share_id,
        chat_id,
        plan: PollPlan {
            title,
            days,
            play_time,
            nudge_time,
        },
        closes_at,
    })
}

type PendingPollRow = (
    i64,
    String,
    Vec<NaiveDate>,
    Option<NaiveTime>,
    NaiveTime,
    DateTime<Utc>,
    bool,
    Option<String>,
    Option<String>,
    Option<i64>,
    bool,
);

/// A group's columns, as stored: sport, format, venue id and public.
type GroupColumns = (Option<String>, Option<String>, Option<i64>, bool);

fn group_from_columns(
    (sport, format, venue_id, public): GroupColumns,
) -> Result<PollGroup, PollRepoError> {
    let sport = sport
        .map(|sport| {
            Sport::parse(&sport)
                .ok_or_else(|| PollRepoError(format!("stored group has an unknown sport {sport}")))
        })
        .transpose()?;
    let format = format
        .map(|format| {
            GameFormat::parse(&format).ok_or_else(|| {
                PollRepoError(format!("stored group has an unknown format {format}"))
            })
        })
        .transpose()?;
    Ok(PollGroup {
        sport,
        format,
        venue_id,
        public,
    })
}

type SeriesRow = (
    i64,
    String,
    i64,
    String,
    i16,
    Option<NaiveTime>,
    NaiveTime,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
    Option<i64>,
    bool,
);

const SERIES_COLUMNS: &str = "id, chat_id, user_id, title, weekdays, play_time, nudge_time,
     next_post_at, sport, game_format, venue_id, public";

fn series_from_row(row: SeriesRow) -> Result<PollSeries, PollRepoError> {
    let (
        id,
        chat_id,
        user,
        title,
        weekdays,
        play_time,
        nudge_time,
        next_post_at,
        sport,
        format,
        venue_id,
        public,
    ) = row;
    let weekdays = u8::try_from(weekdays)
        .ok()
        .filter(|bits| (1..=127).contains(bits))
        .ok_or_else(|| PollRepoError(format!("stored series {id} has invalid weekdays")))?;
    Ok(PollSeries {
        id,
        chat_id,
        user: UserId(user),
        title,
        weekdays,
        play_time,
        nudge_time,
        next_post_at,
        group: group_from_columns((sport, format, venue_id, public))?,
    })
}

type DueRow = (
    i64,
    String,
    String,
    String,
    Vec<NaiveDate>,
    NaiveTime,
    Option<NaiveTime>,
    DateTime<Utc>,
    DateTime<Utc>,
);

const POLL_COLUMNS: &str = "id, share_id, chat_id, title, days, nudge_time, play_time, closes_at";

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
                 nudge_time, expires_at, play_time, weekly, sport, game_format, venue_id, public)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (zalo_user_id, chat_id) DO UPDATE SET
                 user_id = EXCLUDED.user_id, title = EXCLUDED.title, days = EXCLUDED.days,
                 nudge_time = EXCLUDED.nudge_time, expires_at = EXCLUDED.expires_at,
                 play_time = EXCLUDED.play_time, weekly = EXCLUDED.weekly,
                 sport = EXCLUDED.sport, game_format = EXCLUDED.game_format,
                 venue_id = EXCLUDED.venue_id, public = EXCLUDED.public",
        )
        .bind(&pending.zalo_user_id)
        .bind(&pending.chat_id)
        .bind(pending.user.0)
        .bind(&pending.plan.title)
        .bind(&pending.plan.days)
        .bind(pending.plan.nudge_time)
        .bind(pending.expires_at)
        .bind(pending.plan.play_time)
        .bind(pending.weekly)
        .bind(pending.group.sport.map(Sport::as_str))
        .bind(pending.group.format.map(GameFormat::as_str))
        .bind(pending.group.venue_id)
        .bind(pending.group.public)
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
                 RETURNING user_id, title, days, play_time, nudge_time, expires_at, weekly,
                     sport, game_format, venue_id, public",
        )
        .bind(zalo_user_id)
        .bind(chat_id)
        .fetch_optional(self.pool())
        .await
        .map_err(poll_error)?;
        let Some(row) = row.filter(|row| row.5 > now) else {
            return Ok(None);
        };
        let (
            user,
            title,
            days,
            play_time,
            nudge_time,
            expires_at,
            weekly,
            sport,
            format,
            venue_id,
            public,
        ) = row;
        Ok(Some(PendingPoll {
            zalo_user_id: zalo_user_id.to_owned(),
            chat_id: chat_id.to_owned(),
            user: UserId(user),
            plan: PollPlan {
                title,
                days,
                play_time,
                nudge_time,
            },
            weekly,
            group: group_from_columns((sport, format, venue_id, public))?,
            expires_at,
        }))
    }

    async fn add_poll(
        &self,
        chat_id: &str,
        user: UserId,
        plan: &PollPlan,
        share_id: &ShareId,
        series_id: Option<i64>,
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
                 next_nudge_at, closes_at, created_at, play_time, series_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
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
        .bind(plan.play_time)
        .bind(series_id)
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

    async fn open_polls(
        &self,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<Poll>, PollRepoError> {
        let rows: Vec<PollRow> = sqlx::query_as(&format!(
            "SELECT {POLL_COLUMNS} FROM bot_polls
             WHERE chat_id = $1 AND closes_at > $2
             ORDER BY created_at DESC, id DESC"
        ))
        .bind(chat_id)
        .bind(now)
        .fetch_all(self.pool())
        .await
        .map_err(poll_error)?;
        rows.into_iter().map(poll_from_row).collect()
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

    async fn members(
        &self,
        chat_id: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<Member>, PollRepoError> {
        let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT u.id, u.display_name FROM users u
             WHERE u.id IN (
                 SELECT user_id FROM bot_chat_members WHERE chat_id = $1 AND last_seen_at >= $2
                 UNION
                 SELECT a.user_id FROM bot_poll_answers a
                 JOIN bot_polls p ON p.id = a.poll_id
                 WHERE p.chat_id = $1 AND a.answered_at >= $2
             )
             ORDER BY u.id",
        )
        .bind(chat_id)
        .bind(since)
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
                 WHERE next_nudge_at <= $1 AND closes_at > $1
                 ORDER BY next_nudge_at LIMIT $2 FOR UPDATE SKIP LOCKED"
        ))
        .bind(now)
        .bind(NUDGES_PER_RUN)
        .fetch_all(&mut *tx)
        .await
        .map_err(poll_error)?;
        let mut due = Vec::with_capacity(rows.len());
        for (id, share_id, chat_id, title, days, nudge_time, play_time, closes_at, due_at) in rows {
            let poll = poll_from_row((
                id, share_id, chat_id, title, days, nudge_time, play_time, closes_at,
            ))?;
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

    async fn add_series(
        &self,
        series: &PollSeries,
        first_poll: i64,
        now: DateTime<Utc>,
    ) -> Result<Option<i64>, PollRepoError> {
        let mut tx = self.pool().begin().await.map_err(poll_error)?;
        // Serialises adds per chat so two "ok"s cannot both pass the count.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('bot_poll_series:' || $1, 0))")
            .bind(&series.chat_id)
            .execute(&mut *tx)
            .await
            .map_err(poll_error)?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM bot_poll_series WHERE chat_id = $1")
                .bind(&series.chat_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(poll_error)?;
        if count >= MAX_SERIES_PER_CHAT {
            return Ok(None);
        }
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO bot_poll_series (chat_id, user_id, title, weekdays, play_time,
                 nudge_time, next_post_at, created_at, sport, game_format, venue_id, public)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             RETURNING id",
        )
        .bind(&series.chat_id)
        .bind(series.user.0)
        .bind(&series.title)
        .bind(i16::from(series.weekdays))
        .bind(series.play_time)
        .bind(series.nudge_time)
        .bind(series.next_post_at)
        .bind(now)
        .bind(series.group.sport.map(Sport::as_str))
        .bind(series.group.format.map(GameFormat::as_str))
        .bind(series.group.venue_id)
        .bind(series.group.public)
        .fetch_one(&mut *tx)
        .await
        .map_err(poll_error)?;
        sqlx::query("UPDATE bot_polls SET series_id = $2 WHERE id = $1")
            .bind(first_poll)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(poll_error)?;
        tx.commit().await.map_err(poll_error)?;
        Ok(Some(id))
    }

    async fn set_series_public(
        &self,
        chat_id: &str,
        id: i64,
        public: bool,
    ) -> Result<bool, PollRepoError> {
        let updated =
            sqlx::query("UPDATE bot_poll_series SET public = $3 WHERE chat_id = $1 AND id = $2")
                .bind(chat_id)
                .bind(id)
                .bind(public)
                .execute(self.pool())
                .await
                .map_err(poll_error)?
                .rows_affected();
        Ok(updated == 1)
    }

    async fn public_groups(&self, now: DateTime<Utc>) -> Result<Vec<PublicGroup>, PollRepoError> {
        // One statement: a venue shows only once approved; "this week" is the
        // series' poll that closes soonest and is still open.
        let rows: Vec<PublicGroupRow> = sqlx::query_as(
            "SELECT s.title, s.sport, s.game_format, s.venue_id, s.weekdays, s.play_time,
                    v.name, v.address, w.days, w.yes, w.players
             FROM bot_poll_series s
             LEFT JOIN venues v ON v.id = s.venue_id AND v.status = 'approved'
             LEFT JOIN LATERAL (
                 SELECT p.days,
                        ARRAY(
                            SELECT count(a.user_id) FILTER (WHERE a.attending)
                            FROM unnest(p.days) AS d(day)
                            LEFT JOIN bot_poll_answers a ON a.poll_id = p.id AND a.day = d.day
                            GROUP BY d.day ORDER BY d.day
                        ) AS yes,
                        (SELECT count(DISTINCT user_id) FROM bot_poll_answers
                         WHERE poll_id = p.id) AS players
                 FROM bot_polls p
                 WHERE p.series_id = s.id AND p.closes_at > $1
                 ORDER BY p.closes_at LIMIT 1
             ) w ON true
             WHERE s.public
             ORDER BY s.id DESC
             LIMIT 50",
        )
        .bind(now)
        .fetch_all(self.pool())
        .await
        .map_err(poll_error)?;
        rows.into_iter()
            .map(
                |(
                    title,
                    sport,
                    format,
                    venue_id,
                    weekdays,
                    play_time,
                    venue_name,
                    address,
                    days,
                    yes,
                    players,
                )| {
                    let week = match (days, yes, players) {
                        (Some(mut days), Some(yes), Some(players)) => {
                            days.sort();
                            Some((days.into_iter().zip(yes).collect(), players))
                        }
                        _ => None,
                    };
                    let shown_venue = venue_name.is_some();
                    Ok(PublicGroup {
                        title,
                        group: group_from_columns((
                            sport,
                            format,
                            venue_id.filter(|_| shown_venue),
                            true,
                        ))?,
                        venue_name,
                        venue_address: address,
                        weekdays: u8::try_from(weekdays).unwrap_or(0),
                        play_time,
                        week,
                    })
                },
            )
            .collect()
    }

    async fn series(&self, chat_id: &str) -> Result<Vec<PollSeries>, PollRepoError> {
        let rows: Vec<SeriesRow> = sqlx::query_as(&format!(
            "SELECT {SERIES_COLUMNS} FROM bot_poll_series WHERE chat_id = $1 ORDER BY id"
        ))
        .bind(chat_id)
        .fetch_all(self.pool())
        .await
        .map_err(poll_error)?;
        rows.into_iter().map(series_from_row).collect()
    }

    async fn delete_series(&self, chat_id: &str, id: i64) -> Result<bool, PollRepoError> {
        let deleted = sqlx::query("DELETE FROM bot_poll_series WHERE chat_id = $1 AND id = $2")
            .bind(chat_id)
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(poll_error)?
            .rows_affected();
        Ok(deleted == 1)
    }

    async fn claim_due_series(&self, now: DateTime<Utc>) -> Result<Vec<PollSeries>, PollRepoError> {
        let mut tx = self.pool().begin().await.map_err(poll_error)?;
        let rows: Vec<SeriesRow> = sqlx::query_as(&format!(
            "SELECT {SERIES_COLUMNS} FROM bot_poll_series WHERE next_post_at <= $1
             ORDER BY next_post_at LIMIT $2 FOR UPDATE SKIP LOCKED"
        ))
        .bind(now)
        .bind(NUDGES_PER_RUN)
        .fetch_all(&mut *tx)
        .await
        .map_err(poll_error)?;
        let mut due = Vec::with_capacity(rows.len());
        for row in rows {
            let series = series_from_row(row)?;
            // The next week's post after now, skipping weeks missed in an outage.
            let mut week = week_posted_at(series.next_post_at) + chrono::Duration::days(7);
            let mut next = post_at(series.weekdays, series.nudge_time, week);
            while next <= now {
                week += chrono::Duration::days(7);
                next = post_at(series.weekdays, series.nudge_time, week);
            }
            sqlx::query("UPDATE bot_poll_series SET next_post_at = $2 WHERE id = $1")
                .bind(series.id)
                .bind(next)
                .execute(&mut *tx)
                .await
                .map_err(poll_error)?;
            due.push(series);
        }
        tx.commit().await.map_err(poll_error)?;
        Ok(due)
    }

    async fn record_series_post(&self, id: i64, delivered: bool) -> Result<(), PollRepoError> {
        if delivered {
            sqlx::query("UPDATE bot_poll_series SET failures = 0 WHERE id = $1 AND failures > 0")
                .bind(id)
                .execute(self.pool())
                .await
                .map_err(poll_error)?;
            return Ok(());
        }
        // Postgres applies only one change per row per statement: two statements.
        sqlx::query("UPDATE bot_poll_series SET failures = failures + 1 WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(poll_error)?;
        sqlx::query("DELETE FROM bot_poll_series WHERE id = $1 AND failures >= $2")
            .bind(id)
            .bind(MAX_SERIES_FAILURES)
            .execute(self.pool())
            .await
            .map_err(poll_error)?;
        Ok(())
    }
}
