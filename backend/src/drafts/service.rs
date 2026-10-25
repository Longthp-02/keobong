//! Use case: read an organiser's message into a match draft, within the daily limits.

use crate::auth::UserId;
use crate::clock::Clock;
use crate::matches;
use crate::matches::domain::{MatchRepository, local_date};

use super::domain::{
    DraftRepository, LimitScope, MAX_TEXT_CHARS, MatchDraft, MatchTextReader, ReadRequest,
    ReaderError,
};

#[derive(Debug, thiserror::Error)]
pub enum ReadDraftError {
    #[error("message is empty or too long")]
    InvalidText,
    #[error("daily read limit reached")]
    Limited(LimitScope),
    #[error("no reader configured")]
    Unavailable,
    #[error(transparent)]
    Reader(#[from] ReaderError),
    #[error("storage error: {0}")]
    Storage(String),
}

pub async fn read_draft<D, M, C>(
    drafts: &D,
    matches: &M,
    clock: &C,
    reader: Option<&dyn MatchTextReader>,
    user: UserId,
    text: &str,
) -> Result<MatchDraft, ReadDraftError>
where
    D: DraftRepository,
    M: MatchRepository,
    C: Clock + ?Sized,
{
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        return Err(ReadDraftError::InvalidText);
    }
    let reader = reader.ok_or(ReadDraftError::Unavailable)?;
    let now = clock.now();
    // Counted before the call, so failed or slow reads still use up the limit.
    use_read(drafts, user, now).await?;
    let venues = matches::service::venues(matches)
        .await
        .map_err(|error| ReadDraftError::Storage(error.to_string()))?;
    let today = local_date(now);
    let raw = reader
        .read(ReadRequest {
            text,
            today,
            venues: &venues,
        })
        .await?;
    Ok(MatchDraft::checked(raw, now, &venues))
}

/// Uses up one AI read for `user`, or says which daily limit is reached. Other
/// AI readers (the Zalo bot's reminders) share the same limits.
pub async fn use_read<D: DraftRepository>(
    drafts: &D,
    user: UserId,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), ReadDraftError> {
    drafts
        .record_read(user, now)
        .await
        .map_err(|error| ReadDraftError::Storage(error.to_string()))?
        .map_err(ReadDraftError::Limited)
}
