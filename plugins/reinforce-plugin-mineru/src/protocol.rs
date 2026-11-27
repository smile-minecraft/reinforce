//! The stdio host loop used by the MinerU worker.
//!
//! The server owns the child process and the queue; this module only reads one
//! [`HostMessage`] per line, answers with protocol messages and keeps stdout
//! free for those messages. External tools are watched for cancellation while
//! they run, and every failure becomes a structured protocol `error`.

use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use reinforce_plugin_protocol::{
    ErrorMessage, Hello, HostMessage, Operation, PROTOCOL_NAME, PROTOCOL_VERSION, Progress, Ready,
    ResultMessage, WorkerMessage,
};

/// A machine-readable failure that the host stores in the job error details.
#[derive(Debug)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}

impl Failure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// A cancelled job has no terminal message: the host already knows it asked us
/// to stop and kills the child after a short grace period.
#[derive(Debug)]
pub enum RunError {
    Failure(Failure),
    Cancelled,
}

impl From<Failure> for RunError {
    fn from(failure: Failure) -> Self {
        Self::Failure(failure)
    }
}

/// Convenience constructor for handlers that do not spawn an external tool.
pub fn failure(code: &'static str, message: impl Into<String>) -> RunError {
    RunError::Failure(Failure::new(code, message))
}

fn emit(message: &WorkerMessage) -> io::Result<()> {
    let line = serde_json::to_string(message).expect("protocol messages are serializable");
    let mut stdout = io::stdout().lock();
    stdout.write_all(line.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// How much of an external tool's stdout the worker keeps. The pipe is always
/// drained to the end so the tool never blocks, but memory stays bounded.
#[derive(Debug, Clone, Copy)]
pub enum Keep {
    /// Keep the first `n` bytes; more output fails the job as too large.
    Head(usize),
    /// Keep only the last `n` bytes, e.g. a log used for error messages.
    Tail(usize),
}

/// Bytes kept from stderr for error messages.
const STDERR_TAIL_BYTES: usize = 8 * 1024;
/// Characters of tool output quoted in an error message.
const MESSAGE_TAIL_CHARS: usize = 2_000;

pub struct CommandOutput {
    pub stdout: Vec<u8>,
}

struct Captured {
    bytes: Vec<u8>,
    overflowed: bool,
}

fn drain(mut pipe: impl Read, keep: Keep) -> Captured {
    let mut bytes = Vec::new();
    let mut overflowed = false;
    let mut chunk = [0_u8; 8192];
    loop {
        let read = match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        match keep {
            Keep::Head(limit) => {
                let room = limit.saturating_sub(bytes.len());
                if read > room {
                    overflowed = true;
                }
                bytes.extend_from_slice(&chunk[..read.min(room)]);
            }
            Keep::Tail(limit) => {
                bytes.extend_from_slice(&chunk[..read]);
                if bytes.len() > limit {
                    let excess = bytes.len() - limit;
                    bytes.drain(..excess);
                    overflowed = true;
                }
            }
        }
    }
    Captured { bytes, overflowed }
}

fn message_tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let tail: Vec<char> = text.chars().rev().take(MESSAGE_TAIL_CHARS).collect();
    tail.into_iter().rev().collect::<String>().trim().to_owned()
}

/// Runs an external tool inside `cwd` without letting it inherit stdout, and
/// stops it when the host sends `cancel`.
pub fn run_command(
    tool: &Path,
    arguments: &[String],
    cwd: &Path,
    keep_stdout: Keep,
    cancel: &Receiver<HostMessage>,
) -> Result<CommandOutput, RunError> {
    if !tool.is_file() {
        return Err(failure(
            "external_tool_missing",
            format!("external tool does not exist: {}", tool.display()),
        ));
    }

    // kpathsea reads these: TeX may only open files below its working
    // directory (the job directory), so `\input{...}` in submitted source
    // cannot read or write anything else. Other tools ignore them.
    let mut child = Command::new(tool)
        .args(arguments)
        .current_dir(cwd)
        .env("openin_any", "p")
        .env("openout_any", "p")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            failure(
                "external_tool_spawn_failed",
                format!("cannot start {}: {error}", tool.display()),
            )
        })?;

    let stdout_pipe = child.stdout.take().expect("stdout is piped");
    let stdout_reader = thread::spawn(move || drain(stdout_pipe, keep_stdout));
    let stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stderr_reader = thread::spawn(move || drain(stderr_pipe, Keep::Tail(STDERR_TAIL_BYTES)));

    loop {
        match cancel.recv_timeout(Duration::from_millis(50)) {
            Ok(HostMessage::Cancel(_)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Cancelled);
            }
            Ok(HostMessage::Hello(_)) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // The host closed stdin. The tool may still be finishing; the
                // server's timeout remains the backstop for a stuck process.
                thread::sleep(Duration::from_millis(50));
            }
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(error) => {
                return Err(failure(
                    "external_tool_failed",
                    format!("cannot wait for {}: {error}", tool.display()),
                ));
            }
        }
    }

    let status = child.wait().map_err(|error| {
        failure(
            "external_tool_failed",
            format!("cannot reap {}: {error}", tool.display()),
        )
    })?;
    let stdout = stdout_reader.join().unwrap_or(Captured {
        bytes: Vec::new(),
        overflowed: false,
    });
    let stderr = stderr_reader
        .join()
        .map(|captured| captured.bytes)
        .unwrap_or_default();
    if !status.success() {
        // LaTeX reports errors on stdout, so fall back to it when stderr is empty.
        let mut detail = message_tail(&stderr);
        if detail.is_empty() {
            detail = message_tail(&stdout.bytes);
        }
        return Err(failure(
            "external_tool_failed",
            format!("{} exited with {status}: {detail}", tool.display()),
        ));
    }
    if let (Keep::Head(limit), true) = (keep_stdout, stdout.overflowed) {
        return Err(failure(
            "output_too_large",
            format!(
                "{} printed more than {limit} bytes on stdout",
                tool.display()
            ),
        ));
    }
    Ok(CommandOutput {
        stdout: stdout.bytes,
    })
}

/// A unique, freshly created directory for one job's intermediate files,
/// removed on drop.
pub struct Scratch(tempfile::TempDir);

impl Scratch {
    pub fn new(root: &Path, tag: &str) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        tempfile::Builder::new()
            .prefix(&format!("{tag}-"))
            .tempdir_in(root)
            .map(Self)
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

/// Reads one job from stdin and writes the handshake, optional progress and
/// exactly one terminal message. Returns the process exit status.
pub fn run(
    capabilities: &'static [&'static str],
    work: fn(&Hello, &Receiver<HostMessage>) -> Result<Vec<Operation>, RunError>,
) -> i32 {
    let (sender, receiver) = mpsc::channel::<HostMessage>();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<HostMessage>(&line) else {
                break;
            };
            if sender.send(message).is_err() {
                break;
            }
        }
    });

    let hello = loop {
        match receiver.recv() {
            Ok(HostMessage::Hello(hello)) => break hello,
            Ok(HostMessage::Cancel(_)) => continue,
            Err(_) => return 0,
        }
    };

    if hello.protocol != PROTOCOL_NAME || hello.version != PROTOCOL_VERSION {
        let error = Failure::new(
            "protocol_mismatch",
            format!(
                "expected {PROTOCOL_NAME} v{PROTOCOL_VERSION}, got {} v{}",
                hello.protocol, hello.version
            ),
        );
        let _ = emit(&WorkerMessage::Error(ErrorMessage {
            job_id: hello.job_id.clone(),
            code: error.code.to_owned(),
            message: error.message,
        }));
        return 1;
    }
    if !capabilities.contains(&hello.capability.as_str()) {
        let error = Failure::new(
            "unsupported_capability",
            format!("unsupported capability: {}", hello.capability),
        );
        let _ = emit(&WorkerMessage::Error(ErrorMessage {
            job_id: hello.job_id.clone(),
            code: error.code.to_owned(),
            message: error.message,
        }));
        return 1;
    }

    let ready = WorkerMessage::Ready(Ready {
        protocol: PROTOCOL_NAME.to_owned(),
        version: PROTOCOL_VERSION,
        job_id: hello.job_id.clone(),
        plugin: hello.plugin.clone(),
        capabilities: capabilities.iter().map(|name| (*name).to_owned()).collect(),
    });
    if emit(&ready).is_err() {
        return 1;
    }

    match work(&hello, &receiver) {
        Ok(operations) if operations.is_empty() => {
            let _ = emit(&WorkerMessage::Error(ErrorMessage {
                job_id: hello.job_id.clone(),
                code: "empty_result".to_owned(),
                message: "the tool produced no content to propose".to_owned(),
            }));
            1
        }
        Ok(operations) => {
            let progress = WorkerMessage::Progress(Progress {
                job_id: hello.job_id.clone(),
                fraction: 1.0,
                message: Some("operations ready".to_owned()),
            });
            let _ = emit(&progress);
            let result = WorkerMessage::Result(ResultMessage {
                job_id: hello.job_id.clone(),
                operations,
            });
            if emit(&result).is_err() {
                return 1;
            }
            0
        }
        Err(RunError::Cancelled) => 0,
        Err(RunError::Failure(error)) => {
            let _ = emit(&WorkerMessage::Error(ErrorMessage {
                job_id: hello.job_id.clone(),
                code: error.code.to_owned(),
                message: error.message,
            }));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, keep: Keep) -> Result<CommandOutput, RunError> {
        let (_sender, cancel) = mpsc::channel();
        let cwd = std::env::current_dir().expect("cwd");
        run_command(
            Path::new("/bin/sh"),
            &["-c".to_owned(), script.to_owned()],
            &cwd,
            keep,
            &cancel,
        )
    }

    fn failure_of(result: Result<CommandOutput, RunError>) -> Failure {
        match result {
            Err(RunError::Failure(failure)) => failure,
            Err(RunError::Cancelled) => panic!("not a cancellation"),
            Ok(_) => panic!("expected a failure"),
        }
    }

    #[test]
    fn stdout_beyond_the_head_limit_fails_without_buffering_it() {
        let failure = failure_of(sh("head -c 8000000 /dev/zero", Keep::Head(1024)));
        assert_eq!(failure.code, "output_too_large");
    }

    #[test]
    fn stdout_within_the_head_limit_is_returned() {
        let output = sh("printf hello", Keep::Head(1024)).expect("small output");
        assert_eq!(output.stdout, b"hello");
    }

    #[test]
    fn tail_mode_keeps_only_the_end_of_a_long_log() {
        let output = sh(
            "head -c 8000000 /dev/zero | tr '\\000' x; printf END",
            Keep::Tail(64),
        )
        .expect("log output");
        assert_eq!(output.stdout.len(), 64);
        assert!(output.stdout.ends_with(b"END"));
    }

    #[test]
    fn failures_quote_stdout_when_stderr_is_empty() {
        let failure = failure_of(sh(
            "echo '! Undefined control sequence.'; exit 1",
            Keep::Tail(1024),
        ));
        assert_eq!(failure.code, "external_tool_failed");
        assert!(failure.message.contains("Undefined control sequence"));
    }

    #[test]
    fn tools_run_inside_the_given_directory() {
        let directory = tempfile::tempdir().expect("directory");
        let (_sender, cancel) = mpsc::channel();
        let output = run_command(
            Path::new("/bin/sh"),
            &[
                "-c".to_owned(),
                "pwd -P; printf '%s %s' \"$openin_any\" \"$openout_any\"".to_owned(),
            ],
            directory.path(),
            Keep::Head(4096),
            &cancel,
        )
        .expect("pwd");
        let text = String::from_utf8(output.stdout).expect("utf-8");
        let expected = directory.path().canonicalize().expect("canonical");
        assert!(text.starts_with(&expected.display().to_string()), "{text}");
        assert!(text.trim_end().ends_with("p p"), "{text}");
    }

    #[test]
    fn scratch_directories_are_unique_and_removed() {
        let root = tempfile::tempdir().expect("root");
        let first = Scratch::new(root.path(), "job").expect("first");
        let second = Scratch::new(root.path(), "job").expect("second");
        assert_ne!(first.path(), second.path());
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists());
    }
}
