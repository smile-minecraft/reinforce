use std::str::FromStr;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, patch, post};
use reinforce_core::content_store;
use reinforce_core::primitives::{ProposalId, ProposalStatus};
use reinforce_core::proposal::batch::snapshot_from_payload;
use reinforce_core::proposal::{
    ApprovalOptions, BatchOperation, SubmitOutcome, approve_batch, batch_from_proposal,
    reject_batch, submit_batch,
};
use reinforce_core::validation::{Actor, validate_snapshot};
use reinforce_core::{BatchPayload, Proposal, ResourceId, Timestamp};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody, PathParam, QueryParams};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/proposals", get(list_proposals).post(submit))
        .route("/proposals/{id}", get(get_proposal))
        .route("/proposals/{id}/approve", post(approve))
        .route("/proposals/{id}/reject", post(reject))
        .route("/proposals/{id}/pins", patch(adjust_pins))
}

#[derive(Debug, Deserialize, Default)]
struct ListQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubmitBatch {
    operations: Vec<BatchOperation>,
}

#[derive(Debug, Deserialize, Default)]
struct ApproveRequest {
    #[serde(default)]
    promote_verified: bool,
}

#[derive(Debug, Deserialize)]
struct RejectRequest {
    review_note: String,
}

#[derive(Debug, Deserialize)]
struct PinAdjustment {
    no: Option<u32>,
    index: Option<usize>,
    x: f64,
    y: f64,
    label_x: f64,
    label_y: f64,
}

#[derive(Debug, Deserialize)]
struct AdjustPinsRequest {
    resource_id: Option<ResourceId>,
    pins: Vec<PinAdjustment>,
}

fn require_owner(principal: &Principal) -> ApiResult<()> {
    if principal.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden("only the owner can review proposals"))
    }
}

async fn list_proposals(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<Proposal>>> {
    require_owner(&principal)?;
    let status = match query.status.as_deref() {
        None => ProposalStatus::Pending,
        Some(value) => ProposalStatus::from_str(value)
            .map_err(|error| ApiError::bad_request("status", error.to_string()))?,
    };
    let proposals = state
        .with_db(move |database| {
            Ok(content_store::list_proposals(
                database.connection(),
                status,
            )?)
        })
        .await?;
    Ok(Json(proposals))
}

async fn get_proposal(
    State(state): State<AppState>,
    PathParam(id): PathParam<ProposalId>,
    principal: Principal,
) -> ApiResult<Json<Proposal>> {
    require_owner(&principal)?;
    let proposal = state
        .with_db(move |database| Ok(content_store::get_proposal(database.connection(), &id)?))
        .await?
        .ok_or_else(|| ApiError::not_found("proposal"))?;
    Ok(Json(proposal))
}

async fn submit(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(request): JsonBody<SubmitBatch>,
) -> ApiResult<Json<Proposal>> {
    require_owner(&principal)?;
    let proposal = state
        .with_db(move |database| {
            match submit_batch(database.connection_mut(), Actor::Owner, request.operations)? {
                SubmitOutcome::Accepted { proposal } => Ok(proposal),
                SubmitOutcome::Rejected { errors } => Err(ApiError::validation(errors)),
            }
        })
        .await?;
    Ok(Json(proposal))
}

async fn approve(
    State(state): State<AppState>,
    PathParam(id): PathParam<ProposalId>,
    principal: Principal,
    JsonBody(request): JsonBody<ApproveRequest>,
) -> ApiResult<Json<Proposal>> {
    require_owner(&principal)?;
    let proposal = state
        .with_db(move |database| {
            match approve_batch(
                database.connection_mut(),
                &id,
                ApprovalOptions {
                    reviewer: Actor::Owner,
                    promote_verified: request.promote_verified,
                },
            )? {
                reinforce_core::ApprovalOutcome::Applied { proposal, .. } => Ok(proposal),
                reinforce_core::ApprovalOutcome::Rejected { errors, .. } => {
                    Err(ApiError::validation(errors))
                }
            }
        })
        .await?;
    Ok(Json(proposal))
}

async fn reject(
    State(state): State<AppState>,
    PathParam(id): PathParam<ProposalId>,
    principal: Principal,
    JsonBody(request): JsonBody<RejectRequest>,
) -> ApiResult<Json<Proposal>> {
    require_owner(&principal)?;
    if request.review_note.trim().is_empty() {
        return Err(ApiError::bad_request("review_note", "cannot be empty"));
    }
    let proposal = state
        .with_db(move |database| {
            Ok(reject_batch(
                database.connection(),
                &id,
                &request.review_note,
            )?)
        })
        .await?;
    Ok(Json(proposal))
}

async fn adjust_pins(
    State(state): State<AppState>,
    PathParam(id): PathParam<ProposalId>,
    principal: Principal,
    JsonBody(request): JsonBody<AdjustPinsRequest>,
) -> ApiResult<Json<Value>> {
    require_owner(&principal)?;
    let result = state
        .with_db(move |database| {
            let mut proposal = content_store::get_proposal(database.connection(), &id)?
                .ok_or_else(|| ApiError::not_found("proposal"))?;
            if proposal.status != ProposalStatus::Pending {
                return Err(ApiError::conflict(
                    "only a pending proposal can be adjusted",
                ));
            }
            let mut payload = batch_from_proposal(&proposal)?;
            let adjusted = apply_adjustments(&mut payload, &request)?;
            let snapshot = snapshot_from_payload(&payload)?;
            let report = validate_snapshot(&snapshot, database.connection())?;
            if !report.is_ok() {
                return Err(ApiError::validation(report));
            }
            proposal.payload = serde_json::to_value(&payload)?;
            proposal.updated_at = Timestamp::now();
            content_store::update_proposal_payload(
                database.connection(),
                &id,
                &proposal.payload,
                proposal.updated_at,
            )?;
            Ok(json!({ "id": proposal.id, "adjusted": adjusted }))
        })
        .await?;
    Ok(Json(result))
}

fn apply_adjustments(payload: &mut BatchPayload, request: &AdjustPinsRequest) -> ApiResult<usize> {
    if request.pins.is_empty() {
        return Err(ApiError::bad_request(
            "pins",
            "at least one pin is required",
        ));
    }
    let mut adjusted = 0;
    for operation in &mut payload.operations {
        let keys: &[&str] = match operation.op {
            reinforce_core::OperationKind::KnowledgeCard => &["blocks"],
            reinforce_core::OperationKind::QuestionCard => {
                &["blocks", "stem_blocks", "explanation_blocks"]
            }
            _ => &[],
        };
        for key in keys {
            let Some(Value::Array(blocks)) = operation.data.get_mut(*key) else {
                continue;
            };
            for block in blocks.iter_mut() {
                if block.get("type").and_then(Value::as_str) != Some("interactive_pins") {
                    continue;
                }
                if let Some(expected) = &request.resource_id {
                    let actual = block
                        .get("resource_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if actual != expected.as_str() {
                        continue;
                    }
                }
                for adjustment in &request.pins {
                    adjusted += apply_to_block(block, adjustment)?;
                }
            }
        }
    }
    if adjusted == 0 {
        return Err(ApiError::bad_request(
            "pins",
            "no matching interactive pin was found in the proposal",
        ));
    }
    Ok(adjusted)
}

fn apply_to_block(block: &mut Value, adjustment: &PinAdjustment) -> ApiResult<usize> {
    for (field, value) in [
        ("x", adjustment.x),
        ("y", adjustment.y),
        ("label_x", adjustment.label_x),
        ("label_y", adjustment.label_y),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(ApiError::bad_request(
                field,
                "must be a finite value in 0..=1",
            ));
        }
    }
    if adjustment.no.is_none() && adjustment.index.is_none() {
        return Err(ApiError::bad_request(
            "pins",
            "each pin needs either no or index",
        ));
    }
    let Some(Value::Array(pins)) = block.get_mut("pins") else {
        return Ok(0);
    };
    let mut changed = 0;
    for (position, pin) in pins.iter_mut().enumerate() {
        let matches = match (adjustment.no, adjustment.index) {
            (Some(number), _) => pin.get("no").and_then(Value::as_u64) == Some(u64::from(number)),
            (None, Some(index)) => index == position,
            _ => false,
        };
        if !matches {
            continue;
        }
        pin["x"] = json!(adjustment.x);
        pin["y"] = json!(adjustment.y);
        pin["label_x"] = json!(adjustment.label_x);
        pin["label_y"] = json!(adjustment.label_y);
        changed += 1;
    }
    Ok(changed)
}
