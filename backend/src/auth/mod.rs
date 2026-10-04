//! Auth feature: Google sign-in, server-side sessions and the signed-in user.
//! Other features use only `AuthenticatedUser`, `AuthState` and `UserId`.

pub mod domain;
pub mod google;
mod http;
mod repo;
pub mod service;

pub use domain::UserId;
pub use http::{AuthState, AuthenticatedUser, router};
pub use repo::PgAuthRepository;
