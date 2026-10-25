//! Bot feature: "Bot DaGhep" on Zalo. Organisers link their Zalo account once,
//! then create matches from a group or private chat (`docs/zalo-bot-plan.md`).
//! It owns `zalo_links`, `zalo_link_codes`, `bot_drafts`, `bot_updates`,
//! `bot_pending_reminders`, `bot_reminders`, `bot_pending_polls`, `bot_polls`,
//! `bot_poll_answers` and `bot_chat_members`, and
//! uses `drafts`, `matches` and `payments` only through their public services.

pub mod anthropic;
pub mod domain;
mod http;
mod poll_repo;
pub mod polls;
pub mod reminders;
mod repo;
pub mod service;
pub mod zalo;

pub use http::{BotState, ZaloSettings, router};
pub use repo::PgBotRepository;
