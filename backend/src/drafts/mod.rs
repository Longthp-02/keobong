//! Drafts feature: reads an organiser's message (for example a post from a Zalo or
//! Facebook group) into create-form fields with an AI reader, within daily limits.
//! It owns `draft_reads` and uses `matches` only through its public service.

pub mod anthropic;
pub mod domain;
mod http;
mod repo;
pub mod service;

pub use http::router;
pub use repo::PgDraftRepository;
