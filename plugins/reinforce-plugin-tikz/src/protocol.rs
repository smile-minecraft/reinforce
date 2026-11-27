//! The stdio host loop used by the TikZ worker.
//!
//! The server owns the child process and the queue; this module only reads one
//! [`HostMessage`] per line, answers with protocol messages and keeps stdout
//! free for those messages. External tools are watched for cancellation while
//! they run, and every failure becomes a structured protocol `error`.

use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Runs an external tool without letting it inherit stdout, and stops it when
/// the host sends `cancel`.
pub fn run_command(
    tool: &Path,
    arguments: &[String],
    cancel: &Receiver<HostMessage>,
) -> Result<(), RunError> {
    if !tool.is_file() {
        return Err(failure(
            "external_tool_missing",
            format!("external tool does not exist: {}", tool.display()),
        ));
    }

    let mut child = Command::new(tool)
        .args(arguments)
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

    let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut bytes);
        bytes
    });
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut bytes);
        bytes
    });

    loop {
        match cancel.recv_timeout(Duration::from_millis(50)) {
            Ok(HostMessage::Cancel(_)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Cancelled);
            }
            Ok(HostMessage::Hello(_)) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // The host closed stdin. The tool may still be finishing; the
                // server's timeout remains the backstop for a stuck process.
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => thread::sleep(Duration::from_millis(50)),
                        Err(error) => {
                            return Err(failure(
                                "external_tool_failed",
                                format!("cannot wait for {}: {error}", tool.display()),
                            ));
                        }
                    }
                }
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
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
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        let text = String::from_utf8_lossy(&stderr);
        let tail = text.chars().rev().take(2_000).collect::<Vec<_>>();
        let tail: String = tail.into_iter().rev().collect();
        return Err(failure(
            "external_tool_failed",
            format!("{} exited with {status}: {}", tool.display(), tail.trim()),
        ));
    }
    Ok(())
}

/// A unique directory for one job's intermediate files, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(root: &Path, tag: &str) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = root.join(format!("{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
