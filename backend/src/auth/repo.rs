//! sqlx/PostgreSQL adapter for the `AuthRepository` port.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::domain::{
    AuthRepository, LoginAttempt, NewProfile, Provider, ProviderIdentity, RepoError, ReturnTo,
    SessionHash, User, UserId,
};

#[derive(Clone)]
pub struct PgAuthRepository {
    pool: PgPool,
}

impl PgAuthRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn unavailable(err: sqlx::Error) -> RepoError {
    RepoError::Unavailable(err.to_string())
}

#[derive(sqlx::FromRow)]
struct LoginAttemptRow {
    state: String,
    code_verifier: String,
    nonce: String,
    return_to: String,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i64,
    display_name: Option<String>,
    avatar_url: Option<String>,
}

impl AuthRepository for PgAuthRepository {
    async fn save_login_attempt(&self, attempt: &LoginAttempt) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO oauth_login_attempts (state, code_verifier, nonce, return_to, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&attempt.state)
        .bind(&attempt.code_verifier)
        .bind(&attempt.nonce)
        .bind(attempt.return_to.as_str())
        .bind(attempt.created_at)
        .execute(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(())
    }

    async fn take_login_attempt(&self, state: &str) -> Result<Option<LoginAttempt>, RepoError> {
        let row: Option<LoginAttemptRow> = sqlx::query_as(
            "DELETE FROM oauth_login_attempts WHERE state = $1
             RETURNING state, code_verifier, nonce, return_to, created_at",
        )
        .bind(state)
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(row.map(|row| LoginAttempt {
            state: row.state,
            code_verifier: row.code_verifier,
            nonce: row.nonce,
            return_to: ReturnTo::parse(Some(&row.return_to)),
            created_at: row.created_at,
        }))
    }

    async fn delete_login_attempts_before(&self, cutoff: DateTime<Utc>) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM oauth_login_attempts WHERE created_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(())
    }

    async fn find_or_create_user(
        &self,
        provider: Provider,
        identity: &ProviderIdentity,
        profile: &NewProfile,
    ) -> Result<UserId, RepoError> {
        let existing: Option<i64> = sqlx::query_scalar(
            "UPDATE user_identities SET email = $3
             WHERE provider = $1 AND subject = $2
             RETURNING user_id",
        )
        .bind(provider.as_str())
        .bind(&identity.subject)
        .bind(&identity.email)
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        if let Some(id) = existing {
            return Ok(UserId(id));
        }

        // First sign-in. Two concurrent first sign-ins race on the identity's
        // primary key; the loser rolls back its new user and reuses the winner's.
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (display_name, avatar_url) VALUES ($1, $2) RETURNING id",
        )
        .bind(&profile.display_name)
        .bind(&profile.avatar_url)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?;
        let linked: Option<i64> = sqlx::query_scalar(
            "INSERT INTO user_identities (provider, subject, user_id, email)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (provider, subject) DO NOTHING
             RETURNING user_id",
        )
        .bind(provider.as_str())
        .bind(&identity.subject)
        .bind(user_id)
        .bind(&identity.email)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?;
        if linked.is_some() {
            tx.commit().await.map_err(unavailable)?;
            return Ok(UserId(user_id));
        }
        tx.rollback().await.map_err(unavailable)?;
        let winner: i64 = sqlx::query_scalar(
            "SELECT user_id FROM user_identities WHERE provider = $1 AND subject = $2",
        )
        .bind(provider.as_str())
        .bind(&identity.subject)
        .fetch_one(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(UserId(winner))
    }

    async fn create_session(
        &self,
        hash: &SessionHash,
        user: UserId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(hash.0.as_slice())
        .bind(user.0)
        .bind(created_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(())
    }

    async fn find_session_user(
        &self,
        hash: &SessionHash,
        now: DateTime<Utc>,
    ) -> Result<Option<User>, RepoError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT u.id, u.display_name, u.avatar_url
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = $1 AND s.expires_at > $2",
        )
        .bind(hash.0.as_slice())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(row.map(|row| User {
            id: UserId(row.id),
            display_name: row.display_name,
            avatar_url: row.avatar_url,
        }))
    }

    async fn delete_expired_sessions(&self, now: DateTime<Utc>) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(())
    }

    async fn delete_session(&self, hash: &SessionHash) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(hash.0.as_slice())
            .execute(&self.pool)
            .await
            .map_err(unavailable)?;
        Ok(())
    }
}
