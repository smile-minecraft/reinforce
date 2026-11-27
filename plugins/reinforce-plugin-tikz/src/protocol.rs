//! The stdio host loop used by the TikZ worker.
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

/// Bytes kept from the end of each tool output stream for error messages.
/// The pipes are always drained to the end so a tool never blocks, but memory
/// stays bounded however much LaTeX prints.
const LOG_TAIL_BYTES: usize = 64 * 1024;
/// Characters of tool output quoted in an error message.
const MESSAGE_TAIL_CHARS: usize = 2_000;

fn drain_tail(mut pipe: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > LOG_TAIL_BYTES {
            let excess = bytes.len() - LOG_TAIL_BYTES;
            bytes.drain(..excess);
        }
    }
    bytes
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
    cancel: &Receiver<HostMessage>,
) -> Result<(), RunError> {
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
    let stdout_reader = thread::spawn(move || drain_tail(stdout_pipe));
    let stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stderr_reader = thread::spawn(move || drain_tail(stderr_pipe));

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
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        // LaTeX reports errors on stdout, so fall back to it when stderr is empty.
        let mut detail = message_tail(&stderr);
        if detail.is_empty() {
            detail = message_tail(&stdout);
        }
        return Err(failure(
            "external_tool_failed",
            format!("{} exited with {status}: {detail}", tool.display()),
        ));
    }
    Ok(())
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

    fn sh_in(cwd: &Path, script: &str) -> Result<(), RunError> {
        let (_sender, cancel) = mpsc::channel();
        run_command(
            Path::new("/bin/sh"),
            &["-c".to_owned(), script.to_owned()],
            cwd,
            &cancel,
        )
    }

    fn sh(script: &str) -> Result<(), RunError> {
        sh_in(&std::env::current_dir().expect("cwd"), script)
    }

    fn failure_of(result: Result<(), RunError>) -> Failure {
        match result {
            Err(RunError::Failure(failure)) => failure,
            Err(RunError::Cancelled) => panic!("not a cancellation"),
            Ok(()) => panic!("expected a failure"),
        }
    }

    #[test]
    fn long_logs_keep_only_their_end() {
        let failure = failure_of(sh(
            "head -c 8000000 /dev/zero | tr '\\000' x; printf END; exit 1",
        ));
        assert_eq!(failure.code, "external_tool_failed");
        assert!(failure.message.ends_with("END"));
        assert!(failure.message.len() < 4 * MESSAGE_TAIL_CHARS);
    }

    #[test]
    fn failures_quote_stdout_when_stderr_is_empty() {
        let failure = failure_of(sh("echo '! Undefined control sequence.'; exit 1"));
        assert_eq!(failure.code, "external_tool_failed");
        assert!(failure.message.contains("Undefined control sequence"));
    }

    #[test]
    fn tools_run_inside_the_job_directory_with_tex_file_access_confined() {
        let directory = tempfile::tempdir().expect("directory");
        let expected = directory.path().canonicalize().expect("canonical");
        let script = format!(
            "[ \"$(pwd -P)\" = '{}' ] && [ \"$openin_any\" = p ] && [ \"$openout_any\" = p ]",
            expected.display()
        );
        sh_in(directory.path(), &script).expect("tool sees the job directory");
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
