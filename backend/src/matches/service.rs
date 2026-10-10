//! Use cases for matches. Depends only on the domain port.

use crate::auth::UserId;

use chrono::NaiveDate;

use super::domain::{
    CancelOutcome, Clock, Field, GeoPoint, InsertError, LIST_PAGE_SIZE, ListCursor, ListedMatch,
    Match, MatchRepository, MatchType, NewMatch, NewMatchInput, RepoError, ShareId, UpcomingQuery,
    Venue, list_window,
};

#[derive(Debug, thiserror::Error)]
pub enum GetMatchError {
    #[error("match not found")]
    NotFound,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Returns the public view of a match. Malformed ids are treated as unknown
/// without touching storage, so callers cannot probe the id format.
pub async fn get_public_match<R: MatchRepository>(
    repo: &R,
    raw_share_id: &str,
) -> Result<Match, GetMatchError> {
    let Some(share_id) = ShareId::parse(raw_share_id) else {
        return Err(GetMatchError::NotFound);
    };
    repo.find_by_share_id(&share_id)
        .await?
        .ok_or(GetMatchError::NotFound)
}

#[derive(Debug, thiserror::Error)]
pub enum CreateMatchError {
    #[error("malformed request")]
    MalformedRequest,
    #[error("request body too large")]
    PayloadTooLarge,
    #[error("invalid field {0:?}")]
    Invalid(Field),
    #[error("could not allocate a unique share id")]
    ShareIdUnavailable,
    #[error("random source failed: {0}")]
    Randomness(String),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("payout lookup failed: {0}")]
    Payouts(String),
}

/// Collisions are astronomically unlikely with 60-bit ids; a few retries make them harmless.
const SHARE_ID_ATTEMPTS: usize = 3;

/// Validates the input and stores the match, hosted by `host`, under a fresh
/// share id. A paid match needs the host's payout account (confirmed by Long).
pub async fn create_match<R: MatchRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    host: UserId,
    host_has_payout: bool,
    input: NewMatchInput,
) -> Result<Match, CreateMatchError> {
    let new = NewMatch::validate(input, clock.now()).map_err(CreateMatchError::Invalid)?;
    if new.total_fee_vnd > 0 && !host_has_payout {
        return Err(CreateMatchError::Invalid(Field::Payout));
    }
    for _ in 0..SHARE_ID_ATTEMPTS {
        let share_id =
            ShareId::generate().map_err(|e| CreateMatchError::Randomness(e.to_string()))?;
        match repo.insert(&share_id, &new, host).await {
            Ok(venue) => return Ok(Match::from_new(share_id, new, venue)),
            Err(InsertError::DuplicateShareId) => continue,
            Err(InsertError::UnknownVenue) => return Err(CreateMatchError::Invalid(Field::Venue)),
            Err(InsertError::Repo(err)) => return Err(err.into()),
        }
    }
    Err(CreateMatchError::ShareIdUnavailable)
}

/// Venues a host can choose for a new match.
pub async fn venues<R: MatchRepository>(repo: &R) -> Result<Vec<Venue>, RepoError> {
    repo.active_venues().await
}

/// Filters for the list of upcoming matches.
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    /// One local day; `None` lists every listed day.
    pub day: Option<NaiveDate>,
    pub match_type: Option<MatchType>,
    pub near: Option<GeoPoint>,
    pub after: Option<ListCursor>,
}

#[derive(Debug)]
pub struct UpcomingPage {
    pub matches: Vec<ListedMatch>,
    /// Present when there may be more matches after this page.
    pub next: Option<ListCursor>,
}

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("day outside the listed days")]
    DayOutOfRange,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// One page of upcoming, not cancelled matches in kickoff order.
pub async fn upcoming_matches<R: MatchRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    request: ListRequest,
) -> Result<UpcomingPage, ListError> {
    let now = clock.now();
    let (from, until) = list_window(now, request.day).ok_or(ListError::DayOutOfRange)?;
    let matches = repo
        .upcoming(&UpcomingQuery {
            now,
            from,
            until,
            match_type: request.match_type,
            near: request.near,
            after: request.after,
            limit: LIST_PAGE_SIZE,
        })
        .await?;
    let full_page = i64::try_from(matches.len()).is_ok_and(|n| n >= LIST_PAGE_SIZE);
    let next = full_page
        .then(|| matches.last())
        .flatten()
        .map(|last| ListCursor {
            starts_at: last.found.starts_at,
            share_id: last.found.share_id.clone(),
        });
    Ok(UpcomingPage { matches, next })
}

/// The host cancels the match. Malformed ids are unknown matches.
pub async fn cancel_match<R: MatchRepository, C: Clock + ?Sized>(
    repo: &R,
    clock: &C,
    caller: UserId,
    raw_share_id: &str,
) -> Result<CancelOutcome, RepoError> {
    match ShareId::parse(raw_share_id) {
        Some(share_id) => repo.cancel(&share_id, caller, clock.now()).await,
        None => Ok(CancelOutcome::MatchNotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repository double that fails the test if storage is touched.
    struct UnreachableRepo;

    impl MatchRepository for UnreachableRepo {
        async fn find_by_share_id(&self, _id: &ShareId) -> Result<Option<Match>, RepoError> {
            panic!("storage must not be queried for a malformed share id");
        }

        async fn insert(
            &self,
            _id: &ShareId,
            _new: &NewMatch,
            _host: UserId,
        ) -> Result<Venue, InsertError> {
            panic!("storage must not be written for invalid input");
        }

        async fn active_venues(&self) -> Result<Vec<Venue>, RepoError> {
            panic!("not used in these tests");
        }

        async fn upcoming(&self, _: &UpcomingQuery) -> Result<Vec<ListedMatch>, RepoError> {
            panic!("storage must not be queried for a day outside the list");
        }

        async fn cancel(
            &self,
            _: &ShareId,
            _: UserId,
            _: chrono::DateTime<chrono::Utc>,
        ) -> Result<CancelOutcome, RepoError> {
            panic!("not used in these tests");
        }
    }

    /// Repository double whose first `collisions` inserts report a taken share id.
    struct CollidingRepo {
        collisions: usize,
        attempts: std::sync::Mutex<Vec<String>>,
    }

    impl MatchRepository for CollidingRepo {
        async fn find_by_share_id(&self, _id: &ShareId) -> Result<Option<Match>, RepoError> {
            Ok(None)
        }

        async fn insert(
            &self,
            id: &ShareId,
            new: &NewMatch,
            _host: UserId,
        ) -> Result<Venue, InsertError> {
            let mut attempts = self.attempts.lock().unwrap();
            attempts.push(id.as_str().to_owned());
            if attempts.len() <= self.collisions {
                Err(InsertError::DuplicateShareId)
            } else {
                Ok(Venue {
                    slug: new.venue.clone(),
                    name: "SSA Sports Center".to_owned(),
                    address: "28 Duyên Hải".to_owned(),
                })
            }
        }

        async fn active_venues(&self) -> Result<Vec<Venue>, RepoError> {
            panic!("not used in these tests");
        }

        async fn upcoming(&self, _: &UpcomingQuery) -> Result<Vec<ListedMatch>, RepoError> {
            panic!("not used in these tests");
        }

        async fn cancel(
            &self,
            _: &ShareId,
            _: UserId,
            _: chrono::DateTime<chrono::Utc>,
        ) -> Result<CancelOutcome, RepoError> {
            panic!("not used in these tests");
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            "2026-10-04T00:00:00Z".parse().unwrap()
        }
    }

    fn input() -> NewMatchInput {
        NewMatchInput {
            venue: "ssa-amitie".to_owned(),
            starts_at: "2026-10-10T11:30:00Z".parse().unwrap(),
            ends_at: "2026-10-10T13:00:00Z".parse().unwrap(),
            format: super::super::domain::Format::SevenASide,
            match_type: super::super::domain::MatchType::Casual,
            level_min: 2.5,
            level_max: 3.5,
            total_fee_vnd: 900_000,
            slot_count: None,
        }
    }

    #[tokio::test]
    async fn invalid_input_is_rejected_without_touching_storage() {
        let mut bad = input();
        bad.level_min = 0.5;

        let result = create_match(&UnreachableRepo, &FixedClock, UserId(1), true, bad).await;

        assert!(matches!(
            result,
            Err(CreateMatchError::Invalid(Field::LevelMin))
        ));
    }

    #[tokio::test]
    async fn a_paid_match_without_a_payout_account_is_rejected_before_storage() {
        let result = create_match(&UnreachableRepo, &FixedClock, UserId(1), false, input()).await;

        assert!(matches!(
            result,
            Err(CreateMatchError::Invalid(Field::Payout))
        ));
    }

    #[tokio::test]
    async fn share_id_collision_is_retried_with_a_new_id() {
        let repo = CollidingRepo {
            collisions: 1,
            attempts: Default::default(),
        };

        let created = create_match(&repo, &FixedClock, UserId(1), true, input())
            .await
            .unwrap();

        let attempts = repo.attempts.lock().unwrap();
        assert_eq!(attempts.len(), 2);
        assert_ne!(attempts[0], attempts[1]);
        assert_eq!(created.share_id.as_str(), attempts[1]);
        assert_eq!(created.slot_count, 18);
    }

    #[tokio::test]
    async fn gives_up_after_repeated_collisions() {
        let repo = CollidingRepo {
            collisions: usize::MAX,
            attempts: Default::default(),
        };

        let result = create_match(&repo, &FixedClock, UserId(1), true, input()).await;

        assert!(matches!(result, Err(CreateMatchError::ShareIdUnavailable)));
        assert_eq!(repo.attempts.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_day_outside_the_list_is_rejected_without_querying_storage() {
        let request = ListRequest {
            day: NaiveDate::from_ymd_opt(2026, 10, 11),
            ..ListRequest::default()
        };

        let result = upcoming_matches(&UnreachableRepo, &FixedClock, request).await;

        assert!(matches!(result, Err(ListError::DayOutOfRange)));
    }

    #[tokio::test]
    async fn malformed_share_id_is_not_found_without_querying_storage() {
        let result = get_public_match(&UnreachableRepo, "bad!id").await;

        assert!(matches!(result, Err(GetMatchError::NotFound)));
    }
}
