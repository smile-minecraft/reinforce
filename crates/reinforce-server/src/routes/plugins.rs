//! Owner-only HTTP surface for the plugin queue.
//!
//! These routes can queue work for a configured plugin, list jobs, read one job
//! and cancel it. There is deliberately no route that creates, edits or removes
//! a plugin: the manifest is a file only the owner can write.

use std::str::FromStr;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use reinforce_core::plugin_store::JobFilter;
use reinforce_core::{JobId, JobStatus};
use serde::Deserialize;
use serde_json::Value;

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody, PathParam, QueryParams};
use crate::plugins::PluginSummary;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/plugins", get(list_plugins))
        .route("/plugins/jobs", get(list_jobs))
        .route("/plugins/jobs/{id}", get(get_job))
        .route("/plugins/jobs/{id}/cancel", post(cancel_job))
        .route("/plugins/{name}/jobs", post(submit_job))
}

/// Queueing work runs an external program, so it stays owner-only.
fn require_owner(principal: &Principal) -> ApiResult<()> {
    if principal.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden("only the owner can use plugins"))
    }
}

/// The whole submission body. Unknown fields are rejected, so a request cannot
/// smuggle in a command, arguments or a path.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitJobRequest {
    capability: String,
    input: Value,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct JobListQuery {
    status: Option<String>,
    plugin: Option<String>,
    limit: Option<usize>,
}

async fn list_plugins(
    State(state): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<Vec<PluginSummary>>> {
    require_owner(&principal)?;
    Ok(Json(state.plugins.config().summaries()))
}

async fn submit_job(
    State(state): State<AppState>,
    PathParam(name): PathParam<String>,
    principal: Principal,
    JsonBody(request): JsonBody<SubmitJobRequest>,
) -> ApiResult<(StatusCode, Json<reinforce_core::PluginJob>)> {
    require_owner(&principal)?;
    let job = state
        .plugins
        .submit(&name, &request.capability, request.input)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn list_jobs(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<JobListQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<reinforce_core::PluginJob>>> {
    require_owner(&principal)?;
    let status = match query.status.as_deref() {
        None => None,
        Some(value) => Some(
            JobStatus::from_str(value)
                .map_err(|error| ApiError::bad_request("status", error.to_string()))?,
        ),
    };
    let filter = JobFilter {
        status,
        plugin: query.plugin,
        limit: query.limit,
    };
    Ok(Json(state.plugins.list(filter).await?))
}

async fn get_job(
    State(state): State<AppState>,
    PathParam(id): PathParam<JobId>,
    principal: Principal,
) -> ApiResult<Json<reinforce_core::PluginJob>> {
    require_owner(&principal)?;
    let job = state
        .plugins
        .job(&id)
        .await?
        .ok_or_else(|| ApiError::not_found("plugin job"))?;
    Ok(Json(job))
}

async fn cancel_job(
    State(state): State<AppState>,
    PathParam(id): PathParam<JobId>,
    principal: Principal,
) -> ApiResult<Json<reinforce_core::PluginJob>> {
    require_owner(&principal)?;
    let job = state
        .plugins
        .cancel(&id)
        .await?
        .ok_or_else(|| ApiError::not_found("plugin job"))?;
    Ok(Json(job))
}
