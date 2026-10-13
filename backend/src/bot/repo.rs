//! Postgres adapter for the Zalo bot.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use super::domain::{BotDraft, BotRepository, DRAFT_MINUTES, RepoError};
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
    async fn record_update(&self, message_id: &str, now: DateTime<Utc>) -> Result<bool, RepoError> {
        let mut tx = self.pool.begin().await.map_err(repo_error)?;
        sqlx::query("DELETE FROM bot_updates WHERE received_at < $1")
            .bind(now - Duration::days(2))
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
        let inserted = sqlx::query(
            "INSERT INTO bot_updates (message_id, received_at) VALUES ($1, $2)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(message_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(repo_error)?
        .rows_affected()
            == 1;
        tx.commit().await.map_err(repo_error)?;
        Ok(inserted)
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
        // Drafts read for another account must not be created for this one.
        sqlx::query("DELETE FROM bot_drafts WHERE zalo_user_id = $1")
            .bind(&zalo_user_id)
            .execute(&mut *tx)
            .await
            .map_err(repo_error)?;
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
}
