pub mod auth;
pub mod config;
pub mod demo;
pub mod embed;
pub mod error;
pub mod routes;
pub mod state;

use axum::Router;
use clap::Parser;
use config::Cli;
use error::ApiResult;
use state::AppState;

pub fn app() -> Router<AppState> {
    routes::router()
}

pub async fn run() -> ApiResult<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = match cli.command {
        config::Command::Serve(args) => args,
    };
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
    axum::serve(listener, app().with_state(state))
        .await
        .map_err(|error| error::ApiError::internal(format!("server failed: {error}")))
}
