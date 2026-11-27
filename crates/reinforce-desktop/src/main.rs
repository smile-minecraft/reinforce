//! Command line entry point for the GPUI desktop client.
//!
//! The binary deliberately stays small: it parses a handful of flags, opens the
//! store (optionally importing a bundle first) and hands the window over to
//! [`reinforce_desktop::app`].

use std::path::PathBuf;
use std::process::ExitCode;

use reinforce_desktop::app::{self, LaunchOptions, StartScreen};
use reinforce_desktop::{PracticalCountdown, Renderer, Store};

const USAGE: &str = "\
reinforce-desktop — native GPUI reader and quiz runner for a local Reinforce database

USAGE:
    reinforce-desktop [OPTIONS]

OPTIONS:
    --db <PATH>               Database file (default: ~/Library/Application Support/Reinforce/reinforce.db)
    --blobs <DIR>             Blob directory (default: <db folder>/blobs)
    --cache <DIR>             Pre-rendered SVG cache (default: <db folder>/desktop-cache)
    --import <PATH>           Import a bundle.jsonl file or bundle directory, then open it
    --practical-exam <DATE>   Practical exam date as YYYY-MM-DD, shown as a countdown
    --start <SCREEN>          reading (default), exam, daily, topic, practical, weakness
    --select <KEYS>           Preselect option keys on the first question, e.g. A or A,C
    -h, --help                Print this help
";

const PRACTICAL_SETTING: &str = "desktop.practical_exam_at";

#[derive(Debug, Clone)]
struct Args {
    db: PathBuf,
    blobs: Option<PathBuf>,
    cache: Option<PathBuf>,
    import: Option<PathBuf>,
    practical_exam: Option<String>,
    start: StartScreen,
    preselect: Vec<String>,
}

impl Args {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = Self {
            db: default_database_path(),
            blobs: None,
            cache: None,
            import: None,
            practical_exam: None,
            start: StartScreen::Reading,
            preselect: Vec::new(),
        };
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let mut value = |name: &str| -> Result<String, String> {
                arguments
                    .next()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match argument.as_str() {
                "--db" => args.db = PathBuf::from(value("--db")?),
                "--blobs" => args.blobs = Some(PathBuf::from(value("--blobs")?)),
                "--cache" => args.cache = Some(PathBuf::from(value("--cache")?)),
                "--import" => args.import = Some(PathBuf::from(value("--import")?)),
                "--practical-exam" => args.practical_exam = Some(value("--practical-exam")?),
                "--start" => {
                    let name = value("--start")?;
                    args.start = StartScreen::parse(&name)
                        .ok_or_else(|| format!("unknown screen {name}"))?;
                }
                "--select" => {
                    let keys = value("--select")?;
                    args.preselect = keys
                        .split(',')
                        .map(|key| key.trim().to_ascii_uppercase())
                        .filter(|key| !key.is_empty())
                        .collect();
                }
                "-h" | "--help" => return Err(USAGE.to_owned()),
                other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
            }
        }
        Ok(args)
    }
}

fn default_database_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home {
        Some(home) if cfg!(target_os = "macos") => home
            .join("Library/Application Support/Reinforce")
            .join("reinforce.db"),
        Some(home) => home.join(".local/share/reinforce/reinforce.db"),
        None => PathBuf::from("reinforce.db"),
    }
}

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            println!("{message}");
            return ExitCode::from(2);
        }
    };

    let mut store = match &args.blobs {
        Some(blobs) => Store::open_with_blobs(&args.db, blobs),
        None => Store::open(&args.db),
    }
    .unwrap_or_else(|error| {
        eprintln!("無法開啟資料庫 {}：{error}", args.db.display());
        std::process::exit(1);
    });

    if let Some(bundle) = &args.import {
        match store.import_bundle(bundle) {
            Ok(summary) => println!(
                "已匯入 {} 筆操作、{} 個 blob 到 {}",
                summary.applied,
                summary.blobs_written,
                store.db_path().display()
            ),
            Err(error) => {
                eprintln!("匯入失敗：{error}");
                return ExitCode::from(1);
            }
        }
    }

    if let Some(date) = &args.practical_exam {
        if let Err(error) = PracticalCountdown::parse(date) {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
        if let Err(error) = store.set_setting(PRACTICAL_SETTING, date) {
            eprintln!("無法儲存實務考日期：{error}");
        }
    }
    let practical = args
        .practical_exam
        .clone()
        .or_else(|| store.setting(PRACTICAL_SETTING).ok().flatten())
        .and_then(|date| PracticalCountdown::parse(&date).ok());

    let cache = args
        .cache
        .clone()
        .unwrap_or_else(|| app::with_cache_dir(store.db_path()));
    let renderer = match Renderer::new(&cache) {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("無法建立渲染快取 {}：{error}", cache.display());
            return ExitCode::from(1);
        }
    };

    app::run(
        store,
        renderer,
        practical,
        LaunchOptions {
            start: args.start,
            preselect: args.preselect,
        },
    );
    ExitCode::SUCCESS
}
