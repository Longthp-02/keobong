//! Bot feature: "Bot DaGhep" on Zalo. Organisers link their Zalo account once,
//! then create matches from a group or private chat (`docs/zalo-bot-plan.md`).
//! It owns `zalo_links`, `zalo_link_codes`, `bot_drafts` and `bot_updates`, and
//! uses `drafts`, `matches` and `payments` only through their public services.

pub mod domain;
mod http;
mod repo;
pub mod service;
pub mod zalo;

pub use http::{BotState, ZaloSettings, router};
pub use repo::PgBotRepository;
