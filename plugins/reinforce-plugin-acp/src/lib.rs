//! `reinforce-plugin-acp`: official Reinforce plugin that runs an external
//! Agent Client Protocol (ACP) agent over stdio and converts its structured
//! card proposals into `reinforce_plugin` protocol v1 operations.

pub mod input;
pub mod proposal;
pub mod protocol;

use std::path::PathBuf;

use clap::Parser;
use tokio::io::{AsyncBufRead, AsyncWrite};

use crate::input::ResolvedJobInput;
use crate::protocol::{HostWriter, read_hello, spawn_cancel_listener};

/// Command-line configuration supplied in the owner's plugin manifest.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "reinforce-plugin-acp",
    version,
    about = "Reinforce stdio plugin bridging an external Agent Client Protocol (ACP) agent"
)]
pub struct Cli {
    /// Absolute path to the ACP agent executable.
    #[arg(long, value_name = "PATH")]
    pub agent: PathBuf,

    /// Working directory passed to ACP `session/new` (defaults to current working directory).
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Optional directory confining `input.resource_path` reads; disabled when omitted.
    #[arg(long, value_name = "DIR")]
    pub resource_dir: Option<PathBuf>,

    /// Optional capability names advertised in `ready`; defaults to the requested capability.
    #[arg(long = "capability", value_name = "NAME")]
    pub capabilities: Vec<String>,

    /// Timeout in seconds for each ACP request (`initialize`, `session/new`, `session/prompt`).
    #[arg(long, default_value_t = 240, value_name = "SECS")]
    pub request_timeout_secs: u64,

    /// Arguments forwarded verbatim to the ACP agent after `--`.
    #[arg(last = true, value_name = "ARG")]
    pub agent_args: Vec<String>,
}

/// Runs the plugin entrypoint using process stdin/stdout.
pub async fn run() -> std::process::ExitCode {
    let cli = Cli::parse();
    let reader = protocol::stdin_reader();
    let stdout = tokio::io::stdout();
    run_with_io(cli, reader, stdout).await
}

/// Runs one plugin job over the supplied host streams.
pub async fn run_with_io<R, W>(cli: Cli, mut reader: R, writer: W) -> std::process::ExitCode
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let hello = match read_hello(&mut reader).await {
        Ok(hello) => hello,
        Err(error) => {
            eprintln!("reinforce-plugin-acp handshake failed: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let host = HostWriter::new(writer, hello.job_id.clone());
    let capabilities = if cli.capabilities.is_empty() {
        vec![hello.capability.clone()]
    } else {
        cli.capabilities.clone()
    };

    if let Err(error) = host.send_ready(hello.plugin.clone(), capabilities).await {
        eprintln!("reinforce-plugin-acp failed to send ready: {error}");
        return std::process::ExitCode::FAILURE;
    }

    let (cancel_rx, cancel_task) = spawn_cancel_listener(reader, hello.job_id.clone());

    if !cli.agent.is_absolute() {
        let _ = host
            .send_error(
                "invalid_agent_path",
                format!(
                    "--agent must be an absolute path, got {}",
                    cli.agent.display()
                ),
            )
            .await;
        cancel_task.abort();
        return std::process::ExitCode::SUCCESS;
    }

    let _job_input = match ResolvedJobInput::parse(hello.input, cli.resource_dir.as_deref()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = host.send_error(error.code, error.message).await;
            cancel_task.abort();
            return std::process::ExitCode::SUCCESS;
        }
    };

    let _ = cancel_rx;
    let _ = host
        .send_error(
            "acp_not_implemented",
            "ACP client execution is not wired yet",
        )
        .await;
    cancel_task.abort();
    std::process::ExitCode::SUCCESS
}
