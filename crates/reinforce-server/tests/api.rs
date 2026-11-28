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
    test_state_with_host(mode, "127.0.0.1").await
}

async fn test_state_with_host(mode: AuthMode, host: &str) -> (TestApp, AppState, TestDir) {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-api-test-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let config = ServeArgs {
        host: host.to_owned(),
        port: 0,
        db: directory.join("test.db"),
        blobs: directory.join("blobs"),
        fonts: directory.join("fonts"),
        auth: mode,
        demo: true,
        plugins: None,
    };
    let state = AppState::open(&config).await.expect("open test state");
    demo::seed(&state).await.expect("seed demo data");
    (
        reinforce_server::app(state.clone()),
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
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
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
        .header(header::HOST, "127.0.0.1")
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
    let answered = expect_json(
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
    let revealed = &answered["revealed_question"];
    assert_eq!(revealed["id"], QUESTION_ONE);
    assert!(revealed.get("answer").is_some());
    assert!(revealed.get("accepted_answers").is_some());
    assert_eq!(revealed["answer_basis"], "原創題，依據熔化需要吸收潛熱。");
    assert!(revealed["explanation_blocks"].is_array());
    assert!(revealed["options"][0].get("explanation").is_some());
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
    let reps_after_first_finish = scheduling["reps"].as_u64().unwrap_or(0);
    assert!(reps_after_first_finish > 0);
    let finished_again = expect_json(
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
    assert_eq!(finished_again, finished);
    let attempts_after_second_finish = expect_json(
        json_call(&app, Method::GET, "/api/study/attempts", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        attempts_after_second_finish
            .as_array()
            .expect("attempts")
            .len(),
        1
    );
    let scheduling_after_second_finish = expect_json(
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
    assert_eq!(
        scheduling_after_second_finish["reps"].as_u64().unwrap_or(0),
        reps_after_first_finish
    );
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
    let exam_answer = expect_json(
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
    assert!(exam_answer["revealed_question"].is_null());
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
    assert_eq!(exam_finish["graded"].as_array().expect("graded").len(), 1);
    let revealed = exam_finish["revealed_questions"]
        .as_array()
        .expect("revealed");
    assert_eq!(revealed.len(), 4);
    let first = revealed
        .iter()
        .find(|question| question["id"] == QUESTION_ONE)
        .expect("first exam question");
    assert!(first.get("answer").is_some());
    assert!(first.get("accepted_answers").is_some());
    assert!(first.get("answer_basis").is_some());
    assert!(first["explanation_blocks"].is_array());
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

async fn raw_call(
    app: &TestApp,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
) -> StatusCode {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("build request");
    app.clone()
        .oneshot(request)
        .await
        .expect("run request")
        .status()
}

#[tokio::test]
async fn single_mode_rejects_foreign_hosts_and_cross_origin_writes() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;

    assert_eq!(
        raw_call(
            &app,
            Method::GET,
            "/api/meta",
            &[("host", "localhost:3000")]
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        raw_call(
            &app,
            Method::GET,
            "/api/knowledge_cards",
            &[("host", "rebound.attacker.example:3000")]
        )
        .await,
        StatusCode::FORBIDDEN,
        "a DNS-rebinding host must not reach the owner's data"
    );
    assert_eq!(
        raw_call(
            &app,
            Method::POST,
            "/api/proposals",
            &[
                ("host", "127.0.0.1:3000"),
                ("origin", "https://attacker.example")
            ]
        )
        .await,
        StatusCode::FORBIDDEN,
        "a cross-site page must not write through the owner's local server"
    );
    assert_ne!(
        raw_call(
            &app,
            Method::POST,
            "/api/proposals",
            &[
                ("host", "127.0.0.1:3000"),
                ("origin", "http://127.0.0.1:3000")
            ]
        )
        .await,
        StatusCode::FORBIDDEN,
        "same-origin writes from the WebApp still pass the guard"
    );
}

#[tokio::test]
async fn owner_can_upload_files_larger_than_the_default_body_limit() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let pdf = vec![b'%'; 5 * 1024 * 1024];
    let (content_type, bytes) = multipart(
        &[("title", "textbook.pdf"), ("kind", "pdf")],
        "textbook.pdf",
        &pdf,
    );
    let response = call(
        &app,
        Method::POST,
        "/api/resources/upload",
        Some(&content_type),
        bytes,
        None,
    )
    .await;
    assert!(
        response.status().is_success(),
        "a 5 MiB upload must be accepted, got {}",
        response.status()
    );
}

fn card_with_status(title: &str, status: CardStatus) -> reinforce_core::KnowledgeCard {
    let now = Timestamp::now();
    reinforce_core::KnowledgeCard {
        id: uuid::Uuid::now_v7().to_string().parse().expect("card id"),
        subject: "physics".to_owned(),
        topics: vec![],
        title: title.to_owned(),
        blocks: vec![],
        anchor_ids: vec![],
        status,
        verification: reinforce_core::Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn add_edge(
    connection: &rusqlite::Connection,
    edge_type: reinforce_core::primitives::EdgeType,
    from: reinforce_core::EdgeEndpoint,
    to: reinforce_core::EdgeEndpoint,
) {
    reinforce_core::content_store::insert_edge(
        connection,
        &reinforce_core::Edge {
            id: uuid::Uuid::now_v7().to_string().parse().expect("edge id"),
            edge_type,
            from,
            to,
            created_at: Timestamp::now(),
        },
    )
    .expect("insert edge");
}

fn fresh_id<T: std::str::FromStr>() -> T
where
    T::Err: std::fmt::Debug,
{
    uuid::Uuid::now_v7().to_string().parse().expect("fresh id")
}

#[tokio::test]
async fn exam_sets_require_authentication_and_hide_owner_only_sources() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;

    assert_eq!(
        json_call(&app, Method::GET, "/api/exam_sets", None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/exam_sets/{EXAM_SET}"),
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );

    let (content_type, bytes) = multipart(
        &[
            ("title", "private-source.pdf"),
            ("kind", "pdf"),
            ("export_policy", "owner_only"),
        ],
        "private.pdf",
        b"private source",
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
    let owner_only_id: reinforce_core::ResourceId = uploaded["id"]
        .as_str()
        .expect("resource id")
        .parse()
        .expect("resource id");

    let draft_set_id: reinforce_core::ExamSetId = fresh_id();
    let public_set_id: reinforce_core::ExamSetId = fresh_id();
    let draft_question_id: reinforce_core::QuestionCardId = fresh_id();
    let public_question_id: reinforce_core::QuestionCardId = fresh_id();
    let draft_set_id_for_db = draft_set_id.clone();
    let public_set_id_for_db = public_set_id.clone();
    let owner_only_id_for_db = owner_only_id.clone();
    state
        .with_db(move |database| {
            let connection = database.connection();
            let mut draft_set =
                content_store::get_exam_set(connection, &EXAM_SET.parse().unwrap())?
                    .expect("exam set");
            draft_set.id = draft_set_id_for_db.clone();
            draft_set.title = "draft set".to_owned();
            draft_set.source_resource_id = Some(owner_only_id_for_db.clone());
            content_store::insert_exam_set(connection, &draft_set)?;

            let mut public_set = draft_set.clone();
            public_set.id = public_set_id_for_db.clone();
            public_set.title = "published set".to_owned();
            content_store::insert_exam_set(connection, &public_set)?;

            let question =
                content_store::get_question_card(connection, &QUESTION_ONE.parse().unwrap())?
                    .expect("question");
            let mut draft_question = question.clone();
            draft_question.id = draft_question_id;
            draft_question.exam_set_id = Some(draft_set_id_for_db);
            draft_question.status = CardStatus::Draft;
            content_store::insert_question_card(connection, &draft_question)?;
            let mut public_question = question;
            public_question.id = public_question_id;
            public_question.exam_set_id = Some(public_set_id_for_db);
            content_store::insert_question_card(connection, &public_question)?;
            Ok(())
        })
        .await
        .expect("insert exam set fixtures");

    let listed = expect_json(
        json_call(
            &app,
            Method::GET,
            "/api/exam_sets",
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let listed_ids: Vec<&str> = listed
        .as_array()
        .expect("exam set list")
        .iter()
        .filter_map(|set| set["id"].as_str())
        .collect();
    assert!(!listed_ids.iter().any(|id| *id == draft_set_id.as_str()));
    assert!(listed_ids.iter().any(|id| *id == public_set_id.as_str()));

    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/exam_sets/{draft_set_id}"),
            None,
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/exam_sets/{draft_set_id}"),
            None,
            Some(&owner_cookie),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let learner_set = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/exam_sets/{public_set_id}"),
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(learner_set["source_resource_id"].is_null());
    let owner_set = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/exam_sets/{public_set_id}"),
            None,
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(owner_set["source_resource_id"], owner_only_id.as_str());
}

#[tokio::test]
async fn quiz_builders_never_include_unpublished_questions() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    let draft_question_id: reinforce_core::QuestionCardId = fresh_id();
    let draft_question_for_request = draft_question_id.clone();
    let draft_card = card_with_status("draft topic", CardStatus::Draft);
    let draft_card_id = draft_card.id.clone();
    let draft_card_for_request = draft_card_id.clone();
    state
        .with_db(move |database| {
            let connection = database.connection();
            let mut question =
                content_store::get_question_card(connection, &QUESTION_ONE.parse().unwrap())?
                    .expect("question");
            question.id = draft_question_id.clone();
            question.question_type = reinforce_core::primitives::QuestionType::PinIdentify;
            question.status = CardStatus::Draft;
            content_store::insert_question_card(connection, &question)?;
            content_store::insert_knowledge_card(connection, &draft_card)?;
            add_edge(
                connection,
                reinforce_core::primitives::EdgeType::Tests,
                reinforce_core::EdgeEndpoint::Question {
                    id: QUESTION_ONE.parse().expect("question id"),
                },
                reinforce_core::EdgeEndpoint::Card {
                    id: draft_card_id.clone(),
                },
            );
            Ok(())
        })
        .await
        .expect("insert draft quiz fixtures");

    let practical = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "practical_exam",
                "question_ids": [draft_question_for_request.as_str()],
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(practical["question_count"], 0);

    let topic = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "topic_drill",
                "knowledge_card_ids": [draft_card_for_request.as_str()],
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(topic["question_count"], 0);

    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "daily_due", "options": {"question_limit": 1}})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let session_id = created["session_id"].as_str().expect("session id");
    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
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
    expect_json(
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
    state
        .with_db(move |database| {
            database.connection().execute(
                "UPDATE question_cards SET status = 'draft' WHERE id = ?1",
                rusqlite::params![QUESTION_ONE],
            )?;
            Ok(())
        })
        .await
        .expect("unpublish historical question");
    let weakness = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "weakness_drill"})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(weakness["question_count"], 0);
}

#[tokio::test]
async fn graph_neighbors_stays_inside_the_published_subgraph() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;
    let first = card_with_status("graph A", CardStatus::Published);
    let second = card_with_status("graph B", CardStatus::Published);
    let bridge = card_with_status("graph draft", CardStatus::Draft);
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let bridge_id = bridge.id.clone();
    let bridge_edge_id = bridge_id.clone();
    let bridge_id_for_db = bridge_id.clone();
    let option_question_id: reinforce_core::QuestionCardId = fresh_id();
    let first_id_for_db = first_id.clone();
    let second_id_for_db = second_id.clone();
    let option_question_id_for_db = option_question_id.clone();
    state
        .with_db(move |database| {
            let connection = database.connection();
            for card in [&first, &second, &bridge] {
                content_store::insert_knowledge_card(connection, card)?;
            }
            add_edge(
                connection,
                reinforce_core::primitives::EdgeType::ConfusableWith,
                reinforce_core::EdgeEndpoint::Card {
                    id: first_id_for_db,
                },
                reinforce_core::EdgeEndpoint::Card { id: bridge_edge_id },
            );
            add_edge(
                connection,
                reinforce_core::primitives::EdgeType::ConfusableWith,
                reinforce_core::EdgeEndpoint::Card {
                    id: bridge_id_for_db,
                },
                reinforce_core::EdgeEndpoint::Card {
                    id: second_id_for_db.clone(),
                },
            );
            let mut question = content_store::get_question_card(
                connection,
                &QUESTION_ONE.parse().expect("question id"),
            )?
            .expect("question");
            question.id = option_question_id_for_db.clone();
            content_store::insert_question_card(connection, &question)?;
            add_edge(
                connection,
                reinforce_core::primitives::EdgeType::ConfusableWith,
                reinforce_core::EdgeEndpoint::Option {
                    id: option_question_id_for_db,
                    key: "B".to_owned(),
                },
                reinforce_core::EdgeEndpoint::Card {
                    id: second_id_for_db.clone(),
                },
            );
            Ok(())
        })
        .await
        .expect("insert graph fixtures");

    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/graph/card/{bridge_id}/neighbors"),
            None,
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let learner = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/graph/card/{first_id}/neighbors?depth=2"),
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let learner_ids: Vec<&str> = learner
        .as_array()
        .expect("neighbors")
        .iter()
        .filter_map(|node| node["endpoint"]["id"].as_str())
        .collect();
    assert!(!learner_ids.iter().any(|id| *id == second_id.as_str()));
    assert!(!learner_ids.iter().any(|id| *id == bridge_id.as_str()));

    let owner = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/graph/card/{first_id}/neighbors?depth=2"),
            None,
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let owner_ids: Vec<&str> = owner
        .as_array()
        .expect("neighbors")
        .iter()
        .filter_map(|node| node["endpoint"]["id"].as_str())
        .collect();
    assert!(owner_ids.iter().any(|id| *id == second_id.as_str()));

    let options = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/graph/option/{option_question_id}/neighbors?key=B"),
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let option_ids: Vec<&str> = options
        .as_array()
        .expect("option neighbors")
        .iter()
        .filter_map(|node| node["endpoint"]["id"].as_str())
        .collect();
    assert!(option_ids.iter().any(|id| *id == second_id.as_str()));
}

#[tokio::test]
async fn current_question_hides_solution_fields() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    let question_id: reinforce_core::QuestionCardId = fresh_id();
    let question_for_request = question_id.clone();
    state
        .with_db(move |database| {
            let connection = database.connection();
            let mut question = content_store::get_question_card(
                connection,
                &QUESTION_ONE.parse().expect("question id"),
            )?
            .expect("question");
            question.id = question_id.clone();
            question.question_type = reinforce_core::primitives::QuestionType::PinIdentify;
            question.answer_basis = Some("hidden basis".to_owned());
            question.explanation_blocks = vec![reinforce_core::ContentBlock::Markdown {
                text: "hidden explanation".to_owned(),
            }];
            if let Some(option) = question.options.first_mut() {
                option.explanation = Some("hidden option explanation".to_owned());
            }
            content_store::insert_question_card(connection, &question)?;
            Ok(())
        })
        .await
        .expect("insert question");
    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "practical_exam",
                "question_ids": [question_for_request.as_str()],
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
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
    let question = &session["question"];
    assert!(question.get("answer").is_none());
    assert!(question.get("accepted_answers").is_none());
    assert!(question.get("answer_basis").is_none());
    assert!(question.get("explanation_blocks").is_none());
    assert!(question["options"][0].get("explanation").is_none());
}

#[tokio::test]
async fn search_returns_published_results_before_the_limit() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;
    let draft_card = card_with_status("uniquefilterterm draft", CardStatus::Draft);
    let published_card = card_with_status("uniquefilterterm published", CardStatus::Published);
    let draft_card_id = draft_card.id.clone();
    let published_card_id = published_card.id.clone();
    let draft_note_id: reinforce_core::NoteId = fresh_id();
    let published_note_id: reinforce_core::NoteId = fresh_id();
    let draft_note_id_for_db = draft_note_id.clone();
    let published_note_id_for_db = published_note_id.clone();
    state
        .with_db(move |database| {
            let connection = database.connection();
            content_store::insert_knowledge_card(connection, &draft_card)?;
            content_store::insert_knowledge_card(connection, &published_card)?;
            let now = Timestamp::now();
            for (id, status) in [
                (draft_note_id_for_db, CardStatus::Draft),
                (published_note_id_for_db, CardStatus::Published),
            ] {
                let note = reinforce_core::Note {
                    id,
                    subject: "physics".to_owned(),
                    title: "uniquefilterterm note".to_owned(),
                    outline: vec![],
                    view_settings: json!({}),
                    status,
                    created_at: now,
                    updated_at: now,
                };
                content_store::insert_note(connection, &note)?;
            }
            Ok(())
        })
        .await
        .expect("insert search fixtures");

    let hits = expect_json(
        json_call(
            &app,
            Method::GET,
            "/api/search?q=uniquefilterterm&limit=2",
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let ids: Vec<&str> = hits
        .as_array()
        .expect("search hits")
        .iter()
        .filter_map(|hit| hit["id"].as_str())
        .collect();
    assert!(ids.iter().any(|id| *id == published_card_id.as_str()));
    assert!(ids.iter().any(|id| *id == published_note_id.as_str()));
    assert!(!ids.iter().any(|id| *id == draft_card_id.as_str()));
    assert!(!ids.iter().any(|id| *id == draft_note_id.as_str()));
    assert!(hits.as_array().expect("search hits").len() <= 2);
}

fn upload_garbage_count(state: &AppState) -> usize {
    std::fs::read_dir(&state.blobs)
        .expect("blob directory")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".upload-"))
        .count()
}

fn duplicate_file_multipart(bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----reinforce-duplicate-file-boundary";
    let mut body = Vec::new();
    for name in ["file", "file"] {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"dup.svg\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test]
async fn duplicate_uploads_reuse_the_stored_resource() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    let (content_type, bytes) = multipart(
        &[("title", "duplicate.svg"), ("kind", "image")],
        "duplicate.svg",
        b"<svg></svg>",
    );
    let first = expect_json(
        call(
            &app,
            Method::POST,
            "/api/resources/upload",
            Some(&content_type),
            bytes.clone(),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let second = expect_json(
        call(
            &app,
            Method::POST,
            "/api/resources/upload",
            Some(&content_type),
            bytes,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["id"], second["id"]);
    assert_eq!(upload_garbage_count(&state), 0);
}

#[tokio::test]
async fn invalid_uploads_are_rejected_before_creating_blobs() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    let cases = [
        (vec![("kind", "image")], b"missing title".as_slice()),
        (
            vec![("title", "bad-width.svg"), ("width", "0")],
            b"bad width".as_slice(),
        ),
        (
            vec![("title", "bad-height.svg"), ("height", "0")],
            b"bad height".as_slice(),
        ),
        (
            vec![("title", "bad-pages.pdf"), ("page_count", "-1")],
            b"bad page count".as_slice(),
        ),
        (
            vec![("title", "bad-kind.bin"), ("kind", "unknown")],
            b"bad kind".as_slice(),
        ),
    ];
    for (fields, bytes) in cases {
        let field_refs: Vec<(&str, &str)> =
            fields.iter().map(|(name, value)| (*name, *value)).collect();
        let (content_type, body) = multipart(&field_refs, "invalid.bin", bytes);
        assert_eq!(
            call(
                &app,
                Method::POST,
                "/api/resources/upload",
                Some(&content_type),
                body,
                None,
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        let hash = reinforce_core::ContentHash::from_bytes(bytes);
        assert!(!state.blob_path(hash.as_str()).exists());
        assert_eq!(upload_garbage_count(&state), 0);
    }

    let (content_type, body) = duplicate_file_multipart(b"two files");
    assert_eq!(
        call(
            &app,
            Method::POST,
            "/api/resources/upload",
            Some(&content_type),
            body,
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(upload_garbage_count(&state), 0);

    let response = json_call(
        &app,
        Method::POST,
        "/api/resources/upload",
        Some(json!({"title": "not multipart"})),
        None,
    )
    .await;
    let body = expect_json(response, StatusCode::BAD_REQUEST).await;
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn blob_responses_disable_sniffing_and_sandbox_svg() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    let (content_type, bytes) = multipart(
        &[("title", "secure-chart.svg"), ("kind", "image")],
        "secure-chart.svg",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    );
    let uploaded = expect_json(
        call(
            &app,
            Method::POST,
            "/api/resources/upload",
            Some(&content_type),
            bytes,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let id = uploaded["id"].as_str().expect("resource id");
    let response = json_call(
        &app,
        Method::GET,
        &format!("/api/resources/{id}/blob"),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(
        response.headers().get("content-security-policy").unwrap(),
        "sandbox"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/svg+xml"
    );
    let _ = &state;
}

#[tokio::test]
async fn internal_errors_never_expose_storage_details() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    state
        .with_db(move |database| {
            database.connection().execute(
                "UPDATE resources SET width = 'bogus-width' WHERE id = ?1",
                rusqlite::params![BASE_RESOURCE],
            )?;
            Ok(())
        })
        .await
        .expect("corrupt stored resource kind");
    let body = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/resources/{BASE_RESOURCE}"),
            None,
            None,
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
    assert_eq!(body["error"]["code"], "internal_error");
    assert_eq!(body["error"]["message"], "internal server error");
    assert!(
        !serde_json::to_string(&body)
            .expect("encode body")
            .contains("bogus-kind")
    );
}

#[tokio::test]
async fn invite_owner_setup_reuses_the_existing_owner_user() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let session = expect_json(
        json_call(
            &app,
            Method::GET,
            "/api/auth/session",
            None,
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(session["user_id"], state.owner_id.as_str());
    let users: i64 = state
        .with_db(move |database| {
            Ok(database
                .connection()
                .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?)
        })
        .await
        .expect("count users");
    assert_eq!(users, 1);
}

#[tokio::test]
async fn spa_fallback_returns_html_content_type() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let response = json_call(&app, Method::GET, "/quiz", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html; charset=utf-8"
    );
}

#[tokio::test]
async fn single_mode_rejects_missing_or_invalid_host() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;

    let missing = Request::builder()
        .uri("/api/meta")
        .body(Body::empty())
        .expect("request without host");
    assert_eq!(
        app.clone()
            .oneshot(missing)
            .await
            .expect("missing host response")
            .status(),
        StatusCode::FORBIDDEN
    );

    let invalid = Request::builder()
        .uri("/api/meta")
        .header(
            header::HOST,
            axum::http::HeaderValue::from_bytes(&[0xff]).expect("invalid host header"),
        )
        .body(Body::empty())
        .expect("request with invalid host");
    assert_eq!(
        app.clone()
            .oneshot(invalid)
            .await
            .expect("invalid host response")
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn bind_host_comparison_is_case_insensitive() {
    let (app, _state, _dir) = test_state_with_host(AuthMode::Single, "Intranet.Example").await;
    assert_eq!(
        raw_call(
            &app,
            Method::GET,
            "/api/meta",
            &[("host", "intranet.example:3000")]
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        raw_call(
            &app,
            Method::GET,
            "/api/meta",
            &[("host", "attacker.example:3000")]
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn short_queries_and_older_drafts_do_not_crowd_out_published_hits() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;
    // Drafts get older UUIDv7 ids so they sort first under ORDER BY id.
    let draft_card = card_with_status("熔沸草稿", CardStatus::Draft);
    let draft_note_id: reinforce_core::NoteId = fresh_id();
    let published_card = card_with_status("熔沸筆記", CardStatus::Published);
    let published_note_id: reinforce_core::NoteId = fresh_id();
    let draft_card_id = draft_card.id.clone();
    let published_card_id = published_card.id.clone();
    let (draft_note_for_db, published_note_for_db) =
        (draft_note_id.clone(), published_note_id.clone());
    state
        .with_db(move |database| {
            let connection = database.connection();
            content_store::insert_knowledge_card(connection, &draft_card)?;
            content_store::insert_knowledge_card(connection, &published_card)?;
            let now = Timestamp::now();
            for (id, status) in [
                (draft_note_for_db, CardStatus::Draft),
                (published_note_for_db, CardStatus::Published),
            ] {
                content_store::insert_note(
                    connection,
                    &reinforce_core::Note {
                        id,
                        subject: "physics".to_owned(),
                        title: "凝華昇華講義".to_owned(),
                        outline: vec![],
                        view_settings: json!({}),
                        status,
                        created_at: now,
                        updated_at: now,
                    },
                )?;
            }
            let core_hits = reinforce_core::search::search_knowledge_cards_visible(
                connection, "熔沸", 10, true,
            )?;
            assert_eq!(
                core_hits,
                vec![published_card.id.clone()],
                "a two-character query must not return draft cards when published_only is set"
            );
            Ok(())
        })
        .await
        .expect("insert search fixtures");

    for (query, expected, hidden) in [
        ("熔沸", published_card_id.as_str(), draft_card_id.as_str()),
        (
            "凝華昇華",
            published_note_id.as_str(),
            draft_note_id.as_str(),
        ),
    ] {
        let hits = expect_json(
            json_call(
                &app,
                Method::GET,
                &format!("/api/search?q={}&limit=1", urlencoding(query)),
                None,
                Some(&member_cookie),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        let ids: Vec<&str> = hits
            .as_array()
            .expect("search hits")
            .iter()
            .filter_map(|hit| hit["id"].as_str())
            .collect();
        assert!(
            ids.contains(&expected.as_str()),
            "{query}: published hit missing from {ids:?}"
        );
        assert!(
            !ids.contains(&hidden.as_str()),
            "{query}: draft leaked in {ids:?}"
        );
    }
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn answer_error_tags_are_persisted_and_reported() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "daily_due", "options": {"question_limit": 1}})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let session_id = created["session_id"].as_str().expect("session id");
    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
            Some(json!({
                "submission": {"kind": "keys", "values": ["B"]},
                "confidence": "split",
                "error_tags": [" 概念混淆 ", "概念混淆", "計算失誤"]
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    expect_json(
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

    let attempts = expect_json(
        json_call(&app, Method::GET, "/api/study/attempts", None, None).await,
        StatusCode::OK,
    )
    .await;
    let tags = attempts[0]["error_tags"].as_array().expect("error tags");
    let tags: Vec<&str> = tags.iter().filter_map(|tag| tag.as_str()).collect();
    assert_eq!(tags, vec!["incorrect", "概念混淆", "計算失誤"]);

    let report = expect_json(
        json_call(&app, Method::GET, "/api/study/weakness", None, None).await,
        StatusCode::OK,
    )
    .await;
    let question = report["questions"]
        .as_array()
        .expect("questions")
        .iter()
        .find(|entry| entry["question_id"] == QUESTION_ONE)
        .expect("question weakness");
    let counts = question["common_error_tags"]
        .as_array()
        .expect("common error tags");
    assert!(
        counts
            .iter()
            .any(|entry| entry["tag"] == "概念混淆" && entry["count"] == 1)
    );
    assert!(
        counts
            .iter()
            .any(|entry| entry["tag"] == "計算失誤" && entry["count"] == 1)
    );
}

#[tokio::test]
async fn answer_rejects_invalid_error_tags() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "daily_due", "options": {"question_limit": 2}})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let session_id = created["session_id"].as_str().expect("session id");

    for error_tags in [
        json!([" "]),
        json!(["x".repeat(41)]),
        json!(
            (1..=11)
                .map(|index| format!("tag-{index}"))
                .collect::<Vec<_>>()
        ),
    ] {
        let response = json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
            Some(json!({
                "submission": {"kind": "keys", "values": ["A"]},
                "confidence": "certain",
                "error_tags": error_tags
            })),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    expect_json(
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
    let attempts = expect_json(
        json_call(&app, Method::GET, "/api/study/attempts", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(attempts.as_array().expect("attempts").len(), 0);
}

const DEMO_NOTE_ID: &str = "01a21714-c98d-769f-be7a-f92b11e5cc60";
const DEMO_CARD_PHASE_CHANGES: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";
const DEMO_CARD_PHASE_PINS: &str = "01a21714-c98d-7b1d-92e4-b2d361f54d7f";
const DEMO_CARD_CLAPEYRON: &str = "01a21714-c98d-7210-a469-b56decf9f22b";

#[tokio::test]
async fn note_view_settings_saves_canvas_preserves_other_keys_and_enforces_owner() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;

    // Seed an unrelated view_settings key on the demo note so we can verify preservation.
    let note_id: reinforce_core::NoteId = DEMO_NOTE_ID.parse().expect("note id");
    state
        .with_db(move |database| {
            content_store::update_note_view_settings(
                database.connection(),
                &note_id,
                &json!({ "reading": { "font_scale": 1.15 }, "custom_flag": true }),
                Timestamp::now(),
            )?;
            Ok(())
        })
        .await
        .expect("seed other view_settings keys");

    let payload = json!({
        "canvas": {
            "version": 1,
            "nodes": [
                { "card_id": DEMO_CARD_PHASE_CHANGES, "x": 120.5, "y": 40.0, "w": 320.0, "h": 200.0 },
                { "card_id": DEMO_CARD_PHASE_PINS, "x": 520.0, "y": 90.25, "w": 340.0, "h": 210.0 }
            ],
            "viewport": { "x": -48.0, "y": 16.0, "zoom": 1.25 }
        }
    });

    // Learner cannot update note view_settings (403).
    let forbidden = json_call(
        &app,
        Method::PUT,
        &format!("/api/notes/{DEMO_NOTE_ID}/view_settings"),
        Some(payload.clone()),
        Some(&member_cookie),
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Owner saves canvas layout (200).
    let saved = expect_json(
        json_call(
            &app,
            Method::PUT,
            &format!("/api/notes/{DEMO_NOTE_ID}/view_settings"),
            Some(payload.clone()),
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved["view_settings"]["canvas"], payload["canvas"]);
    assert_eq!(
        saved["view_settings"]["reading"],
        json!({ "font_scale": 1.15 })
    );
    assert_eq!(saved["view_settings"]["custom_flag"], json!(true));

    // Reloading note via GET preserves exact saved coordinates and unrelated keys.
    let reloaded = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/notes/{DEMO_NOTE_ID}"),
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(reloaded["view_settings"]["canvas"], payload["canvas"]);
    assert_eq!(
        reloaded["view_settings"]["reading"],
        json!({ "font_scale": 1.15 })
    );
    assert_eq!(reloaded["view_settings"]["custom_flag"], json!(true));
}

#[tokio::test]
async fn note_view_settings_validates_outline_membership_numbers_zoom_and_size() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let uri = format!("/api/notes/{DEMO_NOTE_ID}/view_settings");

    // 1. card_id not in the note's outline (Clapeyron card exists in DB but is not in demo note outline).
    let outside_outline = expect_json(
        json_call(
            &app,
            Method::PUT,
            &uri,
            Some(json!({
                "canvas": {
                    "version": 1,
                    "nodes": [
                        { "card_id": DEMO_CARD_CLAPEYRON, "x": 0.0, "y": 0.0, "w": 320.0, "h": 200.0 }
                    ],
                    "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
                }
            })),
            None,
        )
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        outside_outline["error"]["details"]["fields"][0]["field"],
        "canvas.nodes[0].card_id"
    );

    // 2. Non-finite number in JSON (1e309 overflows f64 and raw NaN token).
    for raw_body in [
        format!(
            r#"{{"canvas":{{"version":1,"nodes":[{{"card_id":"{DEMO_CARD_PHASE_CHANGES}","x":1e309,"y":0,"w":320,"h":200}}],"viewport":{{"x":0,"y":0,"zoom":1}}}}}}"#
        ),
        format!(
            r#"{{"canvas":{{"version":1,"nodes":[{{"card_id":"{DEMO_CARD_PHASE_CHANGES}","x":NaN,"y":0,"w":320,"h":200}}],"viewport":{{"x":0,"y":0,"zoom":1}}}}}}"#
        ),
    ] {
        let resp = call(
            &app,
            Method::PUT,
            &uri,
            Some("application/json"),
            raw_body.into_bytes(),
            None,
        )
        .await;
        let body = expect_json(resp, StatusCode::BAD_REQUEST).await;
        assert!(body["error"]["details"]["fields"].is_array());
    }

    // 3. Zoom out of bounds (< 0.1 or > 4.0).
    for bad_zoom in [0.05, 0.0, -1.0, 4.01, 10.0] {
        let resp = expect_json(
            json_call(
                &app,
                Method::PUT,
                &uri,
                Some(json!({
                    "canvas": {
                        "version": 1,
                        "nodes": [],
                        "viewport": { "x": 0.0, "y": 0.0, "zoom": bad_zoom }
                    }
                })),
                None,
            )
            .await,
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_eq!(
            resp["error"]["details"]["fields"][0]["field"],
            "canvas.viewport.zoom"
        );
    }

    // 4. Too many nodes (> 2000).
    let too_many_nodes: Vec<Value> = (0..2001)
        .map(|i| {
            json!({
                "card_id": DEMO_CARD_PHASE_CHANGES,
                "x": i as f64,
                "y": 0.0,
                "w": 320.0,
                "h": 200.0
            })
        })
        .collect();
    let too_many_resp = expect_json(
        json_call(
            &app,
            Method::PUT,
            &uri,
            Some(json!({
                "canvas": {
                    "version": 1,
                    "nodes": too_many_nodes,
                    "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
                }
            })),
            None,
        )
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        too_many_resp["error"]["details"]["fields"][0]["field"],
        "canvas.nodes"
    );

    // 5. Payload exceeding 256 KiB returns 400.
    let huge_padding = " ".repeat(260 * 1024);
    let oversized = format!(
        r#"{{"canvas":{{"version":1,"nodes":[],"viewport":{{"x":0,"y":0,"zoom":1}}}}{huge_padding}}}"#
    );
    let oversized_resp = expect_json(
        call(
            &app,
            Method::PUT,
            &uri,
            Some("application/json"),
            oversized.into_bytes(),
            None,
        )
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        oversized_resp["error"]["details"]["fields"][0]["field"],
        "view_settings"
    );
}

#[tokio::test]
async fn note_edges_returns_typed_edges_between_outline_cards_only() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;

    // Initially, demo note outline has PHASE_CHANGES and PHASE_PINS; demo edge is CLAPEYRON -> PHASE_CHANGES,
    // which should NOT appear because CLAPEYRON is outside this note's outline.
    let initial = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/notes/{DEMO_NOTE_ID}/edges"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(initial.as_array().expect("edges array").len(), 0);

    // Insert a prerequisite edge between the two cards inside the demo note outline.
    let from_id: reinforce_core::KnowledgeCardId = DEMO_CARD_PHASE_CHANGES.parse().expect("from");
    let to_id: reinforce_core::KnowledgeCardId = DEMO_CARD_PHASE_PINS.parse().expect("to");
    let edge = reinforce_core::Edge {
        id: reinforce_core::EdgeId::generate(),
        edge_type: reinforce_core::primitives::EdgeType::Prerequisite,
        from: reinforce_core::EdgeEndpoint::Card { id: from_id },
        to: reinforce_core::EdgeEndpoint::Card { id: to_id },
        created_at: Timestamp::now(),
    };
    let expected_id = edge.id.as_str();
    state
        .with_db(move |database| {
            content_store::insert_edge(database.connection(), &edge)?;
            Ok(())
        })
        .await
        .expect("insert edge");

    let edges = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/notes/{DEMO_NOTE_ID}/edges"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let list = edges.as_array().expect("edges array");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], expected_id);
    assert_eq!(list[0]["edge_type"], "prerequisite");
    assert_eq!(list[0]["from"]["id"], DEMO_CARD_PHASE_CHANGES);
    assert_eq!(list[0]["to"]["id"], DEMO_CARD_PHASE_PINS);
}

#[tokio::test]
async fn subject_groups_scope_sessions_and_indexed_answers_replace() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let root = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/subject_groups",
            Some(json!({"name": "自然科", "subjects": ["physics"]})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let root_id = root["id"].as_str().expect("root id").to_owned();
    assert_eq!(root["sort_order"], 0);
    assert_eq!(root["subjects"], json!(["physics"]));

    let child = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/subject_groups",
            Some(json!({
                "parent_id": root_id,
                "name": "化學",
                "subjects": ["chemistry", "chemistry"],
                "sort_order": 3
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let child_id = child["id"].as_str().expect("child id").to_owned();
    assert_eq!(child["parent_id"], root_id);
    assert_eq!(child["subjects"], json!(["chemistry"]));

    assert_eq!(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/subject_groups/{root_id}"),
            Some(json!({"parent_id": child_id})),
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST,
        "a group cannot be moved below its own descendant"
    );
    assert_eq!(
        json_call(
            &app,
            Method::POST,
            "/api/subject_groups",
            Some(json!({"name": "   "})),
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    let detached = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/subject_groups/{child_id}"),
            Some(json!({"parent_id": null, "name": "化學科"})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(detached["parent_id"].is_null());
    assert_eq!(detached["name"], "化學科");
    assert_eq!(detached["subjects"], json!(["chemistry"]));
    let reattached = expect_json(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/subject_groups/{child_id}"),
            Some(json!({"parent_id": root_id})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(reattached["parent_id"], root_id);
    assert_eq!(
        json_call(
            &app,
            Method::PATCH,
            "/api/subject_groups/00000000-0000-7000-8000-000000000000",
            Some(json!({"name": "ghost"})),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let groups = expect_json(
        json_call(&app, Method::GET, "/api/subject_groups", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(groups.as_array().expect("groups").len(), 2);

    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "full_exam",
                "subject_group_id": root_id,
                "options": {"question_limit": 10, "total_time_secs": 600}
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(created["question_count"], 4);
    let session_id = created["session_id"]
        .as_str()
        .expect("session id")
        .to_owned();

    let empty = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "daily_due", "subject_group_id": child_id})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        empty["question_count"], 0,
        "a group without matching published questions must build an empty session"
    );
    let single = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "daily_due", "subject": "physics"})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(single["question_count"], 4);
    let blank_subject = json_call(
        &app,
        Method::POST,
        "/api/study/sessions",
        Some(json!({"mode": "daily_due", "subject": "  "})),
        None,
    )
    .await;
    assert_eq!(blank_subject.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "daily_due",
                "subject_group_id": "00000000-0000-7000-8000-000000000000"
            })),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

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
    assert_eq!(session["question_count"], 4);
    let questions = session["questions"].as_array().expect("questions");
    assert_eq!(questions.len(), 4);
    for question in questions {
        assert!(question.get("answer").is_none());
        assert!(question.get("explanation_blocks").is_none());
        assert!(question.get("accepted_answers").is_none());
    }
    assert!(session["answers"].as_array().expect("answers").is_empty());
    assert!(session["graded"].as_array().expect("graded").is_empty());
    assert!(session["exam_sets"].is_object());
    assert!(session["question_groups"].is_object());

    let selected = expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/select"),
            Some(json!({"index": 2})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(selected["state"]["InProgress"]["index"], 2);
    assert_eq!(
        selected["question"]["id"], "01a21714-c98d-7c8f-800b-0200f1cba221",
        "selecting index 2 must expose the third exam question"
    );
    assert_eq!(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/select"),
            Some(json!({"index": 9})),
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    let first = expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
            Some(json!({
                "index": 1,
                "submission": {"kind": "keys", "values": ["A"]},
                "confidence": "certain"
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["index"], 1);
    assert!(first["grading"].is_null(), "full exams defer grading");
    assert!(first["revealed_question"].is_null());
    let replaced = expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
            Some(json!({
                "index": 1,
                "submission": {"kind": "keys", "values": ["B"]},
                "confidence": "split"
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(replaced["index"], 1);
    assert_eq!(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/answers"),
            Some(json!({
                "index": 9,
                "submission": {"kind": "keys", "values": ["A"]},
                "confidence": "certain"
            })),
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let after_answers = expect_json(
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
    let answers = after_answers["answers"].as_array().expect("answers");
    assert_eq!(answers.len(), 1, "re-answering must replace, not append");
    assert_eq!(answers[0]["index"], 1);
    assert_eq!(answers[0]["submission"]["values"], json!(["B"]));
    assert!(
        after_answers["graded"]
            .as_array()
            .expect("graded")
            .is_empty()
    );

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
    assert_eq!(finished["graded"].as_array().expect("graded").len(), 1);

    let deleted = expect_json(
        json_call(
            &app,
            Method::DELETE,
            &format!("/api/subject_groups/{root_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(deleted["ok"], true);
    let remaining = expect_json(
        json_call(&app, Method::GET, "/api/subject_groups", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert!(
        remaining.as_array().expect("groups").is_empty(),
        "deleting a root group cascades to its children"
    );
}

#[tokio::test]
async fn question_groups_and_linked_questions_are_exposed() {
    let (app, state, _dir) = test_state(AuthMode::Single).await;
    let group_id: reinforce_core::QuestionGroupId =
        "01a21714-c98d-7f00-8000-800000000001".parse().unwrap();
    let exam_set_id: reinforce_core::ExamSetId = EXAM_SET.parse().unwrap();
    let question_id: reinforce_core::QuestionCardId = QUESTION_ONE.parse().unwrap();
    let grouped = group_id.clone();
    let linked_question = question_id.clone();
    state
        .with_db(move |database| {
            content_store::insert_question_group(
                database.connection(),
                &reinforce_core::QuestionGroup {
                    id: grouped.clone(),
                    exam_set_id: Some(exam_set_id),
                    title: Some("共用題幹".to_owned()),
                    shared_stem_blocks: Vec::new(),
                },
            )?;
            database.connection().execute(
                "UPDATE question_cards SET question_group_id = ?1 WHERE id = ?2",
                rusqlite::params![grouped, linked_question],
            )?;
            Ok(())
        })
        .await
        .expect("insert question group");

    let groups = expect_json(
        json_call(&app, Method::GET, "/api/question_groups", None, None).await,
        StatusCode::OK,
    )
    .await;
    let list = groups.as_array().expect("groups");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], group_id.as_str());
    assert_eq!(list[0]["title"], "共用題幹");

    let filtered = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/question_groups?exam_set_id={EXAM_SET}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(filtered.as_array().expect("filtered").len(), 1);
    let other_set = "00000000-0000-7000-8000-000000000000";
    let none = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/question_groups?exam_set_id={other_set}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(none.as_array().expect("none").is_empty());
    let group = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/question_groups/{}", group_id.as_str()),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(group["id"], group_id.as_str());
    assert_eq!(group["exam_set_id"], EXAM_SET);
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/question_groups/{other_set}"),
            None,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let linked = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/knowledge_cards/{DEMO_CARD_PHASE_CHANGES}/questions"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(linked["count"], 1);
    assert_eq!(linked["question_ids"], json!([QUESTION_ONE]));
    assert_eq!(
        json_call(
            &app,
            Method::GET,
            &format!("/api/knowledge_cards/{other_set}/questions"),
            None,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "full_exam",
                "exam_set_id": EXAM_SET,
                "options": {"question_limit": 10, "total_time_secs": 600}
            })),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let session_id = created["session_id"]
        .as_str()
        .expect("session id")
        .to_owned();
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
    let groups_map = session["question_groups"].as_object().expect("group map");
    assert_eq!(
        groups_map[group_id.as_str().as_str()]["title"],
        "共用題幹",
        "the session must carry the shared stem of its questions"
    );
    let sets = session["exam_sets"].as_object().expect("exam set map");
    assert_eq!(sets[EXAM_SET]["title"], "相位變化小測");
    assert_eq!(sets[EXAM_SET]["subject"], "physics");
    assert!(sets[EXAM_SET]["exam_year"].is_null());

    let daily = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({"mode": "daily_due"})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let daily_id = daily["session_id"].as_str().expect("daily id").to_owned();
    expect_json(
        json_call(
            &app,
            Method::POST,
            &format!("/api/study/sessions/{daily_id}/answers"),
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
    let revealed = expect_json(
        json_call(
            &app,
            Method::GET,
            &format!("/api/study/sessions/{daily_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let questions = revealed["questions"].as_array().expect("questions");
    assert!(
        questions[0].get("answer").is_some(),
        "an immediately graded question is revealed in the question list"
    );
    assert!(
        questions[1].get("answer").is_none(),
        "unanswered questions keep their answers hidden"
    );
    let answers = revealed["answers"].as_array().expect("answers");
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0]["index"], 0);
    assert_eq!(revealed["graded"].as_array().expect("graded").len(), 1);
}

#[tokio::test]
async fn members_cannot_manage_subject_groups() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner_cookie = setup_invite_owner(&app, &state).await;
    let member_cookie = register_member(&app, &owner_cookie).await;
    let created = expect_json(
        json_call(
            &app,
            Method::POST,
            "/api/subject_groups",
            Some(json!({"name": "自然科", "subjects": ["physics"]})),
            Some(&owner_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        json_call(
            &app,
            Method::POST,
            "/api/subject_groups",
            Some(json!({"name": "member group"})),
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(
            &app,
            Method::PATCH,
            &format!("/api/subject_groups/{}", created["id"].as_str().unwrap()),
            Some(json!({"name": "renamed"})),
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_call(
            &app,
            Method::DELETE,
            &format!("/api/subject_groups/{}", created["id"].as_str().unwrap()),
            None,
            Some(&member_cookie),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let listed = expect_json(
        json_call(
            &app,
            Method::GET,
            "/api/subject_groups",
            None,
            Some(&member_cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(listed.as_array().expect("groups").len(), 1);
}
