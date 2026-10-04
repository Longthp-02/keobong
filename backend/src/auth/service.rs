//! Sign-in, session lookup and sign-out use cases. Depends only on domain ports.

use chrono::Duration;

use super::domain::{
    AuthRepository, IdentityProvider, LOGIN_ATTEMPT_TTL_MINUTES, LoginAttempt, NewProfile,
    ProviderError, RepoError, ReturnTo, SESSION_TTL_DAYS, SessionToken, User,
};
use crate::clock::Clock;
use crate::random::RandomnessError;

/// What the HTTP layer needs to send the browser to the provider.
#[derive(Debug)]
pub struct LoginStart {
    pub redirect_url: String,
    /// Also set as a cookie, binding the sign-in to this browser.
    pub state: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StartLoginError {
    #[error(transparent)]
    Randomness(#[from] RandomnessError),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Records a new sign-in attempt and returns the provider URL to redirect to.
pub async fn start_login<R, P, C>(
    repo: &R,
    provider: &P,
    clock: &C,
    return_to: Option<&str>,
) -> Result<LoginStart, StartLoginError>
where
    R: AuthRepository,
    P: IdentityProvider + ?Sized,
    C: Clock + ?Sized,
{
    let now = clock.now();
    repo.delete_login_attempts_before(now - Duration::minutes(LOGIN_ATTEMPT_TTL_MINUTES))
        .await?;
    let attempt = LoginAttempt::start(ReturnTo::parse(return_to), now)?;
    repo.save_login_attempt(&attempt).await?;
    Ok(LoginStart {
        redirect_url: provider.authorization_url(&attempt),
        state: attempt.state,
    })
}

#[derive(Debug)]
pub struct LoginComplete {
    pub session: SessionToken,
    pub return_to: ReturnTo,
}

#[derive(Debug, thiserror::Error)]
pub enum FinishLoginError {
    /// The `state` parameter does not match this browser's state cookie (login CSRF).
    #[error("state does not match the browser")]
    StateMismatch,
    /// The attempt is unknown, already used or expired.
    #[error("unknown or expired sign-in attempt")]
    UnknownAttempt,
    #[error("id token nonce does not match the sign-in attempt")]
    NonceMismatch,
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Randomness(#[from] RandomnessError),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Completes a sign-in: checks state and nonce, exchanges the code, finds or
/// creates the user and opens a session.
pub async fn finish_login<R, P, C>(
    repo: &R,
    provider: &P,
    clock: &C,
    code: &str,
    state: &str,
    state_cookie: Option<&str>,
) -> Result<LoginComplete, FinishLoginError>
where
    R: AuthRepository,
    P: IdentityProvider + ?Sized,
    C: Clock + ?Sized,
{
    if state_cookie != Some(state) {
        return Err(FinishLoginError::StateMismatch);
    }
    let attempt = repo
        .take_login_attempt(state)
        .await?
        .ok_or(FinishLoginError::UnknownAttempt)?;
    if attempt.is_expired(clock.now()) {
        return Err(FinishLoginError::UnknownAttempt);
    }
    let identity = provider.exchange_code(code, &attempt).await?;
    if identity.nonce.as_deref() != Some(attempt.nonce.as_str()) {
        return Err(FinishLoginError::NonceMismatch);
    }
    let user = repo
        .find_or_create_user(
            provider.provider(),
            &identity,
            &NewProfile::from_identity(&identity),
        )
        .await?;
    let session = SessionToken::generate()?;
    let now = clock.now();
    repo.create_session(
        &session.hash(),
        user,
        now,
        now + Duration::days(SESSION_TTL_DAYS),
    )
    .await?;
    Ok(LoginComplete {
        session,
        return_to: attempt.return_to,
    })
}

/// Returns the user behind a session cookie value, if it is valid and unexpired.
pub async fn current_user<R, C>(
    repo: &R,
    clock: &C,
    cookie_value: Option<&str>,
) -> Result<Option<User>, RepoError>
where
    R: AuthRepository,
    C: Clock + ?Sized,
{
    let Some(token) = cookie_value.and_then(SessionToken::parse) else {
        return Ok(None);
    };
    repo.find_session_user(&token.hash(), clock.now()).await
}

/// Ends the session behind a cookie value. Unknown or malformed values are a no-op.
pub async fn sign_out<R: AuthRepository>(
    repo: &R,
    cookie_value: Option<&str>,
) -> Result<(), RepoError> {
    match cookie_value.and_then(SessionToken::parse) {
        Some(token) => repo.delete_session(&token.hash()).await,
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::super::domain::{Provider, ProviderFuture, ProviderIdentity, SessionHash, UserId};
    use super::*;

    /// Repository double that fails the test if storage is touched.
    struct UnreachableRepo;

    impl AuthRepository for UnreachableRepo {
        async fn save_login_attempt(&self, _: &LoginAttempt) -> Result<(), RepoError> {
            panic!("storage must not be touched");
        }
        async fn take_login_attempt(&self, _: &str) -> Result<Option<LoginAttempt>, RepoError> {
            panic!("storage must not be touched");
        }
        async fn delete_login_attempts_before(&self, _: DateTime<Utc>) -> Result<(), RepoError> {
            panic!("storage must not be touched");
        }
        async fn find_or_create_user(
            &self,
            _: Provider,
            _: &ProviderIdentity,
            _: &NewProfile,
        ) -> Result<UserId, RepoError> {
            panic!("storage must not be touched");
        }
        async fn create_session(
            &self,
            _: &SessionHash,
            _: UserId,
            _: DateTime<Utc>,
            _: DateTime<Utc>,
        ) -> Result<(), RepoError> {
            panic!("storage must not be touched");
        }
        async fn find_session_user(
            &self,
            _: &SessionHash,
            _: DateTime<Utc>,
        ) -> Result<Option<User>, RepoError> {
            panic!("storage must not be touched");
        }
        async fn delete_session(&self, _: &SessionHash) -> Result<(), RepoError> {
            panic!("storage must not be touched");
        }
    }

    struct UnreachableProvider;

    impl IdentityProvider for UnreachableProvider {
        fn provider(&self) -> Provider {
            Provider::Google
        }
        fn authorization_url(&self, _: &LoginAttempt) -> String {
            panic!("provider must not be called");
        }
        fn exchange_code<'a>(&'a self, _: &'a str, _: &'a LoginAttempt) -> ProviderFuture<'a> {
            panic!("provider must not be called");
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            "2026-10-05T00:00:00Z".parse().unwrap()
        }
    }

    #[tokio::test]
    async fn state_mismatch_is_rejected_before_touching_storage_or_provider() {
        for cookie in [None, Some("other-state")] {
            let result = finish_login(
                &UnreachableRepo,
                &UnreachableProvider,
                &FixedClock,
                "code",
                "state",
                cookie,
            )
            .await;

            assert!(matches!(result, Err(FinishLoginError::StateMismatch)));
        }
    }

    #[tokio::test]
    async fn malformed_session_cookies_never_reach_storage() {
        for cookie in [
            None,
            Some(""),
            Some("not-a-token"),
            Some("x'; DROP TABLE users;--"),
        ] {
            let user = current_user(&UnreachableRepo, &FixedClock, cookie).await;
            assert!(matches!(user, Ok(None)));
            assert!(sign_out(&UnreachableRepo, cookie).await.is_ok());
        }
    }
}
