use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::AttemptLog;
use reinforce_core::auth::set_setting;
use reinforce_core::primitives::AttemptId;
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
