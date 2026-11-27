use std::net::IpAddr;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

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
    Export(ExportArgs),
    Plugin(PluginArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    Pdf,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportPreset {
    Standard,
    #[value(name = "goodnotes")]
    GoodNotes,
    Worksheet,
}

#[derive(Debug, Clone, Args)]
#[command(group(
    ArgGroup::new("scope")
        .required(true)
        .args(["note", "subject", "weakness_threshold"]),
))]
pub struct ExportArgs {
    #[arg(long, value_enum)]
    pub format: ExportFormat,
    #[arg(long, value_enum)]
    pub preset: Option<ExportPreset>,
    #[arg(long)]
    pub out: std::path::PathBuf,
    #[arg(long, group = "scope")]
    pub note: Option<String>,
    #[arg(long, group = "scope")]
    pub subject: Option<String>,
    #[arg(long, group = "scope")]
    pub weakness_threshold: Option<f64>,
    #[arg(long, default_value = "./reinforce.db")]
    pub db: std::path::PathBuf,
    #[arg(long, default_value = "./blobs")]
    pub blobs: std::path::PathBuf,
    #[arg(long, default_value = "./fonts")]
    pub fonts: std::path::PathBuf,
    #[arg(long)]
    pub web_base_url: Option<String>,
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
    #[arg(long, default_value = "./fonts")]
    pub fonts: std::path::PathBuf,
    #[arg(long, value_enum, default_value_t = AuthMode::Single)]
    pub auth: AuthMode,
    /// Owner-written plugin manifest file. Only this file can declare which
    /// external programs the server may run.
    #[arg(long)]
    pub plugins: Option<std::path::PathBuf>,
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

/// Owner-facing plugin commands. They operate on the same manifest file and
/// database the server uses, so a job queued here is picked up by a running
/// server within one poll interval.
#[derive(Debug, Clone, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub command: PluginCommand,
    #[arg(long, default_value = "./reinforce.db", global = true)]
    pub db: std::path::PathBuf,
    /// Owner-written plugin manifest file.
    #[arg(long, global = true)]
    pub plugins: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PluginCommand {
    /// List the plugins declared in the manifest.
    List,
    /// Queue one job for a configured plugin.
    Submit(PluginSubmitArgs),
    /// Read the status of one job, or list recent jobs.
    Status(PluginStatusArgs),
}

#[derive(Debug, Clone, Args)]
pub struct PluginSubmitArgs {
    #[arg(long)]
    pub plugin: String,
    #[arg(long)]
    pub capability: String,
    /// JSON object with the job input, `@path` to read a file, or `-` for stdin.
    #[arg(long)]
    pub input: String,
}

#[derive(Debug, Clone, Args)]
pub struct PluginStatusArgs {
    /// Job id to inspect. Without it, recent jobs are listed.
    #[arg(long)]
    pub job: Option<String>,
    #[arg(long)]
    pub plugin: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Wait up to this many seconds for the job to reach a terminal state.
    #[arg(long, default_value_t = 0)]
    pub wait: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_accepts_spec_preset_names() {
        for (name, expected) in [
            ("standard", ExportPreset::Standard),
            ("goodnotes", ExportPreset::GoodNotes),
            ("worksheet", ExportPreset::Worksheet),
        ] {
            let cli = Cli::try_parse_from([
                "reinforce",
                "export",
                "--format",
                "pdf",
                "--preset",
                name,
                "--subject",
                "physics",
                "--out",
                "out.pdf",
            ])
            .unwrap_or_else(|error| panic!("preset {name} should parse: {error}"));
            let Command::Export(args) = cli.command else {
                panic!("expected export command");
            };
            assert_eq!(args.preset, Some(expected));
        }
    }

    #[test]
    fn cli_rejects_unknown_preset_names() {
        let result = Cli::try_parse_from([
            "reinforce",
            "export",
            "--format",
            "pdf",
            "--preset",
            "good-notes",
            "--subject",
            "physics",
            "--out",
            "out.pdf",
        ]);
        assert!(result.is_err());
    }
}
