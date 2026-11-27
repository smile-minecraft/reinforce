//! `reinforce-plugin-acp`: official Reinforce plugin that runs an external
//! Agent Client Protocol (ACP) agent over stdio and converts its structured
//! card proposals into `reinforce_plugin` protocol v1 operations.

pub mod acp;
pub mod input;
pub mod proposal;
pub mod protocol;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use tokio::io::{AsyncBufRead, AsyncWrite};

use crate::acp::{AcpConfig, AcpOutcome};
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

    let exit_code = execute_job(&cli, &hello, &host, cancel_rx).await;
    cancel_task.abort();
    exit_code
}

async fn execute_job<W>(
    cli: &Cli,
    hello: &reinforce_plugin_protocol::Hello,
    host: &HostWriter<W>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> std::process::ExitCode
where
    W: AsyncWrite + Unpin + Send + 'static,
{
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
        return std::process::ExitCode::SUCCESS;
    }
    if cli.request_timeout_secs == 0 {
        let _ = host
            .send_error(
                "invalid_config",
                "--request-timeout-secs must be at least 1",
            )
            .await;
        return std::process::ExitCode::SUCCESS;
    }

    let cwd = match resolve_cwd(cli.cwd.as_deref()) {
        Ok(cwd) => cwd,
        Err(message) => {
            let _ = host.send_error("invalid_cwd", message).await;
            return std::process::ExitCode::SUCCESS;
        }
    };

    let job_input = match ResolvedJobInput::parse(hello.input.clone(), cli.resource_dir.as_deref())
    {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = host.send_error(error.code, error.message).await;
            return std::process::ExitCode::SUCCESS;
        }
    };

    let prompt_text = proposal::build_prompt(&job_input, &hello.capability);
    let acp_config = AcpConfig {
        agent: cli.agent.clone(),
        agent_args: cli.agent_args.clone(),
        cwd,
        request_timeout: Duration::from_secs(cli.request_timeout_secs),
    };

    let progress_host = host.clone();
    let outcome = acp::run_prompt(
        &acp_config,
        &prompt_text,
        cancel_rx,
        move |fraction, msg| {
            let progress_host = progress_host.clone();
            async move {
                let _ = progress_host.send_progress(fraction, msg).await;
            }
        },
    )
    .await;

    let assistant_text = match outcome {
        Ok(AcpOutcome::Completed(text)) => text,
        Ok(AcpOutcome::Cancelled) => return std::process::ExitCode::SUCCESS,
        Err(error) => {
            let _ = host.send_error(error.code, error.message).await;
            return std::process::ExitCode::SUCCESS;
        }
    };

    match proposal::parse_and_convert(&assistant_text, &job_input) {
        Ok(operations) => {
            let _ = host.send_progress(1.0, "proposal ready").await;
            if let Err(error) = host.send_result(operations).await {
                eprintln!("reinforce-plugin-acp failed to send result: {error}");
                return std::process::ExitCode::FAILURE;
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = host.send_error(error.code, error.message).await;
            std::process::ExitCode::SUCCESS
        }
    }
}

fn resolve_cwd(explicit: Option<&std::path::Path>) -> Result<PathBuf, String> {
    let dir = match explicit {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|error| format!("cannot determine current working directory: {error}"))?,
    };
    if !dir.is_absolute() {
        return Err(format!(
            "--cwd must be an absolute path, got {}",
            dir.display()
        ));
    }
    let metadata = std::fs::metadata(&dir)
        .map_err(|error| format!("cannot access working directory {}: {error}", dir.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "working directory {} is not a directory",
            dir.display()
        ));
    }
    Ok(dir)
}
