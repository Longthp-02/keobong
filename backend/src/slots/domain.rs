//! Slots domain: taking a place on team A or B, with up to two named guests.
//! Pure rules plus the storage port. No Axum, sqlx or HTTP types here.

use std::future::Future;

use chrono::{DateTime, Utc};

use crate::auth::UserId;
use crate::matches::domain::ShareId;
use crate::text::clean_name;

/// A holder may bring at most this many named guests (confirmed by Long).
pub const MAX_GUESTS: usize = 2;
pub const GUEST_NAME_MAX_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Team {
    A,
    B,
}

impl Team {
    pub fn as_str(self) -> &'static str {
        match self {
            Team::A => "a",
            Team::B => "b",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "a" => Some(Team::A),
            "b" => Some(Team::B),
            _ => None,
        }
    }

    /// Places split evenly between the teams; team A takes the odd one.
    pub fn capacity(self, slot_count: i16) -> i64 {
        let total = i64::from(slot_count);
        match self {
            Team::A => (total + 1) / 2,
            Team::B => total / 2,
        }
    }
}

/// Up to [`MAX_GUESTS`] cleaned, non-empty guest names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuestNames(Vec<String>);

impl GuestNames {
    pub fn parse(raw: &[String]) -> Option<Self> {
        if raw.len() > MAX_GUESTS {
            return None;
        }
        raw.iter()
            .map(|name| {
                // Reject rather than truncate: the holder should see what is shown.
                let cleaned = clean_name(name, usize::MAX)?;
                (cleaned.chars().count() <= GUEST_NAME_MAX_CHARS).then_some(cleaned)
            })
            .collect::<Option<Vec<_>>>()
            .map(Self)
    }

    pub fn names(&self) -> &[String] {
        &self.0
    }

    /// The holder plus their guests.
    pub fn party_size(&self) -> i64 {
        1 + self.0.len() as i64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRequest {
    pub team: Team,
    pub guests: GuestNames,
}

/// What the claiming transaction knows, read under a lock on the match.
#[derive(Debug, Clone, Copy)]
pub struct ClaimContext {
    pub now: DateTime<Utc>,
    pub starts_at: DateTime<Utc>,
    pub slot_count: i16,
    /// Active places already taken in the requested team.
    pub taken_in_team: i64,
    pub already_joined: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClaimRejected {
    #[error("the host cancelled the match")]
    MatchCancelled,
    #[error("the match has already started")]
    MatchStarted,
    #[error("the user already holds a place in this match")]
    AlreadyJoined,
    #[error("not enough open places in this team")]
    TeamFull,
}

/// The whole party (holder plus guests) joins one team, or nobody does.
pub fn check_claim(ctx: &ClaimContext, request: &JoinRequest) -> Result<(), ClaimRejected> {
    if ctx.cancelled {
        return Err(ClaimRejected::MatchCancelled);
    }
    if ctx.now >= ctx.starts_at {
        return Err(ClaimRejected::MatchStarted);
    }
    if ctx.already_joined {
        return Err(ClaimRejected::AlreadyJoined);
    }
    if ctx.taken_in_team + request.guests.party_size() > request.team.capacity(ctx.slot_count) {
        return Err(ClaimRejected::TeamFull);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LeaveRejected {
    #[error("the match has already started")]
    MatchStarted,
    /// Places of a cancelled match stay as they were, for refunds.
    #[error("the host cancelled the match")]
    MatchCancelled,
}

/// Players can leave until kickoff. Leaving within 2 hours of kickoff will let
/// the host mark a no-show (spec.md); that marking comes in a later step.
pub fn check_leave(
    now: DateTime<Utc>,
    starts_at: DateTime<Utc>,
    cancelled: bool,
) -> Result<(), LeaveRejected> {
    if cancelled {
        return Err(LeaveRejected::MatchCancelled);
    }
    if now >= starts_at {
        return Err(LeaveRejected::MatchStarted);
    }
    Ok(())
}

/// One taken place, as shown publicly on the match page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub team: Team,
    /// The player's display name, or the guest's name.
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_guest: bool,
    /// For a guest, the display name of the player who brought them.
    pub guest_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    pub slot_count: i16,
    pub cancelled: bool,
    /// In the order places were taken.
    pub entries: Vec<RosterEntry>,
}

/// How long an unpaid place is held before it is released (confirmed by Long).
pub const HOLD_MINUTES: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatus {
    /// Held until the hold expires; then released automatically.
    AwaitingPayment,
    /// The player says they transferred: no expiry, waiting for the host.
    PaymentReported,
    /// The host confirmed, or nothing is owed.
    Confirmed,
}

impl PaymentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentStatus::AwaitingPayment => "awaiting_payment",
            PaymentStatus::PaymentReported => "payment_reported",
            PaymentStatus::Confirmed => "confirmed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "awaiting_payment" => Some(PaymentStatus::AwaitingPayment),
            "payment_reported" => Some(PaymentStatus::PaymentReported),
            "confirmed" => Some(PaymentStatus::Confirmed),
            _ => None,
        }
    }
}

/// Status and hold for a party that just joined. Nothing is owed for a free
/// match or for the host's own party, so those are confirmed at once.
pub fn initial_payment(
    price_per_player_vnd: i64,
    is_host: bool,
    now: DateTime<Utc>,
) -> (PaymentStatus, Option<DateTime<Utc>>) {
    if price_per_player_vnd == 0 || is_host {
        (PaymentStatus::Confirmed, None)
    } else {
        (
            PaymentStatus::AwaitingPayment,
            Some(now + chrono::Duration::minutes(HOLD_MINUTES)),
        )
    }
}

/// What one transfer covers: the holder plus their guests.
pub fn party_amount_vnd(price_per_player_vnd: i64, party_size: i64) -> i64 {
    price_per_player_vnd.saturating_mul(party_size)
}

/// The signed-in user's own place in a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyPlace {
    pub team: Team,
    pub guests: Vec<String>,
    /// Identifies the party in the transfer memo and the host's list.
    pub payment_code: i64,
    pub payment_status: PaymentStatus,
    pub hold_expires_at: Option<DateTime<Utc>>,
    pub amount_vnd: i64,
    pub host: UserId,
    /// Nothing is owed for a cancelled match.
    pub match_cancelled: bool,
}

/// One party as the host sees it when checking transfers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostParty {
    pub payment_code: i64,
    pub team: Team,
    pub holder_name: Option<String>,
    pub guests: Vec<String>,
    pub amount_vnd: i64,
    pub payment_status: PaymentStatus,
    pub hold_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostView {
    MatchNotFound,
    NotHost,
    Parties(Vec<HostParty>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOutcome {
    Reported,
    /// Already reported or confirmed; reporting again changes nothing.
    AlreadyDone,
    NotJoined,
    MatchNotFound,
    MatchCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAction {
    Confirm,
    /// "Not received": releases the party's places.
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostActionOutcome {
    Done,
    MatchNotFound,
    NotHost,
    PartyNotFound,
    /// A confirmed payment cannot be rejected.
    AlreadyConfirmed,
    /// Only a transfer the player reported can be rejected; unpaid holds
    /// expire on their own (confirmed by Long 2026-10-09).
    NotReported,
    /// Rejecting closes at kickoff; no-show marking covers later cases.
    MatchStarted,
    /// Payments of a cancelled match are settled between players and host.
    MatchCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    Claimed,
    MatchNotFound,
    Rejected(ClaimRejected),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseOutcome {
    Released,
    NotJoined,
    MatchNotFound,
    Rejected(LeaveRejected),
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("storage unavailable: {0}")]
    Unavailable(String),
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
}

/// Storage port. `claim` and `release` must apply the checks above atomically
/// with their writes (the adapter locks the match row).
pub trait SlotRepository: Send + Sync {
    fn claim(
        &self,
        match_id: &ShareId,
        holder: UserId,
        request: &JoinRequest,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<ClaimOutcome, RepoError>> + Send;

    fn release(
        &self,
        match_id: &ShareId,
        holder: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<ReleaseOutcome, RepoError>> + Send;

    /// `None` when the match does not exist.
    fn roster(
        &self,
        match_id: &ShareId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<Roster>, RepoError>> + Send;

    /// Outer `None` when the match does not exist; inner `None` when not joined.
    fn my_place(
        &self,
        match_id: &ShareId,
        holder: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<Option<MyPlace>>, RepoError>> + Send;

    /// The holder says they transferred: stops the hold's countdown.
    fn report_payment(
        &self,
        match_id: &ShareId,
        holder: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<ReportOutcome, RepoError>> + Send;

    fn host_parties(
        &self,
        match_id: &ShareId,
        caller: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<HostView, RepoError>> + Send;

    fn host_action(
        &self,
        match_id: &ShareId,
        caller: UserId,
        payment_code: i64,
        action: HostAction,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<HostActionOutcome, RepoError>> + Send;
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn names(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn teams_split_places_with_the_odd_one_in_team_a() {
        assert_eq!((Team::A.capacity(18), Team::B.capacity(18)), (9, 9));
        assert_eq!((Team::A.capacity(15), Team::B.capacity(15)), (8, 7));
        assert_eq!((Team::A.capacity(2), Team::B.capacity(2)), (1, 1));
    }

    #[test]
    fn guest_names_are_cleaned_and_limited() {
        let guests = GuestNames::parse(&names(&["  An ", "Bình"])).unwrap();
        assert_eq!(guests.names(), ["An", "Bình"]);
        assert_eq!(guests.party_size(), 3);

        assert!(GuestNames::parse(&names(&["A", "B", "C"])).is_none());
        assert!(GuestNames::parse(&names(&["  "])).is_none());
        assert!(GuestNames::parse(&names(&["\u{0}"])).is_none());
        assert!(GuestNames::parse(&["x".repeat(41)]).is_none());
        assert!(GuestNames::parse(&["x".repeat(40)]).is_some());
        // Rejected, not truncated to the first 40 characters.
        assert!(GuestNames::parse(&[format!("{} y", "x".repeat(40))]).is_none());
        assert_eq!(GuestNames::parse(&[]).unwrap().party_size(), 1);
    }

    fn ctx(taken_in_team: i64) -> ClaimContext {
        let starts_at: DateTime<Utc> = "2099-10-10T11:30:00Z".parse().unwrap();
        ClaimContext {
            now: starts_at - Duration::hours(1),
            starts_at,
            slot_count: 18,
            taken_in_team,
            already_joined: false,
            cancelled: false,
        }
    }

    fn join(team: Team, guests: &[&str]) -> JoinRequest {
        JoinRequest {
            team,
            guests: GuestNames::parse(&names(guests)).unwrap(),
        }
    }

    #[test]
    fn a_party_joins_only_if_the_whole_party_fits() {
        assert_eq!(check_claim(&ctx(8), &join(Team::A, &[])), Ok(()));
        assert_eq!(
            check_claim(&ctx(8), &join(Team::A, &["An"])),
            Err(ClaimRejected::TeamFull)
        );
        assert_eq!(
            check_claim(&ctx(6), &join(Team::B, &["An", "Bình"])),
            Ok(())
        );
        assert_eq!(
            check_claim(&ctx(9), &join(Team::B, &[])),
            Err(ClaimRejected::TeamFull)
        );
    }

    #[test]
    fn nobody_joins_a_cancelled_match() {
        let cancelled = ClaimContext {
            cancelled: true,
            ..ctx(0)
        };

        assert_eq!(
            check_claim(&cancelled, &join(Team::A, &[])),
            Err(ClaimRejected::MatchCancelled)
        );
    }

    #[test]
    fn nobody_joins_twice_or_after_kickoff() {
        let joined = ClaimContext {
            already_joined: true,
            ..ctx(0)
        };
        assert_eq!(
            check_claim(&joined, &join(Team::A, &[])),
            Err(ClaimRejected::AlreadyJoined)
        );

        let started = ClaimContext {
            now: ctx(0).starts_at,
            ..ctx(0)
        };
        assert_eq!(
            check_claim(&started, &join(Team::A, &[])),
            Err(ClaimRejected::MatchStarted)
        );
    }

    #[test]
    fn paid_places_are_held_for_thirty_minutes() {
        let now: DateTime<Utc> = "2099-10-10T10:00:00Z".parse().unwrap();

        assert_eq!(
            initial_payment(50_000, false, now),
            (
                PaymentStatus::AwaitingPayment,
                Some("2099-10-10T10:30:00Z".parse().unwrap())
            )
        );
        assert_eq!(
            initial_payment(0, false, now),
            (PaymentStatus::Confirmed, None)
        );
        assert_eq!(
            initial_payment(50_000, true, now),
            (PaymentStatus::Confirmed, None)
        );
    }

    #[test]
    fn one_transfer_covers_the_whole_party() {
        assert_eq!(party_amount_vnd(50_000, 3), 150_000);
        assert_eq!(party_amount_vnd(i64::MAX, 3), i64::MAX);
    }

    #[test]
    fn payment_statuses_round_trip() {
        for status in [
            PaymentStatus::AwaitingPayment,
            PaymentStatus::PaymentReported,
            PaymentStatus::Confirmed,
        ] {
            assert_eq!(PaymentStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(PaymentStatus::parse("paid"), None);
    }

    #[test]
    fn players_can_leave_until_kickoff() {
        let starts_at: DateTime<Utc> = "2099-10-10T11:30:00Z".parse().unwrap();

        assert_eq!(
            check_leave(starts_at - Duration::seconds(1), starts_at, false),
            Ok(())
        );
        assert_eq!(
            check_leave(starts_at, starts_at, false),
            Err(LeaveRejected::MatchStarted)
        );
    }
}
