use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::AttemptLog;
use reinforce_core::auth::set_setting;
use reinforce_core::db::{content as content_store, study as study_store};
use reinforce_core::primitives::AttemptId;
use reinforce_core::primitives::ExportPolicy;
use reinforce_core::primitives::{AttemptDirection, Confidence, SessionMode};
use serde_json::{Value, json};
use tower::ServiceExt;

use reinforce_server::auth::hash_token;
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::demo;
use reinforce_server::state::AppState;

static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const OWNER_SETUP_TOKEN: &str = "sync-owner-token";
const OWNER_PASSWORD: &str = "sync-owner-password";
const MEMBER_A_PASSWORD: &str = "sync-member-a-password";
const MEMBER_B_PASSWORD: &str = "sync-member-b-password";
const REDACTED_RESOURCE: &str = "01a21714-c98c-78d5-8550-3d0928b9c9d1";
const REDACTED_SHA256: &str = "d5115bdba60ace7b71df3758d174dec49382a539f34fd007ee263692f49e5179";
const ALLOWED_RESOURCE: &str = "01a21714-c98c-7bfe-8cf8-a60b84573f56";
const ALLOWED_SHA256: &str = "22df5f119108ae55882dc11a03d09ba426cf4771ef92eaa418457f8db40cbe4a";
const CARD_ONE: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const CARD_TWO: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";

type TestApp = Router<()>;

struct TestDir(std::path::PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn test_state(mode: AuthMode) -> (TestApp, AppState, TestDir) {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-sync-test-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.join("test.db"),
        blobs: directory.join("blobs"),
        fonts: directory.join("fonts"),
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

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    bearer: Option<&str>,
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
    if let Some(value) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {value}"));
    }
    let body = body.map_or_else(Vec::new, |value| {
        serde_json::to_vec(&value).expect("encode json")
    });
    let request = builder.body(Body::from(body)).expect("build request");
    app.clone().oneshot(request).await.expect("run request")
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
    let actual = response.status();
    let value = response_json(response).await;
    assert_eq!(actual, status, "unexpected response body: {value}");
    value
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

async fn owner_cookie(app: &TestApp, state: &AppState) -> String {
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
        call(
            app,
            Method::POST,
            "/api/auth/owner/setup",
            Some(json!({
                "token": OWNER_SETUP_TOKEN,
                "username": "owner",
                "password": OWNER_PASSWORD,
            })),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let response = call(
        app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "owner", "password": OWNER_PASSWORD})),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    session_cookie(&response)
}

struct Member {
    user_id: String,
    cookie: String,
}

async fn register_member(app: &TestApp, owner_cookie: &str, name: &str, password: &str) -> Member {
    let invite = expect_json(
        call(
            app,
            Method::POST,
            "/api/auth/invites",
            Some(json!({"expires_in_secs": 3600})),
            Some(owner_cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let code = invite["code"].as_str().expect("invite code").to_owned();
    let registered = expect_json(
        call(
            app,
            Method::POST,
            "/api/auth/register",
            Some(json!({
                "invite_code": code,
                "username": name,
                "password": password,
            })),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let response = call(
        app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": name, "password": password})),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    Member {
        user_id: registered["user_id"].as_str().expect("user id").to_owned(),
        cookie: session_cookie(&response),
    }
}

fn attempt_json(user_id: &str, card_id: &str, created_at: &str, is_correct: bool) -> Value {
    let attempt = AttemptLog {
        id: AttemptId::generate(),
        user_id: user_id.parse().expect("user id"),
        card_id: card_id.parse().expect("card id"),
        direction: AttemptDirection::Default,
        session_mode: SessionMode::DailyDue,
        submitted_payload: json!({"keys": ["A"]}),
        is_correct,
        duration_ms: 1_200,
        confidence: Confidence::Certain,
        error_tags: Vec::new(),
        created_at: created_at.parse().expect("timestamp"),
        sync_cursor: 0,
        affects_scheduling: true,
    };
    serde_json::to_value(attempt).expect("attempt json")
}

fn meta_json(user_id: &str, card_id: &str, updated_at: &str, note: &str) -> Value {
    json!({
        "user_id": user_id,
        "card_id": card_id,
        "starred": true,
        "personal_tags": ["hard"],
        "personal_note": note,
        "updated_at": updated_at,
    })
}

#[tokio::test]
async fn sync_requires_a_session_and_rejects_agent_tokens() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;

    let response = call(&app, Method::GET, "/api/sync/content", None, None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = call(
        &app,
        Method::POST,
        "/api/sync/attempts",
        Some(json!({"attempts": []})),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let owner = owner_cookie(&app, &state).await;
    let token = expect_json(
        call(
            &app,
            Method::POST,
            "/api/auth/agent-tokens",
            None,
            Some(&owner),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let token = token["token"].as_str().expect("agent token").to_owned();

    let response = call(
        &app,
        Method::GET,
        "/api/sync/content",
        None,
        None,
        Some(&token),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "an agent token is not a device credential"
    );
    let response = call(
        &app,
        Method::GET,
        "/api/sync/attempts",
        None,
        None,
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Even a real session may not smuggle a bearer token into sync.
    let response = call(
        &app,
        Method::GET,
        "/api/sync/content",
        None,
        Some(&owner),
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn single_mode_syncs_as_the_implicit_owner() {
    let (app, _state, _dir) = test_state(AuthMode::Single).await;
    let content = expect_json(
        call(&app, Method::GET, "/api/sync/content", None, None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(content["unchanged"], false);
    let hash = content["snapshot_sha256"]
        .as_str()
        .expect("snapshot hash")
        .to_owned();
    assert!(
        content["bundle_jsonl"]
            .as_str()
            .expect("bundle")
            .contains(CARD_ONE)
    );

    let again = expect_json(
        call(
            &app,
            Method::GET,
            &format!("/api/sync/content?known={hash}"),
            None,
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(again["unchanged"], true);
    assert!(again.get("bundle_jsonl").is_none());
}

#[tokio::test]
async fn users_cannot_read_or_write_each_others_study_store() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member_a = register_member(&app, &owner, "sync-member-a", MEMBER_A_PASSWORD).await;
    let member_b = register_member(&app, &owner, "sync-member-b", MEMBER_B_PASSWORD).await;

    let attempt = attempt_json(
        &member_a.user_id,
        CARD_ONE,
        "2026-01-02T10:00:00.000000Z",
        true,
    );
    let pushed = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(json!({"attempts": [attempt.clone()]})),
            Some(&member_a.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(pushed["accepted"], 1);

    let pulled = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/attempts",
            None,
            Some(&member_b.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(
        pulled["attempts"].as_array().expect("attempts").is_empty(),
        "another learner's log must stay invisible"
    );

    let forged = attempt_json(
        &member_a.user_id,
        CARD_ONE,
        "2026-01-02T10:01:00.000000Z",
        true,
    );
    let response = call(
        &app,
        Method::POST,
        "/api/sync/attempts",
        Some(json!({"attempts": [forged]})),
        Some(&member_b.cookie),
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a learner cannot write into another study store"
    );

    let mut hijacked = attempt.clone();
    hijacked["user_id"] = json!(member_b.user_id);
    let response = call(
        &app,
        Method::POST,
        "/api/sync/attempts",
        Some(json!({"attempts": [hijacked]})),
        Some(&member_b.cookie),
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "an attempt id owned by someone else cannot be reused"
    );

    let meta = meta_json(
        &member_a.user_id,
        CARD_ONE,
        "2026-01-02T10:00:00.000000Z",
        "a",
    );
    expect_json(
        call(
            &app,
            Method::PUT,
            "/api/sync/user-card-meta",
            Some(json!({"user_card_meta": [meta]})),
            Some(&member_a.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let pulled = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/user-card-meta",
            None,
            Some(&member_b.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(
        pulled["user_card_meta"]
            .as_array()
            .expect("meta")
            .is_empty()
    );

    let forged = meta_json(
        &member_a.user_id,
        CARD_TWO,
        "2026-01-02T10:02:00.000000Z",
        "forged",
    );
    let response = call(
        &app,
        Method::PUT,
        "/api/sync/user-card-meta",
        Some(json!({"user_card_meta": [forged]})),
        Some(&member_b.cookie),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn push_is_idempotent_and_pull_pages_by_cursor() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;

    let attempts = vec![
        attempt_json(
            &member.user_id,
            CARD_ONE,
            "2026-01-02T10:00:00.000000Z",
            true,
        ),
        attempt_json(
            &member.user_id,
            CARD_ONE,
            "2026-01-02T10:05:00.000000Z",
            false,
        ),
        attempt_json(
            &member.user_id,
            CARD_TWO,
            "2026-01-02T10:06:00.000000Z",
            true,
        ),
    ];
    let batch = json!({"attempts": attempts.clone()});
    let first = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(batch.clone()),
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["accepted"], 3);
    assert_eq!(first["duplicates"], 0);
    assert_eq!(first["cursor"], 3);

    // A retry after a lost response is a duplicate, not a new row.
    let retry = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(batch),
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(retry["accepted"], 0);
    assert_eq!(retry["duplicates"], 3);
    assert_eq!(retry["cursor"], 3);

    // The study store and its derived scheduling hold exactly three attempts.
    let stored = expect_json(
        call(
            &app,
            Method::GET,
            "/api/study/attempts",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(stored.as_array().expect("attempts").len(), 3);
    let scheduling = expect_json(
        call(
            &app,
            Method::GET,
            &format!("/api/study/scheduling?card_id={CARD_ONE}"),
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(scheduling["reps"], 2);
    assert_eq!(scheduling["lapses"], 1);

    // Same id with different content is a conflict instead of a silent overwrite.
    let mut tampered = attempts[0].clone();
    tampered["is_correct"] = json!(false);
    let response = call(
        &app,
        Method::POST,
        "/api/sync/attempts",
        Some(json!({"attempts": [tampered]})),
        Some(&member.cookie),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let page = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/attempts?cursor=0&limit=2",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["attempts"].as_array().expect("page").len(), 2);
    assert_eq!(page["has_more"], true);
    assert_eq!(page["next_cursor"], 2);
    let page = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/attempts?cursor=2&limit=2",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["attempts"].as_array().expect("page").len(), 1);
    assert_eq!(page["has_more"], false);
    assert_eq!(page["next_cursor"], 3);
    let empty = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/attempts?cursor=3",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(empty["attempts"].as_array().expect("page").is_empty());
    assert_eq!(empty["next_cursor"], 3);
}

#[tokio::test]
async fn user_card_meta_is_last_writer_wins_on_the_server() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;

    let newer = meta_json(
        &member.user_id,
        CARD_ONE,
        "2026-01-02T10:00:00.000000Z",
        "newer",
    );
    let applied = expect_json(
        call(
            &app,
            Method::PUT,
            "/api/sync/user-card-meta",
            Some(json!({"user_card_meta": [newer]})),
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(applied["applied"], 1);

    let older = meta_json(
        &member.user_id,
        CARD_ONE,
        "2026-01-01T10:00:00.000000Z",
        "older",
    );
    let kept = expect_json(
        call(
            &app,
            Method::PUT,
            "/api/sync/user-card-meta",
            Some(json!({"user_card_meta": [older]})),
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(kept["applied"], 0);
    assert_eq!(kept["kept"], 1);

    let pulled = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/user-card-meta",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let rows = pulled["user_card_meta"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["personal_note"], "newer");
}

#[tokio::test]
async fn content_snapshot_hides_restricted_resources_from_learners() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;

    let owner_snapshot = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/content",
            None,
            Some(&owner),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let owner_bundle = owner_snapshot["bundle_jsonl"]
        .as_str()
        .expect("owner bundle");
    assert!(owner_bundle.contains(REDACTED_RESOURCE));
    let owner_assets = owner_snapshot["assets"].as_array().expect("assets");
    assert_eq!(owner_assets.len(), 2, "the owner receives both blobs");

    let member_snapshot = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/content",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let member_bundle = member_snapshot["bundle_jsonl"]
        .as_str()
        .expect("member bundle");
    assert!(
        !member_bundle.contains(REDACTED_RESOURCE),
        "a redacted resource row must not travel"
    );
    assert!(!member_bundle.contains(REDACTED_SHA256));
    assert!(member_bundle.contains(ALLOWED_RESOURCE));
    assert!(
        member_bundle.contains("Content redacted for this viewer"),
        "the referencing block becomes a notice instead of dangling media"
    );
    let member_assets = member_snapshot["assets"].as_array().expect("assets");
    assert_eq!(member_assets.len(), 1);
    assert_eq!(member_assets[0]["resource_id"], ALLOWED_RESOURCE);
    assert_eq!(member_assets[0]["sha256"], ALLOWED_SHA256);

    // The blob endpoint is the second line of defence for restricted media.
    let response = call(
        &app,
        Method::GET,
        &format!("/api/resources/{REDACTED_RESOURCE}/blob"),
        None,
        Some(&member.cookie),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// --- regression tests for the review findings --------------------------------

const SET_TRIM: &str = "01a21714-c98d-7931-a48a-000000000001";
const SET_ARCHIVED: &str = "01a21714-c98d-7931-a48a-000000000002";
const SET_UNCHECKED: &str = "01a21714-c98d-7931-a48a-000000000010";
const Q_INFERRED: &str = "01a21714-c98d-70f7-90ba-000000000001";
const Q_DUPLICATE: &str = "01a21714-c98d-70f7-90ba-000000000002";
const Q_DRAFT: &str = "01a21714-c98d-70f7-90ba-000000000003";
const Q_ARCHIVED: &str = "01a21714-c98d-70f7-90ba-000000000004";
const Q_PUBLISHED: &str = "01a21714-c98d-70f7-90ba-000000000005";
const Q_DUPLICATE_ARCHIVED: &str = "01a21714-c98d-70f7-90ba-000000000006";
const Q_ARCHIVED_ORIGINAL: &str = "01a21714-c98d-70f7-90ba-000000000007";
const Q_UNCHECKED: &str = "01a21714-c98d-70f7-90ba-000000000010";
const R_REDACT: &str = "01a21714-c98c-78d5-8550-000000000001";
const R_OWNER: &str = "01a21714-c98c-78d5-8550-000000000002";
const A_REDACT: &str = "01a21714-c98d-7814-a2c2-000000000001";
const A_OWNER: &str = "01a21714-c98d-7814-a2c2-000000000002";
const CARD_UNCHECKED: &str = "01a21714-c98d-716e-9b53-000000000010";
const CARD_DRAFT: &str = "01a21714-c98d-716e-9b53-000000000011";
const NOTE_ONE: &str = "01a21714-c98d-769f-be7a-f92b11e5cc60";
const DEMO_EXAM_SET: &str = "01a21714-c98d-7931-a48a-6a1d5cd348bd";

fn stamp(value: &str) -> reinforce_core::Timestamp {
    value.parse().expect("timestamp")
}

fn exam_set(id: &str, expected: Option<u32>, is_complete: bool) -> reinforce_core::ExamSet {
    reinforce_core::ExamSet {
        id: id.parse().expect("set"),
        subject: "physics".to_owned(),
        title: "fixture set".to_owned(),
        source_kind: reinforce_core::primitives::SourceKind::Original,
        exam_year: None,
        source_resource_id: None,
        expected_questions: expected,
        metadata: json!({}),
        is_complete,
    }
}

fn question(id: &str, set: &str, number: u32) -> reinforce_core::QuestionCard {
    reinforce_core::QuestionCard {
        id: id.parse().expect("question"),
        exam_set_id: Some(set.parse().expect("set")),
        question_group_id: None,
        question_no: Some(number),
        subject: "physics".to_owned(),
        topics: Vec::new(),
        stem_blocks: vec![reinforce_core::ContentBlock::Markdown {
            text: "Which phase?".to_owned(),
        }],
        explanation_blocks: Vec::new(),
        anchor_ids: Vec::new(),
        question_type: reinforce_core::primitives::QuestionType::SingleChoice,
        grading_mode: reinforce_core::primitives::GradingMode::Exact,
        options: vec![reinforce_core::content::QuestionOption {
            key: "A".to_owned(),
            content: "solid".to_owned(),
            explanation: None,
            linked_card_ids: Vec::new(),
        }],
        answer: vec!["A".to_owned()],
        accepted_answers: Vec::new(),
        answer_source_kind: reinforce_core::primitives::AnswerSourceKind::Official,
        answer_basis: None,
        duplicate_question_id: None,
        status: reinforce_core::primitives::CardStatus::Published,
        verification: reinforce_core::primitives::Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: stamp("2026-01-01T00:00:00.000000Z"),
        updated_at: stamp("2026-01-01T00:00:00.000000Z"),
    }
}

fn knowledge_card(
    id: &str,
    status: reinforce_core::primitives::CardStatus,
    verification: reinforce_core::primitives::Verification,
    blocks: Vec<reinforce_core::ContentBlock>,
) -> reinforce_core::content::KnowledgeCard {
    reinforce_core::content::KnowledgeCard {
        id: id.parse().expect("card"),
        subject: "physics".to_owned(),
        topics: Vec::new(),
        title: "fixture card".to_owned(),
        blocks,
        anchor_ids: Vec::new(),
        status,
        verification,
        unsure_count: 0,
        metadata: json!({}),
        created_at: stamp("2026-01-01T00:00:00.000000Z"),
        updated_at: stamp("2026-01-01T00:00:00.000000Z"),
    }
}

fn resource(id: &str, policy: ExportPolicy, title: &str) -> reinforce_core::content::Resource {
    reinforce_core::content::Resource {
        id: id.parse().expect("resource"),
        kind: reinforce_core::primitives::ResourceKind::Image,
        title: title.to_owned(),
        sha256: reinforce_core::ContentHash::from_bytes(id.as_bytes()),
        byte_size: 128,
        page_count: None,
        width: None,
        height: None,
        license: None,
        attribution_url: None,
        attribution_notice: None,
        sensitivity: reinforce_core::primitives::Sensitivity::Restricted,
        export_policy: policy,
        created_at: stamp("2026-01-01T00:00:00.000000Z"),
    }
}

fn anchor(id: &str, resource_id: &str) -> reinforce_core::content::Anchor {
    reinforce_core::content::Anchor {
        id: id.parse().expect("anchor"),
        resource_id: resource_id.parse().expect("resource"),
        anchor_kind: reinforce_core::primitives::AnchorKind::Rect,
        pdf_page: None,
        printed_page: None,
        legend_pdf_page: None,
        rect: Some(reinforce_core::content::NormalizedRect {
            x: 0.1,
            y: 0.1,
            w: 0.2,
            h: 0.2,
        }),
        media_span: None,
    }
}

/// Seeds content that the snapshot has to trim: restricted anchors behind an
/// inferred answer, duplicates whose originals are not visible, a complete set
/// with an archived question, a canvas layout pointing at a draft card, and
/// unchecked rows that must stay unchecked.
fn seed_review_fixture(connection: &rusqlite::Connection) -> reinforce_core::Result<()> {
    use reinforce_core::primitives::{AnswerSourceKind, CardStatus, Verification};

    content_store::store_resource(
        connection,
        &resource(R_REDACT, ExportPolicy::Redact, "restricted-title.pdf"),
    )?;
    content_store::store_resource(
        connection,
        &resource(R_OWNER, ExportPolicy::OwnerOnly, "owner-title.pdf"),
    )?;
    content_store::insert_anchor(connection, &anchor(A_REDACT, R_REDACT))?;
    content_store::insert_anchor(connection, &anchor(A_OWNER, R_OWNER))?;

    // An inferred answer whose only anchors are restricted.
    content_store::insert_exam_set(connection, &exam_set(SET_TRIM, None, false))?;
    let mut inferred = question(Q_INFERRED, SET_TRIM, 1);
    inferred.answer_source_kind = AnswerSourceKind::Inferred;
    inferred.answer_basis = Some("read from the figure".to_owned());
    inferred.anchor_ids = vec![
        A_REDACT.parse().expect("anchor"),
        A_OWNER.parse().expect("anchor"),
    ];
    content_store::insert_question_card(connection, &inferred)?;

    // A published duplicate whose original is a draft.
    let mut draft = question(Q_DRAFT, SET_TRIM, 2);
    draft.status = CardStatus::Draft;
    content_store::insert_question_card(connection, &draft)?;
    let mut duplicate = question(Q_DUPLICATE, SET_TRIM, 3);
    duplicate.answer_source_kind = AnswerSourceKind::DuplicateQuestion;
    duplicate.answer_basis = Some("same as the draft question".to_owned());
    duplicate.duplicate_question_id = Some(Q_DRAFT.parse().expect("question"));
    content_store::insert_question_card(connection, &duplicate)?;

    // A complete set where one question is archived, for every viewer.
    content_store::insert_exam_set(connection, &exam_set(SET_ARCHIVED, Some(2), true))?;
    content_store::insert_question_card(connection, &question(Q_PUBLISHED, SET_ARCHIVED, 1))?;
    let mut archived = question(Q_ARCHIVED, SET_ARCHIVED, 2);
    archived.status = CardStatus::Archived;
    content_store::insert_question_card(connection, &archived)?;

    // A published duplicate whose original is archived.
    let mut archived_original = question(Q_ARCHIVED_ORIGINAL, SET_TRIM, 4);
    archived_original.status = CardStatus::Archived;
    content_store::insert_question_card(connection, &archived_original)?;
    let mut duplicate = question(Q_DUPLICATE_ARCHIVED, SET_TRIM, 5);
    duplicate.answer_source_kind = AnswerSourceKind::DuplicateQuestion;
    duplicate.answer_basis = Some("same as the archived question".to_owned());
    duplicate.duplicate_question_id = Some(Q_ARCHIVED_ORIGINAL.parse().expect("question"));
    content_store::insert_question_card(connection, &duplicate)?;

    // A canvas layout that still points at a draft card.
    content_store::insert_knowledge_card(
        connection,
        &knowledge_card(
            CARD_DRAFT,
            CardStatus::Draft,
            Verification::Verified,
            Vec::new(),
        ),
    )?;
    let mut note =
        content_store::get_note(connection, &NOTE_ONE.parse().expect("note"))?.expect("demo note");
    note.outline
        .push(reinforce_core::content::OutlineNode::CardRef {
            card_id: CARD_DRAFT.parse().expect("card"),
        });
    content_store::upsert_note(connection, &note)?;
    content_store::update_note_view_settings(
        connection,
        &NOTE_ONE.parse().expect("note"),
        &json!({
            "canvas": {
                "version": 1,
                "viewport": {"x": 0.0, "y": 0.0, "zoom": 1.0},
                "nodes": [
                    {"card_id": CARD_DRAFT, "x": 10.0, "y": 20.0, "w": 100.0, "h": 80.0}
                ]
            }
        }),
        stamp("2026-01-02T00:00:00.000000Z"),
    )?;

    // Published content the owner never checked.
    content_store::insert_knowledge_card(
        connection,
        &knowledge_card(
            CARD_UNCHECKED,
            CardStatus::Published,
            Verification::Unchecked,
            Vec::new(),
        ),
    )?;
    content_store::insert_exam_set(connection, &exam_set(SET_UNCHECKED, Some(1), true))?;
    let mut unchecked = question(Q_UNCHECKED, SET_UNCHECKED, 1);
    unchecked.verification = Verification::Unchecked;
    content_store::insert_question_card(connection, &unchecked)?;
    Ok(())
}

async fn snapshot_json(app: &TestApp, cookie: &str) -> Value {
    expect_json(
        call(
            app,
            Method::GET,
            "/api/sync/content",
            None,
            Some(cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await
}

fn import_into_device(dir: &TestDir, name: &str, bundle: &str) {
    let path = dir.0.join(format!("{name}.db"));
    let device = reinforce_ffi::LocalDatabase::open(path.to_string_lossy().into_owned())
        .expect("open device");
    device
        .import_content_snapshot(bundle.to_owned(), None)
        .expect("snapshot must import");
}

#[tokio::test]
async fn trimmed_content_snapshots_still_import_for_owner_and_learner() {
    let (app, state, dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;
    state
        .with_db(|database| {
            seed_review_fixture(database.connection())?;
            Ok(())
        })
        .await
        .expect("seed fixture");

    let owner_snapshot = snapshot_json(&app, &owner).await;
    let owner_bundle = owner_snapshot["bundle_jsonl"]
        .as_str()
        .expect("owner bundle");
    // The owner keeps the draft original and the inferred question, but loses
    // the archived original and the duplicate that points at it.
    assert!(owner_bundle.contains(Q_INFERRED));
    assert!(owner_bundle.contains(Q_DUPLICATE));
    assert!(!owner_bundle.contains(Q_DUPLICATE_ARCHIVED));
    import_into_device(&dir, "owner-device", owner_bundle);

    let member_snapshot = snapshot_json(&app, &member.cookie).await;
    let member_bundle = member_snapshot["bundle_jsonl"]
        .as_str()
        .expect("member bundle");
    assert!(!member_bundle.contains(Q_INFERRED));
    assert!(!member_bundle.contains(Q_DUPLICATE));
    assert!(!member_bundle.contains(Q_DUPLICATE_ARCHIVED));
    assert!(!member_bundle.contains(R_REDACT));
    assert!(!member_bundle.contains(A_REDACT));
    assert!(
        !member_bundle.contains("restricted-title.pdf"),
        "a redacted resource must not be named in the notice"
    );
    assert!(
        !member_bundle.contains(CARD_DRAFT),
        "a draft card id must not leak through the canvas layout"
    );
    assert!(member_bundle.contains("Content redacted for this viewer"));
    assert!(
        owner_bundle.contains(CARD_DRAFT),
        "the owner still sees the draft card and its layout"
    );
    import_into_device(&dir, "learner-device", member_bundle);
}

#[tokio::test]
async fn content_snapshot_preserves_the_server_verification_state() {
    let (app, state, dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;
    state
        .with_db(|database| {
            seed_review_fixture(database.connection())?;
            Ok(())
        })
        .await
        .expect("seed fixture");

    let member_snapshot = snapshot_json(&app, &member.cookie).await;
    let bundle = member_snapshot["bundle_jsonl"]
        .as_str()
        .expect("member bundle")
        .to_owned();
    let path = dir.0.join("verification-device.db");
    let device = reinforce_ffi::LocalDatabase::open(path.to_string_lossy().into_owned())
        .expect("open device");
    device
        .import_content_snapshot(bundle, None)
        .expect("import snapshot");

    assert!(
        matches!(
            device
                .knowledge_card(CARD_UNCHECKED.to_owned())
                .expect("card")
                .verification,
            reinforce_ffi::types::Verification::Unchecked
        ),
        "server content must keep its verification state"
    );
    assert!(matches!(
        device
            .question_card(Q_UNCHECKED.to_owned())
            .expect("question")
            .verification,
        reinforce_ffi::types::Verification::Unchecked
    ));
}

#[tokio::test]
async fn negative_durations_are_rejected_before_the_database_check() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;

    let mut attempt = attempt_json(
        &member.user_id,
        CARD_ONE,
        "2026-01-02T10:00:00.000000Z",
        true,
    );
    attempt["duration_ms"] = json!(-5);
    let response = call(
        &app,
        Method::POST,
        "/api/sync/attempts",
        Some(json!({"attempts": [attempt]})),
        Some(&member.cookie),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("duration_ms")
    );

    // Nothing was written, so the device can retry with a valid value.
    let stored = expect_json(
        call(
            &app,
            Method::GET,
            "/api/study/attempts",
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(stored.as_array().expect("attempts").is_empty());
}

#[tokio::test]
async fn pull_pages_never_skip_rows_that_share_a_cursor() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;
    let user_id: reinforce_core::UserId = member.user_id.parse().expect("user");
    state
        .with_db(move |database| {
            let connection = database.connection();
            for _ in 0..3 {
                let attempt = AttemptLog {
                    id: AttemptId::generate(),
                    user_id: user_id.clone(),
                    card_id: CARD_ONE.parse().expect("card"),
                    direction: AttemptDirection::Default,
                    session_mode: SessionMode::DailyDue,
                    submitted_payload: json!({"keys": ["A"]}),
                    is_correct: true,
                    duration_ms: 1_000,
                    confidence: Confidence::Certain,
                    error_tags: Vec::new(),
                    created_at: stamp("2026-01-02T10:00:00.000000Z"),
                    sync_cursor: 5,
                    affects_scheduling: true,
                };
                study_store::append_attempt(connection, &attempt)?;
            }
            Ok(())
        })
        .await
        .expect("seed shared cursor");

    let mut cursor = 0_i64;
    let mut pulled: Vec<String> = Vec::new();
    loop {
        let page = expect_json(
            call(
                &app,
                Method::GET,
                &format!("/api/sync/attempts?cursor={cursor}&limit=1"),
                None,
                Some(&member.cookie),
                None,
            )
            .await,
            StatusCode::OK,
        )
        .await;
        for attempt in page["attempts"].as_array().expect("attempts") {
            pulled.push(attempt["id"].as_str().expect("id").to_owned());
        }
        if !page["has_more"].as_bool().expect("has_more") {
            break;
        }
        cursor = page["next_cursor"].as_i64().expect("next cursor");
    }

    assert_eq!(pulled.len(), 3, "every row of the shared cursor is pulled");
    let unique: std::collections::BTreeSet<&String> = pulled.iter().collect();
    assert_eq!(unique.len(), 3, "no row is pulled twice");
}

#[tokio::test]
async fn finishing_a_session_allocates_a_per_user_cursor() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;

    let member_written = finish_demo_exam(&app, &member.cookie).await;
    assert_eq!(member_written, 3);
    let owner_written = finish_demo_exam(&app, &owner).await;
    assert_eq!(owner_written, 3);

    // The owner's first attempt starts at one even though the member already
    // wrote rows: the server cursor is per account, not global.
    let owner_page = expect_json(
        call(
            &app,
            Method::GET,
            "/api/sync/attempts?cursor=0",
            None,
            Some(&owner),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let cursors: Vec<i64> = owner_page["attempts"]
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| attempt["sync_cursor"].as_i64().expect("cursor"))
        .collect();
    assert_eq!(cursors, vec![1, 2, 3]);
}

async fn finish_demo_exam(app: &TestApp, cookie: &str) -> u64 {
    let created = expect_json(
        call(
            app,
            Method::POST,
            "/api/study/sessions",
            Some(json!({
                "mode": "full_exam",
                "exam_set_id": DEMO_EXAM_SET,
                "options": {"question_limit": 4},
            })),
            Some(cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let session_id = created["session_id"].as_str().expect("session").to_owned();
    for _ in 0..3 {
        expect_json(
            call(
                app,
                Method::POST,
                &format!("/api/study/sessions/{session_id}/answers"),
                Some(json!({
                    "submission": {"kind": "keys", "values": ["A"]},
                    "confidence": "certain",
                })),
                Some(cookie),
                None,
            )
            .await,
            StatusCode::OK,
        )
        .await;
    }
    let finished = expect_json(
        call(
            app,
            Method::POST,
            &format!("/api/study/sessions/{session_id}/finish"),
            None,
            Some(cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    finished["attempts_written"]
        .as_u64()
        .expect("attempts written")
}

#[tokio::test]
async fn voided_uploads_do_not_write_an_empty_schedule() {
    let (app, state, _dir) = test_state(AuthMode::Invite).await;
    let owner = owner_cookie(&app, &state).await;
    let member = register_member(&app, &owner, "learner", MEMBER_A_PASSWORD).await;

    // A client that marks a voided attempt as affecting scheduling must not
    // make the server store the empty replay (reps = 0, updated_at = 1970).
    let mut attempt = attempt_json(
        &member.user_id,
        CARD_ONE,
        "2026-01-02T10:00:00.000000Z",
        false,
    );
    attempt["affects_scheduling"] = json!(true);
    attempt["submitted_payload"] = json!({"voided": true, "grading_mode": "voided"});
    let pushed = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(json!({"attempts": [attempt]})),
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(pushed["accepted"], 1);

    let scheduling = expect_json(
        call(
            &app,
            Method::GET,
            &format!("/api/study/scheduling?card_id={CARD_ONE}"),
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert!(
        scheduling.is_null(),
        "a voided attempt must not create a schedule row"
    );

    // The attempt itself stays in the append-only log for auditability.
    let stored = expect_json(
        call(
            &app,
            Method::GET,
            &format!("/api/study/attempts?card_id={CARD_ONE}"),
            None,
            Some(&member.cookie),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(stored.as_array().expect("attempts").len(), 1);
}
