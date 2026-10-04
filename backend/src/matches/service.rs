//! Use cases for matches. Depends only on the domain port.

use super::domain::{Match, MatchRepository, RepoError, ShareId};

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Repository double that fails the test if storage is touched.
    struct UnreachableRepo;

    impl MatchRepository for UnreachableRepo {
        async fn find_by_share_id(&self, _id: &ShareId) -> Result<Option<Match>, RepoError> {
            panic!("storage must not be queried for a malformed share id");
        }
    }

    #[tokio::test]
    async fn malformed_share_id_is_not_found_without_querying_storage() {
        let result = get_public_match(&UnreachableRepo, "bad!id").await;

        assert!(matches!(result, Err(GetMatchError::NotFound)));
    }
}
