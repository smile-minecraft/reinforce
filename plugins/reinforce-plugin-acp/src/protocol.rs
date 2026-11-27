//! Host-side `reinforce_plugin` protocol v1 stdio NDJSON transport.
//!
//! Reads the initial [`Hello`] line and then watches stdin in the background for
//! [`Cancel`] (or unexpected EOF) while writing [`Ready`], [`Progress`], and a
//! single terminal [`ResultMessage`] or [`ErrorMessage`] to stdout.

use std::sync::Arc;

use reinforce_plugin_protocol::{
    ErrorMessage, Hello, HostMessage, Operation, PROTOCOL_NAME, PROTOCOL_VERSION, Progress, Ready,
    ResultMessage, WorkerMessage,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, watch};

/// Maximum line length accepted from host stdin.
const MAX_HOST_LINE_BYTES: usize = 256 * 1024;

/// Errors raised while speaking the host plugin protocol.
#[derive(Debug, thiserror::Error)]
pub enum HostIoError {
    #[error("i/o error on host pipe: {0}")]
    Io(#[from] std::io::Error),
    #[error("host closed stdin before sending hello")]
    EofBeforeHello,
    #[error("host message exceeded {MAX_HOST_LINE_BYTES} bytes")]
    LineTooLarge,
    #[error("invalid host message: {0}")]
    InvalidMessage(String),
    #[error("unsupported protocol {protocol:?} version {version}")]
    UnsupportedProtocol { protocol: String, version: u32 },
}

/// Reads the initial [`Hello`] handshake from `reader`.
pub async fn read_hello<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Hello, HostIoError> {
    let mut raw = Vec::new();
    let read = read_bounded_line(reader, &mut raw, MAX_HOST_LINE_BYTES).await?;
    if read == 0 {
        return Err(HostIoError::EofBeforeHello);
    }
    let line = trim_line(&raw);
    let message: HostMessage = serde_json::from_str(&line)
        .map_err(|error| HostIoError::InvalidMessage(error.to_string()))?;
    match message {
        HostMessage::Hello(hello) => {
            if hello.protocol != PROTOCOL_NAME || hello.version != PROTOCOL_VERSION {
                return Err(HostIoError::UnsupportedProtocol {
                    protocol: hello.protocol,
                    version: hello.version,
                });
            }
            Ok(hello)
        }
        HostMessage::Cancel(_) => Err(HostIoError::InvalidMessage(
            "expected hello as the first host message, received cancel".to_owned(),
        )),
    }
}

/// Watches the remaining host stdin stream and triggers `cancel_tx` when a
/// matching [`HostMessage::Cancel`] arrives or when stdin reaches EOF.
pub fn spawn_cancel_listener<R>(
    mut reader: R,
    job_id: String,
) -> (watch::Receiver<bool>, tokio::task::JoinHandle<()>)
where
    R: AsyncBufRead + Unpin + Send + 'static,
{
    let (tx, rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        let mut raw = Vec::new();
        loop {
            raw.clear();
            match read_bounded_line(&mut reader, &mut raw, MAX_HOST_LINE_BYTES).await {
                Ok(0) => {
                    // Host closed stdin: treat as cancellation so a detached
                    // worker never lingers behind a dead host.
                    let _ = tx.send(true);
                    break;
                }
                Ok(_) => {
                    let line = trim_line(&raw);
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(HostMessage::Cancel(cancel)) = serde_json::from_str(&line) {
                        if cancel.job_id == job_id {
                            let _ = tx.send(true);
                            break;
                        }
                    }
                }
                Err(_) => {
                    let _ = tx.send(true);
                    break;
                }
            }
        }
    });
    (rx, handle)
}

/// Synchronized writer for outbound [`WorkerMessage`] frames on stdout.
pub struct HostWriter<W> {
    writer: Arc<Mutex<W>>,
    job_id: String,
}

impl<W> Clone for HostWriter<W> {
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
            job_id: self.job_id.clone(),
        }
    }
}

impl<W: AsyncWrite + Unpin> HostWriter<W> {
    pub fn new(writer: W, job_id: impl Into<String>) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            job_id: job_id.into(),
        }
    }

    pub async fn send_ready(
        &self,
        plugin: impl Into<String>,
        capabilities: Vec<String>,
    ) -> std::io::Result<()> {
        let message = WorkerMessage::Ready(Ready {
            protocol: PROTOCOL_NAME.to_owned(),
            version: PROTOCOL_VERSION,
            job_id: self.job_id.clone(),
            plugin: plugin.into(),
            capabilities,
        });
        self.write(&message).await
    }

    pub async fn send_progress(
        &self,
        fraction: f64,
        message: impl Into<String>,
    ) -> std::io::Result<()> {
        let clamped = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let frame = WorkerMessage::Progress(Progress {
            job_id: self.job_id.clone(),
            fraction: clamped,
            message: Some(message.into()),
        });
        self.write(&frame).await
    }

    pub async fn send_result(&self, operations: Vec<Operation>) -> std::io::Result<()> {
        let frame = WorkerMessage::Result(ResultMessage {
            job_id: self.job_id.clone(),
            operations,
        });
        self.write(&frame).await
    }

    pub async fn send_error(
        &self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> std::io::Result<()> {
        let frame = WorkerMessage::Error(ErrorMessage {
            job_id: self.job_id.clone(),
            code: code.into(),
            message: message.into(),
        });
        self.write(&frame).await
    }

    async fn write(&self, message: &WorkerMessage) -> std::io::Result<()> {
        let mut encoded = serde_json::to_vec(message).map_err(std::io::Error::other)?;
        encoded.push(b'\n');
        let mut guard = self.writer.lock().await;
        guard.write_all(&encoded).await?;
        guard.flush().await
    }
}

/// Wraps stdin in a buffered reader for host protocol reads.
pub fn stdin_reader() -> BufReader<tokio::io::Stdin> {
    BufReader::new(tokio::io::stdin())
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_bytes: usize,
) -> Result<usize, HostIoError> {
    use tokio::io::AsyncReadExt;

    let mut total = 0usize;
    loop {
        let remaining = max_bytes.saturating_sub(total).saturating_add(1);
        let mut limited = (&mut *reader).take(remaining as u64);
        let read = limited.read_until(b'\n', buf).await?;
        if read == 0 {
            return Ok(total);
        }
        total += read;
        if total > max_bytes {
            return Err(HostIoError::LineTooLarge);
        }
        if buf.last() == Some(&b'\n') {
            return Ok(total);
        }
    }
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
    use serde_json::json;

    #[tokio::test]
    async fn hello_ready_progress_and_result_round_trip() {
        let hello = HostMessage::Hello(Hello::new(
            "job-1",
            "acp",
            "draft_cards",
            json!({"content": "Newton's laws"}),
        ));
        let line = format!("{}\n", serde_json::to_string(&hello).unwrap());
        let mut reader = BufReader::new(line.as_bytes());
        let parsed = read_hello(&mut reader).await.expect("parse hello");
        assert_eq!(parsed.job_id, "job-1");
        assert_eq!(parsed.plugin, "acp");
        assert_eq!(parsed.capability, "draft_cards");

        let out = Vec::<u8>::new();
        let writer = HostWriter::new(out, "job-1");
        writer
            .send_ready("acp", vec!["draft_cards".to_owned()])
            .await
            .expect("ready");
        writer
            .send_progress(0.5, "working")
            .await
            .expect("progress");
        writer
            .send_error("agent_not_found", "missing binary")
            .await
            .expect("error");

        let guard = writer.writer.lock().await;
        let text = String::from_utf8(guard.clone()).expect("utf8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(matches!(
            WorkerMessage::parse(lines[0]).unwrap(),
            WorkerMessage::Ready(_)
        ));
        assert!(matches!(
            WorkerMessage::parse(lines[1]).unwrap(),
            WorkerMessage::Progress(_)
        ));
        assert!(matches!(
            WorkerMessage::parse(lines[2]).unwrap(),
            WorkerMessage::Error(_)
        ));
    }

    #[tokio::test]
    async fn cancel_listener_fires_on_matching_job_id() {
        let cancel = r#"{"type":"cancel","job_id":"job-42"}"#.to_owned() + "\n";
        let reader = BufReader::new(std::io::Cursor::new(cancel.into_bytes()));
        let (mut rx, handle) = spawn_cancel_listener(reader, "job-42".to_owned());
        rx.changed().await.expect("cancel signal");
        assert!(*rx.borrow());
        handle.await.expect("listener task");
    }

    #[tokio::test]
    async fn rejects_wrong_protocol_version() {
        let bad = r#"{"type":"hello","protocol":"reinforce_plugin","version":99,"job_id":"j","plugin":"acp","capability":"draft_cards","input":{}}"#.to_owned() + "\n";
        let mut reader = BufReader::new(bad.as_bytes());
        let error = read_hello(&mut reader).await.expect_err("bad version");
        assert!(matches!(error, HostIoError::UnsupportedProtocol { .. }));
    }
}
