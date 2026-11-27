//! Versioned JSON messages exchanged with out-of-process Reinforce plugins.
//!
//! A plugin is a separate program that the owner declares in the server
//! configuration. The host and the plugin talk newline-delimited JSON over the
//! plugin's stdin/stdout: the host writes one [`HostMessage`] per line and reads
//! one [`WorkerMessage`] per line. This crate deliberately depends on nothing
//! but `serde`, so the official plugins can reuse the exact same types without
//! pulling in the domain crate.
//!
//! The wire contract is documented in `plugins/README.md`. The short version:
//!
//! 1. Host sends [`Hello`] with the protocol name, version and the job input.
//! 2. Plugin answers [`Ready`] with the same protocol name and version, then any
//!    number of [`Progress`] messages.
//! 3. Plugin ends with exactly one [`ResultMessage`] or [`ErrorMessage`].
//! 4. The host may send [`Cancel`] at any point; the plugin should exit.
//!
//! Every worker message carries the `job_id` it answers. A plugin that echoes a
//! different id, skips the handshake or emits an unknown message type is
//! rejected by the host.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol identifier carried in every handshake message.
pub const PROTOCOL_NAME: &str = "reinforce_plugin";

/// Current protocol version. Bumped only for incompatible wire changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Upper bound for the serialized job input accepted by the host.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

/// Something the host sends to a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Hello(Hello),
    Cancel(Cancel),
}

/// First message of every job: protocol handshake plus the work itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol: String,
    pub version: u32,
    pub job_id: String,
    pub plugin: String,
    pub capability: String,
    pub input: Value,
}

impl Hello {
    /// Builds a hello message for the current protocol version.
    pub fn new(
        job_id: impl Into<String>,
        plugin: impl Into<String>,
        capability: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            protocol: PROTOCOL_NAME.to_owned(),
            version: PROTOCOL_VERSION,
            job_id: job_id.into(),
            plugin: plugin.into(),
            capability: capability.into(),
            input,
        }
    }
}

/// Host request to stop working on a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    pub job_id: String,
}

/// Something a plugin sends back to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMessage {
    Ready(Ready),
    Progress(Progress),
    Result(ResultMessage),
    Error(ErrorMessage),
}

impl WorkerMessage {
    /// Job id echoed by the plugin; the host rejects a mismatch.
    pub fn job_id(&self) -> &str {
        match self {
            Self::Ready(ready) => ready.job_id.as_str(),
            Self::Progress(progress) => progress.job_id.as_str(),
            Self::Result(result) => result.job_id.as_str(),
            Self::Error(error) => error.job_id.as_str(),
        }
    }

    /// Parses one stdout line.
    pub fn parse(line: &str) -> std::result::Result<Self, ProtocolError> {
        Ok(serde_json::from_str(line)?)
    }
}

/// Handshake answer: which plugin is running and what it can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ready {
    pub protocol: String,
    pub version: u32,
    pub job_id: String,
    pub plugin: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl Ready {
    /// Verifies that the plugin speaks this host's protocol version.
    pub fn check_protocol(&self) -> std::result::Result<(), ProtocolError> {
        if self.protocol != PROTOCOL_NAME {
            return Err(ProtocolError::NameMismatch {
                expected: PROTOCOL_NAME.to_owned(),
                found: self.protocol.clone(),
            });
        }
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                found: self.version,
            });
        }
        Ok(())
    }
}

/// Optional progress report; the host stores the latest value for status views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub job_id: String,
    pub fraction: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Progress {
    /// Fraction clamped into `0..=1`, so a plugin cannot corrupt a status view.
    pub fn clamped_fraction(&self) -> f64 {
        if self.fraction.is_finite() {
            self.fraction.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Final message on success: a proposal batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultMessage {
    pub job_id: String,
    pub operations: Vec<Operation>,
}

/// Final message on failure: a machine-readable code and a human message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorMessage {
    pub job_id: String,
    pub code: String,
    pub message: String,
}

/// One content operation inside a proposal batch.
///
/// The JSON shape is identical to the core batch operation format
/// (`{"op": "...", "temp_ref": "...", "data": {...}}`) so the host can hand the
/// values to the normal proposal pipeline unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub op: OperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_ref: Option<String>,
    pub data: Value,
}

/// Entity kinds a plugin may propose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Resource,
    Anchor,
    KnowledgeCard,
    Note,
    ExamSet,
    QuestionGroup,
    QuestionCard,
    Edge,
}

impl OperationKind {
    /// Wire name of the operation kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Anchor => "anchor",
            Self::KnowledgeCard => "knowledge_card",
            Self::Note => "note",
            Self::ExamSet => "exam_set",
            Self::QuestionGroup => "question_group",
            Self::QuestionCard => "question_card",
            Self::Edge => "edge",
        }
    }
}

/// Protocol-level rejection raised while reading worker output.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("plugin output is not a protocol message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("plugin speaks protocol {found:?}, expected {expected:?}")]
    NameMismatch { expected: String, found: String },
    #[error("plugin speaks protocol version {found}, expected {expected}")]
    VersionMismatch { expected: u32, found: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hello_serializes_with_type_tag_and_protocol_metadata() {
        let hello = Hello::new("job-1", "fake", "ingest_pdf", json!({"resource_id": "r1"}));
        let value = serde_json::to_value(HostMessage::Hello(hello)).expect("encode hello");
        assert_eq!(value["type"], "hello");
        assert_eq!(value["protocol"], PROTOCOL_NAME);
        assert_eq!(value["version"], PROTOCOL_VERSION);
        assert_eq!(value["job_id"], "job-1");
        assert_eq!(value["input"]["resource_id"], "r1");
    }

    #[test]
    fn worker_messages_round_trip() {
        let messages = [
            WorkerMessage::Ready(Ready {
                protocol: PROTOCOL_NAME.to_owned(),
                version: PROTOCOL_VERSION,
                job_id: "job-1".to_owned(),
                plugin: "fake".to_owned(),
                capabilities: vec!["ingest_pdf".to_owned()],
            }),
            WorkerMessage::Progress(Progress {
                job_id: "job-1".to_owned(),
                fraction: 0.5,
                message: Some("halfway".to_owned()),
            }),
            WorkerMessage::Result(ResultMessage {
                job_id: "job-1".to_owned(),
                operations: vec![Operation {
                    op: OperationKind::KnowledgeCard,
                    temp_ref: Some("$card".to_owned()),
                    data: json!({"title": "t"}),
                }],
            }),
            WorkerMessage::Error(ErrorMessage {
                job_id: "job-1".to_owned(),
                code: "unsupported".to_owned(),
                message: "no".to_owned(),
            }),
        ];
        for message in messages {
            let line = serde_json::to_string(&message).expect("encode message");
            assert_eq!(
                WorkerMessage::parse(&line).expect("decode message"),
                message
            );
        }
    }

    #[test]
    fn operations_match_the_core_batch_shape() {
        let operation = Operation {
            op: OperationKind::KnowledgeCard,
            temp_ref: Some("$card_1".to_owned()),
            data: json!({"id": "$card_1"}),
        };
        assert_eq!(
            serde_json::to_value(&operation).expect("encode operation"),
            json!({
                "op": "knowledge_card",
                "temp_ref": "$card_1",
                "data": {"id": "$card_1"},
            })
        );
        let without_ref = Operation {
            op: OperationKind::Edge,
            temp_ref: None,
            data: json!({}),
        };
        assert_eq!(
            serde_json::to_value(&without_ref).expect("encode operation"),
            json!({"op": "edge", "data": {}})
        );
    }

    #[test]
    fn unknown_message_types_are_rejected() {
        let error = WorkerMessage::parse(r#"{"type":"shutdown","job_id":"job-1"}"#)
            .expect_err("unknown type must fail");
        assert!(matches!(error, ProtocolError::Json(_)));
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn version_and_name_mismatches_are_explicit() {
        let mut ready = Ready {
            protocol: PROTOCOL_NAME.to_owned(),
            version: PROTOCOL_VERSION + 1,
            job_id: "job-1".to_owned(),
            plugin: "fake".to_owned(),
            capabilities: vec![],
        };
        let error = ready.check_protocol().expect_err("version must match");
        assert!(matches!(
            error,
            ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                found: _
            }
        ));
        ready.version = PROTOCOL_VERSION;
        ready.protocol = "something_else".to_owned();
        let error = ready.check_protocol().expect_err("name must match");
        assert!(matches!(error, ProtocolError::NameMismatch { .. }));
        ready.protocol = PROTOCOL_NAME.to_owned();
        ready.check_protocol().expect("matching handshake");
    }

    #[test]
    fn progress_fraction_is_clamped() {
        let progress = |fraction| Progress {
            job_id: "job-1".to_owned(),
            fraction,
            message: None,
        };
        assert_eq!(progress(0.25).clamped_fraction(), 0.25);
        assert_eq!(progress(-3.0).clamped_fraction(), 0.0);
        assert_eq!(progress(9.0).clamped_fraction(), 1.0);
        assert_eq!(progress(f64::NAN).clamped_fraction(), 0.0);
    }
}
