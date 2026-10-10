//! Use case: one page of upcoming matches that players can still join.

use crate::clock::Clock;
use crate::matches::domain::{ListedMatch, MatchRepository};
use crate::matches::service::{ListError, ListRequest, upcoming_matches};
use crate::slots::domain::SlotRepository;
use crate::slots::service::taken_places;

#[derive(Debug)]
pub struct OpenMatch {
    pub listed: ListedMatch,
    pub places_left: i64,
}

#[derive(Debug)]
pub struct OpenMatchPage {
    pub matches: Vec<OpenMatch>,
    /// Cursor for the next page. Full matches are dropped after paging, so a
    /// page can hold fewer matches than the page size and still have a next one.
    pub next: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenMatchesError {
    #[error(transparent)]
    List(#[from] ListError),
    #[error("slot counts unavailable: {0}")]
    Slots(String),
}

/// Upcoming, not cancelled matches with at least one open place (confirmed by
/// Long 2026-10-10: the list is for finding a place, so full matches are hidden).
pub async fn open_matches<M: MatchRepository, S: SlotRepository, C: Clock + ?Sized>(
    matches: &M,
    slots: &S,
    clock: &C,
    request: ListRequest,
) -> Result<OpenMatchPage, OpenMatchesError> {
    let page = upcoming_matches(matches, clock, request).await?;
    let ids: Vec<_> = page
        .matches
        .iter()
        .map(|m| m.found.share_id.clone())
        .collect();
    let taken = taken_places(slots, clock, &ids)
        .await
        .map_err(|err| OpenMatchesError::Slots(err.to_string()))?;
    let open = page
        .matches
        .into_iter()
        .filter_map(|listed| {
            let held = taken
                .get(listed.found.share_id.as_str())
                .copied()
                .unwrap_or(0);
            let places_left = i64::from(listed.found.slot_count) - held;
            (places_left > 0).then_some(OpenMatch {
                listed,
                places_left,
            })
        })
        .collect();
    Ok(OpenMatchPage {
        matches: open,
        next: page.next.map(|cursor| cursor.encode()),
    })
}
