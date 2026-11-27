//! Offline commands: whole-store validation and standard bundle imports.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, Instant};

use reinforce_core::content_store;
use reinforce_core::plugin_store::{self, JobFilter};
use reinforce_core::proposal::{Preparation, prepare};
use reinforce_core::validation::{Actor, Snapshot, Staged, ValidationReport, validate_snapshot};
use reinforce_core::{
    AnchorId, ApprovalOptions, ApprovalOutcome, Database, EdgeId, ExamSetId, JobId, JobStatus,
    KnowledgeCardId, NoteId, PluginJob, QuestionCardId, QuestionGroupId, ResourceId, SubmitOutcome,
    approve_batch, submit_batch,
};
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::bundle::{self, AssetFile};
use crate::config::{
    CheckArgs, ExportArgs, ExportFormat, ExportPreset, ImportArgs, ImportMode, PluginArgs,
    PluginCommand, PluginStatusArgs, PluginSubmitArgs, ServeArgs,
};
use crate::error::{ApiError, ApiResult};
use crate::export::{
    ExportJob, empty_export, has_content, render_blocking, write_markdown_directory,
};
use crate::export_scope::ExportScope;
use crate::plugins::{PluginConfig, check_submission};
use crate::state::AppState;

const IMPORT_AGENT: &str = "cli-import";

fn print_report(report: &ValidationReport, message: &str) -> ApiResult<()> {
    eprintln!("{message}: {} error(s)", report.errors.len());
    for error in &report.errors {
        let line = serde_json::to_string(error).map_err(ApiError::from)?;
        eprintln!("{line}");
    }
    Ok(())
}

fn validation_error(report: ValidationReport, message: &'static str) -> ApiError {
    let error = ApiError::new(
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        message,
    );
    ApiError {
        details: serde_json::to_value(report).ok(),
        ..error
    }
}

pub(crate) fn ids<T: rusqlite::types::FromSql>(
    connection: &Connection,
    table: &str,
) -> ApiResult<Vec<T>> {
    // `table` is a fixed internal constant at every call site, never user input.
    let mut statement = connection.prepare(&format!("SELECT id FROM {table} ORDER BY id"))?;
    let ids = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

/// Loads the published content store as one snapshot so the same hard gates
/// used for proposals also guard an existing database.
fn store_snapshot(connection: &Connection) -> ApiResult<Snapshot> {
    let mut snapshot = Snapshot::new(Actor::Owner);
    let mut index = 0;
    macro_rules! collect {
        ($table:literal, $id:ty, $get:path, $push:ident) => {{
            for id in ids::<$id>(connection, $table)? {
                let Some(value) = $get(connection, &id)? else {
                    continue;
                };
                snapshot.$push(Staged {
                    op_index: index,
                    temp_ref: None,
                    value,
                });
                index += 1;
            }
        }};
    }
    collect!(
        "resources",
        ResourceId,
        content_store::get_resource,
        push_resource
    );
    collect!("anchors", AnchorId, content_store::get_anchor, push_anchor);
    collect!(
        "exam_sets",
        ExamSetId,
        content_store::get_exam_set,
        push_exam_set
    );
    collect!(
        "question_groups",
        QuestionGroupId,
        content_store::get_question_group,
        push_question_group
    );
    collect!("notes", NoteId, content_store::get_note, push_note);
    collect!(
        "knowledge_cards",
        KnowledgeCardId,
        content_store::get_knowledge_card,
        push_knowledge_card
    );
    collect!(
        "question_cards",
        QuestionCardId,
        content_store::get_question_card,
        push_question_card
    );
    collect!("edges", EdgeId, content_store::get_edge, push_edge);
    let _ = index;
    Ok(snapshot)
}

pub async fn check(args: CheckArgs) -> ApiResult<()> {
    let result = tokio::task::spawn_blocking(move || {
        let database = Database::open(&args.db)?;
        let connection = database.connection();
        let snapshot = store_snapshot(connection)?;
        let report = validate_snapshot(&snapshot, connection)?;
        Ok::<(usize, ValidationReport), ApiError>((snapshot_all_len(&snapshot), report))
    })
    .await
    .map_err(|error| ApiError::internal(format!("validation task failed: {error}")))??;

    let (entities, report) = result;
    if !report.is_ok() {
        print_report(&report, "domain validation failed")?;
        return Err(validation_error(
            report,
            "domain validation failed for the store",
        ));
    }
    println!("reinforce check: ok ({entities} entities validated)");
    Ok(())
}

fn snapshot_all_len(snapshot: &Snapshot) -> usize {
    snapshot.resources.len()
        + snapshot.anchors.len()
        + snapshot.exam_sets.len()
        + snapshot.question_groups.len()
        + snapshot.notes.len()
        + snapshot.knowledge_cards.len()
        + snapshot.question_cards.len()
        + snapshot.edges.len()
}

fn blob_path(root: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or("xx");
    root.join(prefix).join(hash)
}

fn publish_assets(root: &Path, assets: Vec<AssetFile>) -> ApiResult<usize> {
    let mut published = 0;
    for asset in assets {
        let target = blob_path(root, asset.hash.as_str());
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&asset.source, &target)?;
        published += 1;
    }
    Ok(published)
}

/// Import failures are either operational (IO, database) or a structured
/// validation report that must be printed field by field.
enum ImportFailure {
    Api(ApiError),
    Validation(ValidationReport),
}

impl From<ApiError> for ImportFailure {
    fn from(value: ApiError) -> Self {
        Self::Api(value)
    }
}

impl From<reinforce_core::Error> for ImportFailure {
    fn from(value: reinforce_core::Error) -> Self {
        Self::Api(value.into())
    }
}

impl From<ValidationReport> for ImportFailure {
    fn from(value: ValidationReport) -> Self {
        Self::Validation(value)
    }
}

pub async fn import(args: ImportArgs) -> ApiResult<()> {
    let operations = bundle::read_operations(&args.bundle)?;
    let assets = bundle::referenced_assets(&args.bundle, &operations)?;
    let actor = match args.mode {
        ImportMode::Staging => Actor::Agent {
            name: IMPORT_AGENT.to_owned(),
        },
        ImportMode::Owner => Actor::Owner,
    };
    let db = args.db;
    let blobs = args.blobs;
    let mode = args.mode;

    let summary = tokio::task::spawn_blocking(
        move || -> Result<(reinforce_core::ProposalId, usize, usize), ImportFailure> {
            let mut database = Database::open(&db)?;
            // Preflight before touching the blob directory or inserting a proposal.
            // `submit_batch` repeats validation in its own transaction.
            let report = match prepare(database.connection(), actor.clone(), operations.clone())? {
                Preparation::Invalid(report) => report,
                Preparation::Ready(batch) => {
                    validate_snapshot(&batch.snapshot, database.connection())?
                }
            };
            if !report.is_ok() {
                return Err(ImportFailure::Validation(report));
            }
            let blobs_written = publish_assets(&blobs, assets)?;
            let outcome = submit_batch(database.connection_mut(), actor.clone(), operations)?;
            let proposal = match outcome {
                SubmitOutcome::Accepted { proposal } => proposal,
                SubmitOutcome::Rejected { errors } => {
                    return Err(ImportFailure::Validation(errors));
                }
            };
            if mode == ImportMode::Owner {
                match approve_batch(
                    database.connection_mut(),
                    &proposal.id,
                    ApprovalOptions {
                        reviewer: Actor::Owner,
                        promote_verified: false,
                    },
                )? {
                    ApprovalOutcome::Applied { applied, .. } => {
                        Ok((proposal.id, applied, blobs_written))
                    }
                    ApprovalOutcome::Rejected { errors, .. } => {
                        Err(ImportFailure::Validation(errors))
                    }
                }
            } else {
                Ok((proposal.id, 0, blobs_written))
            }
        },
    )
    .await
    .map_err(|error| ApiError::internal(format!("import task failed: {error}")))?;

    let summary = match summary {
        Ok(summary) => summary,
        Err(ImportFailure::Validation(report)) => {
            print_report(&report, "bundle validation failed")?;
            return Err(validation_error(report, "bundle validation failed"));
        }
        Err(ImportFailure::Api(error)) => return Err(error),
    };
    let (proposal_id, applied, blobs_written) = summary;
    println!(
        "{}",
        json!({
            "proposal": proposal_id,
            "mode": match mode { ImportMode::Staging => "staging", ImportMode::Owner => "owner" },
            "applied": applied,
            "blobs_written": blobs_written,
        })
    );
    Ok(())
}

fn export_preset(preset: ExportPreset) -> reinforce_export::PdfPreset {
    match preset {
        ExportPreset::Standard => reinforce_export::PdfPreset::Standard,
        ExportPreset::GoodNotes => reinforce_export::PdfPreset::GoodNotes,
        ExportPreset::Worksheet => reinforce_export::PdfPreset::Worksheet,
    }
}

fn validate_web_base_url(value: Option<&str>) -> ApiResult<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.starts_with("https://") || value.starts_with("http://") {
        Ok(Some(value.to_owned()))
    } else {
        Err(ApiError::bad_request(
            "web_base_url",
            "must be an http or https URL",
        ))
    }
}

fn export_scope(args: &ExportArgs) -> ApiResult<ExportScope> {
    if let Some(note) = &args.note {
        let note_id = note
            .parse()
            .map_err(|_| ApiError::bad_request("note", "invalid note id"))?;
        return Ok(ExportScope::Note { note_id });
    }
    if let Some(subject) = &args.subject {
        if subject.trim().is_empty() {
            return Err(ApiError::bad_request("subject", "cannot be empty"));
        }
        return Ok(ExportScope::Subject {
            subject: subject.clone(),
        });
    }
    let threshold = args
        .weakness_threshold
        .ok_or_else(|| ApiError::bad_request("scope", "exactly one scope is required"))?;
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        return Err(ApiError::bad_request(
            "weakness_threshold",
            "must be between 0 and 1",
        ));
    }
    Ok(ExportScope::Weakness {
        threshold,
        user_id: reinforce_core::UserId::generate(),
    })
}

pub async fn export(mut args: ExportArgs) -> ApiResult<()> {
    let scope = export_scope(&args)?;
    let web_base_url = validate_web_base_url(args.web_base_url.as_deref())?;
    let preset = match (args.format, args.preset) {
        (ExportFormat::Pdf, None) => {
            return Err(ApiError::bad_request("preset", "required for pdf exports"));
        }
        (_, Some(preset)) => Some(export_preset(preset)),
        (ExportFormat::Markdown, None) => None,
    };

    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: std::mem::take(&mut args.db),
        blobs: std::mem::take(&mut args.blobs),
        fonts: std::mem::take(&mut args.fonts),
        auth: crate::config::AuthMode::Single,
        demo: false,
        plugins: None,
    };
    let state = AppState::open(&config).await?;
    let scope = match scope {
        ExportScope::Weakness { threshold, .. } => ExportScope::Weakness {
            threshold,
            user_id: state.owner_id.clone(),
        },
        other => other,
    };
    let blobs = state.blobs.clone();
    let input = state
        .with_db(move |database| {
            crate::export_scope::select(database.connection(), &scope, true, &blobs)
        })
        .await?;
    if !has_content(&input) {
        return Err(empty_export());
    }

    let job = match args.format {
        ExportFormat::Pdf => ExportJob::Pdf {
            preset: preset.expect("pdf preset was validated"),
            fonts_dir: config.fonts.clone(),
            web_base_url,
            viewer_is_owner: true,
        },
        ExportFormat::Markdown => ExportJob::Markdown {
            viewer_is_owner: true,
        },
    };
    let out = args.out;
    if matches!(args.format, ExportFormat::Markdown)
        && out.extension().and_then(|value| value.to_str()) != Some("zip")
    {
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        write_markdown_directory(&input, true, &out)?;
        println!(
            "reinforce export: wrote markdown bundle to {}",
            out.display()
        );
        return Ok(());
    }

    let bytes = render_blocking(&input, job)?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&out, &bytes)?;
    println!(
        "reinforce export: wrote {} bytes to {}",
        bytes.len(),
        out.display()
    );
    Ok(())
}

fn plugin_input(raw: &str) -> ApiResult<Value> {
    let text = if raw == "-" {
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)?;
        buffer
    } else if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path)?
    } else {
        raw.to_owned()
    };
    let input: Value = serde_json::from_str(&text)?;
    if !input.is_object() {
        return Err(ApiError::bad_request("input", "must be a JSON object"));
    }
    Ok(input)
}

fn plugin_job_id(raw: &str) -> ApiResult<JobId> {
    JobId::from_str(raw).map_err(|_| ApiError::bad_request("job", "invalid job id"))
}

async fn insert_plugin_job(db: PathBuf, job: PluginJob) -> ApiResult<PluginJob> {
    tokio::task::spawn_blocking(move || -> ApiResult<PluginJob> {
        let database = Database::open(&db)?;
        plugin_store::insert_job(database.connection(), &job)?;
        Ok(job)
    })
    .await
    .map_err(|error| ApiError::internal(format!("plugin task failed: {error}")))?
}

async fn read_plugin_job(db: PathBuf, id: JobId) -> ApiResult<Option<PluginJob>> {
    tokio::task::spawn_blocking(move || -> ApiResult<Option<PluginJob>> {
        let database = Database::open(&db)?;
        Ok(plugin_store::get_job(database.connection(), &id)?)
    })
    .await
    .map_err(|error| ApiError::internal(format!("plugin task failed: {error}")))?
}

async fn read_plugin_jobs(db: PathBuf, filter: JobFilter) -> ApiResult<Vec<PluginJob>> {
    tokio::task::spawn_blocking(move || -> ApiResult<Vec<PluginJob>> {
        let database = Database::open(&db)?;
        Ok(plugin_store::list_jobs(database.connection(), &filter)?)
    })
    .await
    .map_err(|error| ApiError::internal(format!("plugin task failed: {error}")))?
}

/// Owner CLI for the plugin queue: list manifests, queue a job, read status.
///
/// A queued row is durable, so a running server picks it up within one poll
/// interval even when the CLI process has already exited.
pub async fn plugin(args: PluginArgs) -> ApiResult<()> {
    let config = PluginConfig::load(args.plugins.as_deref())?;
    match args.command {
        PluginCommand::List => {
            println!("{}", serde_json::to_string_pretty(&config.summaries())?);
            Ok(())
        }
        PluginCommand::Submit(submit) => {
            let PluginSubmitArgs {
                plugin,
                capability,
                input,
            } = submit;
            let input = plugin_input(&input)?;
            let definition = check_submission(&config, &plugin, &capability, &input)?;
            let job = PluginJob::queued(&plugin, &capability, input, definition.max_attempts);
            let stored = insert_plugin_job(args.db, job).await?;
            println!("{}", serde_json::to_string_pretty(&stored)?);
            Ok(())
        }
        PluginCommand::Status(status) => status_command(args.db, status).await,
    }
}

async fn status_command(db: PathBuf, status: PluginStatusArgs) -> ApiResult<()> {
    let id = status.job.as_deref().map(plugin_job_id).transpose()?;
    let filter = JobFilter {
        status: match status.status.as_deref() {
            None => None,
            Some(value) => Some(
                JobStatus::from_str(value)
                    .map_err(|error| ApiError::bad_request("status", error.to_string()))?,
            ),
        },
        plugin: status.plugin.clone(),
        limit: Some(status.limit),
    };
    let deadline = Instant::now() + Duration::from_secs(status.wait);
    loop {
        let Some(id) = &id else {
            let jobs = read_plugin_jobs(db, filter).await?;
            println!("{}", serde_json::to_string_pretty(&jobs)?);
            return Ok(());
        };
        let job = read_plugin_job(db.clone(), id.clone())
            .await?
            .ok_or_else(|| ApiError::not_found("plugin job"))?;
        if job.is_terminal() || status.wait == 0 || Instant::now() >= deadline {
            println!("{}", serde_json::to_string_pretty(&job)?);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
