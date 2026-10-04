//! Process entry point: configuration, logging, CORS and graceful shutdown.
//! Usage: `daghep-api` serves HTTP; `daghep-api migrate` applies migrations and exits.

use std::process::ExitCode;
use std::time::Duration;

use axum::http::{HeaderValue, Method};
use daghep_api::config::Config;
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
    // Short acquire timeout: fail fast with a 500 instead of hanging while the
    // database is unreachable or still waking up.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
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

    tracing::info!(cors_allowed_origin = %config.cors_allowed_origin, "configuration loaded");

    let cors = CorsLayer::new()
        .allow_origin(config.cors_allowed_origin.parse::<HeaderValue>()?)
        .allow_methods([Method::GET]);

    let app = daghep_api::app(pool).layer(cors);
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
