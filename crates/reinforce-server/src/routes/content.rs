use std::convert::TryFrom;
use std::io::SeekFrom;
use std::str::FromStr;

use axum::body::Body;
use axum::extract::{Multipart, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::StreamExt;
use reinforce_core::content_store;
use reinforce_core::db::search::{self, SearchHit};
use reinforce_core::primitives::{CardStatus, ExportPolicy, ResourceKind};
use reinforce_core::{
    Anchor, AnchorId, ContentHash, EdgeEndpoint, ExamSet, ExamSetId, GraphNode, KnowledgeCard,
    KnowledgeCardId, Note, NoteId, QuestionCard, QuestionCardId, Resource, ResourceId, Timestamp,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params_from_iter};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, PathParam, QueryParams};
use crate::state::AppState;

const MAX_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;
/// Request body allowance for uploads: the file cap plus room for multipart framing.
const UPLOAD_BODY_LIMIT: usize = MAX_UPLOAD_BYTES as usize + 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notes", get(list_notes))
        .route("/notes/{id}", get(get_note))
        .route("/knowledge_cards", get(list_knowledge_cards))
        .route("/knowledge_cards/{id}", get(get_knowledge_card))
        .route("/questions", get(list_questions))
        .route("/questions/{id}", get(get_question))
        .route("/exam_sets", get(list_exam_sets))
        .route("/exam_sets/{id}", get(get_exam_set))
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

#[derive(Debug, Deserialize)]
struct GraphQuery {
    #[serde(default)]
    depth: Option<u32>,
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

async fn list_exam_sets(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<ListQuery>,
) -> ApiResult<Json<Vec<ExamSet>>> {
    let subject = query.subject.clone();
    let sets = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut sql = "SELECT id FROM exam_sets WHERE 1 = 1".to_owned();
            let mut values: Vec<SqlValue> = Vec::new();
            if let Some(subject) = subject.as_deref() {
                values.push(SqlValue::Text(subject.to_owned()));
                sql.push_str(&format!(" AND subject = ?{}", values.len()));
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
                .map(|id| content_store::get_exam_set(connection, &id))
                .collect::<reinforce_core::Result<Vec<_>>>()
                .map(|items| items.into_iter().flatten().collect())
                .map_err(Into::into)
        })
        .await?;
    Ok(Json(sets))
}

async fn get_exam_set(
    State(state): State<AppState>,
    PathParam(id): PathParam<ExamSetId>,
) -> ApiResult<Json<ExamSet>> {
    let set = state
        .with_db(move |database| Ok(content_store::get_exam_set(database.connection(), &id)?))
        .await?
        .ok_or_else(|| ApiError::not_found("exam set"))?;
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
            for hit in search::search_cards(connection, &term, limit)? {
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
            let mut statement = connection.prepare(
                "SELECT id, title FROM notes WHERE title LIKE ?1 ESCAPE '\\'
                    OR subject LIKE ?1 ESCAPE '\\' ORDER BY id LIMIT ?2",
            )?;
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
    let endpoint = match path.kind.as_str() {
        "card" => EdgeEndpoint::Card {
            id: path.id.parse()?,
        },
        "question" => EdgeEndpoint::Question {
            id: path.id.parse()?,
        },
        "option" => EdgeEndpoint::Option {
            id: path.id.parse()?,
            key: String::new(),
        },
        other => {
            return Err(ApiError::bad_request(
                "kind",
                format!("unknown endpoint kind {other}"),
            ));
        }
    };
    let owner = principal.is_owner();
    let nodes = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut visible = Vec::new();
            for node in search::graph_neighbors(connection, &endpoint, depth)? {
                if owner || endpoint_is_published(connection, &node.endpoint)? {
                    visible.push(node);
                }
            }
            Ok(visible)
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

async fn upload_resource(
    State(state): State<AppState>,
    principal: Principal,
    mut multipart: Multipart,
) -> ApiResult<Json<Resource>> {
    if !principal.is_owner() {
        return Err(ApiError::forbidden("only the owner can upload resources"));
    }
    let mut fields = UploadFields::default();
    let mut uploaded: Option<(String, PathBuf, u64)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request("multipart", error.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "file" {
            let temp = state
                .blobs
                .join(format!(".upload-{}", AppState::new_session_id()));
            let mut file = File::create(&temp).await?;
            let mut hasher = Sha256::new();
            let mut size = 0_u64;
            let mut field = field;
            while let Some(chunk) = field.next().await {
                let chunk =
                    chunk.map_err(|error| ApiError::bad_request("file", error.to_string()))?;
                if size + chunk.len() as u64 > MAX_UPLOAD_BYTES {
                    drop(file);
                    let _ = tokio::fs::remove_file(&temp).await;
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
    let Some((hash, temp, size)) = uploaded else {
        return Err(ApiError::bad_request("file", "missing file field"));
    };
    if size == 0 {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(ApiError::bad_request("file", "file is empty"));
    }
    let sha256: ContentHash = hash
        .parse()
        .map_err(|error: reinforce_core::Error| ApiError::from(error))?;
    let target = state.blob_path(sha256.as_str());
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if target.exists() {
        tokio::fs::remove_file(&temp).await?;
    } else {
        tokio::fs::rename(&temp, &target).await?;
    }

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
    let resource = Resource {
        id: ResourceId::generate(),
        kind,
        title: fields
            .title
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ApiError::bad_request("title", "cannot be empty"))?,
        sha256,
        byte_size: i64::try_from(size).map_err(|_| ApiError::bad_request("file", "too large"))?,
        page_count: parse_optional_i64(fields.page_count.as_deref(), "page_count")?,
        width: parse_optional_i64(fields.width.as_deref(), "width")?,
        height: parse_optional_i64(fields.height.as_deref(), "height")?,
        license: fields.license,
        attribution_url: fields.attribution_url,
        attribution_notice: fields.attribution_notice,
        sensitivity,
        export_policy,
        created_at: Timestamp::now(),
    };
    let response = resource.clone();
    state
        .with_db(move |database| {
            content_store::upsert_resource(database.connection(), &resource)?;
            Ok(())
        })
        .await?;
    Ok(Json(response))
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
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string());
    if partial {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|error| ApiError::internal(format!("cannot build response: {error}")))
}

fn content_type_for(resource: &Resource) -> HeaderValue {
    let mime = match resource.kind {
        ResourceKind::Image => {
            if resource.title.to_lowercase().ends_with(".svg") {
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
