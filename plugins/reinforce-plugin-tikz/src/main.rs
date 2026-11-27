//! TikZ plugin: one stdio job renders TikZ source through LaTeX and dvisvgm.

mod convert;
mod protocol;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;

use clap::Parser;
use reinforce_plugin_protocol::{Hello, HostMessage, Operation};

const CAPABILITIES: &[&str] = &["tikz_render"];

#[derive(Debug, Parser)]
#[command(
    name = "reinforce-plugin-tikz",
    about = "Reinforce stdio plugin that renders TikZ with LaTeX and dvisvgm"
)]
struct Args {
    /// Absolute path to the LaTeX executable.
    #[arg(long)]
    latex: PathBuf,

    /// Absolute path to the dvisvgm executable.
    #[arg(long)]
    dvisvgm: PathBuf,

    /// Absolute path to a writable directory for per-job scratch files.
    #[arg(long)]
    work_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Options {
    pub latex: PathBuf,
    pub dvisvgm: PathBuf,
    pub work_dir: PathBuf,
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
    let scratch = protocol::Scratch::new(&options.work_dir, "tikz").map_err(|error| {
        protocol::failure(
            "io_error",
            format!(
                "cannot create a work directory in {}: {error}",
                options.work_dir.display()
            ),
        )
    })?;
    let tex = scratch.path().join("diagram.tex");
    let dvi = scratch.path().join("diagram.dvi");
    let svg = scratch.path().join("diagram.svg");
    std::fs::write(&tex, convert::tex_source(&input)).map_err(|error| {
        protocol::failure(
            "io_error",
            format!("cannot write {}: {error}", tex.display()),
        )
    })?;

    let latex_arguments = convert::latex_arguments(&tex, scratch.path());
    protocol::run_command(&options.latex, &latex_arguments, cancel)?;
    if !dvi.is_file() {
        return Err(protocol::failure(
            "unparseable_output",
            format!("LaTeX did not produce {}", dvi.display()),
        ));
    }

    let dvisvgm_arguments = convert::dvisvgm_arguments(&dvi, &svg);
    protocol::run_command(&options.dvisvgm, &dvisvgm_arguments, cancel)?;
    let svg_bytes = std::fs::read(&svg).map_err(|error| {
        protocol::failure(
            "unparseable_output",
            format!("dvisvgm did not write {}: {error}", svg.display()),
        )
    })?;
    convert::operations(&input, &svg_bytes)
}

fn main() {
    let args = Args::parse();
    let latex = match absolute(&args.latex, "--latex") {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let dvisvgm = match absolute(&args.dvisvgm, "--dvisvgm") {
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
    OPTIONS
        .set(Options {
            latex,
            dvisvgm,
            work_dir,
        })
        .expect("options are set once");
    std::process::exit(protocol::run(CAPABILITIES, work));
}
