use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::auth::set_setting;
use reinforce_core::primitives::ProposalStatus;
use reinforce_core::{Actor, ApprovalOptions, ApprovalOutcome, ProposalId, approve_batch};
use reinforce_server::auth::hash_token;
use reinforce_server::cli;
use reinforce_server::config::{AuthMode, CheckArgs, ServeArgs};
use reinforce_server::error::ApiError;
use reinforce_server::mcp::{BatchLine, McpServer, SubmitBatchParams};
use reinforce_server::state::AppState;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const OWNER_SETUP_TOKEN: &str = "mcp-test-owner-token";

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "reinforce-mcp-test-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        Self(directory)
    }

    fn db(&self) -> std::path::PathBuf {
        self.0.join("test.db")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn test_state(mode: AuthMode, host: &str, dir: &TestDir) -> (Router<()>, AppState) {
    let config = ServeArgs {
        host: host.to_owned(),
        port: 0,
        db: dir.db(),
        blobs: dir.0.join("blobs"),
        auth: mode,
        demo: false,
    };
    let state = AppState::open(&config).await.expect("open test state");
    (reinforce_server::app(state.clone()), state)
}

async fn request(
    app: &Router<()>,
    method: Method,
    uri: &str,
    body: Value,
    headers: &[(&str, String)],
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .header(header::CONTENT_TYPE, "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    app.clone()
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("run request")
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let text = String::from_utf8_lossy(&bytes);
    if let Some(data) = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .last()
        && let Ok(value) = serde_json::from_str(data)
    {
        return value;
    }
    serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({"raw": text.into_owned()}))
}

async fn expect_json(response: axum::response::Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    json_body(response).await
}

fn cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie")
        .to_str()
        .expect("cookie text")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned()
}

async fn setup_owner(app: &Router<()>, state: &AppState) -> String {
    state
        .with_db(move |database| {
            set_setting(
                database.connection(),
                "owner_setup_token_hash",
                &hash_token(OWNER_SETUP_TOKEN),
            )?;
            Ok(())
        })
        .await
        .expect("set owner setup token");
    expect_json(
        request(
            app,
            Method::POST,
            "/api/auth/owner/setup",
            json!({
                "token": OWNER_SETUP_TOKEN,
                "username": "owner",
                "password": "owner-password-123",
            }),
            &[],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let response = request(
        app,
        Method::POST,
        "/api/auth/login",
        json!({"username": "owner", "password": "owner-password-123"}),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    cookie(&response)
}

async fn register_member(app: &Router<()>, owner_cookie: &str) -> String {
    let response = request(
        app,
        Method::POST,
        "/api/auth/invites",
        json!({"expires_in_secs": 3600}),
        &[(header::COOKIE.as_str(), owner_cookie.to_owned())],
    )
    .await;
    let invite = expect_json(response, StatusCode::OK).await;
    let code = invite["code"].as_str().expect("invite code").to_owned();
    expect_json(
        request(
            app,
            Method::POST,
            "/api/auth/register",
            json!({
                "invite_code": code,
                "username": "learner",
                "password": "learner-password-123",
            }),
            &[],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let response = request(
        app,
        Method::POST,
        "/api/auth/login",
        json!({"username": "learner", "password": "learner-password-123"}),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    cookie(&response)
}

async fn mcp_post(
    app: &Router<()>,
    body: Value,
    token: Option<&str>,
    session: Option<&str>,
    host: &str,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    app.clone()
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("run request")
}

async fn tools_list(app: &Router<()>, token: Option<&str>) -> axum::response::Response {
    let init = mcp_post(
        app,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "reinforce-test", "version": "1.0"}
            }
        }),
        token,
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(init.status(), StatusCode::OK, "initialize failed");
    let session = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("session id")
        .to_owned();
    mcp_post(
        app,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        token,
        Some(&session),
        "127.0.0.1",
    )
    .await
}

#[tokio::test]
async fn mcp_http_requires_and_verifies_agent_tokens_in_invite_mode() {
    let dir = TestDir::new();
    let (app, state) = test_state(AuthMode::Invite, "127.0.0.1", &dir).await;
    let owner_cookie = setup_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;

    let denied = expect_json(
        request(
            &app,
            Method::POST,
            "/api/auth/agent-tokens",
            json!({}),
            &[(header::COOKIE.as_str(), member_cookie)],
        )
        .await,
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(denied["error"]["code"], "forbidden");

    let unauthorized = expect_json(
        mcp_post(
            &app,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
            None,
            None,
            "127.0.0.1",
        )
        .await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert_eq!(unauthorized["error"]["code"], "unauthorized");

    let unauthorized = expect_json(
        mcp_post(
            &app,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
            Some("wrong-token"),
            None,
            "127.0.0.1",
        )
        .await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert_eq!(unauthorized["error"]["code"], "unauthorized");

    let created = expect_json(
        request(
            &app,
            Method::POST,
            "/api/auth/agent-tokens",
            json!({}),
            &[(header::COOKIE.as_str(), owner_cookie.clone())],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let token = created["token"].as_str().expect("agent token").to_owned();

    let listed = expect_json(tools_list(&app, Some(&token)).await, StatusCode::OK).await;
    let tools = listed["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 8);
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"] == "proposal_submit_batch")
    );
    assert!(!tools.iter().any(|tool| tool["name"] == "approve_batch"));
    assert!(!tools.iter().any(|tool| tool["name"] == "reject_batch"));

    expect_json(
        request(
            &app,
            Method::DELETE,
            "/api/auth/agent-tokens",
            json!({"token": token}),
            &[(header::COOKIE.as_str(), owner_cookie)],
        )
        .await,
        StatusCode::OK,
    )
    .await;

    let unauthorized = expect_json(
        mcp_post(
            &app,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
            Some(&token),
            None,
            "127.0.0.1",
        )
        .await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert_eq!(unauthorized["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn mcp_http_rejects_foreign_host_in_single_mode() {
    let dir = TestDir::new();
    let (app, _state) = test_state(AuthMode::Single, "127.0.0.1", &dir).await;
    let denied = expect_json(
        mcp_post(
            &app,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
            None,
            None,
            "attacker.example",
        )
        .await,
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(denied["error"]["code"], "forbidden");

    let listed = expect_json(tools_list(&app, None).await, StatusCode::OK).await;
    assert_eq!(
        listed["result"]["tools"].as_array().expect("tools").len(),
        8
    );
}

fn question_line(ref_name: &str, answer: Value) -> BatchLine {
    BatchLine {
        op: "question_card".to_owned(),
        temp_ref: Some(ref_name.to_owned()),
        data: json!({
            "id": ref_name,
            "exam_set_id": null,
            "question_group_id": null,
            "question_no": null,
            "subject": "physics",
            "topics": [],
            "stem_blocks": [{"type":"markdown","text":"Which option?"}],
            "explanation_blocks": [],
            "anchor_ids": [],
            "question_type": "single_choice",
            "grading_mode": "exact",
            "options": [
                {"key":"A","content":"alpha","explanation":null,"linked_card_ids":[]},
                {"key":"B","content":"beta","explanation":null,"linked_card_ids":[]}
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
async fn mcp_batch_proposal_flow_is_validated_approved_and_checks_clean() {
    let dir = TestDir::new();
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: dir.db(),
        blobs: dir.0.join("blobs"),
        auth: AuthMode::Single,
        demo: false,
    };
    let state = AppState::open(&config).await.expect("open state");
    let server = McpServer::new(state.clone(), "e2e-agent".to_owned());

    let invalid = server
        .proposal_submit_batch(Parameters(SubmitBatchParams {
            operations: vec![question_line("$question", json!(["A", "B"]))],
        }))
        .await
        .expect("invalid submit");
    assert_eq!(invalid.is_error, Some(true));
    let errors = invalid.structured_content.expect("structured errors");
    assert_eq!(errors["error"], "validation_failed");
    assert!(errors["errors"].as_array().is_some_and(|entries| {
        entries
            .iter()
            .any(|entry| entry["rule"] == "type_grading_consistency")
    }));

    let valid = server
        .proposal_submit_batch(Parameters(SubmitBatchParams {
            operations: vec![question_line("$question", json!(["A"]))],
        }))
        .await
        .expect("corrected submit");
    assert_eq!(valid.is_error, Some(false));
    let proposal = valid.structured_content.expect("proposal")["proposal"].clone();
    assert_eq!(proposal["status"], "pending");
    assert_eq!(proposal["submitted_by"], "agent:e2e-agent");
    let proposal_id = proposal["id"]
        .as_str()
        .expect("proposal id")
        .parse::<ProposalId>()
        .expect("parse proposal id");

    let approved = state
        .with_db(move |database| {
            match approve_batch(
                database.connection_mut(),
                &proposal_id,
                ApprovalOptions {
                    reviewer: Actor::Owner,
                    promote_verified: false,
                },
            )? {
                ApprovalOutcome::Applied { proposal, applied } => Ok((proposal, applied)),
                ApprovalOutcome::Rejected { errors, .. } => Err(ApiError::validation(errors)),
            }
        })
        .await
        .expect("owner approved proposal");
    assert_eq!(approved.0.status, ProposalStatus::Approved);
    assert_eq!(approved.1, 1);

    let published = state
        .with_db(move |database| {
            let count: i64 = database.connection().query_row(
                "SELECT COUNT(*) FROM question_cards",
                [],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .expect("read published cards");
    assert_eq!(published, 1);

    cli::check(CheckArgs { db: dir.db() })
        .await
        .expect("domain validation of the published store");
}
