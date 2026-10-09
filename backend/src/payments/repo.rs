//! sqlx/PostgreSQL adapter for the `PayoutRepository` port.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::domain::{PayoutAccount, PayoutRepository, RepoError};
use crate::auth::UserId;

#[derive(Clone)]
pub struct PgPayoutRepository {
    pool: PgPool,
}

impl PgPayoutRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn unavailable(err: sqlx::Error) -> RepoError {
    RepoError::Unavailable(err.to_string())
}

impl PayoutRepository for PgPayoutRepository {
    async fn find(&self, user: UserId) -> Result<Option<PayoutAccount>, RepoError> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT bank_bin, account_number, account_name FROM payout_accounts WHERE user_id = $1",
        )
        .bind(user.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        row.map(|(bin, number, name)| {
            PayoutAccount::from_storage(bin, number, name)
                .ok_or_else(|| RepoError::Corrupt("payout account format".to_owned()))
        })
        .transpose()
    }

    async fn save(
        &self,
        user: UserId,
        account: &PayoutAccount,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO payout_accounts (user_id, bank_bin, account_number, account_name, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id) DO UPDATE
             SET bank_bin = $2, account_number = $3, account_name = $4, updated_at = $5",
        )
        .bind(user.0)
        .bind(&account.bank_bin)
        .bind(&account.account_number)
        .bind(&account.account_name)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(())
    }
}
