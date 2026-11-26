use std::net::IpAddr;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AuthMode {
    Single,
    Invite,
}

impl AuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Invite => "invite",
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "reinforce", version = reinforce_core::VERSION)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve(ServeArgs),
    Mcp(McpArgs),
    Check(CheckArgs),
    Import(ImportArgs),
}

#[derive(Debug, Clone, Args)]
pub struct McpArgs {
    #[arg(long, default_value = "./reinforce.db")]
    pub db: std::path::PathBuf,
    #[arg(long, default_value = "./blobs")]
    pub blobs: std::path::PathBuf,
    #[arg(long, default_value = "reinforce-agent")]
    pub agent_name: String,
}

#[derive(Debug, Clone, Args)]
pub struct CheckArgs {
    #[arg(long, default_value = "./reinforce.db")]
    pub db: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImportMode {
    Staging,
    Owner,
}

#[derive(Debug, Clone, Args)]
pub struct ImportArgs {
    /// A bundle.jsonl file or a directory containing bundle.jsonl and assets/.
    pub bundle: std::path::PathBuf,
    #[arg(long, default_value = "./reinforce.db")]
    pub db: std::path::PathBuf,
    #[arg(long, default_value = "./blobs")]
    pub blobs: std::path::PathBuf,
    #[arg(long, value_enum, default_value_t = ImportMode::Staging)]
    pub mode: ImportMode,
}

#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
    #[arg(long, default_value = "./reinforce.db")]
    pub db: std::path::PathBuf,
    #[arg(long, default_value = "./blobs")]
    pub blobs: std::path::PathBuf,
    #[arg(long, value_enum, default_value_t = AuthMode::Single)]
    pub auth: AuthMode,
    #[arg(long)]
    pub demo: bool,
}

impl ServeArgs {
    pub fn validate(&self) -> ApiResult<IpAddr> {
        let host: IpAddr = self
            .host
            .parse()
            .map_err(|_| ApiError::bad_request("host", "not a valid IP address"))?;
        if self.auth == AuthMode::Single && !host.is_loopback() {
            tracing::warn!(
                "single-auth mode has no login wall; binding to {} exposes all data to the network",
                host
            );
        }
        Ok(host)
    }
}
