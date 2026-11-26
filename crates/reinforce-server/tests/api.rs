use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::auth::set_setting;
use reinforce_core::content_store;
use reinforce_core::primitives::{CardStatus, Verification};
use reinforce_core::{KnowledgeCard, Timestamp};
use serde_json::{Value, json};
use tower::ServiceExt;

use reinforce_server::auth::hash_token;
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::demo;
use reinforce_server::state::AppState;

static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const BASE_RESOURCE: &str = "01a21714-c98c-7bfe-8cf8-a60b84573f56";
const OVERLAY_RESOURCE: &str = "01a21714-c98c-78d5-8550-3d0928b9c9d1";
const QUESTION_ONE: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const EXAM_SET: &str = "01a21714-c98d-7931-a48a-6a1d5cd348bd";
const OWNER_SETUP_TOKEN: &str = "test-owner-token";

struct TestDir(std::path::PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn test_state(mode: AuthMode) -> (TestApp, AppState, TestDir) {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-api-test-{}-{sequence}",
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
        reinforce_server::app().with_state(state.clone()),
        state,
        TestDir(directory),
    )
}

type TestApp = Router<()>;

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
    cookie: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    let request = builder.body(Body::from(body)).expect("build request");
    app.clone().oneshot(request).await.expect("run request")
}

async fn json_call(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> axum::response::Response {
    let (content_type, bytes) = match body {
        Some(value) => (
            Some("application/json"),
            serde_json::to_vec(&value).expect("encode json"),
        ),
        None => (None, Vec::new()),
    };
    call(app, method, uri, content_type, bytes, cookie).await
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
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

fn multipart(fields: &[(&str, &str)], file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----reinforce-api-boundary";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
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
    let response = json_call(
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
    .await;
    expect_json(response, StatusCode::OK).await;
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
    let response = json_call(
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
    .await;
    expect_json(response, StatusCode::OK).await;
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

fn draft_card() -> KnowledgeCard {
    let now = Timestamp::now();
    KnowledgeCard {
        id: uuid::Uuid::now_v7().to_string().parse().expect("card id"),
        subject: "physics".to_owned(),
        topics: vec![],
        title: "unpublished draft".to_owned(),
        blocks: vec![],
        anchor_ids: vec![],
        status: CardStatus::Draft,
        verification: Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn pin_batch(card_id: &str, x: f64) -> Value {
    json!({
        "operations": [{
            "op": "knowledge_card",
            "data": {
                "id": card_id,
                "subject": "physics",
                "topics": ["phase diagram"],
                "title": "pin calibration",
                "blocks": [{
                    "type": "interactive_pins",
                    "resource_id": BASE_RESOURCE,
                    "overlay_resource_id": OVERLAY_RESOURCE,
                    "pins": [{
                        "no": 1,
                        "x": x,
                        "y": 0.2,
                        "label_x": 0.3,
                        "label_y": 0.1,
                        "term_primary": "三相點",
                        "term_secondary": null,
                        "detail": null,
                        "hint": null,
                        "linked_card_id": null
                    }]
                }],
                "anchor_ids": [],
                "status": "draft",
                "verification": "unchecked",
                "unsure_count": 0,
                "metadata": {},
                "created_at": "2026-01-01T00:00:00.000000Z",
                "updated_at": "2026-01-01T00:00:00.000000Z"
            }
        }]
    })
}

#[tokio::test]
async fn single_mode_serves_demo_content_and_ranges() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let meta = expect_json(
        json_call(&app, Method::GET, "/api/meta", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(meta["version"], reinforce_core::VERSION);
    assert_eq!(meta["auth_mode"], "single");

    let cards = expect_json(
        json_call(&app, Method::GET, "/api/knowledge_cards", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(cards.as_array().expect("card array").len(), 3);
    let notes = expect_json(
        json_call(&app, Method::GET, "/api/notes", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(notes.as_array().expect("note array").len(), 1);

    let response = json_call(
        &app,
        Method::GET,
        &format!("/api/resources/{BASE_RESOURCE}/blob"),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCEPT_RANGES).unwrap(),
        "bytes"
    );

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/resources/{BASE_RESOURCE}/blob"))
        .header(header::RANGE, "bytes=10-19")
        .body(Body::empty())
        .expect("range request");
    let response = app.clone().oneshot(request).await.expect("range response");
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 10-19/983"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(bytes.len(), 10);

    let fallback = json_call(&app, Method::GET, "/some/spa/route", None, None).await;
    assert_eq!(fallback.status(), StatusCode::OK);
}

#[tokio::test]
async fn invite_mode_enforces_owner_boundaries_and_resource_policy() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;

    let response = json_call(
        &app,
        Method::POST,
        "/api/auth/register",
        Some(json!({
            "invite_code": "already-used",
            "username": "another",
            "password": "another-password-123",
        })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    assert_eq!(
        json_call(
            &app,
            Method::GET,
            "/api/proposals",
            None,
            Some(&member_cookie)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let forged = json!({
        "actor": "owner",
        "operations": []
    });
    assert_eq!(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(forged),
            Some(&member_cookie)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            "/api/study/attempts?user_id=01999d00-0000-7000-8000-000000000000",
            None,
            Some(&member_cookie)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    let draft = draft_card();
    let draft_id = draft.id.as_str();
    state
        .with_db(move |database| {
            content_store::insert_knowledge_card(database.connection(), &draft)?;
            Ok(())
        })
        .await
        .expect("insert draft");
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/knowledge_cards/{draft_id}"),
            None,
            Some(&member_cookie)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/knowledge_cards/{draft_id}"),
            None,
            Some(&owner_cookie)
        )
        .await
        .status(),
        StatusCode::OK
    );

    let metadata = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/resources/{OVERLAY_RESOURCE}"),
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(metadata["export_policy"], "redact");
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/resources/{OVERLAY_RESOURCE}/blob"),
            None,
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/resources/{OVERLAY_RESOURCE}/blob"),
            None,
            Some(&owner_cookie),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let (content_type, bytes) = multipart(
        &[
            ("title", "private-note.svg"),
            ("kind", "image"),
            ("width", "20"),
            ("height", "10"),
            ("export_policy", "owner_only"),
        ],
        "private.svg",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            "/api/resources/upload",
            Some(&content_type),
            bytes.clone(),
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let uploaded = expect_json(
        call(
            &app,
            Method::POST,
            "/api/resources/upload",
            Some(&content_type),
            bytes,
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let owner_only = uploaded["id"].as_str().expect("uploaded id");
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/resources/{owner_only}"),
            None,
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/resources/{owner_only}/blob"),
            None,
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn quiz_answers_write_attempts_and_update_scheduling() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let response = json_call(
        &app,
        Method::POST,
        "/api/study/sessions",
        Some(json!({
            "mode": "daily_due",
            "options": {"question_limit": 4}
        })),
        None,
    )
    .await;
    let created = expect_json(response, StatusCode::OK).await;
    let session_id = created["session_id"].as_str().expect("session id");
    let session = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/study/sessions/{session_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(session["question"].is_object());
    assert!(session["question"].get("answer").is_none());

    let answer = json!({
        "submission": {"kind": "keys", "values": ["A"]},
        "confidence": "certain"
    });
    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
            Some(answer),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let finished = expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/finish"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(finished["attempts_written"], 1);

    let attempts = expect_json(
        json_call(&app, Method::GET, "/api/study/attempts", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(attempts.as_array().expect("attempts").len(), 1);
    let scheduling = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/study/scheduling?card_id={QUESTION_ONE}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(scheduling.is_object());
    assert!(scheduling["reps"].as_u64().unwrap_or(0) > 0);
    let report = expect_json(
        json_call(&app, Method::GET, "/api/study/weakness", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert!(report["questions"].is_array());

    let response = json_call(
        &app,
        Method::POST,
        "/api/study/sessions",
        Some(json!({
            "mode": "full_exam",
            "exam_set_id": EXAM_SET,
            "options": {"question_limit": 4, "total_time_secs": 120}
        })),
        None,
    )
    .await;
    let exam = expect_json(response, StatusCode::OK).await;
    let exam_id = exam["session_id"].as_str().expect("exam session id");
    let score_before = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/study/sessions/{exam_id}/score"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(score_before["graded_count"], 0);
    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{exam_id}/answers"),
            Some(json!({
                "submission": {"kind": "keys", "values": ["A"]},
                "confidence": "certain"
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let score_deferred = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/study/sessions/{exam_id}/score"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(score_deferred["graded_count"], 0);
    let exam_finish = expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{exam_id}/finish"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(exam_finish["attempts_written"], 1);
}

#[tokio::test]
async fn proposal_review_pins_and_rejection() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let card_id = uuid::Uuid::now_v7().to_string();
    let proposal = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(pin_batch(&card_id, 0.1)),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let proposal_id = proposal["id"].as_str().expect("proposal id");
    let pending = expect_json(
        json_call(&app, Method::GET, "/api/proposals", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(pending.as_array().expect("pending").len(), 1);

    expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/proposals/{proposal_id}/pins"),
            Some(json!({
                "resource_id": BASE_RESOURCE,
                "pins": [{"no": 1, "x": 0.42, "y": 0.22, "label_x": 0.52, "label_y": 0.12}]
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
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
    let pin = &stored["payload"]["operations"][0]["data"]["blocks"][0]["pins"][0];
    assert_eq!(pin["x"], 0.42);
    assert_eq!(pin["label_x"], 0.52);

    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/proposals/{proposal_id}/approve"),
            Some(json!({"promote_verified": true})),
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
    assert_eq!(applied["verification"], "verified");
    assert_eq!(applied["blocks"][0]["pins"][0]["x"], 0.42);

    let invalid = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(json!({
                "operations": [{
                    "op": "question_card",
                    "data": {
                        "id": uuid::Uuid::now_v7().to_string(),
                        "subject": "physics",
                        "question_type": "single_choice",
                        "grading_mode": "exact",
                        "options": [],
                        "answer": ["A"],
                        "answer_source_kind": "official",
                        "status": "draft"
                    }
                }]
            })),
            None,
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert!(invalid["error"]["details"]["errors"].is_array());

    let second_id = uuid::Uuid::now_v7().to_string();
    let second = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/proposals",
            Some(pin_batch(&second_id, 0.2)),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let second_id = second["id"].as_str().expect("second proposal id");
    let rejected = expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/proposals/{second_id}/reject"),
            Some(json!({"review_note": "please move the pin"})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["review_note"], "please move the pin");
}

#[tokio::test]
async fn api_errors_use_the_shared_json_shape() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let response = json_call(&app, Method::GET, "/api/missing", None, None).await;
    let body = expect_json(response, StatusCode::NOT_FOUND).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(body["error"]["message"].is_string());
}
