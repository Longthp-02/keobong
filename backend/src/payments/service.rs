//! Payout account use cases, also the public API other features use.

use super::domain::{
    PaymentInstructions, PayoutAccount, PayoutField, PayoutRepository, RepoError, transfer_memo,
};
use crate::auth::UserId;
use crate::clock::Clock;

pub async fn payout_account<R: PayoutRepository>(
    repo: &R,
    user: UserId,
) -> Result<Option<PayoutAccount>, RepoError> {
    repo.find(user).await
}

#[derive(Debug, thiserror::Error)]
pub enum SavePayoutError {
    #[error("invalid field {0:?}")]
    Invalid(PayoutField),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

pub async fn save_payout_account<R: PayoutRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    user: UserId,
    bank_bin: &str,
    account_number: &str,
    account_name: &str,
) -> Result<PayoutAccount, SavePayoutError> {
    let account = PayoutAccount::validate(bank_bin, account_number, account_name)
        .map_err(SavePayoutError::Invalid)?;
    repo.save(user, &account, clock.now()).await?;
    Ok(account)
}

/// Whether `user` can host a paid match.
pub async fn has_payout_account<R: PayoutRepository>(
    repo: &R,
    user: UserId,
) -> Result<bool, RepoError> {
    Ok(repo.find(user).await?.is_some())
}

/// Transfer instructions for a player paying `host`. `None` when the host has
/// no payout account (only possible for matches created before it was required).
pub async fn payment_instructions<R: PayoutRepository>(
    repo: &R,
    host: UserId,
    amount_vnd: i64,
    payment_code: i64,
) -> Result<Option<PaymentInstructions>, RepoError> {
    Ok(repo.find(host).await?.and_then(|account| {
        PaymentInstructions::new(&account, amount_vnd, transfer_memo(payment_code))
    }))
}
