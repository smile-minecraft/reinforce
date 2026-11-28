use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::auth::set_setting;
use reinforce_core::primitives::ProposalStatus;
use reinforce_core::subject_groups_store::create_subject_group;
use reinforce_core::{
    Actor, ApprovalOptions, ApprovalOutcome, ProposalId, Timestamp, approve_batch,
};
use reinforce_server::auth::hash_token;
use reinforce_server::cli;
use reinforce_server::config::{AuthMode, CheckArgs, ServeArgs};
use reinforce_server::error::ApiError;
use reinforce_server::mcp::{
    BatchLine, CardGetParams, CardSearchParams, ExamSetListParams, GraphParams, McpServer,
    NoteGetParams, NoteListParams, ProposalGetParams, QuestionGetParams, ResourceListParams,
    ResourcePageParams, SubjectGroupListParams, SubmitBatchParams, WeaknessParams,
};
use reinforce_server::state::AppState;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::Extensions;
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
        fonts: dir.0.join("fonts"),
        auth: mode,
        demo: false,
        plugins: None,
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
        .next_back()
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
    tools_list_at(app, token, "127.0.0.1").await
}

async fn tools_list_at(
    app: &Router<()>,
    token: Option<&str>,
    host: &str,
) -> axum::response::Response {
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
        host,
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
        host,
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
    assert_eq!(tools.len(), 14);
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
        14
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
        fonts: dir.0.join("fonts"),
        auth: AuthMode::Single,
        demo: false,
        plugins: None,
    };
    let state = AppState::open(&config).await.expect("open state");
    let server = McpServer::new(state.clone(), "e2e-agent".to_owned());

    let invalid = server
        .proposal_submit_batch(
            Extensions::default(),
            Parameters(SubmitBatchParams {
                operations: vec![question_line("$question", json!(["A", "B"]))],
            }),
        )
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
        .proposal_submit_batch(
            Extensions::default(),
            Parameters(SubmitBatchParams {
                operations: vec![question_line("$question", json!(["A"]))],
            }),
        )
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

#[tokio::test]
async fn invite_mode_mcp_accepts_public_hostnames_behind_a_proxy() {
    let dir = TestDir::new();
    let (app, state) = test_state(AuthMode::Invite, "127.0.0.1", &dir).await;
    let owner_cookie = setup_owner(&app, &state).await;
    let created = expect_json(
        request(
            &app,
            Method::POST,
            "/api/auth/agent-tokens",
            json!({}),
            &[(header::COOKIE.as_str(), owner_cookie)],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let token = created["token"].as_str().expect("agent token").to_owned();

    // A reverse proxy forwards the public hostname; the bearer token, not the
    // Host header, is what authorizes invite-mode agents.
    let listed = expect_json(
        tools_list_at(&app, Some(&token), "reinforce.example.org").await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(listed["result"]["tools"].as_array().map(Vec::len), Some(14));
}

#[test]
fn stdio_binary_writes_only_json_rpc_to_stdout() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = TestDir::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_reinforce"))
        .arg("mcp")
        .arg("--db")
        .arg(dir.db())
        .arg("--blobs")
        .arg(dir.0.join("blobs"))
        .env("RUST_LOG", "debug")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn reinforce mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut read_message = || {
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read stdout line");
        serde_json::from_str::<Value>(&line)
            .unwrap_or_else(|error| panic!("stdout line is not JSON-RPC ({error}): {line:?}"))
    };

    let send = |stdin: &mut std::process::ChildStdin, message: Value| {
        writeln!(stdin, "{message}").expect("write stdin");
        stdin.flush().expect("flush stdin");
    };
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "stdio-test", "version": "0"}
            }
        }),
    );
    let init = read_message();
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "reinforce");
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        reinforce_core::VERSION
    );

    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    let tools = read_message();
    assert_eq!(tools["id"], 2);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    );

    drop(stdin);
    let status = child.wait().expect("wait for reinforce mcp");
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).expect("drain stdout");
    for line in rest.lines().filter(|line| !line.trim().is_empty()) {
        serde_json::from_str::<Value>(line)
            .unwrap_or_else(|error| panic!("stdout line is not JSON-RPC ({error}): {line:?}"));
    }
    assert!(status.success(), "reinforce mcp exited with {status}");
}

#[tokio::test]
async fn named_agent_tokens_list_revoke_by_id_and_attribute_http_proposals() {
    let dir = TestDir::new();
    let (app, state) = test_state(AuthMode::Invite, "127.0.0.1", &dir).await;
    let owner_cookie = setup_owner(&app, &state).await;

    let created = expect_json(
        request(
            &app,
            Method::POST,
            "/api/auth/agent-tokens",
            json!({ "name": "claude-writer" }),
            &[(header::COOKIE.as_str(), owner_cookie.clone())],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let token_id = created["id"].as_str().expect("token id").to_owned();
    let token = created["token"].as_str().expect("token").to_owned();
    assert_eq!(created["name"], "claude-writer");
    assert!(created["created_at"].is_string());

    let listed = expect_json(
        request(
            &app,
            Method::GET,
            "/api/auth/agent-tokens",
            json!({}),
            &[(header::COOKIE.as_str(), owner_cookie.clone())],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let tokens = listed["tokens"].as_array().expect("tokens array");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0]["id"], token_id);
    assert_eq!(tokens[0]["name"], "claude-writer");
    assert!(tokens[0].get("token").is_none());
    assert!(tokens[0].get("hash").is_none());

    let init = mcp_post(
        &app,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "claude-writer", "version": "1.0"}
            }
        }),
        Some(&token),
        None,
        "127.0.0.1",
    )
    .await;
    assert_eq!(init.status(), StatusCode::OK);
    let session = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("session id")
        .to_owned();
    let init_body = json_body(init).await;
    let instructions = init_body["result"]["instructions"]
        .as_str()
        .expect("server instructions");
    assert!(instructions.contains("proposal_submit_batch"));

    let submit = expect_json(
        mcp_post(
            &app,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "proposal_submit_batch",
                    "arguments": {
                        "operations": [{
                            "op": "question_card",
                            "temp_ref": "$q1",
                            "data": question_line("$q1", json!(["A"])).data
                        }]
                    }
                }
            }),
            Some(&token),
            Some(&session),
            "127.0.0.1",
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let proposal = &submit["result"]["structuredContent"]["proposal"];
    assert_eq!(proposal["submitted_by"], "agent:claude-writer");

    let revoked = expect_json(
        request(
            &app,
            Method::DELETE,
            &format!("/api/auth/agent-tokens/{token_id}"),
            json!({}),
            &[(header::COOKIE.as_str(), owner_cookie.clone())],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(revoked["revoked"], true);

    let after_revoke = expect_json(
        mcp_post(
            &app,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {}}),
            Some(&token),
            None,
            "127.0.0.1",
        )
        .await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert_eq!(after_revoke["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn legacy_hash_only_agent_tokens_remain_usable_and_revocable() {
    let dir = TestDir::new();
    let (app, state) = test_state(AuthMode::Invite, "127.0.0.1", &dir).await;
    let owner_cookie = setup_owner(&app, &state).await;
    let legacy_token = "legacy-plain-secret-token";
    let legacy_hash = hash_token(legacy_token);
    state
        .with_db(move |database| {
            set_setting(
                database.connection(),
                "agent_token_hashes",
                &serde_json::to_string(&vec![legacy_hash])?,
            )?;
            Ok(())
        })
        .await
        .expect("seed legacy token hash");

    let listed_tools =
        expect_json(tools_list(&app, Some(legacy_token)).await, StatusCode::OK).await;
    assert_eq!(
        listed_tools["result"]["tools"].as_array().map(Vec::len),
        Some(14)
    );

    let listed = expect_json(
        request(
            &app,
            Method::GET,
            "/api/auth/agent-tokens",
            json!({}),
            &[(header::COOKIE.as_str(), owner_cookie.clone())],
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let tokens = listed["tokens"].as_array().expect("tokens");
    assert_eq!(tokens.len(), 1);
    let legacy_id = tokens[0]["id"].as_str().expect("legacy id").to_owned();
    assert!(legacy_id.starts_with("legacy-"));
    assert_eq!(tokens[0]["name"], "mcp-http");

    expect_json(
        request(
            &app,
            Method::DELETE,
            &format!("/api/auth/agent-tokens/{legacy_id}"),
            json!({}),
            &[(header::COOKIE.as_str(), owner_cookie)],
        )
        .await,
        StatusCode::OK,
    )
    .await;

    expect_json(
        mcp_post(
            &app,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
            Some(legacy_token),
            None,
            "127.0.0.1",
        )
        .await,
        StatusCode::UNAUTHORIZED,
    )
    .await;
}

#[tokio::test]
async fn mcp_read_tools_return_full_entities_and_previews() {
    let dir = TestDir::new();
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: dir.db(),
        blobs: dir.0.join("blobs"),
        fonts: dir.0.join("fonts"),
        auth: AuthMode::Single,
        demo: false,
        plugins: None,
    };
    let state = AppState::open(&config).await.expect("open state");
    let server = McpServer::new(state.clone(), "reader-agent".to_owned());

    let resource_hash = reinforce_core::ContentHash::from_bytes(b"pdf-bytes")
        .as_str()
        .to_owned();
    let operations = vec![
        BatchLine {
            op: "resource".to_owned(),
            temp_ref: Some("$res1".to_owned()),
            data: json!({
                "id": "$res1",
                "kind": "pdf",
                "title": "Mechanics Textbook",
                "sha256": resource_hash,
                "byte_size": 9,
                "width": null,
                "height": null,
                "page_count": 10,
                "license": "CC0-1.0",
                "attribution_url": null,
                "attribution_notice": null,
                "sensitivity": "normal",
                "export_policy": "allow",
                "created_at": "2026-01-01T00:00:00.000000Z"
            }),
        },
        BatchLine {
            op: "anchor".to_owned(),
            temp_ref: Some("$anc1".to_owned()),
            data: json!({
                "id": "$anc1",
                "resource_id": "$res1",
                "anchor_kind": "pdf_page",
                "pdf_page": 3,
                "printed_page": 3,
                "legend_pdf_page": null,
                "rect": null,
                "media_span": null
            }),
        },
        BatchLine {
            op: "knowledge_card".to_owned(),
            temp_ref: Some("$card1".to_owned()),
            data: json!({
                "id": "$card1",
                "subject": "physics",
                "title": "Newton Second Law",
                "topics": ["mechanics"],
                "blocks": [{"type": "markdown", "text": "F = m a"}],
                "anchor_ids": ["$anc1"],
                "status": "published",
                "verification": "unchecked",
                "unsure_count": 0,
                "metadata": {},
                "created_at": "2026-01-01T00:00:00.000000Z",
                "updated_at": "2026-01-01T00:00:00.000000Z"
            }),
        },
        BatchLine {
            op: "note".to_owned(),
            temp_ref: Some("$note1".to_owned()),
            data: json!({
                "id": "$note1",
                "subject": "physics",
                "title": "Classical Mechanics Notes",
                "outline": [
                    {"type": "section", "heading": "Dynamics", "intro_markdown": null, "children": [
                        {"type": "card_ref", "card_id": "$card1"}
                    ]}
                ],
                "view_settings": {},
                "status": "published",
                "created_at": "2026-01-01T00:00:00.000000Z",
                "updated_at": "2026-01-01T00:00:00.000000Z"
            }),
        },
        BatchLine {
            op: "exam_set".to_owned(),
            temp_ref: Some("$exam1".to_owned()),
            data: json!({
                "id": "$exam1",
                "subject": "physics",
                "title": "Midterm Practice",
                "source_kind": "original",
                "exam_year": null,
                "source_resource_id": "$res1",
                "expected_questions": null,
                "is_complete": false,
                "metadata": {}
            }),
        },
        BatchLine {
            op: "question_card".to_owned(),
            temp_ref: Some("$q1".to_owned()),
            data: json!({
                "id": "$q1",
                "exam_set_id": "$exam1",
                "question_group_id": null,
                "question_no": null,
                "subject": "physics",
                "topics": ["mechanics"],
                "stem_blocks": [{"type": "markdown", "text": "Which law states F = ma?"}],
                "explanation_blocks": [],
                "anchor_ids": ["$anc1"],
                "question_type": "single_choice",
                "grading_mode": "exact",
                "options": [
                    {"key": "A", "content": "Newton Second Law", "explanation": null, "linked_card_ids": ["$card1"]},
                    {"key": "B", "content": "Ohm Law", "explanation": null, "linked_card_ids": []}
                ],
                "answer": ["A"],
                "accepted_answers": [],
                "answer_source_kind": "original_authored",
                "answer_basis": "Standard definition of Newton's second law.",
                "duplicate_question_id": null,
                "status": "published",
                "verification": "unchecked",
                "unsure_count": 0,
                "metadata": {},
                "created_at": "2026-01-01T00:00:00.000000Z",
                "updated_at": "2026-01-01T00:00:00.000000Z"
            }),
        },
        BatchLine {
            op: "edge".to_owned(),
            temp_ref: Some("$edge1".to_owned()),
            data: json!({
                "id": "$edge1",
                "edge_type": "tests",
                "from": {"kind": "question", "id": "$q1"},
                "to": {"kind": "card", "id": "$card1"},
                "created_at": "2026-01-01T00:00:00.000000Z"
            }),
        },
    ];

    let submitted = server
        .proposal_submit_batch(
            Extensions::default(),
            Parameters(SubmitBatchParams { operations }),
        )
        .await
        .expect("submit batch");
    assert_eq!(submitted.is_error, Some(false), "{submitted:?}");
    let proposal_id_str = submitted.structured_content.expect("proposal")["proposal"]["id"]
        .as_str()
        .expect("proposal id")
        .to_owned();
    let proposal_id = proposal_id_str.parse::<ProposalId>().expect("parse id");

    let fetched_proposal = server
        .proposal_get(Parameters(ProposalGetParams {
            id: proposal_id_str.clone(),
        }))
        .await
        .expect("proposal_get")
        .structured_content
        .expect("proposal_get content");
    assert_eq!(
        fetched_proposal["operations"].as_array().map(Vec::len),
        Some(7)
    );

    state
        .with_db(move |database| {
            approve_batch(
                database.connection_mut(),
                &proposal_id,
                ApprovalOptions {
                    reviewer: Actor::Owner,
                    promote_verified: true,
                },
            )?;
            create_subject_group(
                database.connection(),
                None,
                "STEM",
                &["physics".to_owned()],
                0,
                Timestamp::now(),
            )?;
            Ok(())
        })
        .await
        .expect("approve and seed subject group");

    let resources = server
        .resource_list(Parameters(ResourceListParams {
            kind: Some("pdf".to_owned()),
            limit: None,
        }))
        .await
        .expect("resource_list")
        .structured_content
        .expect("resources");
    let resource_id = resources["resources"][0]["id"]
        .as_str()
        .expect("resource id")
        .to_owned();

    let page = server
        .resource_read_page(Parameters(ResourcePageParams {
            resource_id,
            page: 3,
        }))
        .await
        .expect("resource_read_page")
        .structured_content
        .expect("page");
    assert_eq!(page["anchors"].as_array().map(Vec::len), Some(1));

    let search = server
        .card_search(Parameters(CardSearchParams {
            q: "Newton".to_owned(),
            limit: None,
        }))
        .await
        .expect("card_search")
        .structured_content
        .expect("search");
    let hits = search["hits"].as_array().expect("hits");
    let card_hit = hits
        .iter()
        .find(|hit| hit["kind"] == "knowledge_card")
        .expect("knowledge card hit");
    assert_eq!(card_hit["title"], "Newton Second Law");
    assert_eq!(card_hit["subject"], "physics");
    let card_id = card_hit["id"].as_str().expect("card id").to_owned();
    let question_hit = hits
        .iter()
        .find(|hit| hit["kind"] == "question_card")
        .expect("question card hit");
    assert_eq!(question_hit["question_type"], "single_choice");
    let question_id = question_hit["id"].as_str().expect("question id").to_owned();

    let card = server
        .card_get(Parameters(CardGetParams {
            id: card_id.clone(),
        }))
        .await
        .expect("card_get")
        .structured_content
        .expect("card");
    assert_eq!(card["card"]["title"], "Newton Second Law");

    let question = server
        .question_get(Parameters(QuestionGetParams {
            id: question_id.clone(),
        }))
        .await
        .expect("question_get")
        .structured_content
        .expect("question");
    assert_eq!(question["question"]["answer"], json!(["A"]));

    let notes = server
        .note_list(Parameters(NoteListParams {
            subject: Some("physics".to_owned()),
            status: Some("published".to_owned()),
            limit: None,
        }))
        .await
        .expect("note_list")
        .structured_content
        .expect("notes");
    assert_eq!(notes["notes"].as_array().map(Vec::len), Some(1));
    let note_id = notes["notes"][0]["id"]
        .as_str()
        .expect("note id")
        .to_owned();

    let note = server
        .note_get(Parameters(NoteGetParams { id: note_id }))
        .await
        .expect("note_get")
        .structured_content
        .expect("note");
    assert_eq!(note["note"]["title"], "Classical Mechanics Notes");
    assert_eq!(note["cards"].as_array().map(Vec::len), Some(1));
    assert_eq!(note["cards"][0]["id"], card_id);

    let exam_sets = server
        .exam_set_list(Parameters(ExamSetListParams {
            subject: Some("physics".to_owned()),
            limit: None,
        }))
        .await
        .expect("exam_set_list")
        .structured_content
        .expect("exam_sets");
    assert_eq!(exam_sets["exam_sets"].as_array().map(Vec::len), Some(1));
    assert_eq!(exam_sets["exam_sets"][0]["question_count"], 1);

    let groups = server
        .subject_group_list(Parameters(SubjectGroupListParams {}))
        .await
        .expect("subject_group_list")
        .structured_content
        .expect("groups");
    assert_eq!(groups["groups"].as_array().map(Vec::len), Some(1));
    assert_eq!(groups["groups"][0]["name"], "STEM");

    let graph = server
        .card_get_graph(Parameters(GraphParams {
            kind: "question".to_owned(),
            id: question_id,
            depth: Some(1),
            key: None,
        }))
        .await
        .expect("card_get_graph")
        .structured_content
        .expect("graph");
    assert_eq!(graph["nodes"].as_array().map(Vec::len), Some(1));

    let weakness = server
        .study_weakness_report(Parameters(WeaknessParams {
            user_id: None,
            limit: None,
        }))
        .await
        .expect("study_weakness_report")
        .structured_content
        .expect("weakness");
    assert!(weakness["report"].is_object());
}
