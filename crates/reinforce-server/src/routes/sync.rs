//! Device sync endpoints.
//!
//! A device uploads its offline attempts, pulls everything the account stored
//! since its last cursor, and fetches a published-content snapshot. All three
//! calls are idempotent: attempts are keyed by id, personal rows by
//! `updated_at`, and content by a snapshot hash.
//!
//! Authentication reuses the session cookie of the account (or the implicit
//! owner in single-user mode). Agent tokens are rejected on purpose: an MCP
//! client reads content and submits proposals, it never writes a learner's
//! study log.

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use reinforce_core::db::study as study_store;
use reinforce_core::primitives::AttemptDirection;
use reinforce_core::study::DEFAULT_DESIRED_RETENTION;
use reinforce_core::{AttemptLog, CardId, UserCardMeta};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody, QueryParams};
use crate::state::AppState;
use crate::sync_content;

/// One upload batch must stay small enough for a single request.
const MAX_ATTEMPTS_PER_PUSH: usize = 500;
const MAX_META_PER_PUSH: usize = 500;
const DEFAULT_PULL_LIMIT: usize = 200;
const MAX_PULL_LIMIT: usize = 1000;
/// Batches are JSON and carry only text, but a full exam can be a few hundred
/// attempts, so the default 2 MiB body cap is too tight.
const SYNC_BODY_LIMIT: usize = 4 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/sync/attempts",
            get(pull_attempts)
                .post(push_attempts)
                .layer(DefaultBodyLimit::max(SYNC_BODY_LIMIT)),
        )
        .route(
            "/sync/user-card-meta",
            get(pull_user_card_meta)
                .put(push_user_card_meta)
                .layer(DefaultBodyLimit::max(SYNC_BODY_LIMIT)),
        )
        .route("/sync/content", get(content_snapshot))
        .route_layer(middleware::from_fn(reject_agent_tokens))
}

/// Sync is for devices only. A bearer token is an MCP agent credential and is
/// never accepted here, even in single-user mode where the owner is implicit.
async fn reject_agent_tokens(request: Request, next: Next) -> Response {
    if request.headers().contains_key(header::AUTHORIZATION) {
        return ApiError::forbidden("agent tokens cannot access the device sync endpoints")
            .into_response();
    }
    next.run(request).await
}

/// Every stored field except the server-assigned cursor and the `user_id`,
/// which is validated separately against the authenticated principal.
fn same_attempt(stored: &AttemptLog, incoming: &AttemptLog) -> bool {
    stored.card_id == incoming.card_id
        && stored.direction == incoming.direction
        && stored.session_mode == incoming.session_mode
        && stored.submitted_payload == incoming.submitted_payload
        && stored.is_correct == incoming.is_correct
        && stored.duration_ms == incoming.duration_ms
        && stored.confidence == incoming.confidence
        && stored.error_tags == incoming.error_tags
        && stored.created_at == incoming.created_at
        && stored.affects_scheduling == incoming.affects_scheduling
}

#[derive(Debug, Deserialize)]
struct PushAttemptsRequest {
    attempts: Vec<AttemptLog>,
}

#[derive(Debug, Deserialize, Default)]
struct PullAttemptsQuery {
    #[serde(default)]
    cursor: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

/// Uploads offline attempts. `accepted` counts newly stored rows, `duplicates`
/// counts rows the server already had with identical content, and `cursor` is
/// the account's current server cursor so the device can start pulling.
async fn push_attempts(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(request): JsonBody<PushAttemptsRequest>,
) -> ApiResult<Json<Value>> {
    if request.attempts.is_empty() {
        return Err(ApiError::bad_request(
            "attempts",
            "at least one attempt is required",
        ));
    }
    if request.attempts.len() > MAX_ATTEMPTS_PER_PUSH {
        return Err(ApiError::bad_request(
            "attempts",
            format!("at most {MAX_ATTEMPTS_PER_PUSH} attempts are accepted per request"),
        ));
    }
    let user_id = principal.user_id.clone();
    let (accepted, duplicates, cursor) = state
        .with_db(move |database| {
            let transaction = database.transaction()?;
            let mut accepted = 0_usize;
            let mut duplicates = 0_usize;
            let mut affected: Vec<(CardId, AttemptDirection)> = Vec::new();
            for attempt in &request.attempts {
                if attempt.user_id != user_id {
                    return Err(ApiError::forbidden(
                        "attempts must belong to the authenticated user",
                    ));
                }
                match study_store::attempt_by_id(&transaction, &attempt.id)? {
                    Some(stored) => {
                        if stored.user_id != user_id {
                            return Err(ApiError::forbidden(
                                "this attempt id belongs to another study store",
                            ));
                        }
                        if !same_attempt(&stored, attempt) {
                            return Err(ApiError::conflict(format!(
                                "attempt {} was already stored with different content",
                                attempt.id.as_str()
                            )));
                        }
                        duplicates += 1;
                    }
                    None => {
                        let stored = AttemptLog {
                            sync_cursor: study_store::next_user_attempt_cursor(
                                &transaction,
                                &user_id,
                            )?,
                            ..attempt.clone()
                        };
                        study_store::append_attempt(&transaction, &stored)?;
                        if stored.affects_scheduling {
                            let key = (stored.card_id.clone(), stored.direction);
                            if !affected.contains(&key) {
                                affected.push(key);
                            }
                        }
                        accepted += 1;
                    }
                }
            }
            for (card_id, direction) in affected {
                let scheduling = study_store::recompute_for_card(
                    &transaction,
                    &user_id,
                    &card_id,
                    direction,
                    DEFAULT_DESIRED_RETENTION,
                )?;
                study_store::save_scheduling(&transaction, &scheduling)?;
            }
            let cursor = study_store::max_attempt_cursor(&transaction, &user_id)?;
            transaction.commit()?;
            Ok((accepted, duplicates, cursor))
        })
        .await?;
    Ok(Json(json!({
        "accepted": accepted,
        "duplicates": duplicates,
        "cursor": cursor,
    })))
}

/// Pulls the account's attempts in server-cursor order.
async fn pull_attempts(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<PullAttemptsQuery>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let cursor = query.cursor.unwrap_or(0);
    if cursor < 0 {
        return Err(ApiError::bad_request(
            "cursor",
            "cursor must not be negative",
        ));
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PULL_LIMIT)
        .clamp(1, MAX_PULL_LIMIT);
    let user_id = principal.user_id;
    let (attempts, has_more) = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut attempts =
                study_store::list_attempts_after(connection, &user_id, cursor, limit + 1)?;
            let has_more = attempts.len() > limit;
            attempts.truncate(limit);
            Ok((attempts, has_more))
        })
        .await?;
    let next_cursor = attempts
        .last()
        .map_or(cursor, |attempt| attempt.sync_cursor);
    Ok(Json(json!({
        "attempts": attempts,
        "next_cursor": next_cursor,
        "has_more": has_more,
    })))
}

#[derive(Debug, Deserialize)]
struct PushUserCardMetaRequest {
    user_card_meta: Vec<UserCardMeta>,
}

/// Uploads personal card rows. Last writer wins on `updated_at`; a row the
/// server already has at the same or a newer stamp is reported as `kept` so the
/// device can pull the winning value.
async fn push_user_card_meta(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(request): JsonBody<PushUserCardMetaRequest>,
) -> ApiResult<Json<Value>> {
    if request.user_card_meta.len() > MAX_META_PER_PUSH {
        return Err(ApiError::bad_request(
            "user_card_meta",
            format!("at most {MAX_META_PER_PUSH} rows are accepted per request"),
        ));
    }
    let user_id = principal.user_id.clone();
    let (applied, kept) = state
        .with_db(move |database| {
            let transaction = database.transaction()?;
            let mut applied = 0_usize;
            let mut kept = 0_usize;
            for meta in &request.user_card_meta {
                if meta.user_id != user_id {
                    return Err(ApiError::forbidden(
                        "study rows must belong to the authenticated user",
                    ));
                }
                match study_store::get_user_card_meta(&transaction, &user_id, &meta.card_id)? {
                    Some(stored) if stored.updated_at >= meta.updated_at => kept += 1,
                    _ => {
                        study_store::save_user_card_meta(&transaction, meta)?;
                        applied += 1;
                    }
                }
            }
            transaction.commit()?;
            Ok((applied, kept))
        })
        .await?;
    Ok(Json(json!({ "applied": applied, "kept": kept })))
}

/// Returns every personal card row of the account. The set is small, so it is
/// sent whole and merged last-writer-wins on the device instead of paging it.
async fn pull_user_card_meta(
    State(state): State<AppState>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let user_id = principal.user_id;
    let rows = state
        .with_db(move |database| {
            Ok(study_store::list_user_card_meta(
                database.connection(),
                &user_id,
            )?)
        })
        .await?;
    Ok(Json(json!({ "user_card_meta": rows })))
}

#[derive(Debug, Deserialize, Default)]
struct ContentQuery {
    /// Snapshot hash the device already stores; an unchanged snapshot is not
    /// re-sent.
    #[serde(default)]
    known: Option<String>,
}

/// Returns the visible content as a bundle plus the blobs still to download.
async fn content_snapshot(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ContentQuery>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let viewer_is_owner = principal.is_owner();
    let blobs = state.blobs.clone();
    let snapshot = state
        .with_db(move |database| {
            sync_content::snapshot(database.connection(), &blobs, viewer_is_owner)
        })
        .await?;
    if query.known.as_deref() == Some(snapshot.snapshot_sha256.as_str()) {
        return Ok(Json(json!({
            "unchanged": true,
            "snapshot_sha256": snapshot.snapshot_sha256,
        })));
    }
    let assets: Vec<Value> = snapshot
        .assets
        .iter()
        .map(|asset| {
            json!({
                "resource_id": asset.resource_id,
                "sha256": asset.sha256,
                "byte_size": asset.byte_size,
            })
        })
        .collect();
    Ok(Json(json!({
        "unchanged": false,
        "snapshot_sha256": snapshot.snapshot_sha256,
        "bundle_jsonl": snapshot.bundle_jsonl,
        "assets": assets,
    })))
}
