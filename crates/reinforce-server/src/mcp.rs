//! Built-in MCP server.
//!
//! Every write tool submits as `Actor::Agent { name }`; agents can read all
//! content and proposals but can never approve or reject a proposal. Both the
//! stdio transport and the Streamable HTTP transport share this handler.

use std::path::PathBuf;
use std::str::FromStr;

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use reinforce_core::content::collect_outline_card_ids;
use reinforce_core::db::search::{self, SearchHit};
use reinforce_core::primitives::{CardStatus, ProposalStatus, ResourceKind};
use reinforce_core::proposal::{
    BatchOperation, OperationKind, SubmitOutcome, batch_from_proposal, submit_batch,
};
use reinforce_core::validation::Actor;
use reinforce_core::weakness::build_report;
use reinforce_core::{
    AnchorId, EdgeEndpoint, KnowledgeCardId, NoteId, ProposalId, QuestionCardId, ResourceId, UserId,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Extensions};
use rmcp::schemars::JsonSchema;
use rmcp::service::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::auth::resolve_agent_token;
use crate::cli;
use crate::config::{AuthMode, McpArgs, ServeArgs};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResourceListParams {
    /// Optional media kind: pdf, image, audio, video, transcript or web.
    pub kind: Option<String>,
    /// Maximum number of resources to return, from 1 to 200 (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResourcePageParams {
    /// UUIDv7 of the resource to inspect.
    pub resource_id: String,
    /// Page number to inspect. PDF anchors with a matching page are returned.
    pub page: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CardSearchParams {
    /// Free-text query matched against card topics and block text.
    pub q: String,
    /// Maximum number of hits, from 1 to 200 (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CardGetParams {
    /// UUIDv7 of the knowledge card to fetch.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuestionGetParams {
    /// UUIDv7 of the question card to fetch.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteListParams {
    /// Optional subject filter.
    pub subject: Option<String>,
    /// Optional card status filter: draft, published or archived.
    pub status: Option<String>,
    /// Maximum number of notes to return, from 1 to 200 (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteGetParams {
    /// UUIDv7 of the note to fetch along with its outline cards.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExamSetListParams {
    /// Optional subject filter.
    pub subject: Option<String>,
    /// Maximum number of exam sets to return, from 1 to 200 (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct SubjectGroupListParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GraphParams {
    /// Endpoint kind: card, question or option.
    pub kind: String,
    /// UUIDv7 of the endpoint entity.
    pub id: String,
    /// Traversal depth, capped at 16 (default 1).
    pub depth: Option<u32>,
    /// Required option key when kind is option.
    pub key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WeaknessParams {
    /// User id for the study store; defaults to the local owner.
    pub user_id: Option<String>,
    /// Maximum number of weak items per category, from 1 to 500 (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchLine {
    /// Operation name: resource, anchor, knowledge_card, note, exam_set,
    /// question_group, question_card or edge.
    pub op: String,
    /// Optional in-batch alias such as $card_1; must be unique in the batch.
    pub temp_ref: Option<String>,
    /// Complete entity snapshot. Ids and references may use $aliases.
    pub data: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubmitBatchParams {
    /// Ordered batch lines. Validation is atomic: no row is written on failure.
    pub operations: Vec<BatchLine>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProposalListParams {
    /// Proposal status: pending, approved or rejected (default pending).
    pub status: Option<String>,
    /// Maximum number of proposals to return, from 1 to 200 (default 50).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProposalGetParams {
    /// UUIDv7 of the proposal; rejected proposals include their review note.
    pub id: String,
}

#[derive(Debug, Clone)]
struct AgentIdentity(String);

#[derive(Clone)]
pub struct McpServer {
    state: AppState,
    agent_name: String,
}

impl McpServer {
    pub fn new(state: AppState, agent_name: String) -> Self {
        Self { state, agent_name }
    }

    fn actor_from_extensions(&self, extensions: &Extensions) -> Actor {
        let name = extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<AgentIdentity>())
            .map(|identity| identity.0.clone())
            .unwrap_or_else(|| self.agent_name.clone());
        Actor::Agent { name }
    }
}

fn limit_or(value: Option<usize>, default: usize, max: usize) -> usize {
    value.unwrap_or(default).clamp(1, max)
}

fn success(value: Value) -> Result<CallToolResult, ErrorData> {
    let mut result = CallToolResult::success(vec![ContentBlock::json(&value)?]);
    result.structured_content = Some(value);
    Ok(result)
}

fn tool_error(value: Value) -> Result<CallToolResult, ErrorData> {
    let mut result = CallToolResult::error(vec![ContentBlock::json(&value)?]);
    result.structured_content = Some(value);
    Ok(result)
}

fn mcp_error(error: ApiError) -> ErrorData {
    ErrorData::internal_error(error.message, error.details)
}

fn parse_id<T: FromStr>(value: &str, field: &str) -> Result<T, ErrorData>
where
    T::Err: std::fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| ErrorData::invalid_params(format!("invalid {field}: {error}"), None))
}

fn batch_lines_to_operations(lines: Vec<BatchLine>) -> Result<Vec<BatchOperation>, ErrorData> {
    lines
        .into_iter()
        .map(|line| {
            let kind = serde_json::from_value::<OperationKind>(json!(line.op)).map_err(|_| {
                ErrorData::invalid_params(
                    format!("unknown operation {}", line.op),
                    Some(json!({ "op": line.op })),
                )
            })?;
            Ok(BatchOperation::new(kind, line.temp_ref, line.data))
        })
        .collect()
}

#[tool_router]
impl McpServer {
    #[tool(description = "List stored resources with optional kind filter and result limit.")]
    pub async fn resource_list(
        &self,
        Parameters(params): Parameters<ResourceListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind = params
            .kind
            .as_deref()
            .map(ResourceKind::from_str)
            .transpose()
            .map_err(|error| ErrorData::invalid_params(format!("invalid kind: {error}"), None))?;
        let limit = limit_or(params.limit, 50, 200);
        let resources = self
            .state
            .with_db(move |database| {
                let connection = database.connection();
                let mut ids = cli::ids::<ResourceId>(connection, "resources")?;
                if let Some(kind) = kind {
                    ids.retain(|id| {
                        reinforce_core::content_store::get_resource(connection, id)
                            .is_ok_and(|resource| resource.is_some_and(|r| r.kind == kind))
                    });
                }
                ids.truncate(limit);
                ids.into_iter()
                    .map(|id| {
                        reinforce_core::content_store::get_resource(connection, &id)?
                            .ok_or_else(|| ApiError::not_found("resource"))
                    })
                    .collect::<ApiResult<Vec<_>>>()
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "resources": resources }))
    }

    #[tool(description = "Read PDF/image page metadata and source anchors. No OCR is performed.")]
    pub async fn resource_read_page(
        &self,
        Parameters(params): Parameters<ResourcePageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = parse_id::<ResourceId>(&params.resource_id, "resource_id")?;
        let page = params.page;
        let blobs = self.state.blobs.clone();
        let result = self
            .state
            .with_db(move |database| {
                let connection = database.connection();
                let resource = reinforce_core::content_store::get_resource(connection, &id)?
                    .ok_or_else(|| ApiError::not_found("resource"))?;
                let anchor_ids = cli::ids::<AnchorId>(connection, "anchors")?;
                let mut anchors = Vec::new();
                for anchor_id in anchor_ids {
                    let Some(anchor) =
                        reinforce_core::content_store::get_anchor(connection, &anchor_id)?
                    else {
                        continue;
                    };
                    if anchor.resource_id != id {
                        continue;
                    }
                    if [anchor.pdf_page, anchor.printed_page, anchor.legend_pdf_page]
                        .into_iter()
                        .flatten()
                        .any(|value| value == page)
                    {
                        anchors.push(anchor);
                    }
                }
                let hash = resource.sha256.as_str();
                let blob = blobs.join(hash.get(..2).unwrap_or("xx")).join(hash);
                Ok(json!({
                    "resource": resource,
                    "page": page,
                    "anchors": anchors,
                    "blob": {
                        "sha256": resource.sha256.as_str(),
                        "path": blob,
                        "stored": blob.exists(),
                    }
                }))
            })
            .await
            .map_err(mcp_error)?;
        success(result)
    }

    #[tool(
        description = "Search knowledge cards and question cards visible to an agent, returning ids and compact previews."
    )]
    pub async fn card_search(
        &self,
        Parameters(params): Parameters<CardSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let term = params.q.trim().to_owned();
        if term.is_empty() {
            return Err(ErrorData::invalid_params("q cannot be empty", None));
        }
        let limit = limit_or(params.limit, 50, 200);
        let hits = self
            .state
            .with_db(move |database| {
                let connection = database.connection();
                let mut results = Vec::new();
                for hit in search::search_cards_visible(connection, &term, limit, false)? {
                    match hit {
                        SearchHit::KnowledgeCard(id) => {
                            let card =
                                reinforce_core::content_store::get_knowledge_card(connection, &id)?;
                            results.push(json!({
                                "kind": "knowledge_card",
                                "id": id,
                                "subject": card.as_ref().map(|c| c.subject.as_str()),
                                "title": card.as_ref().map(|c| c.title.as_str()),
                                "status": card.as_ref().map(|c| c.status),
                            }));
                        }
                        SearchHit::QuestionCard(id) => {
                            let card =
                                reinforce_core::content_store::get_question_card(connection, &id)?;
                            results.push(json!({
                                "kind": "question_card",
                                "id": id,
                                "subject": card.as_ref().map(|c| c.subject.as_str()),
                                "question_type": card.as_ref().map(|c| c.question_type),
                                "status": card.as_ref().map(|c| c.status),
                            }));
                        }
                    }
                }
                Ok(results)
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "hits": hits }))
    }

    #[tool(
        description = "Fetch a complete knowledge card by UUIDv7, including all content blocks."
    )]
    pub async fn card_get(
        &self,
        Parameters(params): Parameters<CardGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = parse_id::<KnowledgeCardId>(&params.id, "id")?;
        let card = self
            .state
            .with_db(move |database| {
                reinforce_core::content_store::get_knowledge_card(database.connection(), &id)?
                    .ok_or_else(|| ApiError::not_found("knowledge_card"))
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "card": card }))
    }

    #[tool(
        description = "Fetch a complete question card by UUIDv7, including stem, options and answer."
    )]
    pub async fn question_get(
        &self,
        Parameters(params): Parameters<QuestionGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = parse_id::<QuestionCardId>(&params.id, "id")?;
        let question = self
            .state
            .with_db(move |database| {
                reinforce_core::content_store::get_question_card(database.connection(), &id)?
                    .ok_or_else(|| ApiError::not_found("question_card"))
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "question": question }))
    }

    #[tool(description = "List notes with optional subject and status filters.")]
    pub async fn note_list(
        &self,
        Parameters(params): Parameters<NoteListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let status = params
            .status
            .as_deref()
            .map(CardStatus::from_str)
            .transpose()
            .map_err(|error| ErrorData::invalid_params(format!("invalid status: {error}"), None))?;
        let subject = params
            .subject
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let limit = limit_or(params.limit, 50, 200);
        let notes = self
            .state
            .with_db(move |database| {
                Ok(reinforce_core::catalog_store::list_notes(
                    database.connection(),
                    subject.as_deref(),
                    status,
                    limit,
                )?)
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "notes": notes }))
    }

    #[tool(
        description = "Fetch a complete note by UUIDv7 along with the knowledge cards referenced in its outline."
    )]
    pub async fn note_get(
        &self,
        Parameters(params): Parameters<NoteGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = parse_id::<NoteId>(&params.id, "id")?;
        let result = self
            .state
            .with_db(move |database| {
                let connection = database.connection();
                let note = reinforce_core::content_store::get_note(connection, &id)?
                    .ok_or_else(|| ApiError::not_found("note"))?;
                let card_ids = collect_outline_card_ids(&note.outline);
                let mut cards = Vec::with_capacity(card_ids.len());
                for card_id in card_ids {
                    if let Some(card) =
                        reinforce_core::content_store::get_knowledge_card(connection, &card_id)?
                    {
                        cards.push(card);
                    }
                }
                Ok(json!({ "note": note, "cards": cards }))
            })
            .await
            .map_err(mcp_error)?;
        success(result)
    }

    #[tool(description = "List exam sets with question counts and optional subject filter.")]
    pub async fn exam_set_list(
        &self,
        Parameters(params): Parameters<ExamSetListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let subject = params
            .subject
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let limit = limit_or(params.limit, 50, 200);
        let exam_sets = self
            .state
            .with_db(move |database| {
                let summaries = reinforce_core::catalog_store::list_exam_sets(
                    database.connection(),
                    subject.as_deref(),
                    limit,
                )?;
                Ok(summaries
                    .into_iter()
                    .map(|summary| {
                        json!({
                            "exam_set": summary.exam_set,
                            "question_count": summary.question_count,
                            "published_question_count": summary.published_question_count,
                        })
                    })
                    .collect::<Vec<_>>())
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "exam_sets": exam_sets }))
    }

    #[tool(description = "List all nested subject groups and their assigned subjects.")]
    pub async fn subject_group_list(
        &self,
        Parameters(_params): Parameters<SubjectGroupListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let groups = self
            .state
            .with_db(move |database| {
                Ok(reinforce_core::subject_groups_store::list_subject_groups(
                    database.connection(),
                )?)
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "groups": groups }))
    }

    #[tool(description = "Traverse typed semantic edges from a card, question or question option.")]
    pub async fn card_get_graph(
        &self,
        Parameters(params): Parameters<GraphParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let depth = params.depth.unwrap_or(1).min(16);
        let endpoint = match params.kind.as_str() {
            "card" => EdgeEndpoint::Card {
                id: parse_id::<KnowledgeCardId>(&params.id, "id")?,
            },
            "question" => EdgeEndpoint::Question {
                id: parse_id::<QuestionCardId>(&params.id, "id")?,
            },
            "option" => EdgeEndpoint::Option {
                id: parse_id::<QuestionCardId>(&params.id, "id")?,
                key: params
                    .key
                    .ok_or_else(|| ErrorData::invalid_params("key is required for option", None))?,
            },
            other => {
                return Err(ErrorData::invalid_params(
                    format!("unknown endpoint kind {other}"),
                    None,
                ));
            }
        };
        let nodes = self
            .state
            .with_db(move |database| {
                Ok(search::graph_neighbors(
                    database.connection(),
                    &endpoint,
                    depth,
                )?)
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "nodes": nodes }))
    }

    #[tool(description = "Build a study weakness report for a user; defaults to the owner.")]
    pub async fn study_weakness_report(
        &self,
        Parameters(params): Parameters<WeaknessParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let user_id = match params.user_id {
            Some(value) => value,
            None => self.state.owner_id.as_str().to_owned(),
        };
        let limit = limit_or(params.limit, 50, 500);
        let report = self
            .state
            .with_db(move |database| {
                Ok(build_report(
                    database.connection(),
                    &UserId::from_str(&user_id)
                        .map_err(|error| ApiError::bad_request("user_id", error.to_string()))?,
                    limit,
                )?)
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "report": report }))
    }

    #[tool(
        description = "Submit an atomic batch as an agent proposal; validation errors are returned as a structured report."
    )]
    pub async fn proposal_submit_batch(
        &self,
        extensions: Extensions,
        Parameters(params): Parameters<SubmitBatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let operations = batch_lines_to_operations(params.operations)?;
        let actor = self.actor_from_extensions(&extensions);
        let outcome = self
            .state
            .with_db(move |database| {
                Ok(submit_batch(database.connection_mut(), actor, operations)?)
            })
            .await
            .map_err(mcp_error)?;
        match outcome {
            SubmitOutcome::Accepted { proposal } => success(json!({ "proposal": proposal })),
            SubmitOutcome::Rejected { errors } => tool_error(json!({
                "error": "validation_failed",
                "errors": errors.errors,
            })),
        }
    }

    #[tool(
        description = "List proposals by status, including rejected ones with their review notes."
    )]
    pub async fn proposal_list(
        &self,
        Parameters(params): Parameters<ProposalListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let status_text = params.status.unwrap_or_else(|| "pending".to_owned());
        let status = ProposalStatus::from_str(&status_text)
            .map_err(|error| ErrorData::invalid_params(format!("invalid status: {error}"), None))?;
        let limit = limit_or(params.limit, 50, 200);
        let proposals = self
            .state
            .with_db(move |database| {
                let mut proposals =
                    reinforce_core::content_store::list_proposals(database.connection(), status)?;
                proposals.truncate(limit);
                Ok(proposals)
            })
            .await
            .map_err(mcp_error)?;
        success(json!({ "proposals": proposals }))
    }

    #[tool(description = "Read one proposal, its review note and its decoded batch operations.")]
    pub async fn proposal_get(
        &self,
        Parameters(params): Parameters<ProposalGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = parse_id::<ProposalId>(&params.id, "id")?;
        let result = self
            .state
            .with_db(move |database| {
                let proposal =
                    reinforce_core::content_store::get_proposal(database.connection(), &id)?
                        .ok_or_else(|| ApiError::not_found("proposal"))?;
                let operations = batch_from_proposal(&proposal)?.operations;
                Ok(json!({ "proposal": proposal, "operations": operations }))
            })
            .await
            .map_err(mcp_error)?;
        success(result)
    }
}

#[tool_handler(
    router = Self::tool_router(),
    name = "reinforce",
    instructions = "Reinforce is an AI-authored, human-reviewed study platform. Use read tools first (subject_group_list, note_list, exam_set_list, resource_list, card_search) and fetch full entities with note_get, card_get, question_get, resource_read_page, card_get_graph or study_weakness_report before drafting changes. All writes must go through proposal_submit_batch as ordered operations (op: resource | anchor | knowledge_card | note | exam_set | question_group | question_card | edge) containing complete entity snapshots. Use unique $alias strings in temp_ref and reference them across operations in the same batch; permanent UUIDv7 ids are assigned on commit. Proposals are always staged as pending with verification=unchecked for human review—agents cannot approve or reject proposals. If a proposal is rejected, inspect feedback via proposal_list(status='rejected') and proposal_get, then submit a revised batch."
)]
impl ServerHandler for McpServer {}

pub async fn run_stdio(args: McpArgs) -> ApiResult<()> {
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 3000,
        db: args.db,
        blobs: args.blobs,
        fonts: PathBuf::from("./fonts"),
        auth: AuthMode::Single,
        demo: false,
        plugins: None,
    };
    let state = AppState::open(&config).await?;
    let service = McpServer::new(state, args.agent_name)
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await
        .map_err(|error| ApiError::internal(format!("MCP handshake failed: {error}")))?;
    service
        .waiting()
        .await
        .map_err(|error| ApiError::internal(format!("MCP transport stopped: {error}")))
        .map(|_| ())
}

pub fn http_router(state: AppState) -> axum::Router<AppState> {
    // HTTP callers act as one shared agent identity: no per-client privilege
    // difference exists, and approval is never exposed over MCP.
    let server = McpServer::new(state.clone(), "mcp-http".to_owned());
    let allowed_hosts = [
        state.bind_host.clone(),
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    let config = StreamableHttpServerConfig::default().with_json_response(true);
    // Single mode has no login, so DNS-rebinding protection stays on. Invite
    // mode authorizes every call with a bearer token and is usually served
    // behind a reverse proxy under a public hostname, so the Host check is off.
    let config = match state.auth_mode {
        AuthMode::Single => config.with_allowed_hosts(allowed_hosts),
        AuthMode::Invite => config.disable_allowed_hosts(),
    };
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );

    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_agent_token,
        ))
}

async fn require_agent_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if state.auth_mode != AuthMode::Invite {
        return next.run(request).await;
    }

    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim())
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    let Some(token) = token else {
        return ApiError::unauthorized().into_response();
    };
    let resolved = state
        .with_db(move |database| resolve_agent_token(database.connection(), &token))
        .await;
    match resolved {
        Ok(Some(summary)) => {
            request.extensions_mut().insert(AgentIdentity(summary.name));
            next.run(request).await
        }
        Ok(None) => ApiError::unauthorized().into_response(),
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_expose_descriptions_and_required_fields() {
        let tools = McpServer::tool_router();
        let tools = tools.list_all();
        assert_eq!(tools.len(), 14);
        let submit = tools
            .iter()
            .find(|tool| tool.name == "proposal_submit_batch")
            .expect("submit tool");
        assert!(
            submit
                .description
                .as_deref()
                .is_some_and(|text| text.contains("agent proposal"))
        );
        let schema = submit
            .input_schema
            .get("properties")
            .and_then(Value::as_object);
        assert!(schema.is_some_and(|fields| fields.contains_key("operations")));
        let required = submit
            .input_schema
            .get("required")
            .and_then(Value::as_array);
        assert!(required.is_some_and(|fields| fields.iter().any(|v| v == "operations")));
    }

    #[test]
    fn batch_lines_reject_unknown_operations() {
        let lines = vec![BatchLine {
            op: "flashcard".to_owned(),
            temp_ref: None,
            data: json!({"id": "x"}),
        }];
        assert!(batch_lines_to_operations(lines).is_err());
    }

    async fn test_state() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("reinforce-mcp-{}", uuid::Uuid::now_v7()));
        let config = ServeArgs {
            host: "127.0.0.1".to_owned(),
            port: 3000,
            db: root.join("reinforce.db"),
            blobs: root.join("blobs"),
            fonts: root.join("fonts"),
            auth: AuthMode::Single,
            demo: false,
            plugins: None,
        };
        let state = AppState::open(&config).await.expect("state");
        (state, root)
    }

    fn question_line(id: &str, answer: Value) -> BatchLine {
        BatchLine {
            op: "question_card".to_owned(),
            temp_ref: Some(id.to_owned()),
            data: json!({
                "id": id,
                "exam_set_id": null,
                "question_group_id": null,
                "question_no": null,
                "subject": "physics",
                "topics": [],
                "stem_blocks": [{"type":"markdown","text":"Which?"}],
                "explanation_blocks": [],
                "anchor_ids": [],
                "question_type": "single_choice",
                "grading_mode": "exact",
                "options": [
                    {"key":"A","content":"a","explanation":null,"linked_card_ids":[]},
                    {"key":"B","content":"b","explanation":null,"linked_card_ids":[]}
                ],
                "answer": answer,
                "accepted_answers": [],
                "answer_source_kind": "official",
                "answer_basis": null,
                "duplicate_question_id": null,
                "status": "draft",
                "verification": "unchecked",
                "unsure_count": 0,
                "metadata": {},
                "created_at": "2026-01-01T00:00:00.000000Z",
                "updated_at": "2026-01-01T00:00:00.000000Z"
            }),
        }
    }

    #[tokio::test]
    async fn submit_returns_structured_errors_and_accepts_a_corrected_batch() {
        let (state, root) = test_state().await;
        let server = McpServer::new(state, "test-agent".to_owned());

        let invalid = server
            .proposal_submit_batch(
                Extensions::default(),
                Parameters(SubmitBatchParams {
                    operations: vec![question_line("$q1", json!(["A", "B"]))],
                }),
            )
            .await
            .expect("tool call");
        assert_eq!(invalid.is_error, Some(true));
        let structured = invalid.structured_content.expect("structured errors");
        assert_eq!(structured["error"], "validation_failed");
        let errors = structured["errors"].as_array().expect("errors array");
        assert!(errors.iter().any(|error| {
            error["field_path"] == "answer" && error["rule"] == "type_grading_consistency"
        }));

        let valid = server
            .proposal_submit_batch(
                Extensions::default(),
                Parameters(SubmitBatchParams {
                    operations: vec![question_line("$q1", json!(["A"]))],
                }),
            )
            .await
            .expect("tool call");
        assert_eq!(valid.is_error, Some(false));
        let proposal = valid.structured_content.expect("proposal")["proposal"].clone();
        let submitted_by = proposal["submitted_by"].as_str().expect("actor");
        assert_eq!(submitted_by, "agent:test-agent");

        let listed = server
            .proposal_list(Parameters(ProposalListParams {
                status: None,
                limit: None,
            }))
            .await
            .expect("list");
        let listed_value = listed.structured_content.expect("list");
        let proposals = listed_value["proposals"].as_array().expect("array");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0]["status"], "pending");
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
