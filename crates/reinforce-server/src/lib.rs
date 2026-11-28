pub mod auth;
pub mod bundle;
pub mod cli;
pub mod config;
pub mod demo;
pub mod embed;
pub mod error;
pub mod export;
pub mod export_scope;
pub mod guard;
pub mod mcp;
pub mod plugins;
pub mod routes;
pub mod state;
pub mod sync_content;

use axum::Router;
use clap::Parser;
use config::Cli;
use error::ApiResult;
use state::AppState;

/// The complete application: every route behind the origin guard.
pub fn app(state: AppState) -> Router {
    routes::router()
        .merge(mcp::http_router(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            guard::check_origin,
        ))
        .with_state(state)
}

pub async fn run() -> ApiResult<()> {
    let cli = Cli::parse();
    init_tracing();
    match cli.command {
        config::Command::Serve(config) => serve(config).await,
        config::Command::Mcp(args) => mcp::run_stdio(args).await,
        config::Command::Check(args) => cli::check(args).await,
        config::Command::Import(args) => cli::import(args).await,
        config::Command::Export(args) => cli::export(args).await,
        config::Command::Plugin(args) => cli::plugin(args).await,
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        // stdout belongs to command output: JSON-RPC for `mcp`, reports for the CLI.
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
}

async fn serve(config: config::ServeArgs) -> ApiResult<()> {
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
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| error::ApiError::internal(format!("server failed: {error}")))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!("failed to install Ctrl+C handler: {error}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!("failed to install SIGTERM handler: {error}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {
            tracing::info!("received SIGINT, shutting down gracefully");
        }
        () = terminate => {
            tracing::info!("received SIGTERM, shutting down gracefully");
        }
    }
}
