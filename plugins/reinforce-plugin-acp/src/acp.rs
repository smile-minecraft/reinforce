//! Minimal Agent Client Protocol (ACP) v1 client over JSON-RPC 2.0 stdio.
//!
//! Lifecycle:
//! 1. Spawn the configured external agent executable directly (no shell).
//! 2. Send `initialize` with `fs` and `terminal` capabilities disabled.
//! 3. Send `session/new` with `cwd` and `mcpServers: []`.
//! 4. Send `session/prompt`, streaming `session/update` `agent_message_chunk`
//!    text blocks while strictly denying any agent-initiated permission,
//!    filesystem, or terminal requests.
//! 5. On host cancellation, send `session/cancel` and terminate the subprocess
//!    promptly so cancellation never stalls.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::proposal::MAX_PROPOSAL_BYTES;

/// ACP wire protocol version supported by this client.
pub const ACP_PROTOCOL_VERSION: u64 = 1;
/// Maximum size of a single JSON-RPC line read from the agent's stdout.
const MAX_AGENT_LINE_BYTES: usize = 2 * 1024 * 1024;
/// Maximum agent stderr bytes retained for error diagnostics.
const STDERR_TAIL_BYTES: usize = 8 * 1024;
/// Timeout for writing a normal JSON-RPC frame to the agent's stdin.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Timeout for writing `session/cancel` during cancellation.
const CANCEL_WRITE_TIMEOUT: Duration = Duration::from_millis(50);
/// Grace period for the agent to exit after `session/cancel` or completion.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(100);
/// Grace period to reap the agent after `start_kill`.
const KILL_GRACE: Duration = Duration::from_millis(500);
/// Minimum interval between streaming progress callbacks.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Configuration for running one ACP prompt session.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub agent: PathBuf,
    pub agent_args: Vec<String>,
    pub cwd: PathBuf,
    pub request_timeout: Duration,
}

/// Outcome of an ACP prompt session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpOutcome {
    /// Assistant response text assembled from `agent_message_chunk` updates.
    Completed(String),
    /// The host requested cancellation while the agent was running.
    Cancelled,
}

/// Structured error mapped onto `WorkerMessage::Error`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct AcpError {
    pub code: &'static str,
    pub message: String,
}

impl AcpError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn with_stderr(mut self, stderr: &str) -> Self {
        let trimmed = stderr.trim();
        if !trimmed.is_empty() {
            let tail: String = trimmed
                .chars()
                .rev()
                .take(400)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            self.message = format!("{} (agent stderr: {tail})", self.message);
        }
        self
    }
}

/// Runs `initialize` → `session/new` → `session/prompt` against an external ACP
/// agent and returns the assembled assistant response text.
pub async fn run_prompt<F, Fut>(
    config: &AcpConfig,
    prompt_text: &str,
    mut cancel_rx: watch::Receiver<bool>,
    mut on_progress: F,
) -> Result<AcpOutcome, AcpError>
where
    F: FnMut(f64, String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    if *cancel_rx.borrow() {
        return Ok(AcpOutcome::Cancelled);
    }
    if !config.agent.is_absolute() {
        return Err(AcpError::new(
            "invalid_agent_path",
            format!(
                "--agent must be an absolute path, got {}",
                config.agent.display()
            ),
        ));
    }
    if !config.cwd.is_absolute() {
        return Err(AcpError::new(
            "invalid_cwd",
            format!(
                "--cwd must be an absolute path, got {}",
                config.cwd.display()
            ),
        ));
    }

    let mut command = Command::new(&config.agent);
    command
        .args(&config.agent_args)
        .current_dir(&config.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AcpError::new(
                "agent_not_found",
                format!("ACP agent executable not found: {}", config.agent.display()),
            ));
        }
        Err(error) => {
            return Err(AcpError::new(
                "agent_spawn_failed",
                format!(
                    "failed to spawn ACP agent {}: {error}",
                    config.agent.display()
                ),
            ));
        }
    };

    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        terminate_child(&mut child, Duration::ZERO).await;
        return Err(AcpError::new(
            "agent_spawn_failed",
            "ACP agent subprocess did not expose stdin/stdout pipes",
        ));
    };
    let stderr_task = tokio::spawn(capture_stderr(child.stderr.take()));

    let mut transport = AgentTransport {
        stdin: BufWriter::new(stdin),
        stdout: BufReader::new(stdout),
        next_id: 1,
        session_id: None,
    };

    let result = drive_session(
        &mut transport,
        &config.cwd,
        prompt_text,
        config.request_timeout,
        &mut cancel_rx,
        &mut on_progress,
    )
    .await;

    match result {
        Ok(AcpOutcome::Completed(text)) => {
            drop(transport);
            terminate_child(&mut child, SHUTDOWN_GRACE).await;
            let _ = collect_stderr(stderr_task).await;
            Ok(AcpOutcome::Completed(text))
        }
        Ok(AcpOutcome::Cancelled) => {
            if let Some(session_id) = transport.session_id.clone() {
                let cancel_notification = json!({
                    "jsonrpc": "2.0",
                    "method": "session/cancel",
                    "params": { "sessionId": session_id }
                });
                let _ = transport
                    .write_frame(&cancel_notification, CANCEL_WRITE_TIMEOUT)
                    .await;
            }
            drop(transport);
            terminate_child(&mut child, SHUTDOWN_GRACE).await;
            let _ = collect_stderr(stderr_task).await;
            Ok(AcpOutcome::Cancelled)
        }
        Err(error) => {
            drop(transport);
            terminate_child(&mut child, Duration::ZERO).await;
            let stderr = collect_stderr(stderr_task).await;
            Err(error.with_stderr(&stderr))
        }
    }
}

struct AgentTransport {
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    session_id: Option<String>,
}

impl AgentTransport {
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn write_frame(&mut self, value: &Value, timeout: Duration) -> Result<(), AcpError> {
        let mut encoded = serde_json::to_vec(value).map_err(|error| {
            AcpError::new("acp_protocol_error", format!("encode error: {error}"))
        })?;
        encoded.push(b'\n');

        let write_fut = async {
            self.stdin.write_all(&encoded).await?;
            self.stdin.flush().await
        };
        match tokio::time::timeout(timeout, write_fut).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(AcpError::new(
                "acp_crashed",
                format!("failed writing to ACP agent stdin: {error}"),
            )),
            Err(_) => Err(AcpError::new(
                "acp_timeout",
                "timed out writing JSON-RPC message to ACP agent",
            )),
        }
    }

    async fn read_line_before(
        &mut self,
        deadline: Instant,
        cancel_rx: &mut watch::Receiver<bool>,
    ) -> Result<LineRead, AcpError> {
        let mut buf = Vec::new();
        let mut total = 0usize;
        loop {
            if *cancel_rx.borrow() {
                return Ok(LineRead::Cancelled);
            }
            let remaining = MAX_AGENT_LINE_BYTES.saturating_sub(total).saturating_add(1);
            let mut limited = (&mut self.stdout).take(remaining as u64);
            let read = tokio::select! {
                biased;
                changed = cancel_rx.changed() => {
                    if changed.is_err() || *cancel_rx.borrow() {
                        return Ok(LineRead::Cancelled);
                    }
                    continue;
                }
                () = tokio::time::sleep_until(deadline) => {
                    return Err(AcpError::new(
                        "acp_timeout",
                        "ACP agent did not respond before request timeout",
                    ));
                }
                result = limited.read_until(b'\n', &mut buf) => {
                    result.map_err(|error| {
                        AcpError::new("acp_crashed", format!("failed reading ACP agent stdout: {error}"))
                    })?
                }
            };
            if read == 0 {
                if buf.is_empty() {
                    return Ok(LineRead::Eof);
                }
                return Ok(LineRead::Line(trim_line(&buf)));
            }
            total += read;
            if total > MAX_AGENT_LINE_BYTES {
                return Err(AcpError::new(
                    "acp_output_too_large",
                    format!(
                        "ACP agent emitted a single line exceeding {MAX_AGENT_LINE_BYTES} bytes"
                    ),
                ));
            }
            if buf.last() == Some(&b'\n') {
                return Ok(LineRead::Line(trim_line(&buf)));
            }
        }
    }
}

enum LineRead {
    Line(String),
    Eof,
    Cancelled,
}

async fn drive_session<F, Fut>(
    transport: &mut AgentTransport,
    cwd: &Path,
    prompt_text: &str,
    request_timeout: Duration,
    cancel_rx: &mut watch::Receiver<bool>,
    on_progress: &mut F,
) -> Result<AcpOutcome, AcpError>
where
    F: FnMut(f64, String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    on_progress(0.1, "initializing ACP agent".to_owned()).await;
    let init_id = transport.alloc_id();
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": init_id,
        "method": "initialize",
        "params": {
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientCapabilities": {
                "fs": {
                    "readTextFile": false,
                    "writeTextFile": false
                },
                "terminal": false
            },
            "clientInfo": {
                "name": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    transport.write_frame(&init_req, WRITE_TIMEOUT).await?;
    let init_result = match await_response(
        transport,
        init_id,
        "initialize",
        request_timeout,
        cancel_rx,
        None,
        on_progress,
    )
    .await?
    {
        ResponseWait::Result(value) => value,
        ResponseWait::Cancelled => return Ok(AcpOutcome::Cancelled),
    };
    let negotiated_version = init_result
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AcpError::new(
                "acp_protocol_error",
                "ACP initialize response is missing numeric protocolVersion",
            )
        })?;
    if negotiated_version != ACP_PROTOCOL_VERSION {
        return Err(AcpError::new(
            "acp_protocol_error",
            format!(
                "unsupported ACP protocolVersion {negotiated_version}, expected {ACP_PROTOCOL_VERSION}"
            ),
        ));
    }

    on_progress(0.25, "creating ACP session".to_owned()).await;
    let new_session_id = transport.alloc_id();
    let new_session_req = json!({
        "jsonrpc": "2.0",
        "id": new_session_id,
        "method": "session/new",
        "params": {
            "cwd": cwd.display().to_string(),
            "mcpServers": []
        }
    });
    transport
        .write_frame(&new_session_req, WRITE_TIMEOUT)
        .await?;
    let session_result = match await_response(
        transport,
        new_session_id,
        "session/new",
        request_timeout,
        cancel_rx,
        None,
        on_progress,
    )
    .await?
    {
        ResponseWait::Result(value) => value,
        ResponseWait::Cancelled => return Ok(AcpOutcome::Cancelled),
    };
    let session_id = session_result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AcpError::new(
                "acp_protocol_error",
                "ACP session/new response is missing a non-empty sessionId",
            )
        })?
        .to_owned();
    transport.session_id = Some(session_id.clone());

    on_progress(0.4, "running ACP prompt".to_owned()).await;
    let prompt_id = transport.alloc_id();
    let prompt_req = json!({
        "jsonrpc": "2.0",
        "id": prompt_id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": prompt_text
                }
            ]
        }
    });
    transport.write_frame(&prompt_req, WRITE_TIMEOUT).await?;

    let mut collector = PromptCollector::new(session_id);
    let prompt_result = match await_response(
        transport,
        prompt_id,
        "session/prompt",
        request_timeout,
        cancel_rx,
        Some(&mut collector),
        on_progress,
    )
    .await?
    {
        ResponseWait::Result(value) => value,
        ResponseWait::Cancelled => return Ok(AcpOutcome::Cancelled),
    };

    let stop_reason = prompt_result
        .get("stopReason")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AcpError::new(
                "acp_protocol_error",
                "ACP session/prompt response is missing stopReason",
            )
        })?;
    match stop_reason {
        "end_turn" => {
            on_progress(0.9, "validating agent proposal".to_owned()).await;
            Ok(AcpOutcome::Completed(collector.text))
        }
        "cancelled" => Err(AcpError::new(
            "acp_cancelled",
            "ACP agent reported stopReason=cancelled",
        )),
        "refusal" => Err(AcpError::new(
            "acp_refused",
            "ACP agent refused to complete the prompt",
        )),
        other => Err(AcpError::new(
            "acp_incomplete",
            format!("ACP agent stopped early with stopReason={other:?}"),
        )),
    }
}

enum ResponseWait {
    Result(Value),
    Cancelled,
}

struct PromptCollector {
    session_id: String,
    text: String,
    chunks: usize,
    last_progress: Instant,
}

impl PromptCollector {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            text: String::new(),
            chunks: 0,
            last_progress: Instant::now() - PROGRESS_INTERVAL,
        }
    }

    fn ingest_update(&mut self, params: &Value) -> Result<bool, AcpError> {
        if let Some(sid) = params.get("sessionId").and_then(Value::as_str) {
            if sid != self.session_id {
                return Ok(false);
            }
        }
        let Some(update) = params.get("update") else {
            return Ok(false);
        };
        let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
            return Ok(false);
        };
        if kind != "agent_message_chunk" {
            return Ok(false);
        }
        let Some(content) = update.get("content") else {
            return Ok(false);
        };
        if content.get("type").and_then(Value::as_str) != Some("text") {
            return Ok(false);
        }
        let Some(chunk) = content.get("text").and_then(Value::as_str) else {
            return Ok(false);
        };
        if self.text.len().saturating_add(chunk.len()) > MAX_PROPOSAL_BYTES {
            return Err(AcpError::new(
                "acp_output_too_large",
                format!("ACP assistant response exceeded {MAX_PROPOSAL_BYTES} bytes"),
            ));
        }
        self.text.push_str(chunk);
        self.chunks += 1;
        Ok(true)
    }
}

async fn await_response<F, Fut>(
    transport: &mut AgentTransport,
    expected_id: u64,
    method: &str,
    request_timeout: Duration,
    cancel_rx: &mut watch::Receiver<bool>,
    mut collector: Option<&mut PromptCollector>,
    on_progress: &mut F,
) -> Result<ResponseWait, AcpError>
where
    F: FnMut(f64, String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let deadline = Instant::now() + request_timeout;
    loop {
        let line = match transport.read_line_before(deadline, cancel_rx).await? {
            LineRead::Line(line) => line,
            LineRead::Cancelled => return Ok(ResponseWait::Cancelled),
            LineRead::Eof => {
                return Err(AcpError::new(
                    "acp_crashed",
                    format!("ACP agent exited before responding to {method}"),
                ));
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let message: Value = serde_json::from_str(trimmed).map_err(|error| {
            AcpError::new(
                "acp_protocol_error",
                format!("ACP agent emitted invalid JSON while awaiting {method}: {error}"),
            )
        })?;
        let Some(object) = message.as_object() else {
            return Err(AcpError::new(
                "acp_protocol_error",
                format!("ACP agent emitted non-object JSON while awaiting {method}"),
            ));
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(AcpError::new(
                "acp_protocol_error",
                format!("ACP message is missing jsonrpc=\"2.0\" while awaiting {method}"),
            ));
        }

        let id = object.get("id");
        let incoming_method = object.get("method").and_then(Value::as_str);

        match (incoming_method, id) {
            // Agent-initiated request to client (always deny permissions, fs, terminal, etc.)
            (Some(req_method), Some(req_id)) if !req_id.is_null() => {
                let response =
                    build_client_denial_response(req_id, req_method, object.get("params"));
                transport.write_frame(&response, WRITE_TIMEOUT).await?;
            }
            // Agent notification
            (Some(notif_method), _) => {
                if notif_method == "session/update" {
                    if let (Some(params), Some(collector)) =
                        (object.get("params"), collector.as_deref_mut())
                    {
                        if collector.ingest_update(params)?
                            && collector.last_progress.elapsed() >= PROGRESS_INTERVAL
                        {
                            collector.last_progress = Instant::now();
                            let fraction =
                                0.4 + 0.45 * (1.0 - 1.0 / (1.0 + collector.chunks as f64 / 8.0));
                            on_progress(
                                fraction,
                                format!("receiving agent output ({} bytes)", collector.text.len()),
                            )
                            .await;
                        }
                    }
                }
            }
            // JSON-RPC response
            (None, Some(resp_id)) => {
                let matches_expected = resp_id.as_u64() == Some(expected_id);
                if let Some(error) = object.get("error") {
                    if matches_expected || resp_id.is_null() {
                        let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32000);
                        let msg = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown ACP error");
                        return Err(AcpError::new(
                            "acp_error",
                            format!("ACP {method} failed (code {code}): {msg}"),
                        ));
                    }
                }
                if !matches_expected {
                    return Err(AcpError::new(
                        "acp_protocol_error",
                        format!(
                            "unexpected JSON-RPC response id {resp_id} while awaiting {method} (id {expected_id})"
                        ),
                    ));
                }
                let result = object.get("result").cloned().ok_or_else(|| {
                    AcpError::new(
                        "acp_protocol_error",
                        format!("ACP response for {method} has neither result nor error"),
                    )
                })?;
                return Ok(ResponseWait::Result(result));
            }
            (None, None) => {
                return Err(AcpError::new(
                    "acp_protocol_error",
                    format!("malformed JSON-RPC frame (no method or id) while awaiting {method}"),
                ));
            }
        }
    }
}

/// Builds a strict denial response for any agent-to-client JSON-RPC request.
///
/// - `session/request_permission` selects a `reject_always`/`reject_once`
///   option if offered, otherwise returns `outcome: "cancelled"`.
/// - All other client methods (`fs/read_text_file`, `fs/write_text_file`,
///   `terminal/*`, etc.) are refused with JSON-RPC error `-32601`.
pub(crate) fn build_client_denial_response(
    id: &Value,
    method: &str,
    params: Option<&Value>,
) -> Value {
    if method == "session/request_permission" {
        if let Some(option_id) = find_reject_option_id(params) {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id
                    }
                }
            });
        }
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "outcome": {
                    "outcome": "cancelled"
                }
            }
        });
    }

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32601,
            "message": format!("client method {method:?} is disabled by reinforce-plugin-acp")
        }
    })
}

fn find_reject_option_id(params: Option<&Value>) -> Option<String> {
    let options = params?.get("options")?.as_array()?;
    for preferred_kind in ["reject_always", "reject_once"] {
        for option in options {
            if option.get("kind").and_then(Value::as_str) == Some(preferred_kind) {
                if let Some(option_id) = option.get("optionId").and_then(Value::as_str) {
                    if !option_id.is_empty() {
                        return Some(option_id.to_owned());
                    }
                }
            }
        }
    }
    None
}

async fn capture_stderr(stderr: Option<ChildStderr>) -> String {
    let Some(stderr) = stderr else {
        return String::new();
    };
    let mut reader = BufReader::new(stderr);
    let mut kept = Vec::new();
    let mut chunk = [0_u8; 4096];
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
        _ => String::new(),
    }
}

async fn terminate_child(child: &mut Child, grace: Duration) {
    if !grace.is_zero() {
        if let Ok(Ok(_)) = tokio::time::timeout(grace, child.wait()).await {
            return;
        }
    }
    let _ = child.start_kill();
    let _ = tokio::time::timeout(KILL_GRACE, child.wait()).await;
}

fn trim_line(bytes: &[u8]) -> String {
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_denial_prefers_reject_option_and_falls_back_to_cancelled() {
        let with_reject = build_client_denial_response(
            &json!(7),
            "session/request_permission",
            Some(&json!({
                "sessionId": "sess-1",
                "options": [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "deny", "name": "Reject", "kind": "reject_once"}
                ]
            })),
        );
        assert_eq!(with_reject["result"]["outcome"]["outcome"], "selected");
        assert_eq!(with_reject["result"]["outcome"]["optionId"], "deny");

        let without_reject = build_client_denial_response(
            &json!(8),
            "session/request_permission",
            Some(&json!({
                "sessionId": "sess-1",
                "options": [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"}
                ]
            })),
        );
        assert_eq!(without_reject["result"]["outcome"]["outcome"], "cancelled");
    }

    #[test]
    fn fs_and_terminal_requests_return_method_not_found() {
        for method in ["fs/read_text_file", "fs/write_text_file", "terminal/create"] {
            let reply = build_client_denial_response(&json!(9), method, Some(&json!({})));
            assert_eq!(reply["error"]["code"], -32601);
            assert!(reply["error"]["message"].as_str().unwrap().contains(method));
        }
    }

    #[test]
    fn prompt_collector_reassembles_text_chunks_and_enforces_cap() {
        let mut collector = PromptCollector::new("sess-1".to_owned());
        assert!(
            collector
                .ingest_update(&json!({
                    "sessionId": "sess-1",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "{\"cards\":"}
                    }
                }))
                .unwrap()
        );
        // Updates for another session or non-message updates are ignored.
        assert!(
            !collector
                .ingest_update(&json!({
                    "sessionId": "other",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "ignored"}
                    }
                }))
                .unwrap()
        );
        assert!(
            !collector
                .ingest_update(&json!({
                    "sessionId": "sess-1",
                    "update": {
                        "sessionUpdate": "agent_thought_chunk",
                        "content": {"type": "text", "text": "thinking"}
                    }
                }))
                .unwrap()
        );
        assert!(
            collector
                .ingest_update(&json!({
                    "sessionId": "sess-1",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "[]}"}
                    }
                }))
                .unwrap()
        );
        assert_eq!(collector.text, r#"{"cards":[]}"#);

        let oversized = "x".repeat(MAX_PROPOSAL_BYTES);
        let error = collector
            .ingest_update(&json!({
                "sessionId": "sess-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": oversized}
                }
            }))
            .expect_err("cap exceeded");
        assert_eq!(error.code, "acp_output_too_large");
    }
}
