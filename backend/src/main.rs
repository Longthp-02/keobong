//! Process entry point: configuration, logging, CORS and graceful shutdown.
//! Usage: `daghep-api` serves HTTP; `daghep-api migrate` applies migrations and exits.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderValue, Method};
use daghep_api::Deps;
use daghep_api::auth::domain::IdentityProvider;
use daghep_api::auth::google::GoogleProvider;
use daghep_api::bot::ZaloSettings;
use daghep_api::bot::zalo::ZaloReply;
use daghep_api::clock::{Clock, SystemClock};
use daghep_api::config::Config;
use daghep_api::drafts::anthropic::AnthropicReader;
use daghep_api::drafts::domain::MatchTextReader;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "fatal error");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    // Lazy pool: no connection is opened until the first request, which keeps
    // scale-to-zero cold starts fast.
    // Acquire timeout: long enough for a scaled-to-zero Neon database to wake
    // (usually under a second, sometimes a few), short enough to fail with a
    // 500 instead of hanging when the database is unreachable.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&config.database_url)?;

    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("migrate") => {
            daghep_api::MIGRATOR.run(&pool).await?;
            tracing::info!("migrations applied");
            return Ok(());
        }
        Some(other) => {
            return Err(format!("unknown command `{other}`; expected `migrate` or none").into());
        }
    }

    tracing::info!(
        frontend_origin = %config.frontend_origin,
        google_sign_in = config.google.is_some(),
        match_drafts = config.anthropic.is_some(),
        zalo_bot = config.zalo_bot.is_some(),
        "configuration loaded"
    );

    let cors = CorsLayer::new()
        .allow_origin(config.frontend_origin.parse::<HeaderValue>()?)
        .allow_methods([Method::GET]);

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let google = match config.google {
        Some(google) => {
            let provider: Arc<dyn IdentityProvider> =
                Arc::new(GoogleProvider::new(google, clock.clone())?);
            Some(provider)
        }
        None => None,
    };
    let reader = match config.anthropic {
        Some(anthropic) => {
            let reader: Arc<dyn MatchTextReader> =
                Arc::new(AnthropicReader::new(anthropic.api_key, anthropic.model)?);
            Some(reader)
        }
        None => None,
    };
    // A broken copy file stops the server here, not on the first message.
    daghep_api::bot::domain::check_copy()?;
    let zalo = match config.zalo_bot {
        Some(bot) => Some(Arc::new(ZaloSettings {
            reply: Arc::new(ZaloReply::new(bot.token)?),
            webhook_secret: bot.webhook_secret,
            bot_name: bot.bot_name,
        })),
        None => None,
    };
    let app = daghep_api::app(Deps {
        pool,
        clock,
        google,
        frontend_origin: config.frontend_origin,
        reader,
        zalo,
    })
    .layer(cors);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await?;
    tracing::info!(port = config.port, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to listen for ctrl-c");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => tracing::error!(error = %err, "failed to listen for SIGTERM"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
