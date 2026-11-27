//! Runs one job against a stdio plugin process.
//!
//! The runner owns every side of the process contract:
//!
//! - the command comes from the validated manifest, never from a request;
//! - the process is spawned directly (no shell) with a cleared environment and
//!   an explicit allowlist, so it cannot read server secrets;
//! - stdout is read line by line with a byte budget, so a runaway plugin
//!   cannot exhaust memory;
//! - a timeout and a cancellation token both end in a real kill, with
//!   `kill_on_drop` as the last-resort backstop;
//! - only a well-formed handshake followed by exactly one `result` or `error`
//!   message counts as a finished run.

use std::process::Stdio;
use std::time::Duration;

use reinforce_core::{JobError, JobErrorCode, JobId, PluginJob, plugin_store};
use reinforce_plugin_protocol::{Cancel, Hello, HostMessage, Operation, WorkerMessage};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio_util::sync::CancellationToken;

use crate::state::Store;

use super::config::{ENV_ALLOWLIST, PluginDefinition};

/// How long a worker gets to exit on its own after a cancel message.
const CANCEL_GRACE: Duration = Duration::from_millis(250);
/// How long a worker gets to exit on its own after a successful result.
const EXIT_GRACE: Duration = Duration::from_millis(500);
/// How long to wait for the reaper after `start_kill`.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// Stderr kept for diagnostics; the rest is discarded.
const STDERR_TAIL_BYTES: usize = 8 * 1024;

/// Result of one plugin run, before it is written to the queue.
pub enum RunOutcome {
    /// The plugin produced proposal operations.
    Operations(Vec<Operation>),
    /// The plugin reported a failure of its own.
    PluginError { code: String, message: String },
    /// The host stopped the run: timeout, crash, protocol violation or spawn
    /// failure.
    Failed(JobError),
    /// A cancel request stopped the run.
    Cancelled,
}

/// Best-effort progress writer for the job row.
#[derive(Clone)]
pub struct ProgressReporter {
    store: Store,
    job_id: JobId,
}

impl ProgressReporter {
    pub fn new(store: Store, job_id: JobId) -> Self {
        Self { store, job_id }
    }

    pub async fn report(&self, fraction: f64, message: Option<String>) {
        let job_id = self.job_id.clone();
        let result = self
            .store
            .call(move |database| {
                plugin_store::set_progress(database.connection(), &job_id, fraction, message)?;
                Ok(())
            })
            .await;
        if let Err(error) = result {
            tracing::debug!(job = %self.job_id, error = %error.message, "cannot record plugin progress");
        }
    }
}

enum ReadError {
    Cancelled,
    TooLarge,
    Io(std::io::Error),
}

enum DriveOutcome {
    Operations(Vec<Operation>),
    PluginError { code: String, message: String },
    Failed(JobError),
    Cancelled,
}

/// Runs `job` against `definition` and reports what happened.
pub async fn run(
    definition: &PluginDefinition,
    job: &PluginJob,
    cancel: CancellationToken,
    progress: ProgressReporter,
) -> RunOutcome {
    let mut command = Command::new(&definition.command);
    command
        .args(&definition.args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for key in ENV_ALLOWLIST {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return RunOutcome::Failed(JobError::new(
                JobErrorCode::SpawnFailed,
                format!(
                    "cannot start plugin command {}: {error}",
                    definition.command.display()
                ),
            ));
        }
    };
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr_task = tokio::spawn(capture_stderr(child.stderr.take()));

    let outcome = match tokio::time::timeout(
        definition.timeout,
        drive(
            definition, job, &mut child, stdin, stdout, &cancel, &progress,
        ),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => {
            terminate_after(&mut child, Duration::ZERO).await;
            let stderr = collect_stderr(stderr_task).await;
            let error = JobError::new(
                JobErrorCode::Timeout,
                format!(
                    "plugin did not finish within {}s",
                    definition.timeout.as_secs()
                ),
            );
            return RunOutcome::Failed(with_stderr(error, &stderr));
        }
    };

    match outcome {
        DriveOutcome::Operations(operations) => {
            finish(&mut child).await;
            let stderr = collect_stderr(stderr_task).await;
            if !stderr.trim().is_empty() {
                tracing::debug!(job = %job.id, "plugin stderr: {stderr}");
            }
            RunOutcome::Operations(operations)
        }
        DriveOutcome::PluginError { code, message } => {
            finish(&mut child).await;
            let stderr = collect_stderr(stderr_task).await;
            RunOutcome::PluginError {
                code,
                message: append_stderr(message, &stderr),
            }
        }
        DriveOutcome::Failed(error) => {
            terminate(&mut child).await;
            let stderr = collect_stderr(stderr_task).await;
            RunOutcome::Failed(with_stderr(error, &stderr))
        }
        DriveOutcome::Cancelled => {
            terminate(&mut child).await;
            let _ = collect_stderr(stderr_task).await;
            RunOutcome::Cancelled
        }
    }
}

async fn drive(
    definition: &PluginDefinition,
    job: &PluginJob,
    child: &mut Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    cancel: &CancellationToken,
    progress: &ProgressReporter,
) -> DriveOutcome {
    let (Some(mut stdin), Some(stdout)) = (stdin, stdout) else {
        return DriveOutcome::Failed(JobError::new(
            JobErrorCode::SpawnFailed,
            "plugin process has no stdio pipes",
        ));
    };
    let hello = HostMessage::Hello(Hello::new(
        job.id.as_str(),
        job.plugin.clone(),
        job.capability.clone(),
        job.input.clone(),
    ));
    // A worker that never reads stdin must not hold a cancel until the timeout.
    let written = tokio::select! {
        biased;
        () = cancel.cancelled() => return DriveOutcome::Cancelled,
        written = write_message(&mut stdin, &hello) => written,
    };
    if let Err(error) = written {
        return DriveOutcome::Failed(JobError::new(
            JobErrorCode::SpawnFailed,
            format!("cannot write the plugin handshake: {error}"),
        ));
    }

    let mut reader = BufReader::new(stdout);
    let mut budget = definition.max_output_bytes;

    let line = match next_line(&mut reader, &mut budget, cancel).await {
        Ok(Some(line)) => line,
        Ok(None) => return crashed(child, "before the handshake").await,
        Err(ReadError::Cancelled) => {
            send_cancel(&mut stdin, job).await;
            return DriveOutcome::Cancelled;
        }
        Err(ReadError::TooLarge) => return DriveOutcome::Failed(output_too_large(definition)),
        Err(ReadError::Io(error)) => return read_failure(error),
    };

    let ready = match WorkerMessage::parse(&line) {
        Ok(WorkerMessage::Ready(ready)) => ready,
        Ok(other) => {
            return protocol_violation(format!("expected a ready handshake, received {other:?}"));
        }
        Err(error) => {
            return protocol_violation(format!("cannot parse the plugin handshake: {error}"));
        }
    };
    if let Err(error) = ready.check_protocol() {
        return protocol_violation(error.to_string());
    }
    if ready.job_id != job.id.as_str() {
        return protocol_violation(format!(
            "handshake echoes job id {:?}, expected {:?}",
            ready.job_id,
            job.id.as_str()
        ));
    }
    if ready.plugin != job.plugin {
        return protocol_violation(format!(
            "handshake reports plugin {:?}, expected {:?}",
            ready.plugin, job.plugin
        ));
    }
    if !ready.capabilities.is_empty()
        && !ready
            .capabilities
            .iter()
            .any(|item| item == &job.capability)
    {
        return protocol_violation(format!(
            "plugin does not advertise the requested capability {:?}",
            job.capability
        ));
    }

    loop {
        let line = match next_line(&mut reader, &mut budget, cancel).await {
            Ok(Some(line)) => line,
            Ok(None) => return crashed(child, "without a result").await,
            Err(ReadError::Cancelled) => {
                send_cancel(&mut stdin, job).await;
                return DriveOutcome::Cancelled;
            }
            Err(ReadError::TooLarge) => return DriveOutcome::Failed(output_too_large(definition)),
            Err(ReadError::Io(error)) => return read_failure(error),
        };
        match WorkerMessage::parse(&line) {
            Ok(WorkerMessage::Progress(report)) => {
                if report.job_id != job.id.as_str() {
                    return protocol_violation(format!(
                        "progress echoes job id {:?}, expected {:?}",
                        report.job_id,
                        job.id.as_str()
                    ));
                }
                progress
                    .report(report.clamped_fraction(), report.message.clone())
                    .await;
            }
            Ok(WorkerMessage::Result(result)) => {
                if result.job_id != job.id.as_str() {
                    return protocol_violation(format!(
                        "result echoes job id {:?}, expected {:?}",
                        result.job_id,
                        job.id.as_str()
                    ));
                }
                if result.operations.is_empty() {
                    return protocol_violation("the result contains no operations");
                }
                return DriveOutcome::Operations(result.operations);
            }
            Ok(WorkerMessage::Error(error)) => {
                if error.job_id != job.id.as_str() {
                    return protocol_violation(format!(
                        "error echoes job id {:?}, expected {:?}",
                        error.job_id,
                        job.id.as_str()
                    ));
                }
                return DriveOutcome::PluginError {
                    code: error.code,
                    message: error.message,
                };
            }
            Ok(WorkerMessage::Ready(_)) => {
                return protocol_violation("the plugin sent a second ready handshake");
            }
            Err(error) => {
                return protocol_violation(format!("cannot parse a plugin message: {error}"));
            }
        }
    }
}

/// Reads one newline-terminated message, enforcing the output budget.
///
/// A trailing line without a newline at EOF is accepted: many programs close
/// stdout right after their last write.
async fn next_line(
    reader: &mut BufReader<ChildStdout>,
    budget: &mut usize,
    cancel: &CancellationToken,
) -> Result<Option<String>, ReadError> {
    let mut buffer = Vec::new();
    loop {
        // Bound each read by the remaining budget (plus one byte to detect
        // overflow) so output without a newline cannot grow memory.
        let limit = u64::try_from(*budget).unwrap_or(u64::MAX).saturating_add(1);
        let mut bounded = (&mut *reader).take(limit);
        let read = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(ReadError::Cancelled),
            read = bounded.read_until(b'\n', &mut buffer) => read.map_err(ReadError::Io)?,
        };
        if read == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            return Ok(Some(trim_line(&buffer)));
        }
        if *budget < read {
            return Err(ReadError::TooLarge);
        }
        *budget -= read;
        if buffer.last() == Some(&b'\n') {
            return Ok(Some(trim_line(&buffer)));
        }
    }
}

fn trim_line(buffer: &[u8]) -> String {
    let mut end = buffer.len();
    while end > 0 && matches!(buffer[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

async fn write_message(stdin: &mut ChildStdin, message: &HostMessage) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(std::io::Error::other)?;
    line.push(b'\n');
    stdin.write_all(&line).await?;
    stdin.flush().await
}

async fn send_cancel(stdin: &mut ChildStdin, job: &PluginJob) {
    let cancel = HostMessage::Cancel(Cancel {
        job_id: job.id.as_str(),
    });
    // The worker may already be gone, or may never read stdin again and leave
    // the pipe full; either way the caller terminates it, so a failed or
    // stalled write is not an error.
    let _ = tokio::time::timeout(CANCEL_GRACE, write_message(stdin, &cancel)).await;
}

async fn capture_stderr(stderr: Option<ChildStderr>) -> String {
    let Some(stderr) = stderr else {
        return String::new();
    };
    let mut reader = BufReader::new(stderr);
    let mut kept = Vec::new();
    let mut chunk = [0_u8; 4_096];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                kept.extend_from_slice(&chunk[..read]);
                if kept.len() > STDERR_TAIL_BYTES {
                    let overflow = kept.len() - STDERR_TAIL_BYTES;
                    kept.drain(..overflow);
                }
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

async fn collect_stderr(task: tokio::task::JoinHandle<String>) -> String {
    match tokio::time::timeout(KILL_GRACE, task).await {
        Ok(Ok(text)) => text,
        Ok(Err(_)) => String::new(),
        Err(_) => String::new(),
    }
}

async fn crashed(child: &mut Child, when: &str) -> DriveOutcome {
    let status = tokio::time::timeout(KILL_GRACE, child.wait()).await;
    let detail = match status {
        Ok(Ok(status)) => format!("plugin exited ({status}) {when}"),
        Ok(Err(error)) => format!("cannot wait for the plugin {when}: {error}"),
        Err(_) => format!("plugin closed its output {when}"),
    };
    DriveOutcome::Failed(JobError::new(JobErrorCode::Crashed, detail))
}

/// Waits for a worker that already produced its result to exit on its own.
async fn finish(child: &mut Child) {
    match tokio::time::timeout(EXIT_GRACE, child.wait()).await {
        Ok(_) => {}
        Err(_) => terminate(child).await,
    }
}

/// Ends the worker after a short grace period, then reaps it.
async fn terminate(child: &mut Child) {
    terminate_after(child, CANCEL_GRACE).await;
}

async fn terminate_after(child: &mut Child, grace: Duration) {
    if !grace.is_zero() {
        let _ = tokio::time::timeout(grace, child.wait()).await;
    }
    let _ = child.start_kill();
    let _ = tokio::time::timeout(KILL_GRACE, child.wait()).await;
}

fn protocol_violation(message: impl Into<String>) -> DriveOutcome {
    DriveOutcome::Failed(JobError::new(JobErrorCode::ProtocolViolation, message))
}

fn read_failure(error: std::io::Error) -> DriveOutcome {
    DriveOutcome::Failed(JobError::new(
        JobErrorCode::Crashed,
        format!("cannot read plugin output: {error}"),
    ))
}

fn output_too_large(definition: &PluginDefinition) -> JobError {
    JobError::new(
        JobErrorCode::OutputTooLarge,
        format!(
            "plugin wrote more than {} bytes",
            definition.max_output_bytes
        ),
    )
}

fn with_stderr(error: JobError, stderr: &str) -> JobError {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        return error;
    }
    let tail: String = stderr
        .chars()
        .rev()
        .take(512)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    error.with_details(serde_json::json!({ "stderr": tail }))
}

fn append_stderr(message: String, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        message
    } else {
        let tail: String = stderr
            .chars()
            .rev()
            .take(512)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{message} (stderr: {tail})")
    }
}
