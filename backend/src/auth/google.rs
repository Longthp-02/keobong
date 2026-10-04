//! Google OpenID Connect adapter for the `IdentityProvider` port.
//!
//! The ID token comes straight from Google's token endpoint over TLS in a
//! request authenticated with our client secret, so per OpenID Connect Core
//! 3.1.3.7 (step 6) TLS server validation stands in for checking the token's
//! signature. The issuer, audience, expiry and nonce are still checked.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::domain::{
    IdentityProvider, LoginAttempt, Provider, ProviderError, ProviderFuture, ProviderIdentity,
};
use crate::clock::Clock;
use crate::config::{GoogleConfig, Secret};

const AUTHORIZATION_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];
const SUBJECT_MAX_CHARS: usize = 255;

pub struct GoogleProvider {
    client_id: String,
    client_secret: Secret,
    redirect_uri: String,
    http: reqwest::Client,
    clock: Arc<dyn Clock>,
}

impl GoogleProvider {
    pub fn new(config: GoogleConfig, clock: Arc<dyn Clock>) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            client_id: config.client_id,
            client_secret: config.client_secret,
            redirect_uri: config.redirect_uri,
            http,
            clock,
        })
    }

    async fn exchange(
        &self,
        code: &str,
        attempt: &LoginAttempt,
    ) -> Result<ProviderIdentity, ProviderError> {
        let response = self
            .http
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.expose()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("code_verifier", attempt.code_verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.without_url().to_string()))?;
        let status = response.status();
        if status.is_client_error() {
            // Google's error body has only an error code and description, no secrets.
            let body: TokenError = response.json().await.unwrap_or_default();
            return Err(ProviderError::Rejected(format!("{status}: {}", body.error)));
        }
        if !status.is_success() {
            return Err(ProviderError::Unavailable(format!(
                "token endpoint returned {status}"
            )));
        }
        let body: TokenResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Unavailable(e.without_url().to_string()))?;
        decode_id_token(&body.id_token, &self.client_id, self.clock.now())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Deserialize, Default)]
struct TokenError {
    #[serde(default)]
    error: String,
}

impl IdentityProvider for GoogleProvider {
    fn provider(&self) -> Provider {
        Provider::Google
    }

    fn authorization_url(&self, attempt: &LoginAttempt) -> String {
        let challenge = attempt.code_challenge();
        let params = [
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", "openid email profile"),
            ("state", attempt.state.as_str()),
            ("nonce", attempt.nonce.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ];
        match reqwest::Url::parse_with_params(AUTHORIZATION_ENDPOINT, params) {
            Ok(url) => url.into(),
            // The endpoint is a valid constant; parsing cannot fail.
            Err(_) => AUTHORIZATION_ENDPOINT.to_owned(),
        }
    }

    fn exchange_code<'a>(&'a self, code: &'a str, attempt: &'a LoginAttempt) -> ProviderFuture<'a> {
        Box::pin(self.exchange(code, attempt))
    }
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    sub: String,
    exp: i64,
    email: Option<String>,
    name: Option<String>,
    picture: Option<String>,
    nonce: Option<String>,
}

/// Decodes and validates the claims of an ID token received from the token endpoint.
pub fn decode_id_token(
    id_token: &str,
    client_id: &str,
    now: DateTime<Utc>,
) -> Result<ProviderIdentity, ProviderError> {
    let invalid = |why: &str| ProviderError::InvalidToken(why.to_owned());
    let mut parts = id_token.split('.');
    let (Some(_header), Some(payload), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid("not a JWT"));
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid("payload is not base64url"))?;
    let claims: Claims =
        serde_json::from_slice(&bytes).map_err(|_| invalid("payload is not the expected JSON"))?;
    if !ISSUERS.contains(&claims.iss.as_str()) {
        return Err(invalid("unexpected issuer"));
    }
    if claims.aud != client_id {
        return Err(invalid("unexpected audience"));
    }
    if claims.exp <= now.timestamp() {
        return Err(invalid("expired"));
    }
    if claims.sub.is_empty() || claims.sub.chars().count() > SUBJECT_MAX_CHARS {
        return Err(invalid("bad subject"));
    }
    Ok(ProviderIdentity {
        subject: claims.sub,
        email: claims.email,
        name: claims.name,
        picture: claims.picture,
        nonce: claims.nonce,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const CLIENT_ID: &str = "client-123.apps.googleusercontent.com";

    fn now() -> DateTime<Utc> {
        "2026-10-05T00:00:00Z".parse().unwrap()
    }

    fn claims() -> Value {
        json!({
            "iss": "https://accounts.google.com",
            "aud": CLIENT_ID,
            "sub": "1234567890",
            "exp": now().timestamp() + 3600,
            "email": "player@example.com",
            "name": "Long",
            "picture": "https://lh3.googleusercontent.com/a/photo",
            "nonce": "n-0S6_WzA2Mj"
        })
    }

    fn token(claims: &Value) -> String {
        format!(
            "eyJhbGciOiJSUzI1NiJ9.{}.sig",
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    #[test]
    fn valid_token_yields_the_identity() {
        let identity = decode_id_token(&token(&claims()), CLIENT_ID, now()).unwrap();

        assert_eq!(
            identity,
            ProviderIdentity {
                subject: "1234567890".to_owned(),
                email: Some("player@example.com".to_owned()),
                name: Some("Long".to_owned()),
                picture: Some("https://lh3.googleusercontent.com/a/photo".to_owned()),
                nonce: Some("n-0S6_WzA2Mj".to_owned()),
            }
        );
    }

    #[test]
    fn tokens_with_wrong_claims_are_rejected() {
        let cases = [
            ("iss", json!("https://evil.example")),
            ("aud", json!("someone-else")),
            ("exp", json!(now().timestamp())),
            ("sub", json!("")),
        ];
        for (claim, value) in cases {
            let mut bad = claims();
            bad[claim] = value;

            let result = decode_id_token(&token(&bad), CLIENT_ID, now());

            assert!(
                matches!(result, Err(ProviderError::InvalidToken(_))),
                "{claim}"
            );
        }
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        for raw in [
            "",
            "a.b",
            "a.b.c.d",
            "a.!!!.c",
            &format!("a.{}.c", URL_SAFE_NO_PAD.encode("[]")),
        ] {
            assert!(
                matches!(
                    decode_id_token(raw, CLIENT_ID, now()),
                    Err(ProviderError::InvalidToken(_))
                ),
                "{raw}"
            );
        }
    }

    #[test]
    fn authorization_url_carries_pkce_state_and_nonce() {
        let provider = GoogleProvider::new(
            GoogleConfig {
                client_id: CLIENT_ID.to_owned(),
                client_secret: Secret::new("secret".to_owned()),
                redirect_uri: "https://daghep.vn/api/auth/google/callback".to_owned(),
            },
            Arc::new(crate::clock::SystemClock),
        )
        .unwrap();
        let attempt =
            LoginAttempt::start(super::super::domain::ReturnTo::parse(None), now()).unwrap();

        let url = reqwest::Url::parse(&provider.authorization_url(&attempt)).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        assert_eq!(url.host_str(), Some("accounts.google.com"));
        assert_eq!(query["client_id"], CLIENT_ID);
        assert_eq!(
            query["redirect_uri"],
            "https://daghep.vn/api/auth/google/callback"
        );
        assert_eq!(query["scope"], "openid email profile");
        assert_eq!(query["state"], attempt.state);
        assert_eq!(query["nonce"], attempt.nonce);
        assert_eq!(query["code_challenge"], attempt.code_challenge());
        assert_eq!(query["code_challenge_method"], "S256");
        assert!(!url.as_str().contains("secret"));
    }
}
