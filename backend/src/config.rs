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

/// Anthropic API settings for reading pasted match messages.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: Secret,
    pub model: String,
}

/// Zalo Bot Platform settings ("Bot DaGhep").
#[derive(Debug, Clone)]
pub struct ZaloBotConfig {
    pub token: Secret,
    /// Sent by Zalo in `X-Bot-Api-Secret-Token`; set with the `setWebhook` API.
    pub webhook_secret: Secret,
    /// The bot's display name, as tagged in groups.
    pub bot_name: String,
    /// Sent by the scheduler in `X-Cron-Secret` to post due reminders;
    /// `BOT_CRON_SECRET`, at least 16 characters. `None` keeps reminders unposted.
    pub cron_secret: Option<Secret>,
}

pub const DEFAULT_ZALO_BOT_NAME: &str = "Bot DaGhep";

/// A small, inexpensive model is enough to read a few lines of text.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-haiku-4-5";

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// The web app's origin: allowed for CORS and same-origin checks, and the
    /// base for redirects after sign-in. HTTPS origins get `Secure` cookies.
    pub frontend_origin: String,
    /// `None` disables Google sign-in (sign-in endpoints answer 503).
    pub google: Option<GoogleConfig>,
    /// `None` disables match drafts (`POST /api/match-drafts` answers 503).
    pub anthropic: Option<AnthropicConfig>,
    /// `None` disables the Zalo bot.
    pub zalo_bot: Option<ZaloBotConfig>,
    /// Đá Ghép user ids that approve venues (`ADMIN_USER_IDS`, comma separated).
    pub admin_user_ids: Vec<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value}")]
    Invalid { name: &'static str, value: String },
    #[error("set all of GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET and GOOGLE_REDIRECT_URI, or none")]
    PartialGoogle,
    #[error(
        "set both ZALO_BOT_TOKEN and ZALO_BOT_WEBHOOK_SECRET (at least 16 characters), or neither"
    )]
    PartialZaloBot,
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
        let anthropic = get("ANTHROPIC_API_KEY").map(|api_key| AnthropicConfig {
            api_key: Secret::new(api_key),
            model: get("ANTHROPIC_MODEL").unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_owned()),
        });
        let cron_secret = match get("BOT_CRON_SECRET") {
            Some(secret) if secret.chars().count() >= 16 => Some(Secret::new(secret)),
            Some(_) => {
                return Err(ConfigError::Invalid {
                    name: "BOT_CRON_SECRET",
                    value: "(shorter than 16 characters)".to_owned(),
                });
            }
            None => None,
        };
        let zalo_bot = match (get("ZALO_BOT_TOKEN"), get("ZALO_BOT_WEBHOOK_SECRET")) {
            (Some(token), Some(secret)) if secret.chars().count() >= 16 => Some(ZaloBotConfig {
                token: Secret::new(token),
                webhook_secret: Secret::new(secret),
                bot_name: get("ZALO_BOT_NAME").unwrap_or_else(|| DEFAULT_ZALO_BOT_NAME.to_owned()),
                cron_secret,
            }),
            (None, None) => None,
            _ => return Err(ConfigError::PartialZaloBot),
        };
        let admin_user_ids = match get("ADMIN_USER_IDS") {
            Some(raw) => raw
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(|id| {
                    id.parse::<i64>().map_err(|_| ConfigError::Invalid {
                        name: "ADMIN_USER_IDS",
                        value: raw.clone(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        Ok(Self {
            database_url,
            port,
            frontend_origin,
            google,
            anthropic,
            zalo_bot,
            admin_user_ids,
        })
    }
}

/// In production the callback goes through the web app's `/api` proxy, so the
/// redirect URI must be on the frontend origin, over HTTPS; that keeps the
/// session cookie first-party and `Secure`. Plain HTTP is allowed only for
/// local development, where the API runs on its own port.
fn redirect_uri_fits(redirect_uri: &str, frontend_origin: &str) -> bool {
    let (Ok(redirect), Ok(frontend)) = (
        reqwest::Url::parse(redirect_uri),
        reqwest::Url::parse(frontend_origin),
    ) else {
        return false;
    };
    let local = |url: &reqwest::Url| {
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
    };
    let plain = redirect.username().is_empty() && redirect.password().is_none();
    match redirect.scheme() {
        "https" => plain && redirect.origin() == frontend.origin(),
        _ => plain && local(&redirect) && local(&frontend),
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
    fn match_drafts_need_an_anthropic_key_and_default_to_a_small_model() {
        assert!(config(&[DB]).unwrap().anthropic.is_none());

        let with_key = config(&[DB, ("ANTHROPIC_API_KEY", "sk-ant-secret")]).unwrap();
        let anthropic = with_key.anthropic.clone().unwrap();
        assert_eq!(anthropic.api_key.expose(), "sk-ant-secret");
        assert_eq!(anthropic.model, DEFAULT_ANTHROPIC_MODEL);
        assert!(!format!("{with_key:?}").contains("sk-ant-secret"));

        let other_model = config(&[
            DB,
            ("ANTHROPIC_API_KEY", "k"),
            ("ANTHROPIC_MODEL", "claude-other"),
        ])
        .unwrap();
        assert_eq!(other_model.anthropic.unwrap().model, "claude-other");
    }

    #[test]
    fn the_zalo_bot_needs_a_token_and_a_long_webhook_secret() {
        assert!(config(&[DB]).unwrap().zalo_bot.is_none());

        let bot = config(&[
            DB,
            ("ZALO_BOT_TOKEN", "123:token-value"),
            ("ZALO_BOT_WEBHOOK_SECRET", "a-long-webhook-secret"),
        ])
        .unwrap();
        let zalo = bot.zalo_bot.clone().unwrap();
        assert_eq!(zalo.bot_name, DEFAULT_ZALO_BOT_NAME);
        assert_eq!(zalo.token.expose(), "123:token-value");
        assert!(!format!("{bot:?}").contains("token-value"));
        assert!(!format!("{bot:?}").contains("a-long-webhook-secret"));

        let short = config(&[
            DB,
            ("ZALO_BOT_TOKEN", "t"),
            ("ZALO_BOT_WEBHOOK_SECRET", "short"),
        ]);
        assert!(matches!(short, Err(ConfigError::PartialZaloBot)));
        let partial = config(&[DB, ("ZALO_BOT_TOKEN", "t")]);
        assert!(matches!(partial, Err(ConfigError::PartialZaloBot)));
    }

    #[test]
    fn admins_are_a_comma_separated_list_of_user_ids() {
        assert!(config(&[DB]).unwrap().admin_user_ids.is_empty());
        assert_eq!(
            config(&[DB, ("ADMIN_USER_IDS", "1, 42")])
                .unwrap()
                .admin_user_ids,
            vec![1, 42]
        );
        assert!(matches!(
            config(&[DB, ("ADMIN_USER_IDS", "1,long")]),
            Err(ConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn the_reminder_scheduler_needs_a_long_secret() {
        let bot = |extra: &[(&'static str, &'static str)]| {
            let mut vars = vec![
                DB,
                ("ZALO_BOT_TOKEN", "123:token-value"),
                ("ZALO_BOT_WEBHOOK_SECRET", "a-long-webhook-secret"),
            ];
            vars.extend_from_slice(extra);
            config(&vars)
        };
        assert!(bot(&[]).unwrap().zalo_bot.unwrap().cron_secret.is_none());
        let with = bot(&[("BOT_CRON_SECRET", "a-long-cron-secret-1")]).unwrap();
        assert!(!format!("{with:?}").contains("a-long-cron-secret-1"));
        assert_eq!(
            with.zalo_bot.unwrap().cron_secret.unwrap().expose(),
            "a-long-cron-secret-1"
        );
        let short = bot(&[("BOT_CRON_SECRET", "short")]);
        assert!(matches!(short, Err(ConfigError::Invalid { .. })));
        assert!(!format!("{short:?}").contains("short\""));
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
            // Userinfo trick: the host is evil.example, not localhost.
            (
                "http://localhost:3000",
                "http://localhost:8080@evil.example/callback",
            ),
            ("http://localhost:3000", "not a url"),
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
