use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::auth::set_setting;
use serde_json::{Value, json};
use tower::ServiceExt;

use reinforce_server::auth::hash_token;
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::demo;
use reinforce_server::state::AppState;

static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const OWNER_SETUP_TOKEN: &str = "operation-edit-owner-token";

struct TestDir(std::path::PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type TestApp = Router<()>;

async fn test_state(mode: AuthMode) -> (TestApp, AppState, TestDir) {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-operation-test-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.join("test.db"),
        blobs: directory.join("blobs"),
        auth: mode,
        demo: true,
    };
    let state = AppState::open(&config).await.expect("open test state");
    demo::seed(&state).await.expect("seed demo data");
    (
        reinforce_server::app(state.clone()),
        state,
        TestDir(directory),
    )
}

async fn json_call(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    let bytes = body
        .map(|value| serde_json::to_vec(&value).expect("encode json"))
        .unwrap_or_default();
    let request = builder.body(Body::from(bytes)).expect("build request");
    app.clone().oneshot(request).await.expect("run request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse json response")
}

async fn expect_json(response: axum::response::Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    response_json(response).await
}

fn session_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .expect("cookie text")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned()
}

async fn setup_invite_owner(app: &TestApp, state: &AppState) -> String {
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
        json_call(
            app,
            Method::POST,
            "/api/auth/owner/setup",
            Some(json!({
                "token": OWNER_SETUP_TOKEN,
                "username": "owner",
                "password": "owner-password-123",
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let response = json_call(
        app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "owner", "password": "owner-password-123"})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    session_cookie(&response)
}

async fn register_member(app: &TestApp, owner_cookie: &str) -> String {
    let response = json_call(
        app,
        Method::POST,
        "/api/auth/invites",
        Some(json!({"expires_in_secs": 3600})),
        Some(owner_cookie),
    )
    .await;
    let invite = expect_json(response, StatusCode::OK).await;
    let code = invite["code"].as_str().expect("invite code").to_owned();
    expect_json(
        json_call(
            app,
            Method::POST,
            "/api/auth/register",
            Some(json!({
                "invite_code": code,
                "username": "learner",
                "password": "learner-password-123",
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let response = json_call(
        app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "learner", "password": "learner-password-123"})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    session_cookie(&response)
}

fn card_data(card_id: &str, title: &str, text: &str) -> Value {
    json!({
        "id": card_id,
        "subject": "physics",
        "topics": ["phase diagram"],
        "title": title,
        "blocks": [{"type": "markdown", "text": text}],
        "anchor_ids": [],
        "status": "draft",
        "verification": "unchecked",
        "unsure_count": 0,
        "metadata": {},
        "created_at": "2026-01-01T00:00:00.000000Z",
        "updated_at": "2026-01-01T00:00:00.000000Z"
    })
}

fn card_batch(card_id: &str, title: &str, text: &str) -> Value {
    json!({
        "operations": [{
            "op": "knowledge_card",
            "data": card_data(card_id, title, text)
        }]
    })
}

fn question_data(question_id: &str, answer: Vec<&str>) -> Value {
    json!({
        "id": question_id,
        "subject": "physics",
        "topics": [],
        "stem_blocks": [{"type": "markdown", "text": "Which option is correct?"}],
        "explanation_blocks": [],
        "anchor_ids": [],
        "question_type": "single_choice",
        "grading_mode": "exact",
        "options": [
            {"key": "A", "content": "first", "explanation": null, "linked_card_ids": []},
            {"key": "B", "content": "second", "explanation": null, "linked_card_ids": []}
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
    })
}

fn question_batch(question_id: &str, answer: Vec<&str>) -> Value {
    json!({
        "operations": [{
            "op": "question_card",
            "data": question_data(question_id, answer)
        }]
    })
}

#[tokio::test]
async fn edit_operation_updates_card_and_approval_applies_new_content() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let card_id = uuid::Uuid::now_v7().to_string();
    let proposal = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(card_batch(&card_id, "old title", "old body")),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let proposal_id = proposal["id"].as_str().expect("proposal id").to_owned();

    let updated = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/proposals/{proposal_id}/operations/0"),
            Some(json!({"data": card_data(&card_id, "new title", "new body")})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(updated["id"], json!(proposal_id));
    assert_eq!(
        updated["payload"]["operations"][0]["data"]["title"],
        "new title"
    );

    let stored = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/proposals/{proposal_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        stored["payload"]["operations"][0]["data"]["title"],
        "new title"
    );
    assert_eq!(
        stored["payload"]["operations"][0]["data"]["blocks"][0]["text"],
        "new body"
    );

    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/proposals/{proposal_id}/approve"),
            Some(json!({})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let applied = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/knowledge_cards/{card_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(applied["title"], "new title");
    assert_eq!(applied["blocks"][0]["text"], "new body");
}

#[tokio::test]
async fn edit_operation_rejects_invalid_grading_without_writing() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let question_id = uuid::Uuid::now_v7().to_string();
    let proposal = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(question_batch(&question_id, vec!["A"])),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let proposal_id = proposal["id"].as_str().expect("proposal id").to_owned();
    let before = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/proposals/{proposal_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let original_data = before["payload"]["operations"][0]["data"].clone();

    let invalid = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/proposals/{proposal_id}/operations/0"),
            Some(json!({"data": question_data(&question_id, vec!["A", "B"])})),
            None,
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(invalid["error"]["code"], "validation_failed");
    let errors = invalid["error"]["details"]["errors"]
        .as_array()
        .expect("validation errors");
    assert!(errors.iter().any(|error| {
        error["rule"] == "type_grading_consistency" && error["field_path"] == "answer"
    }));

    let after = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/proposals/{proposal_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(after["payload"]["operations"][0]["data"], original_data);
}

#[tokio::test]
async fn edit_operation_rejects_out_of_range_non_owner_and_non_pending() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let card_id = uuid::Uuid::now_v7().to_string();
    let proposal = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(card_batch(&card_id, "old title", "old body")),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let proposal_id = proposal["id"].as_str().expect("proposal id").to_owned();

    let missing = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/proposals/{proposal_id}/operations/1"),
            Some(json!({"data": card_data(&card_id, "new title", "new body")})),
            None,
        )
        .await,
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(missing["error"]["code"], "not_found");

    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/proposals/{proposal_id}/approve"),
            Some(json!({})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let settled = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/proposals/{proposal_id}/operations/0"),
            Some(json!({"data": card_data(&card_id, "new title", "new body")})),
            None,
        )
        .await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(settled["error"]["code"], "conflict");

    let (invite_app, invite_state, _invite_dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&invite_app, &invite_state).await;
    let learner_cookie = register_member(&invite_app, &owner_cookie).await;
    let invite_card_id = uuid::Uuid::now_v7().to_string();
    let invite_proposal = expect_json(
        json_call(
            &invite_app,
            Method::POST,
            "/api/proposals",
            Some(card_batch(&invite_card_id, "old title", "old body")),
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let invite_proposal_id = invite_proposal["id"]
        .as_str()
        .expect("invite proposal id")
        .to_owned();
    let forbidden = expect_json(
        json_call(
            &invite_app,
            Method::PATCH,
            &format!("/api/proposals/{invite_proposal_id}/operations/0"),
            Some(json!({"data": card_data(&invite_card_id, "new title", "new body")})),
            Some(&learner_cookie),
        )
        .await,
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(forbidden["error"]["code"], "forbidden");
}

#[tokio::test]
async fn edit_operation_rejects_data_of_another_operation_type() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let card_id = uuid::Uuid::now_v7().to_string();
    let proposal = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(card_batch(&card_id, "old title", "old body")),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let proposal_id = proposal["id"].as_str().expect("proposal id").to_owned();
    let before = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/proposals/{proposal_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let original_data = before["payload"]["operations"][0]["data"].clone();

    let invalid = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/proposals/{proposal_id}/operations/0"),
            Some(json!({"data": question_data(&uuid::Uuid::now_v7().to_string(), vec!["A"])})),
            None,
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(invalid["error"]["code"], "validation_failed");
    let errors = invalid["error"]["details"]["errors"]
        .as_array()
        .expect("validation errors");
    assert!(
        errors
            .iter()
            .any(|error| error["rule"] == "operation_shape" && error["op_index"] == 0)
    );

    let after = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/proposals/{proposal_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(after["payload"]["operations"][0]["data"], original_data);
}
