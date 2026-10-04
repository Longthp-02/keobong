//! Auth domain: accounts, sessions and sign-in attempts, plus the ports for
//! storage and identity providers. No Axum, sqlx, HTTP or SDK types here.

use std::future::Future;
use std::pin::Pin;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

use crate::random::{RandomnessError, random_bytes};
use crate::text::clean_name;

/// How long a started sign-in stays valid.
pub const LOGIN_ATTEMPT_TTL_MINUTES: i64 = 10;
/// How long a session lasts after sign-in. TODO: verify with Long (30 days assumed).
pub const SESSION_TTL_DAYS: i64 = 30;
pub const DISPLAY_NAME_MAX_CHARS: usize = 80;
pub const AVATAR_URL_MAX_CHARS: usize = 500;
const RETURN_TO_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserId(pub i64);

/// A signed-in user's public profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
        }
    }
}

/// Identity confirmed by a provider after the authorization code exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    /// Stable account id at the provider (Google `sub`).
    pub subject: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
    /// Echo of the nonce sent with the authorization request.
    pub nonce: Option<String>,
}

/// Public profile fields taken from a provider identity on first sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

impl NewProfile {
    pub fn from_identity(identity: &ProviderIdentity) -> Self {
        let display_name = identity
            .name
            .as_deref()
            .and_then(|name| clean_name(name, DISPLAY_NAME_MAX_CHARS));
        let avatar_url = identity
            .picture
            .as_deref()
            .filter(|url| {
                url.starts_with("https://")
                    && url.len() <= AVATAR_URL_MAX_CHARS
                    && !url.chars().any(|c| c.is_control() || c.is_whitespace())
            })
            .map(str::to_owned);
        Self {
            display_name,
            avatar_url,
        }
    }
}

/// Where to send the browser after sign-in. Only same-site absolute paths are
/// kept; anything else becomes `/`, so the callback can never be an open redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnTo(String);

impl ReturnTo {
    pub fn parse(raw: Option<&str>) -> Self {
        let safe = raw.filter(|path| {
            path.starts_with('/')
                && !path.starts_with("//")
                && path.chars().count() <= RETURN_TO_MAX_CHARS
                && !path
                    .chars()
                    .any(|c| c == '\\' || c.is_control() || c.is_whitespace())
        });
        Self(safe.unwrap_or("/").to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn random_token() -> Result<String, RandomnessError> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes::<32>()?))
}

/// One pending sign-in: CSRF `state`, PKCE `code_verifier` and OIDC `nonce`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginAttempt {
    pub state: String,
    pub code_verifier: String,
    pub nonce: String,
    pub return_to: ReturnTo,
    pub created_at: DateTime<Utc>,
}

impl LoginAttempt {
    pub fn start(return_to: ReturnTo, now: DateTime<Utc>) -> Result<Self, RandomnessError> {
        Ok(Self {
            state: random_token()?,
            code_verifier: random_token()?,
            nonce: random_token()?,
            return_to,
            created_at: now,
        })
    }

    /// PKCE S256 challenge: base64url(SHA-256(code_verifier)).
    pub fn code_challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.code_verifier.as_bytes()))
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now - self.created_at > Duration::minutes(LOGIN_ATTEMPT_TTL_MINUTES)
    }
}

/// Opaque session token carried by the cookie. Only its hash is stored.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

/// 32 random bytes encode to 43 base64url characters.
const SESSION_TOKEN_LEN: usize = 43;

impl SessionToken {
    pub fn generate() -> Result<Self, RandomnessError> {
        random_token().map(Self)
    }

    /// Accepts only well-formed tokens, so garbage cookies never reach storage.
    pub fn parse(raw: &str) -> Option<Self> {
        (raw.len() == SESSION_TOKEN_LEN
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        .then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> SessionHash {
        SessionHash(Sha256::digest(self.0.as_bytes()).into())
    }
}

/// SHA-256 of a session token, the only form stored in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionHash(pub [u8; 32]);

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("storage unavailable: {0}")]
    Unavailable(String),
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
}

/// Storage port for accounts, sessions and sign-in attempts.
pub trait AuthRepository: Send + Sync {
    fn save_login_attempt(
        &self,
        attempt: &LoginAttempt,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;

    /// Removes and returns the attempt, so each `state` can be used only once.
    fn take_login_attempt(
        &self,
        state: &str,
    ) -> impl Future<Output = Result<Option<LoginAttempt>, RepoError>> + Send;

    fn delete_login_attempts_before(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;

    /// Finds the user for this provider account, creating it on first sign-in.
    fn find_or_create_user(
        &self,
        provider: Provider,
        identity: &ProviderIdentity,
        profile: &NewProfile,
    ) -> impl Future<Output = Result<UserId, RepoError>> + Send;

    fn create_session(
        &self,
        hash: &SessionHash,
        user: UserId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;

    /// Returns the user of an unexpired session.
    fn find_session_user(
        &self,
        hash: &SessionHash,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<User>, RepoError>> + Send;

    fn delete_session(
        &self,
        hash: &SessionHash,
    ) -> impl Future<Output = Result<(), RepoError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The provider refused the code (expired, reused, wrong verifier).
    #[error("authorization code rejected: {0}")]
    Rejected(String),
    /// The returned ID token failed validation.
    #[error("invalid id token: {0}")]
    InvalidToken(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
}

pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderIdentity, ProviderError>> + Send + 'a>>;

/// Port for an OpenID Connect provider. Adapters live in `google.rs`.
pub trait IdentityProvider: Send + Sync {
    fn provider(&self) -> Provider;

    /// URL the browser is sent to for sign-in.
    fn authorization_url(&self, attempt: &LoginAttempt) -> String;

    /// Exchanges the authorization code for a validated identity.
    fn exchange_code<'a>(&'a self, code: &'a str, attempt: &'a LoginAttempt) -> ProviderFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        "2026-10-05T00:00:00Z".parse().unwrap()
    }

    #[test]
    fn return_to_keeps_same_site_paths() {
        assert_eq!(ReturnTo::parse(Some("/create")).as_str(), "/create");
        assert_eq!(
            ReturnTo::parse(Some("/m/k7Qm2xPa?x=1")).as_str(),
            "/m/k7Qm2xPa?x=1"
        );
    }

    #[test]
    fn return_to_rejects_anything_that_could_leave_the_site() {
        for raw in [
            "//evil.example",
            "/\\evil.example",
            "https://evil.example",
            "evil",
            "",
            "/a b",
            "/a\r\nSet-Cookie: x=1",
        ] {
            assert_eq!(ReturnTo::parse(Some(raw)).as_str(), "/", "{raw:?}");
        }
        assert_eq!(ReturnTo::parse(None).as_str(), "/");
        assert_eq!(
            ReturnTo::parse(Some(&format!("/{}", "a".repeat(200)))).as_str(),
            "/"
        );
    }

    #[test]
    fn login_attempt_has_distinct_random_values_and_s256_challenge() {
        let attempt = LoginAttempt::start(ReturnTo::parse(None), now()).unwrap();

        assert_eq!(attempt.state.len(), 43);
        assert_ne!(attempt.state, attempt.code_verifier);
        assert_ne!(attempt.state, attempt.nonce);
        // RFC 7636 appendix B test vector.
        let rfc = LoginAttempt {
            code_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_owned(),
            ..attempt
        };
        assert_eq!(
            rfc.code_challenge(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn login_attempt_expires_after_ten_minutes() {
        let attempt = LoginAttempt::start(ReturnTo::parse(None), now()).unwrap();

        assert!(!attempt.is_expired(now() + Duration::minutes(10)));
        assert!(attempt.is_expired(now() + Duration::minutes(10) + Duration::seconds(1)));
    }

    #[test]
    fn session_tokens_round_trip_and_reject_garbage() {
        let token = SessionToken::generate().unwrap();

        assert_eq!(SessionToken::parse(token.as_str()), Some(token.clone()));
        assert_ne!(token.hash(), SessionToken::generate().unwrap().hash());
        assert!(SessionToken::parse("short").is_none());
        assert!(SessionToken::parse(&"!".repeat(43)).is_none());
        assert!(!format!("{token:?}").contains(token.as_str()));
    }

    fn identity(name: Option<&str>, picture: Option<&str>) -> ProviderIdentity {
        ProviderIdentity {
            subject: "1234567890".to_owned(),
            email: Some("player@example.com".to_owned()),
            name: name.map(str::to_owned),
            picture: picture.map(str::to_owned),
            nonce: None,
        }
    }

    #[test]
    fn profile_cleans_the_name_and_keeps_only_https_avatars() {
        let profile = NewProfile::from_identity(&identity(
            Some("  Long\u{202E} "),
            Some("https://lh3.googleusercontent.com/a/photo"),
        ));
        assert_eq!(profile.display_name.as_deref(), Some("Long"));
        assert_eq!(
            profile.avatar_url.as_deref(),
            Some("https://lh3.googleusercontent.com/a/photo")
        );

        let profile = NewProfile::from_identity(&identity(Some(" "), Some("http://x.example/p")));
        assert_eq!(profile.display_name, None);
        assert_eq!(profile.avatar_url, None);

        let long_url = format!("https://x.example/{}", "a".repeat(500));
        assert_eq!(
            NewProfile::from_identity(&identity(None, Some(&long_url))).avatar_url,
            None
        );
    }
}
