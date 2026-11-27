//! Storage for the external plugin work queue.
//!
//! The queue is deliberately small: one table, one worker (the server's
//! dispatcher), and a fixed state machine. Every transition is a single
//! `UPDATE ... WHERE status = ...` so two processes can never both claim the
//! same job, and every terminal state records why it happened.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::plugin::{JobError, JobErrorCode, PluginJob};
use crate::primitives::{Error, JobStatus, Result};
use crate::{JobId, ProposalId, Timestamp};

const JOB_COLUMNS: &str = "id, plugin, capability, input_json, status, attempts, max_attempts,
     cancel_requested, progress, progress_message, proposal_id, error_json,
     created_at, updated_at, started_at, finished_at";

/// Upper bound for one page of job listings.
pub const MAX_JOB_PAGE: usize = 200;

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    Ok(serde_json::from_str(value)?)
}

fn conversion_error(error: Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn row_to_job(row: &Row<'_>) -> rusqlite::Result<PluginJob> {
    let input_json: String = row.get(3)?;
    let error_json: Option<String> = row.get(11)?;
    let input = from_json(&input_json).map_err(conversion_error)?;
    let error = error_json
        .as_deref()
        .map(from_json::<JobError>)
        .transpose()
        .map_err(conversion_error)?;
    Ok(PluginJob {
        id: row.get(0)?,
        plugin: row.get(1)?,
        capability: row.get(2)?,
        input,
        status: row.get(4)?,
        attempts: row.get(5)?,
        max_attempts: row.get(6)?,
        cancel_requested: row.get(7)?,
        progress: row.get(8)?,
        progress_message: row.get(9)?,
        proposal_id: row.get(10)?,
        error,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        started_at: row.get(14)?,
        finished_at: row.get(15)?,
    })
}

/// Optional filters for [`list_jobs`].
#[derive(Debug, Clone, Default)]
pub struct JobFilter {
    pub status: Option<JobStatus>,
    pub plugin: Option<String>,
    pub limit: Option<usize>,
}

impl JobFilter {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, MAX_JOB_PAGE)
    }
}

/// Inserts a freshly queued job.
pub fn insert_job(connection: &Connection, job: &PluginJob) -> Result<()> {
    job.validate()?;
    connection.execute(
        "INSERT INTO plugin_jobs (
            id, plugin, capability, input_json, status, attempts, max_attempts,
            cancel_requested, progress, progress_message, proposal_id, error_json,
            created_at, updated_at, started_at, finished_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            job.id,
            job.plugin,
            job.capability,
            to_json(&job.input)?,
            job.status,
            job.attempts,
            job.max_attempts,
            job.cancel_requested,
            job.progress,
            job.progress_message,
            job.proposal_id,
            job.error.as_ref().map(to_json).transpose()?,
            job.created_at,
            job.updated_at,
            job.started_at,
            job.finished_at,
        ],
    )?;
    Ok(())
}

pub fn get_job(connection: &Connection, id: &JobId) -> Result<Option<PluginJob>> {
    connection
        .query_row(
            &format!("SELECT {JOB_COLUMNS} FROM plugin_jobs WHERE id = ?1"),
            params![id],
            row_to_job,
        )
        .optional()
        .map_err(Into::into)
}

/// Lists jobs newest first.
pub fn list_jobs(connection: &Connection, filter: &JobFilter) -> Result<Vec<PluginJob>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {JOB_COLUMNS} FROM plugin_jobs
         WHERE (?1 IS NULL OR status = ?1)
           AND (?2 IS NULL OR plugin = ?2)
         ORDER BY created_at DESC, id DESC
         LIMIT ?3"
    ))?;
    let rows = statement.query_map(
        params![filter.status, filter.plugin, filter.limit() as i64],
        row_to_job,
    )?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Atomically flips the oldest queued job to `running` and counts the attempt.
///
/// The immediate transaction keeps two dispatchers (or a dispatcher and a CLI
/// submission) from claiming the same row.
pub fn claim_next_queued(connection: &mut Connection) -> Result<Option<PluginJob>> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let next: Option<JobId> = transaction
        .query_row(
            "SELECT id FROM plugin_jobs WHERE status = 'queued'
             ORDER BY created_at, id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(id) = next else {
        return Ok(None);
    };
    let now = Timestamp::now();
    transaction.execute(
        "UPDATE plugin_jobs
         SET status = 'running', attempts = attempts + 1, started_at = ?2,
             updated_at = ?2, finished_at = NULL, progress = 0.0,
             progress_message = NULL, error_json = NULL
         WHERE id = ?1 AND status = 'queued'",
        params![id, now],
    )?;
    let job = get_job(&transaction, &id)?.ok_or_else(|| {
        Error::InvalidValue("plugin job", "job disappeared while claiming".into())
    })?;
    transaction.commit()?;
    Ok(Some(job))
}

/// Records the latest progress report of a running job.
pub fn set_progress(
    connection: &Connection,
    id: &JobId,
    fraction: f64,
    message: Option<String>,
) -> Result<()> {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    connection.execute(
        "UPDATE plugin_jobs
         SET progress = ?2, progress_message = ?3, updated_at = ?4
         WHERE id = ?1 AND status = 'running'",
        params![id, fraction, message, Timestamp::now()],
    )?;
    Ok(())
}

/// Asks a job to stop.
///
/// A queued job becomes `cancelled` immediately; a running job only gets the
/// flag because its worker owns the terminal write. Terminal jobs are returned
/// unchanged so cancelling is safe to retry. The read and the write share one
/// immediate transaction, so a cancel cannot be lost to a concurrent claim.
pub fn request_cancel(connection: &mut Connection, id: &JobId) -> Result<Option<PluginJob>> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let Some(job) = get_job(&transaction, id)? else {
        return Ok(None);
    };
    if job.is_terminal() {
        return Ok(Some(job));
    }
    let now = Timestamp::now();
    if job.status == JobStatus::Queued {
        let error = JobError::new(JobErrorCode::Cancelled, "cancelled before it started");
        transaction.execute(
            "UPDATE plugin_jobs
             SET status = 'cancelled', cancel_requested = 1, error_json = ?2,
                 finished_at = ?3, updated_at = ?3
             WHERE id = ?1 AND status = 'queued'",
            params![id, to_json(&error)?, now],
        )?;
    } else {
        transaction.execute(
            "UPDATE plugin_jobs SET cancel_requested = 1, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
    }
    let job = get_job(&transaction, id)?;
    transaction.commit()?;
    Ok(job)
}

/// Records a successful run that produced a pending proposal.
pub fn finish_succeeded(
    connection: &Connection,
    id: &JobId,
    proposal_id: &ProposalId,
) -> Result<PluginJob> {
    let now = Timestamp::now();
    connection.execute(
        "UPDATE plugin_jobs
         SET status = 'succeeded', proposal_id = ?2, progress = 1.0,
             error_json = NULL, finished_at = ?3, updated_at = ?3
         WHERE id = ?1 AND status = 'running'",
        params![id, proposal_id, now],
    )?;
    get_job(connection, id)?
        .ok_or_else(|| Error::InvalidValue("plugin job", "job does not exist".to_owned()))
}

/// Records a failed run, returning the job to `queued` when a bounded retry is
/// allowed. Only transient codes retry; contract and validation failures are
/// deterministic and fail immediately.
pub fn finish_failed(
    connection: &Connection,
    id: &JobId,
    error: &JobError,
    allow_retry: bool,
) -> Result<PluginJob> {
    let job = get_job(connection, id)?
        .ok_or_else(|| Error::InvalidValue("plugin job", "job does not exist".to_owned()))?;
    let now = Timestamp::now();
    let retry = allow_retry && error.code.is_retryable() && job.attempts < job.max_attempts;
    if retry {
        connection.execute(
            "UPDATE plugin_jobs
             SET status = 'queued', started_at = NULL, finished_at = NULL,
                 progress = 0.0, progress_message = NULL, error_json = ?2,
                 updated_at = ?3
             WHERE id = ?1 AND status = 'running'",
            params![id, to_json(error)?, now],
        )?;
    } else {
        connection.execute(
            "UPDATE plugin_jobs
             SET status = 'failed', error_json = ?2, finished_at = ?3, updated_at = ?3
             WHERE id = ?1 AND status = 'running'",
            params![id, to_json(error)?, now],
        )?;
    }
    get_job(connection, id)?
        .ok_or_else(|| Error::InvalidValue("plugin job", "job does not exist".to_owned()))
}

/// Records that a running job stopped because it was cancelled.
pub fn mark_cancelled(connection: &Connection, id: &JobId, error: &JobError) -> Result<PluginJob> {
    let now = Timestamp::now();
    connection.execute(
        "UPDATE plugin_jobs
         SET status = 'cancelled', cancel_requested = 1, error_json = ?2,
             finished_at = ?3, updated_at = ?3
         WHERE id = ?1 AND status = 'running'",
        params![id, to_json(error)?, now],
    )?;
    get_job(connection, id)?
        .ok_or_else(|| Error::InvalidValue("plugin job", "job does not exist".to_owned()))
}

/// Fails every job that was still running when the server stopped.
///
/// The child process died with the server, so the work cannot be resumed; a
/// retry would restart the worker from scratch and is the owner's decision.
pub fn fail_interrupted_running(connection: &Connection) -> Result<usize> {
    let error = JobError::new(
        JobErrorCode::Interrupted,
        "the server stopped while this job was running",
    );
    let now = Timestamp::now();
    let updated = connection.execute(
        "UPDATE plugin_jobs
         SET status = 'failed', error_json = ?1, finished_at = ?2, updated_at = ?2
         WHERE status = 'running'",
        params![to_json(&error)?, now],
    )?;
    Ok(updated)
}

/// Queued jobs of a plugin that no longer exists in the configuration can never
/// run; failing them keeps the queue honest after a manifest is removed.
pub fn fail_orphaned(connection: &Connection, configured: &[String]) -> Result<usize> {
    let mut failed = 0;
    let mut statement = connection
        .prepare("SELECT DISTINCT plugin FROM plugin_jobs WHERE status IN ('queued','running')")?;
    let plugins = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for plugin in plugins {
        if configured.iter().any(|name| name == &plugin) {
            continue;
        }
        let error = JobError::new(
            JobErrorCode::PluginNotConfigured,
            format!("plugin {plugin:?} is no longer configured"),
        );
        let now = Timestamp::now();
        failed += connection.execute(
            "UPDATE plugin_jobs
             SET status = 'failed', error_json = ?2, finished_at = ?3, updated_at = ?3
             WHERE plugin = ?1 AND status IN ('queued','running')",
            params![plugin, to_json(&error)?, now],
        )?;
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use serde_json::json;

    fn queued(plugin: &str) -> PluginJob {
        PluginJob::queued(plugin, "ingest_pdf", json!({"resource_id": "r1"}), 1)
    }

    fn open() -> Database {
        Database::open_in_memory().expect("open in-memory database")
    }

    #[test]
    fn inserted_jobs_round_trip() {
        let database = open();
        let job = queued("fake");
        insert_job(database.connection(), &job).expect("insert");
        let stored = get_job(database.connection(), &job.id)
            .expect("read")
            .expect("job exists");
        assert_eq!(stored, job);
        assert_eq!(stored.status, JobStatus::Queued);
    }

    #[test]
    fn claim_is_exclusive_and_counts_attempts() {
        let mut database = open();
        let job = queued("fake");
        insert_job(database.connection(), &job).expect("insert");
        let claimed = claim_next_queued(database.connection_mut())
            .expect("claim")
            .expect("one job");
        assert_eq!(claimed.id, job.id);
        assert_eq!(claimed.status, JobStatus::Running);
        assert_eq!(claimed.attempts, 1);
        assert!(claimed.started_at.is_some());
        assert!(
            claim_next_queued(database.connection_mut())
                .expect("claim")
                .is_none()
        );
    }

    #[test]
    fn success_records_the_proposal() {
        let mut database = open();
        let job = queued("fake");
        insert_job(database.connection(), &job).expect("insert");
        claim_next_queued(database.connection_mut()).expect("claim");
        let proposal = ProposalId::generate();
        insert_pending_proposal(&database, &proposal);
        let finished = finish_succeeded(database.connection(), &job.id, &proposal).expect("finish");
        assert_eq!(finished.status, JobStatus::Succeeded);
        assert_eq!(finished.proposal_id, Some(proposal));
        assert_eq!(finished.progress, 1.0);
        assert!(finished.finished_at.is_some());
    }

    fn insert_pending_proposal(database: &Database, id: &ProposalId) {
        let now = Timestamp::now();
        let proposal = crate::content::Proposal {
            id: id.clone(),
            target: crate::content::ProposalTarget::Note(crate::NoteId::generate()),
            payload: json!({}),
            submitted_by: "agent:test".to_owned(),
            status: crate::primitives::ProposalStatus::Pending,
            review_note: None,
            created_at: now,
            updated_at: now,
            sync_cursor: 1,
        };
        crate::content_store::insert_proposal(database.connection(), &proposal)
            .expect("insert proposal");
    }

    #[test]
    fn transient_failures_requeue_until_attempts_run_out() {
        let mut database = open();
        let mut job = queued("fake");
        job.max_attempts = 2;
        insert_job(database.connection(), &job).expect("insert");
        claim_next_queued(database.connection_mut()).expect("claim");
        let error = JobError::new(JobErrorCode::Crashed, "exit 1");
        let retried =
            finish_failed(database.connection(), &job.id, &error, true).expect("fail once");
        assert_eq!(retried.status, JobStatus::Queued);
        assert_eq!(retried.attempts, 1);
        assert!(retried.started_at.is_none());

        claim_next_queued(database.connection_mut()).expect("claim again");
        let failed =
            finish_failed(database.connection(), &job.id, &error, true).expect("fail twice");
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(failed.attempts, 2);
        assert_eq!(
            failed.error.expect("stored error").code,
            JobErrorCode::Crashed
        );
    }

    #[test]
    fn deterministic_failures_never_retry() {
        let mut database = open();
        let mut job = queued("fake");
        job.max_attempts = 5;
        insert_job(database.connection(), &job).expect("insert");
        claim_next_queued(database.connection_mut()).expect("claim");
        let error = JobError::new(JobErrorCode::ProtocolViolation, "bad line");
        let failed = finish_failed(database.connection(), &job.id, &error, true).expect("fail");
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(failed.attempts, 1);
    }

    #[test]
    fn queued_cancellation_is_immediate_and_running_only_flags() {
        let mut database = open();
        let first = queued("fake");
        let second = queued("fake");
        insert_job(database.connection(), &first).expect("insert");
        insert_job(database.connection(), &second).expect("insert");
        let running = claim_next_queued(database.connection_mut())
            .expect("claim")
            .expect("one job");
        let flagged = request_cancel(database.connection_mut(), &running.id)
            .expect("cancel running")
            .expect("job exists");
        assert_eq!(flagged.status, JobStatus::Running);
        assert!(flagged.cancel_requested);

        let cancelled = request_cancel(database.connection_mut(), &second.id)
            .expect("cancel queued")
            .expect("job exists");
        assert_eq!(cancelled.status, JobStatus::Cancelled);
        assert_eq!(
            cancelled.error.expect("cancel reason").code,
            JobErrorCode::Cancelled
        );

        let marked = mark_cancelled(
            database.connection(),
            &running.id,
            &JobError::new(JobErrorCode::Cancelled, "stopped"),
        )
        .expect("mark cancelled");
        assert_eq!(marked.status, JobStatus::Cancelled);
        assert!(marked.finished_at.is_some());

        // Cancelling again is a no-op, not an error.
        let again = request_cancel(database.connection_mut(), &running.id)
            .expect("cancel again")
            .expect("job exists");
        assert_eq!(again.status, JobStatus::Cancelled);
    }

    #[test]
    fn interrupted_running_jobs_fail_on_recovery() {
        let mut database = open();
        let job = queued("fake");
        insert_job(database.connection(), &job).expect("insert");
        claim_next_queued(database.connection_mut()).expect("claim");
        assert_eq!(
            fail_interrupted_running(database.connection()).expect("recover"),
            1
        );
        let recovered = get_job(database.connection(), &job.id)
            .expect("read")
            .expect("job exists");
        assert_eq!(recovered.status, JobStatus::Failed);
        assert_eq!(
            recovered.error.expect("interrupt reason").code,
            JobErrorCode::Interrupted
        );
        assert_eq!(
            fail_interrupted_running(database.connection()).expect("recover"),
            0
        );
    }

    #[test]
    fn orphaned_jobs_fail_when_the_manifest_disappears() {
        let database = open();
        let kept = queued("kept");
        let removed = queued("removed");
        insert_job(database.connection(), &kept).expect("insert");
        insert_job(database.connection(), &removed).expect("insert");
        let failed = fail_orphaned(database.connection(), &["kept".to_owned()]).expect("sweep");
        assert_eq!(failed, 1);
        let orphan = get_job(database.connection(), &removed.id)
            .expect("read")
            .expect("job exists");
        assert_eq!(orphan.status, JobStatus::Failed);
        assert_eq!(
            orphan.error.expect("orphan reason").code,
            JobErrorCode::PluginNotConfigured
        );
        let alive = get_job(database.connection(), &kept.id)
            .expect("read")
            .expect("job exists");
        assert_eq!(alive.status, JobStatus::Queued);
    }

    #[test]
    fn progress_is_clamped_and_only_updates_running_jobs() {
        let mut database = open();
        let job = queued("fake");
        insert_job(database.connection(), &job).expect("insert");
        set_progress(
            database.connection(),
            &job.id,
            0.5,
            Some("early".to_owned()),
        )
        .expect("progress");
        let untouched = get_job(database.connection(), &job.id)
            .expect("read")
            .expect("job exists");
        assert_eq!(untouched.progress, 0.0);

        claim_next_queued(database.connection_mut()).expect("claim");
        set_progress(database.connection(), &job.id, 7.5, Some("late".to_owned()))
            .expect("progress");
        let updated = get_job(database.connection(), &job.id)
            .expect("read")
            .expect("job exists");
        assert_eq!(updated.progress, 1.0);
        assert_eq!(updated.progress_message.as_deref(), Some("late"));
    }

    #[test]
    fn listing_filters_by_status_and_plugin() {
        let database = open();
        let first = queued("alpha");
        let second = queued("beta");
        insert_job(database.connection(), &first).expect("insert");
        insert_job(database.connection(), &second).expect("insert");
        let all = list_jobs(database.connection(), &JobFilter::default()).expect("list");
        assert_eq!(all.len(), 2);
        let alpha = list_jobs(
            database.connection(),
            &JobFilter {
                plugin: Some("alpha".to_owned()),
                ..JobFilter::default()
            },
        )
        .expect("list");
        assert_eq!(alpha.len(), 1);
        assert_eq!(alpha[0].id, first.id);
        let failed = list_jobs(
            database.connection(),
            &JobFilter {
                status: Some(JobStatus::Failed),
                ..JobFilter::default()
            },
        )
        .expect("list");
        assert!(failed.is_empty());
    }
}
