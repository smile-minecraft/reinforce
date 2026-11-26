pub mod auth;
pub mod config;
pub mod demo;
pub mod embed;
pub mod error;
pub mod guard;
pub mod routes;
pub mod state;

use axum::Router;
use clap::Parser;
use config::Cli;
use error::ApiResult;
use state::AppState;

/// The complete application: every route behind the origin guard.
pub fn app(state: AppState) -> Router {
    routes::router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            guard::check_origin,
        ))
        .with_state(state)
}

pub async fn run() -> ApiResult<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config::Command::Serve(config) = cli.command;
    let host = config.validate()?;
    let state = AppState::open(&config).await?;
    if config.demo {
        demo::seed(&state).await?;
    }
    let listener = tokio::net::TcpListener::bind((host, config.port))
        .await
        .map_err(|error| error::ApiError::internal(format!("cannot bind server: {error}")))?;
    tracing::info!(
        "reinforce {} listening on http://{}:{} (auth: {})",
        reinforce_core::VERSION,
        host,
        config.port,
        config.auth.as_str()
    );
    axum::serve(listener, app(state))
        .await
        .map_err(|error| error::ApiError::internal(format!("server failed: {error}")))
}
