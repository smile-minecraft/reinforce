//! The plugin work queue: submission, dispatch, execution and recovery.
//!
//! One [`PluginRuntime`] lives in the server state. It owns the immutable
//! manifest list and a single dispatcher task that runs at most one job at a
//! time. Submitting only validates against the manifest and inserts a row, so
//! the HTTP handler never waits for a plugin; the dispatcher picks the row up,
//! runs the worker and writes the terminal state.
//!
//! Every failure path ends in a recorded job state, and the only content path
//! is [`reinforce_core::submit_batch`] with an agent actor, which forces
//! `verification = unchecked` and leaves the result pending for review.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reinforce_core::proposal::submit_batch;
use reinforce_core::validation::Actor;
use reinforce_core::{
    BatchOperation, JobError, JobErrorCode, JobId, JobStatus, PluginJob, SubmitOutcome,
    plugin_store,
};
use reinforce_plugin_protocol::{MAX_INPUT_BYTES, Operation};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::{ApiError, ApiResult};
use crate::state::Store;

use super::config::{PluginConfig, PluginDefinition};
use super::runner::{self, ProgressReporter, RunOutcome};

/// How long the dispatcher sleeps when the queue is empty.
const IDLE_POLL: Duration = Duration::from_millis(250);

/// What a recovery pass changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct RecoveryReport {
    /// Jobs that were running when the previous process stopped.
    pub interrupted: usize,
    /// Jobs whose plugin is no longer configured.
    pub orphaned: usize,
}

/// Shared boundary check for API and CLI submissions.
///
/// Only a configured plugin, one of its declared capabilities and a bounded
/// JSON object are accepted. Nothing here can influence the command line.
pub fn check_submission<'a>(
    config: &'a PluginConfig,
    plugin: &str,
    capability: &str,
    input: &Value,
) -> ApiResult<&'a PluginDefinition> {
    let definition = config
        .get(plugin)
        .ok_or_else(|| ApiError::not_found(format!("plugin {plugin}")))?;
    if !definition.supports(capability) {
        return Err(ApiError::bad_request(
            "capability",
            format!("plugin {plugin} does not declare the capability {capability}"),
        ));
    }
    if !input.is_object() {
        return Err(ApiError::bad_request("input", "must be a JSON object"));
    }
    let encoded = serde_json::to_vec(input)?;
    if encoded.len() > MAX_INPUT_BYTES {
        return Err(ApiError::bad_request(
            "input",
            format!("must be at most {MAX_INPUT_BYTES} bytes"),
        ));
    }
    Ok(definition)
}

/// The server-side plugin subsystem.
pub struct PluginRuntime {
    store: Store,
    config: PluginConfig,
    running: Mutex<HashMap<JobId, CancellationToken>>,
    dispatcher_started: AtomicBool,
}

impl PluginRuntime {
    pub fn new(store: Store, config: PluginConfig) -> Arc<Self> {
        Arc::new(Self {
            store,
            config,
            running: Mutex::new(HashMap::new()),
            dispatcher_started: AtomicBool::new(false),
        })
    }

    pub fn config(&self) -> &PluginConfig {
        &self.config
    }

    /// Validates a submission against the manifest and queues it.
    ///
    /// The request can only choose a configured plugin, one of its declared
    /// capabilities and a bounded JSON object. It cannot influence the command.
    pub async fn submit(
        &self,
        plugin: &str,
        capability: &str,
        input: Value,
    ) -> ApiResult<PluginJob> {
        let definition = check_submission(&self.config, plugin, capability, &input)?;
        let job = PluginJob::queued(plugin, capability, input, definition.max_attempts);
        self.store
            .call(move |database| {
                plugin_store::insert_job(database.connection(), &job)?;
                Ok(job)
            })
            .await
    }

    pub async fn job(&self, id: &JobId) -> ApiResult<Option<PluginJob>> {
        let id = id.clone();
        self.store
            .call(move |database| Ok(plugin_store::get_job(database.connection(), &id)?))
            .await
    }

    pub async fn list(&self, filter: plugin_store::JobFilter) -> ApiResult<Vec<PluginJob>> {
        self.store
            .call(move |database| Ok(plugin_store::list_jobs(database.connection(), &filter)?))
            .await
    }

    /// Requests cancellation and wakes the runner when the job is running.
    ///
    /// The database write happens first and the token lookup second, so a job
    /// that was claimed concurrently is still stopped: either this lookup sees
    /// the token, or the runner sees the flag when it re-reads the row.
    pub async fn cancel(&self, id: &JobId) -> ApiResult<Option<PluginJob>> {
        let job_id = id.clone();
        let job = self
            .store
            .call(move |database| {
                Ok(plugin_store::request_cancel(
                    database.connection_mut(),
                    &job_id,
                )?)
            })
            .await?;
        if let Some(job) = &job {
            if job.status == JobStatus::Running {
                let token = self
                    .running
                    .lock()
                    .ok()
                    .and_then(|running| running.get(id).cloned());
                if let Some(token) = token {
                    token.cancel();
                }
            }
        }
        Ok(job)
    }

    /// Settles the queue after a restart.
    ///
    /// A job that was running lost its child process with the previous server,
    /// so it becomes `failed/interrupted`. Queued jobs survive and run again;
    /// jobs of a plugin that disappeared from the manifest fail immediately.
    ///
    /// Recovery only runs for a process the owner gave a manifest to. Other
    /// entry points (`reinforce export`, `reinforce mcp`) open the same database
    /// without a manifest and must not fail a running server's jobs or destroy
    /// a queue they do not know about.
    pub async fn recover(&self) -> ApiResult<RecoveryReport> {
        if !self.config.from_file() {
            return Ok(RecoveryReport {
                interrupted: 0,
                orphaned: 0,
            });
        }
        let names = self.config.names();
        self.store
            .call(move |database| {
                let interrupted = plugin_store::fail_interrupted_running(database.connection())?;
                let orphaned = plugin_store::fail_orphaned(database.connection(), &names)?;
                Ok(RecoveryReport {
                    interrupted,
                    orphaned,
                })
            })
            .await
    }

    /// Claims and runs one queued job. Returns false when the queue is empty.
    pub async fn run_pending_once(&self) -> ApiResult<bool> {
        let claimed = self
            .store
            .call(|database| Ok(plugin_store::claim_next_queued(database.connection_mut())?))
            .await?;
        let Some(job) = claimed else {
            return Ok(false);
        };
        self.execute(job).await;
        Ok(true)
    }

    /// Starts the single dispatcher task. Safe to call more than once.
    pub fn start(self: &Arc<Self>) {
        if self.config.is_empty() {
            return;
        }
        if self.dispatcher_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match runtime.run_pending_once().await {
                    Ok(true) => {}
                    Ok(false) => tokio::time::sleep(IDLE_POLL).await,
                    Err(error) => {
                        tracing::error!(
                            error = %error.message,
                            "plugin dispatcher could not read the queue"
                        );
                        tokio::time::sleep(IDLE_POLL).await;
                    }
                }
            }
        });
    }

    async fn execute(&self, job: PluginJob) {
        let Some(definition) = self.config.get(&job.plugin).cloned() else {
            let error = JobError::new(
                JobErrorCode::PluginNotConfigured,
                format!("plugin {:?} is not configured", job.plugin),
            );
            self.fail(&job, error, false).await;
            return;
        };
        // The manifest may have changed since the job was queued; a capability
        // the owner removed must not keep running from old queue rows.
        if !definition.supports(&job.capability) {
            let error = JobError::new(
                JobErrorCode::PluginNotConfigured,
                format!(
                    "plugin {:?} no longer declares the capability {:?}",
                    job.plugin, job.capability
                ),
            );
            self.fail(&job, error, false).await;
            return;
        }
        if job.cancel_requested {
            self.cancel_running(&job).await;
            return;
        }

        let token = CancellationToken::new();
        if let Ok(mut running) = self.running.lock() {
            running.insert(job.id.clone(), token.clone());
        }
        // A cancel may have landed between the claim and this insert; re-read
        // the row so the flag is never lost.
        if let Ok(Some(current)) = self.job(&job.id).await {
            if current.cancel_requested {
                token.cancel();
            }
        }
        let progress = ProgressReporter::new(self.store.clone(), job.id.clone());
        let outcome = runner::run(&definition, &job, token.clone(), progress).await;
        if let Ok(mut running) = self.running.lock() {
            running.remove(&job.id);
        }

        match outcome {
            RunOutcome::Operations(operations) => self.submit_operations(&job, operations).await,
            RunOutcome::PluginError { code, message } => {
                let error = JobError::new(JobErrorCode::PluginReported, message)
                    .with_details(serde_json::json!({ "plugin_code": code }));
                self.fail(&job, error, true).await;
            }
            RunOutcome::Failed(error) => self.fail(&job, error, true).await,
            RunOutcome::Cancelled => self.cancel_running(&job).await,
        }
    }

    /// Sends the operations through the normal proposal pipeline.
    async fn submit_operations(&self, job: &PluginJob, operations: Vec<Operation>) {
        let actor = Actor::Agent {
            name: format!("plugin:{}", job.plugin),
        };
        let job_id = job.id.clone();
        let result = self
            .store
            .call(move |database| {
                let mut batch = Vec::with_capacity(operations.len());
                for operation in operations {
                    let value = match serde_json::to_value(&operation) {
                        Ok(value) => value,
                        Err(error) => {
                            return Ok(Err(JobError::new(
                                JobErrorCode::ProtocolViolation,
                                format!("cannot encode a plugin operation: {error}"),
                            )));
                        }
                    };
                    match serde_json::from_value::<BatchOperation>(value) {
                        Ok(operation) => batch.push(operation),
                        Err(error) => {
                            return Ok(Err(JobError::new(
                                JobErrorCode::ProtocolViolation,
                                format!("cannot decode a plugin operation: {error}"),
                            )));
                        }
                    }
                }
                match submit_batch(database.connection_mut(), actor, batch)? {
                    SubmitOutcome::Accepted { proposal } => {
                        plugin_store::finish_succeeded(
                            database.connection(),
                            &job_id,
                            &proposal.id,
                        )?;
                        Ok(Ok(()))
                    }
                    SubmitOutcome::Rejected { errors } => {
                        let details = serde_json::to_value(&errors)?;
                        Ok(Err(JobError::new(
                            JobErrorCode::ValidationFailed,
                            "the plugin batch failed validation",
                        )
                        .with_details(details)))
                    }
                }
            })
            .await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.fail(job, error, false).await,
            Err(error) => {
                tracing::error!(
                    job = %job.id,
                    error = %error.message,
                    "cannot record the plugin result"
                );
                let error = JobError::new(
                    JobErrorCode::Internal,
                    "the server could not record the plugin result",
                );
                self.fail(job, error, true).await;
            }
        }
    }

    async fn fail(&self, job: &PluginJob, error: JobError, allow_retry: bool) {
        let job_id = job.id.clone();
        let job_code = error.code;
        let result = self
            .store
            .call(move |database| {
                Ok(plugin_store::finish_failed(
                    database.connection(),
                    &job_id,
                    &error,
                    allow_retry,
                )?)
            })
            .await;
        if let Err(store_error) = result {
            tracing::error!(
                job = %job.id,
                code = %job_code,
                error = %store_error.message,
                "cannot record a plugin job failure"
            );
        }
    }

    async fn cancel_running(&self, job: &PluginJob) {
        let error = JobError::new(JobErrorCode::Cancelled, "the job was cancelled");
        let job_id = job.id.clone();
        let result = self
            .store
            .call(move |database| {
                Ok(plugin_store::mark_cancelled(
                    database.connection(),
                    &job_id,
                    &error,
                )?)
            })
            .await;
        if let Err(store_error) = result {
            tracing::error!(
                job = %job.id,
                error = %store_error.message,
                "cannot record a cancelled plugin job"
            );
        }
    }
}
