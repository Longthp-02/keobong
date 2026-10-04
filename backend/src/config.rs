//! Runtime configuration from environment variables. Secrets never have defaults.

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// Origin allowed to call the API from a browser (the frontend URL).
    pub cors_allowed_origin: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value}")]
    Invalid { name: &'static str, value: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let port = match env::var("PORT") {
            Ok(value) => value.parse().map_err(|_| ConfigError::Invalid {
                name: "PORT",
                value,
            })?,
            Err(_) => 8080,
        };
        let cors_allowed_origin =
            env::var("CORS_ALLOWED_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".to_owned());
        Ok(Self {
            database_url,
            port,
            cors_allowed_origin,
        })
    }
}
