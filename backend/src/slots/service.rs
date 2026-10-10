//! Join, leave and roster use cases. Depends only on domain ports.

use super::domain::{
    ClaimOutcome, ClaimRejected, HostAction, HostActionOutcome, HostView, JoinRequest,
    LeaveRejected, MyPlace, ReleaseOutcome, RepoError, ReportOutcome, Roster, SlotRepository,
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

/// Places held per match, for lists of many matches (one query).
pub async fn taken_places<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    match_ids: &[ShareId],
) -> Result<std::collections::HashMap<String, i64>, RepoError> {
    if match_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    repo.taken_places(match_ids, clock.now()).await
}

/// Public list of who holds a place (expired holds excluded). `None` when the
/// match does not exist.
pub async fn roster<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    raw_share_id: &str,
) -> Result<Option<Roster>, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.roster(&share_id, clock.now()).await,
        None => Ok(None),
    }
}

/// The user's own place. Outer `None` when the match does not exist.
pub async fn my_place<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    holder: UserId,
    raw_share_id: &str,
) -> Result<Option<Option<MyPlace>>, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.my_place(&share_id, holder, clock.now()).await,
        None => Ok(None),
    }
}

/// The holder says they transferred; the hold stops expiring.
pub async fn report_payment<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    holder: UserId,
    raw_share_id: &str,
) -> Result<ReportOutcome, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.report_payment(&share_id, holder, clock.now()).await,
        None => Ok(ReportOutcome::MatchNotFound),
    }
}

/// Parties and their payment status, for the match's host only.
pub async fn host_parties<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    caller: UserId,
    raw_share_id: &str,
) -> Result<HostView, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.host_parties(&share_id, caller, clock.now()).await,
        None => Ok(HostView::MatchNotFound),
    }
}

/// The host confirms a transfer or reports it missing (which releases the party).
pub async fn host_action<R: SlotRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    caller: UserId,
    raw_share_id: &str,
    payment_code: i64,
    action: HostAction,
) -> Result<HostActionOutcome, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => {
            repo.host_action(&share_id, caller, payment_code, action, clock.now())
                .await
        }
        None => Ok(HostActionOutcome::MatchNotFound),
    }
}
