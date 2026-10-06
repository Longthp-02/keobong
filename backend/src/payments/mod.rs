//! Payments feature: host payout accounts and VietQR transfer instructions.
//! Other features use `service` and `PgPayoutRepository`.

pub mod domain;
mod http;
mod repo;
pub mod service;

pub use http::router;
pub use repo::PgPayoutRepository;
