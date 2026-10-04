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
            [Some(client_id), Some(client_secret), Some(redirect_uri)] => {
                // A forgotten FRONTEND_ORIGIN would silently send users to localhost.
                if get("FRONTEND_ORIGIN").is_none() {
                    return Err(ConfigError::Missing("FRONTEND_ORIGIN"));
                }
                if !redirect_uri_fits(&redirect_uri, &frontend_origin) {
                    return Err(ConfigError::Invalid {
                        name: "GOOGLE_REDIRECT_URI",
                        value: redirect_uri,
                    });
                }
                Some(GoogleConfig {
                    client_id,
                    client_secret: Secret::new(client_secret),
                    redirect_uri,
                })
            }
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

/// In production the callback goes through the web app's `/api` proxy, so the
/// redirect URI must be on the frontend origin, over HTTPS; that keeps the
/// session cookie first-party and `Secure`. Plain HTTP is allowed only for
/// local development, where the API runs on its own port.
fn redirect_uri_fits(redirect_uri: &str, frontend_origin: &str) -> bool {
    let local =
        |url: &str| url.starts_with("http://localhost:") || url.starts_with("http://127.0.0.1:");
    if redirect_uri.starts_with("https://") {
        redirect_uri.starts_with(&format!("{frontend_origin}/"))
    } else {
        local(redirect_uri) && local(frontend_origin)
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
            ("FRONTEND_ORIGIN", "https://daghep.vn"),
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
            ("FRONTEND_ORIGIN", "https://daghep.vn"),
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

    fn google_vars(redirect_uri: &'static str) -> [(&'static str, &'static str); 3] {
        [
            ("GOOGLE_CLIENT_ID", "id"),
            ("GOOGLE_CLIENT_SECRET", "shh"),
            ("GOOGLE_REDIRECT_URI", redirect_uri),
        ]
    }

    #[test]
    fn google_sign_in_needs_an_explicit_frontend_origin() {
        let result = config(
            &[
                &[DB][..],
                &google_vars("https://daghep.vn/api/auth/google/callback"),
            ]
            .concat(),
        );

        assert!(matches!(
            result,
            Err(ConfigError::Missing("FRONTEND_ORIGIN"))
        ));
    }

    #[test]
    fn production_redirect_uri_must_live_on_the_frontend_origin() {
        let ok = [
            (
                "https://daghep.vn",
                "https://daghep.vn/api/auth/google/callback",
            ),
            (
                "http://localhost:3000",
                "http://localhost:8080/api/auth/google/callback",
            ),
        ];
        for (origin, redirect) in ok {
            let vars = [
                &[DB, ("FRONTEND_ORIGIN", origin)][..],
                &google_vars(redirect),
            ]
            .concat();
            assert!(config(&vars).is_ok(), "{origin} {redirect}");
        }
        let bad = [
            // Forgot to switch the origin to production: cookies would not be Secure.
            (
                "http://localhost:3000",
                "https://daghep.vn/api/auth/google/callback",
            ),
            (
                "https://daghep.vn",
                "https://api.example.run.app/api/auth/google/callback",
            ),
            (
                "https://daghep.vn",
                "https://daghep.vn.evil.example/api/auth/google/callback",
            ),
            // Plain HTTP only for local development.
            (
                "http://daghep.vn",
                "http://daghep.vn/api/auth/google/callback",
            ),
        ];
        for (origin, redirect) in bad {
            let vars = [
                &[DB, ("FRONTEND_ORIGIN", origin)][..],
                &google_vars(redirect),
            ]
            .concat();
            assert!(
                matches!(
                    config(&vars),
                    Err(ConfigError::Invalid {
                        name: "GOOGLE_REDIRECT_URI",
                        ..
                    })
                ),
                "{origin} {redirect}"
            );
        }
    }

    #[test]
    fn frontend_origin_must_be_a_bare_http_origin() {
        for bad in ["daghep.vn", "https://daghep.vn/"] {
            assert!(config(&[DB, ("FRONTEND_ORIGIN", bad)]).is_err(), "{bad}");
        }
    }
}
