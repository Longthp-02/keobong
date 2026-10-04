//! Runtime configuration from environment variables. Secrets never have defaults.

use std::env;

/// A secret value that never appears in `Debug` output or logs.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Google OAuth client settings (Google Cloud console → Credentials).
#[derive(Debug, Clone)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: Secret,
    /// Must match an authorized redirect URI exactly.
    pub redirect_uri: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// The web app's origin: allowed for CORS and same-origin checks, and the
    /// base for redirects after sign-in. HTTPS origins get `Secure` cookies.
    pub frontend_origin: String,
    /// `None` disables Google sign-in (sign-in endpoints answer 503).
    pub google: Option<GoogleConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value}")]
    Invalid { name: &'static str, value: String },
    #[error("set all of GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET and GOOGLE_REDIRECT_URI, or none")]
    PartialGoogle,
}

const GOOGLE_VARS: [&str; 3] = [
    "GOOGLE_CLIENT_ID",
    "GOOGLE_CLIENT_SECRET",
    "GOOGLE_REDIRECT_URI",
];

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| env::var(name).ok().filter(|value| !value.is_empty()))
    }

    /// Builds the configuration from any variable source (tests pass a map).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let database_url = get("DATABASE_URL").ok_or(ConfigError::Missing("DATABASE_URL"))?;
        let port = match get("PORT") {
            Some(value) => value.parse().map_err(|_| ConfigError::Invalid {
                name: "PORT",
                value,
            })?,
            None => 8080,
        };
        let frontend_origin =
            get("FRONTEND_ORIGIN").unwrap_or_else(|| "http://localhost:3000".to_owned());
        if !(frontend_origin.starts_with("https://") || frontend_origin.starts_with("http://"))
            || frontend_origin.ends_with('/')
        {
            return Err(ConfigError::Invalid {
                name: "FRONTEND_ORIGIN",
                value: frontend_origin,
            });
        }
        let google = match GOOGLE_VARS.map(&get) {
            [Some(client_id), Some(client_secret), Some(redirect_uri)] => Some(GoogleConfig {
                client_id,
                client_secret: Secret::new(client_secret),
                redirect_uri,
            }),
            [None, None, None] => None,
            _ => return Err(ConfigError::PartialGoogle),
        };
        Ok(Self {
            database_url,
            port,
            frontend_origin,
            google,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|name| map.get(name).cloned())
    }

    const DB: (&str, &str) = ("DATABASE_URL", "postgres://localhost/db");

    #[test]
    fn defaults_suit_local_development() {
        let config = config(&[DB]).unwrap();

        assert_eq!(config.port, 8080);
        assert_eq!(config.frontend_origin, "http://localhost:3000");
        assert!(config.google.is_none());
    }

    #[test]
    fn google_needs_all_three_variables() {
        let full = config(&[
            DB,
            ("GOOGLE_CLIENT_ID", "id"),
            ("GOOGLE_CLIENT_SECRET", "shh"),
            (
                "GOOGLE_REDIRECT_URI",
                "https://daghep.vn/api/auth/google/callback",
            ),
        ])
        .unwrap();
        assert_eq!(full.google.unwrap().client_secret.expose(), "shh");

        let partial = config(&[DB, ("GOOGLE_CLIENT_ID", "id")]);
        assert!(matches!(partial, Err(ConfigError::PartialGoogle)));
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let config = config(&[
            DB,
            ("GOOGLE_CLIENT_ID", "id"),
            ("GOOGLE_CLIENT_SECRET", "very-secret-value"),
            (
                "GOOGLE_REDIRECT_URI",
                "https://daghep.vn/api/auth/google/callback",
            ),
        ])
        .unwrap();

        assert!(!format!("{config:?}").contains("very-secret-value"));
    }

    #[test]
    fn frontend_origin_must_be_a_bare_http_origin() {
        for bad in ["daghep.vn", "https://daghep.vn/"] {
            assert!(config(&[DB, ("FRONTEND_ORIGIN", bad)]).is_err(), "{bad}");
        }
    }
}
