//! Slots feature: players take a place on team A or B, with up to two named guests.

pub mod domain;
mod http;
mod repo;
pub mod service;

pub use http::router;
pub use repo::PgSlotRepository;
