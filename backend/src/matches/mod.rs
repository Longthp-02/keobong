//! Matches feature: one scheduled pickup game and its public view.

pub mod domain;
mod http;
mod repo;
pub mod service;

pub use http::router;
pub use repo::PgMatchRepository;
