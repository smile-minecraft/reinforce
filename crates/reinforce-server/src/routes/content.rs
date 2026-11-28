use std::collections::HashSet;
use std::convert::TryFrom;
use std::io::SeekFrom;
use std::str::FromStr;

use axum::body::{Body, Bytes};
use axum::extract::rejection::BytesRejection;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, patch, put};
use axum::{Json, Router};
use futures_util::StreamExt;
use reinforce_core::content_store;
use reinforce_core::db::search::{self, SearchHit};
use reinforce_core::db::{quiz as quiz_store, subject_groups as subject_groups_store};
use reinforce_core::primitives::{CardStatus, ExportPolicy, ResourceKind};
use reinforce_core::{
    Anchor, AnchorId, ContentHash, Edge, EdgeEndpoint, ExamSet, ExamSetId, GraphNode,
    KnowledgeCard, KnowledgeCardId, Note, NoteId, QuestionCard, QuestionCardId, QuestionGroup,
    QuestionGroupId, Resource, ResourceId, SubjectGroup, SubjectGroupId, Timestamp,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody, MultipartBody, PathParam, QueryParams};
use crate::state::AppState;

const MAX_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;
/// Request body allowance for uploads: the file cap plus room for multipart framing.
const UPLOAD_BODY_LIMIT: usize = MAX_UPLOAD_BYTES as usize + 1024 * 1024;
const MAX_VIEW_SETTINGS_BYTES: usize = 256 * 1024;
const MAX_CANVAS_NODES: usize = 2000;
const MIN_CANVAS_ZOOM: f64 = 0.1;
const MAX_CANVAS_ZOOM: f64 = 4.0;
const CANVAS_VERSION: u32 = 1;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notes", get(list_notes))
        .route("/notes/{id}", get(get_note))
        .route("/notes/{id}/edges", get(list_note_edges))
        .route("/notes/{id}/view_settings", put(update_note_view_settings))
        .route("/knowledge_cards", get(list_knowledge_cards))
        .route("/knowledge_cards/{id}", get(get_knowledge_card))
        .route(
            "/knowledge_cards/{id}/questions",
            get(list_knowledge_card_questions),
        )
        .route("/questions", get(list_questions))
        .route("/questions/{id}", get(get_question))
        .route("/exam_sets", get(list_exam_sets))
        .route("/exam_sets/{id}", get(get_exam_set))
        .route("/question_groups", get(list_question_groups))
        .route("/question_groups/{id}", get(get_question_group))
        .route(
            "/subject_groups",
            get(list_subject_groups).post(create_subject_group),
        )
        .route(
            "/subject_groups/{id}",
            patch(update_subject_group).delete(delete_subject_group),
        )
        .route("/search", get(search_content))
        .route("/graph/{kind}/{id}/neighbors", get(graph_neighbors))
        .route("/resources/{id}", get(get_resource))
        .route("/resources/{id}/blob", get(stream_blob))
        .route(
            "/resources/upload",
            axum::routing::post(upload_resource)
                // The handler enforces the real per-file cap; this only lifts
                // axum's 2 MiB default so textbook-sized PDFs can reach it.
                .layer(axum::extract::DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/anchors/{id}", get(get_anchor))
}

#[derive(Debug, Deserialize, Default)]
struct ListQuery {
    subject: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct QuestionGroupQuery {
    #[serde(default)]
    exam_set_id: Option<ExamSetId>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct CreateSubjectGroupRequest {
    #[serde(default)]
    parent_id: Option<SubjectGroupId>,
    name: String,
    #[serde(default)]
    subjects: Vec<String>,
    #[serde(default)]
    sort_order: i32,
}

/// `parent_id` distinguishes "field absent" (`None`) from "explicitly null"
/// (`Some(None)`, which moves the group back to the top level).
fn deserialize_optional_parent<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<SubjectGroupId>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<SubjectGroupId>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
struct UpdateSubjectGroupRequest {
    #[serde(default, deserialize_with = "deserialize_optional_parent")]
    parent_id: Option<Option<SubjectGroupId>>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    subjects: Option<Vec<String>>,
    #[serde(default)]
    sort_order: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct GraphQuery {
    #[serde(default)]
    depth: Option<u32>,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphPath {
    kind: String,
    id: String,
}

#[derive(Debug, Default)]
struct UploadFields {
    title: Option<String>,
    kind: Option<String>,
    page_count: Option<String>,
    width: Option<String>,
    height: Option<String>,
    license: Option<String>,
    attribution_url: Option<String>,
    attribution_notice: Option<String>,
    sensitivity: Option<String>,
    export_policy: Option<String>,
}

fn limit_of(value: Option<usize>) -> usize {
    value.unwrap_or(50).clamp(1, 200)
}

fn requested_status(query: &ListQuery, principal: &Principal) -> ApiResult<Option<CardStatus>> {
    match query.status.as_deref() {
        None => Ok(None),
        Some("published") => Ok(Some(CardStatus::Published)),
        Some(_) if !principal.is_owner() => Err(ApiError::forbidden(
            "learners can only read published content",
        )),
        Some(value) => {
            Ok(Some(CardStatus::from_str(value).map_err(|error| {
                ApiError::bad_request("status", error.to_string())
            })?))
        }
    }
}

fn select_ids<T>(
    connection: &Connection,
    table: &'static str,
    subject: Option<&str>,
    status: Option<CardStatus>,
    limit: usize,
) -> ApiResult<Vec<T>>
where
    T: rusqlite::types::FromSql,
{
    let mut sql = format!("SELECT id FROM {table} WHERE 1 = 1");
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(subject) = subject {
        values.push(SqlValue::Text(subject.to_owned()));
        sql.push_str(&format!(" AND subject = ?{}", values.len()));
    }
    if let Some(status) = status {
        values.push(SqlValue::Text(status.as_str().to_owned()));
        sql.push_str(&format!(" AND status = ?{}", values.len()));
    }
    values.push(SqlValue::Integer(i64::try_from(limit).unwrap_or(200)));
    sql.push_str(&format!(" ORDER BY id LIMIT ?{}", values.len()));
    let mut statement = connection.prepare(&sql)?;
    let ids = statement
        .query_map(params_from_iter(values.iter()), |row| row.get::<_, T>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn ensure_visible(status: CardStatus, principal: &Principal) -> ApiResult<()> {
    if principal.is_owner() || status == CardStatus::Published {
        Ok(())
    } else {
        Err(ApiError::not_found("content"))
    }
}

async fn list_notes(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<Note>>> {
    let subject = query.subject.clone();
    let status = requested_status(&query, &principal)?;
    let owner = principal.is_owner();
    let effective = if owner {
        status
    } else {
        Some(CardStatus::Published)
    };
    let notes = state
        .with_db(move |database| {
            let connection = database.connection();
            select_ids::<NoteId>(
                connection,
                "notes",
                subject.as_deref(),
                effective,
                limit_of(query.limit),
            )?
            .into_iter()
            .map(|id| content_store::get_note(connection, &id))
            .collect::<reinforce_core::Result<Vec<_>>>()
            .map(|items| items.into_iter().flatten().collect())
            .map_err(Into::into)
        })
        .await?;
    Ok(Json(notes))
}

async fn get_note(
    State(state): State<AppState>,
    PathParam(id): PathParam<NoteId>,
    principal: Principal,
) -> ApiResult<Json<Note>> {
    let note = state
        .with_db(move |database| Ok(content_store::get_note(database.connection(), &id)?))
        .await?
        .ok_or_else(|| ApiError::not_found("note"))?;
    ensure_visible(note.status, &principal)?;
    Ok(Json(note))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateNoteViewSettingsRequest {
    canvas: CanvasViewSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanvasViewSettings {
    version: u32,
    nodes: Vec<CanvasNodeLayout>,
    viewport: CanvasViewport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanvasNodeLayout {
    card_id: KnowledgeCardId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanvasViewport {
    x: f64,
    y: f64,
    zoom: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FieldError {
    field: String,
    message: String,
}

fn require_owner(principal: &Principal, action: &str) -> ApiResult<()> {
    if principal.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden(format!("only the owner can {action}")))
    }
}

fn invalid_view_settings(errors: Vec<FieldError>) -> ApiError {
    let summary = errors
        .first()
        .map(|first| format!("{}: {}", first.field, first.message))
        .unwrap_or_else(|| "view_settings: invalid canvas layout".to_owned());
    ApiError {
        status: StatusCode::BAD_REQUEST,
        code: "invalid_request".to_owned(),
        message: summary,
        details: Some(json!({ "fields": errors })),
        diagnostic: None,
    }
}

fn validate_canvas_view_settings(
    canvas: &CanvasViewSettings,
    outline_cards: &HashSet<KnowledgeCardId>,
) -> std::result::Result<(), Vec<FieldError>> {
    let mut errors = Vec::new();

    if canvas.version != CANVAS_VERSION {
        errors.push(FieldError {
            field: "canvas.version".to_owned(),
            message: format!("must be {CANVAS_VERSION}"),
        });
    }

    if !canvas.viewport.x.is_finite() {
        errors.push(FieldError {
            field: "canvas.viewport.x".to_owned(),
            message: "must be a finite number".to_owned(),
        });
    }
    if !canvas.viewport.y.is_finite() {
        errors.push(FieldError {
            field: "canvas.viewport.y".to_owned(),
            message: "must be a finite number".to_owned(),
        });
    }
    if !canvas.viewport.zoom.is_finite()
        || !(MIN_CANVAS_ZOOM..=MAX_CANVAS_ZOOM).contains(&canvas.viewport.zoom)
    {
        errors.push(FieldError {
            field: "canvas.viewport.zoom".to_owned(),
            message: format!(
                "must be a finite number between {MIN_CANVAS_ZOOM} and {MAX_CANVAS_ZOOM}"
            ),
        });
    }

    if canvas.nodes.len() > MAX_CANVAS_NODES {
        errors.push(FieldError {
            field: "canvas.nodes".to_owned(),
            message: format!("must contain at most {MAX_CANVAS_NODES} nodes"),
        });
    }

    let mut seen_cards = HashSet::with_capacity(canvas.nodes.len().min(MAX_CANVAS_NODES));
    for (index, node) in canvas.nodes.iter().take(MAX_CANVAS_NODES).enumerate() {
        if !outline_cards.contains(&node.card_id) {
            errors.push(FieldError {
                field: format!("canvas.nodes[{index}].card_id"),
                message: "card_id is not referenced in this note's outline".to_owned(),
            });
        } else if !seen_cards.insert(node.card_id.clone()) {
            errors.push(FieldError {
                field: format!("canvas.nodes[{index}].card_id"),
                message: "duplicate card_id in canvas nodes".to_owned(),
            });
        }

        if !node.x.is_finite() {
            errors.push(FieldError {
                field: format!("canvas.nodes[{index}].x"),
                message: "must be a finite number".to_owned(),
            });
        }
        if !node.y.is_finite() {
            errors.push(FieldError {
                field: format!("canvas.nodes[{index}].y"),
                message: "must be a finite number".to_owned(),
            });
        }
        if !node.w.is_finite() || node.w <= 0.0 {
            errors.push(FieldError {
                field: format!("canvas.nodes[{index}].w"),
                message: "must be a finite number greater than 0".to_owned(),
            });
        }
        if !node.h.is_finite() || node.h <= 0.0 {
            errors.push(FieldError {
                field: format!("canvas.nodes[{index}].h"),
                message: "must be a finite number greater than 0".to_owned(),
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

async fn update_note_view_settings(
    State(state): State<AppState>,
    PathParam(id): PathParam<NoteId>,
    principal: Principal,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Json<Note>> {
    require_owner(&principal, "update note view settings")?;

    let body = body.map_err(|error| {
        invalid_view_settings(vec![FieldError {
            field: "view_settings".to_owned(),
            message: format!("invalid or oversized payload: {error}"),
        }])
    })?;

    if body.len() > MAX_VIEW_SETTINGS_BYTES {
        return Err(invalid_view_settings(vec![FieldError {
            field: "view_settings".to_owned(),
            message: format!("payload exceeds {MAX_VIEW_SETTINGS_BYTES} bytes (256 KiB)"),
        }]));
    }

    let request: UpdateNoteViewSettingsRequest =
        serde_json::from_slice(&body).map_err(|error| {
            invalid_view_settings(vec![FieldError {
                field: "body".to_owned(),
                message: error.to_string(),
            }])
        })?;

    let updated = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut note = content_store::get_note(connection, &id)?
                .ok_or_else(|| ApiError::not_found("note"))?;
            let outline_cards: HashSet<KnowledgeCardId> =
                note.outline_card_ids().into_iter().collect();

            if let Err(errors) = validate_canvas_view_settings(&request.canvas, &outline_cards) {
                return Err(invalid_view_settings(errors));
            }

            let mut merged = match note.view_settings {
                Value::Object(map) => map,
                _ => serde_json::Map::new(),
            };
            merged.insert("canvas".to_owned(), serde_json::to_value(&request.canvas)?);
            let next_settings = Value::Object(merged);
            let serialized = serde_json::to_vec(&next_settings)?;
            if serialized.len() > MAX_VIEW_SETTINGS_BYTES {
                return Err(invalid_view_settings(vec![FieldError {
                    field: "view_settings".to_owned(),
                    message: format!(
                        "merged view_settings exceeds {MAX_VIEW_SETTINGS_BYTES} bytes (256 KiB)"
                    ),
                }]));
            }

            let now = Timestamp::now();
            content_store::update_note_view_settings(connection, &id, &next_settings, now)?;
            note.view_settings = next_settings;
            note.updated_at = now;
            Ok(note)
        })
        .await?;

    Ok(Json(updated))
}

async fn list_note_edges(
    State(state): State<AppState>,
    PathParam(id): PathParam<NoteId>,
    principal: Principal,
) -> ApiResult<Json<Vec<Edge>>> {
    let owner = principal.is_owner();
    let edges = state
        .with_db(move |database| {
            let connection = database.connection();
            let note = content_store::get_note(connection, &id)?
                .ok_or_else(|| ApiError::not_found("note"))?;
            ensure_visible(note.status, &principal)?;
            let card_ids = note.outline_card_ids();
            let edges = content_store::list_edges_between_cards(connection, &card_ids)?;
            if owner {
                return Ok(edges);
            }
            let mut visible = Vec::with_capacity(edges.len());
            for edge in edges {
                if endpoint_is_published(connection, &edge.from)?
                    && endpoint_is_published(connection, &edge.to)?
                {
                    visible.push(edge);
                }
            }
            Ok(visible)
        })
        .await?;
    Ok(Json(edges))
}

async fn list_knowledge_cards(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<KnowledgeCard>>> {
    let subject = query.subject.clone();
    let status = requested_status(&query, &principal)?;
    let effective = if principal.is_owner() {
        status
    } else {
        Some(CardStatus::Published)
    };
    let cards = state
        .with_db(move |database| {
            let connection = database.connection();
            select_ids::<KnowledgeCardId>(
                connection,
                "knowledge_cards",
                subject.as_deref(),
                effective,
                limit_of(query.limit),
            )?
            .into_iter()
            .map(|id| content_store::get_knowledge_card(connection, &id))
            .collect::<reinforce_core::Result<Vec<_>>>()
            .map(|items| items.into_iter().flatten().collect())
            .map_err(Into::into)
        })
        .await?;
    Ok(Json(cards))
}

async fn get_knowledge_card(
    State(state): State<AppState>,
    PathParam(id): PathParam<KnowledgeCardId>,
    principal: Principal,
) -> ApiResult<Json<KnowledgeCard>> {
    let card = state
        .with_db(move |database| {
            Ok(content_store::get_knowledge_card(
                database.connection(),
                &id,
            )?)
        })
        .await?
        .ok_or_else(|| ApiError::not_found("knowledge card"))?;
    ensure_visible(card.status, &principal)?;
    Ok(Json(card))
}

async fn list_questions(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<QuestionCard>>> {
    let subject = query.subject.clone();
    let status = requested_status(&query, &principal)?;
    let effective = if principal.is_owner() {
        status
    } else {
        Some(CardStatus::Published)
    };
    let cards = state
        .with_db(move |database| {
            let connection = database.connection();
            select_ids::<QuestionCardId>(
                connection,
                "question_cards",
                subject.as_deref(),
                effective,
                limit_of(query.limit),
            )?
            .into_iter()
            .map(|id| content_store::get_question_card(connection, &id))
            .collect::<reinforce_core::Result<Vec<_>>>()
            .map(|items| items.into_iter().flatten().collect())
            .map_err(Into::into)
        })
        .await?;
    Ok(Json(cards))
}

async fn get_question(
    State(state): State<AppState>,
    PathParam(id): PathParam<QuestionCardId>,
    principal: Principal,
) -> ApiResult<Json<QuestionCard>> {
    let card = state
        .with_db(move |database| {
            Ok(content_store::get_question_card(
                database.connection(),
                &id,
            )?)
        })
        .await?
        .ok_or_else(|| ApiError::not_found("question card"))?;
    ensure_visible(card.status, &principal)?;
    Ok(Json(card))
}

async fn list_knowledge_card_questions(
    State(state): State<AppState>,
    PathParam(id): PathParam<KnowledgeCardId>,
    QueryParams(query): QueryParams<ListQuery>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let limit = limit_of(query.limit);
    let (question_ids, count) = state
        .with_db(move |database| {
            let connection = database.connection();
            let card = content_store::get_knowledge_card(connection, &id)?
                .ok_or_else(|| ApiError::not_found("knowledge card"))?;
            ensure_visible(card.status, &principal)?;
            let ids = quiz_store::topic_question_ids(connection, std::slice::from_ref(&id), limit)?;
            Ok((
                ids.iter()
                    .map(QuestionCardId::as_str)
                    .collect::<Vec<String>>(),
                ids.len(),
            ))
        })
        .await?;
    Ok(Json(
        json!({ "question_ids": question_ids, "count": count }),
    ))
}

fn question_group_is_visible(connection: &Connection, id: &QuestionGroupId) -> ApiResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM question_cards
             WHERE question_group_id = ?1 AND status = 'published')",
        rusqlite::params![id],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

async fn list_question_groups(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<QuestionGroupQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<QuestionGroup>>> {
    let owner = principal.is_owner();
    let exam_set_id = query.exam_set_id;
    let limit = limit_of(query.limit);
    let groups = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut sql =
                "SELECT id FROM question_groups WHERE (?1 IS NULL OR exam_set_id = ?1)".to_owned();
            if !owner {
                sql.push_str(
                    " AND EXISTS (SELECT 1 FROM question_cards q
                       WHERE q.question_group_id = question_groups.id
                         AND q.status = 'published')",
                );
            }
            sql.push_str(" ORDER BY id LIMIT ?2");
            let mut statement = connection.prepare(&sql)?;
            let ids = statement
                .query_map(
                    rusqlite::params![exam_set_id, i64::try_from(limit).unwrap_or(50)],
                    |row| row.get::<_, QuestionGroupId>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| content_store::get_question_group(connection, &id))
                .collect::<reinforce_core::Result<Vec<_>>>()
                .map(|items| items.into_iter().flatten().collect())
                .map_err(Into::into)
        })
        .await?;
    Ok(Json(groups))
}

async fn get_question_group(
    State(state): State<AppState>,
    PathParam(id): PathParam<QuestionGroupId>,
    principal: Principal,
) -> ApiResult<Json<QuestionGroup>> {
    let owner = principal.is_owner();
    let group = state
        .with_db(move |database| {
            let connection = database.connection();
            let group = content_store::get_question_group(connection, &id)?
                .ok_or_else(|| ApiError::not_found("question group"))?;
            if !owner && !question_group_is_visible(connection, &id)? {
                return Err(ApiError::not_found("question group"));
            }
            Ok(group)
        })
        .await?;
    Ok(Json(group))
}

async fn list_subject_groups(
    State(state): State<AppState>,
    _principal: Principal,
) -> ApiResult<Json<Vec<SubjectGroup>>> {
    let groups = state
        .with_db(move |database| {
            Ok(subject_groups_store::list_subject_groups(
                database.connection(),
            )?)
        })
        .await?;
    Ok(Json(groups))
}

async fn create_subject_group(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(request): JsonBody<CreateSubjectGroupRequest>,
) -> ApiResult<Json<SubjectGroup>> {
    require_owner(&principal, "create subject groups")?;
    let now = Timestamp::now();
    let group = state
        .with_db(move |database| {
            Ok(subject_groups_store::create_subject_group(
                database.connection(),
                request.parent_id.as_ref(),
                &request.name,
                &request.subjects,
                request.sort_order,
                now,
            )?)
        })
        .await?;
    Ok(Json(group))
}

async fn update_subject_group(
    State(state): State<AppState>,
    PathParam(id): PathParam<SubjectGroupId>,
    principal: Principal,
    JsonBody(request): JsonBody<UpdateSubjectGroupRequest>,
) -> ApiResult<Json<SubjectGroup>> {
    require_owner(&principal, "update subject groups")?;
    let now = Timestamp::now();
    let group = state
        .with_db(move |database| {
            let connection = database.connection();
            let existing = subject_groups_store::get_subject_group(connection, &id)?
                .ok_or_else(|| ApiError::not_found("subject group"))?;
            let parent_id = request.parent_id.unwrap_or(existing.parent_id);
            let name = request.name.unwrap_or(existing.name);
            let subjects = request.subjects.unwrap_or(existing.subjects);
            let sort_order = request.sort_order.unwrap_or(existing.sort_order);
            Ok(subject_groups_store::update_subject_group(
                connection,
                &id,
                parent_id.as_ref(),
                &name,
                &subjects,
                sort_order,
                now,
            )?)
        })
        .await?;
    Ok(Json(group))
}

async fn delete_subject_group(
    State(state): State<AppState>,
    PathParam(id): PathParam<SubjectGroupId>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    require_owner(&principal, "delete subject groups")?;
    let deleted = state
        .with_db(move |database| {
            Ok(subject_groups_store::delete_subject_group(
                database.connection(),
                &id,
            )?)
        })
        .await?;
    Ok(Json(json!({ "ok": deleted })))
}

fn has_published_question(connection: &Connection, set: &ExamSet) -> ApiResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM question_cards
             WHERE exam_set_id = ?1 AND status = 'published')",
        rusqlite::params![set.id],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

fn public_exam_set(connection: &Connection, set: ExamSet, owner: bool) -> ApiResult<ExamSet> {
    if !owner {
        if !has_published_question(connection, &set)? {
            return Err(ApiError::not_found("exam set"));
        }
        let mut set = set;
        if let Some(id) = set.source_resource_id.clone() {
            let hidden = content_store::get_resource(connection, &id)?
                .is_none_or(|resource| resource.export_policy == ExportPolicy::OwnerOnly);
            if hidden {
                set.source_resource_id = None;
            }
        }
        Ok(set)
    } else {
        Ok(set)
    }
}

async fn list_exam_sets(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<ExamSet>>> {
    let subject = query.subject.clone();
    let owner = principal.is_owner();
    let sets = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut sql = "SELECT id FROM exam_sets WHERE 1 = 1".to_owned();
            let mut values: Vec<SqlValue> = Vec::new();
            if let Some(subject) = subject.as_deref() {
                values.push(SqlValue::Text(subject.to_owned()));
                sql.push_str(&format!(" AND subject = ?{}", values.len()));
            }
            if !owner {
                sql.push_str(
                    " AND EXISTS (SELECT 1 FROM question_cards q
                       WHERE q.exam_set_id = exam_sets.id AND q.status = 'published')",
                );
            }
            values.push(SqlValue::Integer(
                i64::try_from(limit_of(query.limit)).unwrap_or(50),
            ));
            sql.push_str(&format!(" ORDER BY id LIMIT ?{}", values.len()));
            let mut statement = connection.prepare(&sql)?;
            let ids = statement
                .query_map(params_from_iter(values.iter()), |row| {
                    row.get::<_, ExamSetId>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| {
                    content_store::get_exam_set(connection, &id)?
                        .map(|set| public_exam_set(connection, set, owner))
                        .transpose()
                })
                .collect::<ApiResult<Vec<_>>>()
                .map(|items| items.into_iter().flatten().collect())
        })
        .await?;
    Ok(Json(sets))
}

async fn get_exam_set(
    State(state): State<AppState>,
    PathParam(id): PathParam<ExamSetId>,
    principal: Principal,
) -> ApiResult<Json<ExamSet>> {
    let owner = principal.is_owner();
    let set = state
        .with_db(move |database| {
            let connection = database.connection();
            content_store::get_exam_set(connection, &id)?
                .ok_or_else(|| ApiError::not_found("exam set"))
                .and_then(|set| public_exam_set(connection, set, owner))
        })
        .await?;
    Ok(Json(set))
}

async fn search_content(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<SearchQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<Value>>> {
    let term = query.q.trim().to_owned();
    if term.is_empty() {
        return Err(ApiError::bad_request("q", "cannot be empty"));
    }
    let limit = limit_of(query.limit);
    let owner = principal.is_owner();
    let hits = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut results = Vec::new();
            for hit in search::search_cards_visible(connection, &term, limit, !owner)? {
                match hit {
                    SearchHit::KnowledgeCard(id) => {
                        let card = content_store::get_knowledge_card(connection, &id)?;
                        if let Some(card) = card {
                            if owner || card.status == CardStatus::Published {
                                results.push(json!({ "kind": "knowledge_card", "id": id, "title": card.title }));
                            }
                        }
                    }
                    SearchHit::QuestionCard(id) => {
                        let card = content_store::get_question_card(connection, &id)?;
                        if let Some(card) = card {
                            if owner || card.status == CardStatus::Published {
                                results.push(json!({ "kind": "question_card", "id": id, "title": card.stem_blocks }));
                            }
                        }
                    }
                }
            }
            let escaped = term
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let status_filter = if owner { "" } else { " AND status = 'published'" };
            let mut statement = connection.prepare(&format!(
                "SELECT id, title FROM notes WHERE (title LIKE ?1 ESCAPE '\\'
                    OR subject LIKE ?1 ESCAPE '\\'){status_filter} ORDER BY id LIMIT ?2"
            ))?;
            let notes = statement
                .query_map(rusqlite::params![format!("%{escaped}%"), i64::try_from(limit).unwrap_or(50)], |row| {
                    Ok((row.get::<_, NoteId>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (id, title) in notes {
                let note = content_store::get_note(connection, &id)?;
                if let Some(note) = note {
                    if owner || note.status == CardStatus::Published {
                        results.push(json!({ "kind": "note", "id": id, "title": title }));
                    }
                }
            }
            Ok(results)
        })
        .await?;
    Ok(Json(hits))
}

async fn graph_neighbors(
    State(state): State<AppState>,
    PathParam(path): PathParam<GraphPath>,
    QueryParams(query): QueryParams<GraphQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<GraphNode>>> {
    let depth = query.depth.unwrap_or(1).min(16);
    let requested_key = query.key;
    let owner = principal.is_owner();
    let nodes = state
        .with_db(move |database| {
            let connection = database.connection();
            let endpoint = match path.kind.as_str() {
                "card" => EdgeEndpoint::Card {
                    id: path
                        .id
                        .parse::<KnowledgeCardId>()
                        .map_err(|_| ApiError::bad_request("id", "invalid id"))?,
                },
                "question" => EdgeEndpoint::Question {
                    id: path
                        .id
                        .parse::<QuestionCardId>()
                        .map_err(|_| ApiError::bad_request("id", "invalid id"))?,
                },
                "option" => {
                    let id = path
                        .id
                        .parse::<QuestionCardId>()
                        .map_err(|_| ApiError::bad_request("id", "invalid id"))?;
                    let key = match requested_key {
                        Some(key) => key,
                        None => content_store::get_question_card(connection, &id)?
                            .and_then(|card| card.options.first().map(|option| option.key.clone()))
                            .ok_or_else(|| ApiError::not_found("question option"))?,
                    };
                    EdgeEndpoint::Option { id, key }
                }
                other => {
                    return Err(ApiError::bad_request(
                        "kind",
                        format!("unknown endpoint kind {other}"),
                    ));
                }
            };
            if !owner && !endpoint_is_published(connection, &endpoint)? {
                return Err(ApiError::not_found("content"));
            }
            if owner {
                search::graph_neighbors(connection, &endpoint, depth)
            } else {
                let visible = |candidate: &EdgeEndpoint| {
                    endpoint_is_published(connection, candidate).unwrap_or(false)
                };
                search::graph_neighbors_filtered(connection, &endpoint, depth, visible)
            }
            .map_err(Into::into)
        })
        .await?;
    Ok(Json(nodes))
}

fn endpoint_is_published(connection: &Connection, endpoint: &EdgeEndpoint) -> ApiResult<bool> {
    match endpoint {
        EdgeEndpoint::Card { id } => Ok(content_store::get_knowledge_card(connection, id)?
            .is_some_and(|card| card.status == CardStatus::Published)),
        EdgeEndpoint::Question { id } | EdgeEndpoint::Option { id, .. } => {
            Ok(content_store::get_question_card(connection, id)?
                .is_some_and(|card| card.status == CardStatus::Published))
        }
    }
}

async fn get_resource(
    State(state): State<AppState>,
    PathParam(id): PathParam<ResourceId>,
    principal: Principal,
) -> ApiResult<Json<Resource>> {
    let resource = state
        .with_db(move |database| Ok(content_store::get_resource(database.connection(), &id)?))
        .await?
        .ok_or_else(|| ApiError::not_found("resource"))?;
    if !principal.is_owner() && resource.export_policy == ExportPolicy::OwnerOnly {
        return Err(ApiError::forbidden("this resource is owner-only"));
    }
    Ok(Json(resource))
}

async fn get_anchor(
    State(state): State<AppState>,
    PathParam(id): PathParam<AnchorId>,
    principal: Principal,
) -> ApiResult<Json<Anchor>> {
    let anchor = state
        .with_db(move |database| Ok(content_store::get_anchor(database.connection(), &id)?))
        .await?
        .ok_or_else(|| ApiError::not_found("anchor"))?;
    let resource_id = anchor.resource_id.clone();
    let resource = state
        .with_db(move |database| {
            Ok(content_store::get_resource(
                database.connection(),
                &resource_id,
            )?)
        })
        .await?
        .ok_or_else(|| ApiError::not_found("resource"))?;
    if !principal.is_owner() && resource.export_policy == ExportPolicy::OwnerOnly {
        return Err(ApiError::forbidden("this resource is owner-only"));
    }
    Ok(Json(anchor))
}

struct TempUpload(Option<PathBuf>);

impl TempUpload {
    fn new(root: &Path) -> Self {
        Self(Some(
            root.join(format!(".upload-{}", AppState::new_session_id())),
        ))
    }

    async fn publish(&mut self, target: &Path) -> ApiResult<()> {
        let Some(path) = self.0.clone() else {
            return Ok(());
        };
        if target.exists() {
            tokio::fs::remove_file(&path).await?;
        } else {
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&path, target).await?;
        }
        self.0 = None;
        Ok(())
    }
}

impl Drop for TempUpload {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn upload_resource(
    State(state): State<AppState>,
    principal: Principal,
    MultipartBody(mut multipart): MultipartBody,
) -> ApiResult<Json<Resource>> {
    if !principal.is_owner() {
        return Err(ApiError::forbidden("only the owner can upload resources"));
    }
    let mut fields = UploadFields::default();
    let mut uploaded: Option<(String, TempUpload, u64)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request("multipart", error.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "file" {
            if uploaded.is_some() {
                return Err(ApiError::bad_request(
                    "file",
                    "only one file field is allowed",
                ));
            }
            let temp = TempUpload::new(&state.blobs);
            let path = temp
                .0
                .clone()
                .ok_or_else(|| ApiError::internal("missing upload path"))?;
            let mut file = File::create(&path).await?;
            let mut hasher = Sha256::new();
            let mut size = 0_u64;
            let mut field = field;
            while let Some(chunk) = field.next().await {
                let chunk =
                    chunk.map_err(|error| ApiError::bad_request("file", error.to_string()))?;
                if size + chunk.len() as u64 > MAX_UPLOAD_BYTES {
                    return Err(ApiError::bad_request("file", "upload exceeds 64 MiB"));
                }
                hasher.update(&chunk);
                file.write_all(&chunk).await?;
                size += chunk.len() as u64;
            }
            file.flush().await?;
            drop(file);
            let digest = hasher.finalize();
            let hash = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            uploaded = Some((hash, temp, size));
        } else {
            let text = field
                .text()
                .await
                .map_err(|error| ApiError::bad_request("multipart", error.to_string()))?;
            match name.as_str() {
                "title" => fields.title = Some(text),
                "kind" => fields.kind = Some(text),
                "page_count" => fields.page_count = Some(text),
                "width" => fields.width = Some(text),
                "height" => fields.height = Some(text),
                "license" => fields.license = Some(text),
                "attribution_url" => fields.attribution_url = Some(text),
                "attribution_notice" => fields.attribution_notice = Some(text),
                "sensitivity" => fields.sensitivity = Some(text),
                "export_policy" => fields.export_policy = Some(text),
                _ => {}
            }
        }
    }
    let Some((hash, mut temp, size)) = uploaded else {
        return Err(ApiError::bad_request("file", "missing file field"));
    };
    if size == 0 {
        return Err(ApiError::bad_request("file", "file is empty"));
    }
    let sha256: ContentHash = hash
        .parse()
        .map_err(|error: reinforce_core::Error| ApiError::from(error))?;
    let kind = ResourceKind::from_str(fields.kind.as_deref().unwrap_or("image"))
        .map_err(|error| ApiError::bad_request("kind", error.to_string()))?;
    let sensitivity = fields
        .sensitivity
        .as_deref()
        .unwrap_or("normal")
        .parse()
        .map_err(|error: reinforce_core::Error| {
            ApiError::bad_request("sensitivity", error.to_string())
        })?;
    let export_policy = fields
        .export_policy
        .as_deref()
        .unwrap_or("allow")
        .parse()
        .map_err(|error: reinforce_core::Error| {
            ApiError::bad_request("export_policy", error.to_string())
        })?;
    let width = parse_optional_i64(fields.width.as_deref(), "width")?;
    let height = parse_optional_i64(fields.height.as_deref(), "height")?;
    let page_count = parse_optional_i64(fields.page_count.as_deref(), "page_count")?;
    if width.is_some_and(|value| value <= 0) {
        return Err(ApiError::bad_request("width", "must be greater than zero"));
    }
    if height.is_some_and(|value| value <= 0) {
        return Err(ApiError::bad_request("height", "must be greater than zero"));
    }
    if page_count.is_some_and(|value| value < 0) {
        return Err(ApiError::bad_request("page_count", "must not be negative"));
    }
    let resource = Resource {
        id: ResourceId::generate(),
        kind,
        title: fields
            .title
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ApiError::bad_request("title", "cannot be empty"))?,
        sha256: sha256.clone(),
        byte_size: i64::try_from(size).map_err(|_| ApiError::bad_request("file", "too large"))?,
        page_count,
        width,
        height,
        license: fields.license,
        attribution_url: fields.attribution_url,
        attribution_notice: fields.attribution_notice,
        sensitivity,
        export_policy,
        created_at: Timestamp::now(),
    };
    let target = state.blob_path(sha256.as_str());
    temp.publish(&target).await?;
    let stored = state
        .with_db(move |database| {
            let id = content_store::store_resource(database.connection(), &resource)?;
            content_store::get_resource(database.connection(), &id)?
                .ok_or_else(|| ApiError::not_found("resource"))
        })
        .await?;
    Ok(Json(stored))
}

fn parse_optional_i64(value: Option<&str>, field: &str) -> ApiResult<Option<i64>> {
    match value {
        None => Ok(None),
        Some(text) if text.trim().is_empty() => Ok(None),
        Some(text) => text
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| ApiError::bad_request(field, "must be an integer")),
    }
}

async fn stream_blob(
    State(state): State<AppState>,
    PathParam(id): PathParam<ResourceId>,
    principal: Principal,
    request: Request,
) -> ApiResult<Response> {
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let resource = state
        .with_db(move |database| Ok(content_store::get_resource(database.connection(), &id)?))
        .await?
        .ok_or_else(|| ApiError::not_found("resource"))?;
    if !principal.is_owner() {
        match resource.export_policy {
            ExportPolicy::OwnerOnly => {
                return Err(ApiError::forbidden("this resource is owner-only"));
            }
            ExportPolicy::Redact => {
                return Err(ApiError::forbidden("this resource is redacted"));
            }
            ExportPolicy::Allow => {}
        }
    }
    let path = state.blob_path(resource.sha256.as_str());
    if !path.exists() {
        return Err(ApiError::not_found("resource blob"));
    }
    let size = tokio::fs::metadata(&path).await?.len();
    let content_type = content_type_for(&resource);
    let (start, end, partial) = match range {
        None => (0_u64, size.saturating_sub(1), false),
        Some(header) => parse_range(&header, size).ok_or_else(|| {
            ApiError::new(
                StatusCode::from_u16(416).unwrap_or(StatusCode::BAD_REQUEST),
                "invalid_range",
                "requested range is not satisfiable",
            )
        })?,
    };
    let length = if size == 0 { 0 } else { end - start + 1 };
    let mut file = File::open(&path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    let stream = ReaderStream::new(file.take(length));
    let status = if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header("x-content-type-options", "nosniff")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string());
    if is_svg_resource(&resource) {
        builder = builder.header("content-security-policy", "sandbox");
    }
    if partial {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|error| ApiError::internal(format!("cannot build response: {error}")))
}

fn is_svg_resource(resource: &Resource) -> bool {
    resource.kind == ResourceKind::Image && resource.title.to_lowercase().ends_with(".svg")
}

fn content_type_for(resource: &Resource) -> HeaderValue {
    let mime = match resource.kind {
        ResourceKind::Image => {
            if is_svg_resource(resource) {
                "image/svg+xml"
            } else {
                "image/png"
            }
        }
        ResourceKind::Pdf => "application/pdf",
        ResourceKind::Audio => "audio/mpeg",
        ResourceKind::Video => "video/mp4",
        ResourceKind::Transcript | ResourceKind::Web => "text/plain; charset=utf-8",
    };
    HeaderValue::from_static(mime)
}

fn parse_range(header: &str, size: u64) -> Option<(u64, u64, bool)> {
    let spec = header.strip_prefix("bytes=")?;
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (start_text, end_text) = spec.split_once('-')?;
    if start_text.is_empty() {
        if end_text.is_empty() {
            return None;
        }
        let suffix = end_text.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((size.saturating_sub(suffix), size - 1, true));
    }
    let start = start_text.parse::<u64>().ok()?;
    if start >= size {
        return None;
    }
    let end = if end_text.is_empty() {
        size - 1
    } else {
        end_text.parse::<u64>().ok()?.min(size - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_canvas_rejects_non_finite_values_and_zoom_bounds() {
        let card_a = KnowledgeCardId::generate();
        let outline = HashSet::from([card_a.clone()]);

        let bad_canvas = CanvasViewSettings {
            version: 1,
            nodes: vec![CanvasNodeLayout {
                card_id: card_a,
                x: f64::NAN,
                y: f64::INFINITY,
                w: -10.0,
                h: f64::NEG_INFINITY,
            }],
            viewport: CanvasViewport {
                x: f64::INFINITY,
                y: f64::NAN,
                zoom: 4.5,
            },
        };

        let errors = validate_canvas_view_settings(&bad_canvas, &outline).unwrap_err();
        let fields: HashSet<&str> = errors.iter().map(|err| err.field.as_str()).collect();
        for expected in [
            "canvas.viewport.x",
            "canvas.viewport.y",
            "canvas.viewport.zoom",
            "canvas.nodes[0].x",
            "canvas.nodes[0].y",
            "canvas.nodes[0].w",
            "canvas.nodes[0].h",
        ] {
            assert!(
                fields.contains(expected),
                "expected field error {expected}, got {fields:?}"
            );
        }
    }
}
