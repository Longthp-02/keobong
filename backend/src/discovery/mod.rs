//! Discovery feature: the public list of upcoming matches that still have open
//! places. It owns no tables; it combines the `matches` and `slots` services.

mod http;
pub mod service;

pub use http::router;
