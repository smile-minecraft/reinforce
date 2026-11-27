//! Durable records for work handed to out-of-process plugins.
//!
//! A [`PluginJob`] is the queue row: it remembers which configured plugin and
//! capability the work targets, the validated input, how many runs were
//! started, the latest progress, and how the job ended. The result of a job is
//! never content itself: a successful job points at the [`ProposalId`] it
//! produced, so the normal review pipeline stays the only way into the store.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::primitives::{Error, JobStatus, Result};
use crate::{JobId, ProposalId, Timestamp};

/// Why a plugin job failed or was cancelled.
///
/// The code set is closed so clients can branch on it; a plugin's own error
/// code is preserved in `details.plugin_code` instead of replacing the host
/// code. `validation_failed` keeps the full field-level report in `details`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobErrorCode {
    /// The worker did not finish within the configured timeout.
    Timeout,
    /// The worker exited (or closed its output) without a result.
    Crashed,
    /// The worker wrote more bytes than the configured output cap.
    OutputTooLarge,
    /// The worker broke the handshake, ordering or message contract.
    ProtocolViolation,
    /// The configured command could not be started.
    SpawnFailed,
    /// The submitted operations failed domain validation.
    ValidationFailed,
    /// The server restarted while the job was running.
    Interrupted,
    /// The job was cancelled before it produced a result.
    Cancelled,
    /// The plugin named by the job is no longer in the configuration.
    PluginNotConfigured,
    /// The plugin reported an error of its own.
    PluginReported,
    /// The server could not record the result (for example a database error).
    Internal,
}

impl JobErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Crashed => "crashed",
            Self::OutputTooLarge => "output_too_large",
            Self::ProtocolViolation => "protocol_violation",
            Self::SpawnFailed => "spawn_failed",
            Self::ValidationFailed => "validation_failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
            Self::PluginNotConfigured => "plugin_not_configured",
            Self::PluginReported => "plugin_reported",
            Self::Internal => "internal",
        }
    }

    /// Whether a retry can plausibly succeed. Contract and validation failures
    /// are deterministic, so retrying them only burns CPU.
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Crashed | Self::SpawnFailed | Self::Internal
        )
    }
}

impl std::fmt::Display for JobErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Structured failure stored on the job row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobError {
    pub code: JobErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl JobError {
    pub fn new(code: JobErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// One durable queue row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginJob {
    pub id: JobId,
    pub plugin: String,
    pub capability: String,
    pub input: Value,
    pub status: JobStatus,
    /// Runs started so far, including the current one.
    pub attempts: u32,
    pub max_attempts: u32,
    pub cancel_requested: bool,
    pub progress: f64,
    pub progress_message: Option<String>,
    pub proposal_id: Option<ProposalId>,
    pub error: Option<JobError>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

impl PluginJob {
    /// Queues a new job with a fresh id.
    pub fn queued(
        plugin: impl Into<String>,
        capability: impl Into<String>,
        input: Value,
        max_attempts: u32,
    ) -> Self {
        let now = Timestamp::now();
        Self {
            id: JobId::generate(),
            plugin: plugin.into(),
            capability: capability.into(),
            input,
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts: max_attempts.max(1),
            cancel_requested: false,
            progress: 0.0,
            progress_message: None,
            proposal_id: None,
            error: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            finished_at: None,
        }
    }

    /// True while the job can still change state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
        )
    }

    /// Rejects a job row whose stored state is internally inconsistent.
    pub fn validate(&self) -> Result<()> {
        if self.plugin.trim().is_empty() {
            return Err(Error::InvalidValue(
                "plugin",
                "a job needs a plugin name".to_owned(),
            ));
        }
        if self.capability.trim().is_empty() {
            return Err(Error::InvalidValue(
                "capability",
                "a job needs a capability".to_owned(),
            ));
        }
        if !self.input.is_object() {
            return Err(Error::InvalidValue(
                "input",
                "job input must be a JSON object".to_owned(),
            ));
        }
        if self.max_attempts == 0 {
            return Err(Error::InvalidValue(
                "max_attempts",
                "at least one attempt is required".to_owned(),
            ));
        }
        if self.status == JobStatus::Succeeded && self.proposal_id.is_none() {
            return Err(Error::InvalidValue(
                "proposal_id",
                "a succeeded job must point at its proposal".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_jobs_are_queued_and_valid() {
        let job = PluginJob::queued("fake", "ingest_pdf", json!({"resource_id": "r1"}), 2);
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.attempts, 0);
        assert_eq!(job.max_attempts, 2);
        assert!(!job.is_terminal());
        job.validate().expect("a queued job is valid");
    }

    #[test]
    fn zero_attempts_are_lifted_to_one() {
        let job = PluginJob::queued("fake", "ingest_pdf", json!({}), 0);
        assert_eq!(job.max_attempts, 1);
    }

    #[test]
    fn succeeded_jobs_must_point_at_a_proposal() {
        let mut job = PluginJob::queued("fake", "ingest_pdf", json!({}), 1);
        job.status = JobStatus::Succeeded;
        assert!(job.validate().is_err());
    }

    #[test]
    fn only_transient_codes_are_retryable() {
        for code in [
            JobErrorCode::Timeout,
            JobErrorCode::Crashed,
            JobErrorCode::SpawnFailed,
            JobErrorCode::Internal,
        ] {
            assert!(code.is_retryable(), "{code}");
        }
        for code in [
            JobErrorCode::ProtocolViolation,
            JobErrorCode::ValidationFailed,
            JobErrorCode::OutputTooLarge,
            JobErrorCode::Cancelled,
        ] {
            assert!(!code.is_retryable(), "{code}");
        }
    }

    #[test]
    fn error_codes_serialize_snake_case() {
        assert_eq!(
            serde_json::to_value(JobErrorCode::OutputTooLarge).expect("encode"),
            json!("output_too_large")
        );
        assert_eq!(
            serde_json::to_value(JobErrorCode::PluginNotConfigured).expect("encode"),
            json!("plugin_not_configured")
        );
    }
}
