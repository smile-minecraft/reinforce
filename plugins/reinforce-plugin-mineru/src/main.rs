//! MinerU plugin: one stdio job turns a PDF into a pending proposal batch.

mod convert;
mod protocol;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;

use clap::Parser;
use reinforce_plugin_protocol::{Hello, HostMessage, Operation};

const CAPABILITIES: &[&str] = &["ingest_pdf"];
/// MinerU's `--json` summary is small; anything larger is not a summary.
const MAX_SUMMARY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "reinforce-plugin-mineru",
    about = "Reinforce stdio plugin that parses PDFs with MinerU"
)]
struct Args {
    /// Absolute path to the MinerU executable.
    #[arg(long)]
    mineru: PathBuf,

    /// Absolute path to a writable directory for per-job scratch files.
    #[arg(long)]
    work_dir: Option<PathBuf>,

    /// MinerU parse tier. `flash` is text-only and needs no local engine.
    #[arg(long, default_value = "flash")]
    tier: String,

    /// Maximum seconds MinerU may spend on one document.
    #[arg(long, default_value_t = 180)]
    wait: u64,
}

#[derive(Debug)]
pub struct Options {
    pub tool: PathBuf,
    pub work_dir: PathBuf,
    pub tier: String,
    pub wait: u64,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();

fn absolute(path: &Path, flag: &str) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Err(format!(
            "{flag} must be an absolute path: {}",
            path.display()
        ))
    }
}

fn work(
    hello: &Hello,
    cancel: &Receiver<HostMessage>,
) -> Result<Vec<Operation>, protocol::RunError> {
    let input = convert::parse_input(&hello.input)?;
    let options = OPTIONS.get().expect("options are set before serving");
    let scratch = protocol::Scratch::new(&options.work_dir, "mineru").map_err(|error| {
        protocol::failure(
            "io_error",
            format!(
                "cannot create a work directory in {}: {error}",
                options.work_dir.display()
            ),
        )
    })?;
    let output = scratch.path().join("content.md");
    let arguments = convert::tool_arguments(options, &input.pdf_path, &output);
    let result = protocol::run_command(
        &options.tool,
        &arguments,
        scratch.path(),
        protocol::Keep::Head(MAX_SUMMARY_BYTES),
        cancel,
    )?;
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).map_err(|error| {
        protocol::failure(
            "unparseable_output",
            format!("mineru did not print a JSON summary: {error}"),
        )
    })?;
    let markdown = std::fs::read_to_string(&output).map_err(|error| {
        protocol::failure(
            "unparseable_output",
            format!("mineru did not write {}: {error}", output.display()),
        )
    })?;
    convert::operations(&input, &report, &markdown)
}

fn main() {
    let args = Args::parse();
    let tool = match absolute(&args.mineru, "--mineru") {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let work_dir = match absolute(
        &args.work_dir.unwrap_or_else(std::env::temp_dir),
        "--work-dir",
    ) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if !matches!(
        args.tier.as_str(),
        "flash" | "basic" | "standard" | "advanced"
    ) {
        eprintln!("--tier must be flash, basic, standard or advanced");
        std::process::exit(2);
    }
    OPTIONS
        .set(Options {
            tool,
            work_dir,
            tier: args.tier,
            wait: args.wait,
        })
        .expect("options are set once");
    std::process::exit(protocol::run(CAPABILITIES, work));
}
