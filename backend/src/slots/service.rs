//! Join, leave and roster use cases. Depends only on domain ports.

use super::domain::{
    ClaimOutcome, ClaimRejected, JoinRequest, LeaveRejected, MyPlace, ReleaseOutcome, RepoError,
    Roster, SlotRepository,
};
use crate::auth::UserId;
use crate::clock::Clock;
use crate::matches::domain::ShareId;

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("match not found")]
    MatchNotFound,
    #[error(transparent)]
    Rejected(#[from] ClaimRejected),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Takes places for the holder and their guests on one team, atomically.
pub async fn join<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    holder: UserId,
    raw_share_id: &str,
    request: &JoinRequest,
) -> Result<(), JoinError> {
    let share_id = ShareId::parse(raw_share_id).ok_or(JoinError::MatchNotFound)?;
    match repo.claim(&share_id, holder, request, clock.now()).await? {
        ClaimOutcome::Claimed => Ok(()),
        ClaimOutcome::MatchNotFound => Err(JoinError::MatchNotFound),
        ClaimOutcome::Rejected(reason) => Err(reason.into()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LeaveError {
    #[error("match not found")]
    MatchNotFound,
    #[error(transparent)]
    Rejected(#[from] LeaveRejected),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Releases the holder's place and their guests' places. Leaving a match one
/// has not joined is a no-op.
pub async fn leave<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    holder: UserId,
    raw_share_id: &str,
) -> Result<(), LeaveError> {
    let share_id = ShareId::parse(raw_share_id).ok_or(LeaveError::MatchNotFound)?;
    match repo.release(&share_id, holder, clock.now()).await? {
        ReleaseOutcome::Released | ReleaseOutcome::NotJoined => Ok(()),
        ReleaseOutcome::MatchNotFound => Err(LeaveError::MatchNotFound),
        ReleaseOutcome::Rejected(reason) => Err(reason.into()),
    }
}

/// Public list of who holds a place. `None` when the match does not exist.
pub async fn roster<R: SlotRepository>(
    repo: &R,
    raw_share_id: &str,
) -> Result<Option<Roster>, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.roster(&share_id).await,
        None => Ok(None),
    }
}

/// The user's own place. Outer `None` when the match does not exist.
pub async fn my_place<R: SlotRepository>(
    repo: &R,
    holder: UserId,
    raw_share_id: &str,
) -> Result<Option<Option<MyPlace>>, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.my_place(&share_id, holder).await,
        None => Ok(None),
    }
}
