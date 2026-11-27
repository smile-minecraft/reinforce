//! End-to-end sync convergence between `reinforce serve` and two devices.
//!
//! The server runs in-process through the same router `reinforce serve` mounts,
//! and the devices are real `reinforce-ffi` databases that answered questions
//! offline. Both devices push in a different order, then pull, and the test
//! checks that the attempt set and the derived FSRS state are identical on all
//! three sides.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::auth::set_setting;
use reinforce_ffi::resources::UserCardMeta;
use reinforce_ffi::sync::{PendingSync, SyncMetaStamp};
use reinforce_ffi::types::{
    AttemptDirection, AttemptLogRecord, CardSchedulingRecord, Confidence, SchedulingState,
    SessionMode,
};
use reinforce_ffi::{LocalDatabase, new_attempt_id};
use reinforce_server::auth::hash_token;
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::demo;
use reinforce_server::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;

static TEST_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const OWNER_SETUP_TOKEN: &str = "sync-e2e-owner-token";
const OWNER_PASSWORD: &str = "sync-e2e-owner-password";
const MEMBER_PASSWORD: &str = "sync-e2e-member-password";
const CARD_ONE: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const CARD_TWO: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";
const PINNED_CARD: &str = "01a21714-c98d-7b1d-92e4-b2d361f54d7f";
const QUESTION_ONE: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const REDACTED_RESOURCE: &str = "01a21714-c98c-78d5-8550-3d0928b9c9d1";
const ALLOWED_RESOURCE: &str = "01a21714-c98c-7bfe-8cf8-a60b84573f56";
const ALLOWED_SHA256: &str = "22df5f119108ae55882dc11a03d09ba426cf4771ef92eaa418457f8db40cbe4a";

type TestApp = Router<()>;

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "reinforce-sync-e2e-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn test_state(directory: &TestDir) -> (TestApp, AppState) {
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.join("server.db"),
        blobs: directory.join("blobs"),
        fonts: directory.join("fonts"),
        auth: AuthMode::Invite,
        demo: true,
    };
    let state = AppState::open(&config).await.expect("open test state");
    demo::seed(&state).await.expect("seed demo data");
    (reinforce_server::app(state.clone()), state)
}

async fn call(
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

/// Logs a learner in and returns the session cookie plus the server user id.
async fn learner_session(app: &TestApp, state: &AppState) -> (String, String) {
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
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let owner_cookie = session_cookie(&response);

    let invite = expect_json(
        call(
            app,
            Method::POST,
            "/api/auth/invites",
            Some(json!({"expires_in_secs": 3600})),
            Some(&owner_cookie),
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
                "username": "learner",
                "password": MEMBER_PASSWORD,
            })),
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
        Some(json!({"username": "learner", "password": MEMBER_PASSWORD})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    (
        session_cookie(&response),
        registered["user_id"].as_str().expect("user id").to_owned(),
    )
}

fn demo_bundle() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo/bundle.jsonl")
}

fn demo_asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/demo/assets")
        .join(name)
}

struct Device {
    database: Arc<LocalDatabase>,
    local_user: String,
    database_path: PathBuf,
}

/// A device database with the demo content already imported.
fn device(directory: &TestDir, label: &str) -> Device {
    let root = directory.join(label);
    std::fs::create_dir_all(&root).expect("device directory");
    let database_path = root.join("reinforce.db");
    let database = LocalDatabase::open(database_path.to_string_lossy().into_owned())
        .expect("open device database");
    database
        .import_bundle(demo_bundle().to_string_lossy().into_owned())
        .expect("import demo content");
    let local_user = database.create_user(label.to_owned()).expect("local user");
    Device {
        database,
        local_user,
        database_path,
    }
}

fn offline_attempt(
    database: &LocalDatabase,
    user_id: &str,
    card_id: &str,
    created_at: &str,
    is_correct: bool,
) -> String {
    let id = new_attempt_id();
    let cursor = database.next_attempt_cursor().expect("cursor");
    database
        .append_attempt(AttemptLogRecord {
            id: id.clone(),
            user_id: user_id.to_owned(),
            card_id: card_id.to_owned(),
            direction: AttemptDirection::Default,
            session_mode: SessionMode::DailyDue,
            submitted_payload_json: "{\"keys\":[\"A\"]}".to_owned(),
            is_correct,
            duration_ms: 1_500,
            confidence: Confidence::Certain,
            error_tags: Vec::new(),
            created_at: created_at.to_owned(),
            sync_cursor: cursor,
            affects_scheduling: true,
        })
        .expect("append attempt");
    id
}

fn direction_wire(value: AttemptDirection) -> &'static str {
    match value {
        AttemptDirection::Default => "default",
        AttemptDirection::Forward => "forward",
        AttemptDirection::Reverse => "reverse",
    }
}

fn session_wire(value: SessionMode) -> &'static str {
    match value {
        SessionMode::DailyDue => "daily_due",
        SessionMode::FullExam => "full_exam",
        SessionMode::TopicDrill => "topic_drill",
        SessionMode::PracticalExam => "practical_exam",
        SessionMode::WeaknessDrill => "weakness_drill",
    }
}

fn confidence_wire(value: Confidence) -> &'static str {
    match value {
        Confidence::Certain => "certain",
        Confidence::Split => "split",
        Confidence::Guessed => "guessed",
    }
}

/// Renders an FFI record the way a native client serialises it for the API.
fn attempt_body(record: &AttemptLogRecord) -> Value {
    json!({
        "id": record.id,
        "user_id": record.user_id,
        "card_id": record.card_id,
        "direction": direction_wire(record.direction),
        "session_mode": session_wire(record.session_mode),
        "submitted_payload": serde_json::from_str::<Value>(&record.submitted_payload_json)
            .expect("payload"),
        "is_correct": record.is_correct,
        "duration_ms": record.duration_ms,
        "confidence": confidence_wire(record.confidence),
        "error_tags": record.error_tags,
        "created_at": record.created_at,
        "sync_cursor": record.sync_cursor,
        "affects_scheduling": record.affects_scheduling,
    })
}

fn meta_body(record: &UserCardMeta) -> Value {
    json!({
        "user_id": record.user_id,
        "card_id": record.card_id,
        "starred": record.starred,
        "personal_tags": record.personal_tags,
        "personal_note": record.personal_note,
        "updated_at": record.updated_at,
    })
}

fn attempt_from_wire(value: &Value) -> AttemptLogRecord {
    let text = |key: &str| {
        value[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} must be a string"))
            .to_owned()
    };
    let direction = match text("direction").as_str() {
        "default" => AttemptDirection::Default,
        "forward" => AttemptDirection::Forward,
        "reverse" => AttemptDirection::Reverse,
        other => panic!("unknown direction {other}"),
    };
    let session_mode = match text("session_mode").as_str() {
        "daily_due" => SessionMode::DailyDue,
        "full_exam" => SessionMode::FullExam,
        "topic_drill" => SessionMode::TopicDrill,
        "practical_exam" => SessionMode::PracticalExam,
        "weakness_drill" => SessionMode::WeaknessDrill,
        other => panic!("unknown session mode {other}"),
    };
    let confidence = match text("confidence").as_str() {
        "certain" => Confidence::Certain,
        "split" => Confidence::Split,
        "guessed" => Confidence::Guessed,
        other => panic!("unknown confidence {other}"),
    };
    AttemptLogRecord {
        id: text("id"),
        user_id: text("user_id"),
        card_id: text("card_id"),
        direction,
        session_mode,
        submitted_payload_json: value["submitted_payload"].to_string(),
        is_correct: value["is_correct"].as_bool().expect("is_correct"),
        duration_ms: value["duration_ms"].as_i64().expect("duration_ms"),
        confidence,
        error_tags: value["error_tags"]
            .as_array()
            .expect("error tags")
            .iter()
            .map(|tag| tag.as_str().expect("tag").to_owned())
            .collect(),
        created_at: text("created_at"),
        sync_cursor: value["sync_cursor"].as_i64().expect("sync_cursor"),
        affects_scheduling: value["affects_scheduling"]
            .as_bool()
            .expect("affects_scheduling"),
    }
}

fn meta_from_wire(value: &Value) -> UserCardMeta {
    UserCardMeta {
        user_id: value["user_id"].as_str().expect("user id").to_owned(),
        card_id: value["card_id"].as_str().expect("card id").to_owned(),
        starred: value["starred"].as_bool().expect("starred"),
        personal_tags: value["personal_tags"]
            .as_array()
            .expect("tags")
            .iter()
            .map(|tag| tag.as_str().expect("tag").to_owned())
            .collect(),
        personal_note: value["personal_note"].as_str().map(str::to_owned),
        updated_at: value["updated_at"].as_str().expect("updated_at").to_owned(),
    }
}

/// Pushes everything pending, acknowledges only after a 2xx, and returns the
/// upload report so callers can assert on duplicates.
async fn push(app: &TestApp, cookie: &str, device: &Device) -> Value {
    let pending = device
        .database
        .pending_sync(device.local_user.clone(), 200)
        .expect("pending");
    let attempts: Vec<Value> = pending.attempts.iter().map(attempt_body).collect();
    let metas: Vec<Value> = pending.user_card_meta.iter().map(meta_body).collect();
    let pushed = expect_json(
        call(
            app,
            Method::POST,
            "/api/sync/attempts",
            Some(json!({"attempts": attempts})),
            Some(cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    expect_json(
        call(
            app,
            Method::PUT,
            "/api/sync/user-card-meta",
            Some(json!({"user_card_meta": metas})),
            Some(cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    acknowledge(&device.database, &device.local_user, &pending);
    pushed
}

fn acknowledge(database: &Arc<LocalDatabase>, local_user: &str, pending: &PendingSync) {
    let ids = pending
        .attempts
        .iter()
        .map(|attempt| attempt.id.clone())
        .collect();
    let stamps: Vec<SyncMetaStamp> = pending
        .user_card_meta
        .iter()
        .map(|meta| SyncMetaStamp {
            card_id: meta.card_id.clone(),
            updated_at: meta.updated_at.clone(),
        })
        .collect();
    database
        .confirm_upload(local_user.to_owned(), ids, stamps)
        .expect("confirm upload");
}

/// Pulls every page with a page size of one, so cursor handling is exercised.
async fn pull(app: &TestApp, cookie: &str, device: &Device) {
    loop {
        let cursor = device
            .database
            .sync_state(device.local_user.clone())
            .expect("sync state")
            .map_or(0, |state| state.pull_cursor);
        let page = expect_json(
            call(
                app,
                Method::GET,
                &format!("/api/sync/attempts?cursor={cursor}&limit=1"),
                None,
                Some(cookie),
            )
            .await,
            StatusCode::OK,
        )
        .await;
        let attempts: Vec<AttemptLogRecord> = page["attempts"]
            .as_array()
            .expect("attempts")
            .iter()
            .map(attempt_from_wire)
            .collect();
        device
            .database
            .apply_remote_batch(
                device.local_user.clone(),
                attempts,
                Vec::new(),
                page["next_cursor"].as_i64().expect("next cursor"),
            )
            .expect("apply attempt page");
        if page["has_more"] != Value::Bool(true) {
            break;
        }
    }

    let page = expect_json(
        call(
            app,
            Method::GET,
            "/api/sync/user-card-meta",
            None,
            Some(cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let metas: Vec<UserCardMeta> = page["user_card_meta"]
        .as_array()
        .expect("meta")
        .iter()
        .map(meta_from_wire)
        .collect();
    let cursor = device
        .database
        .sync_state(device.local_user.clone())
        .expect("sync state")
        .map_or(0, |state| state.pull_cursor);
    device
        .database
        .apply_remote_batch(device.local_user.clone(), Vec::new(), metas, cursor)
        .expect("apply meta page");
}

/// Reads the append-only log straight from the device file so the assertion
/// does not depend on a sync helper.
fn stored_attempt_ids(database_path: &Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(database_path).expect("open device database");
    let mut statement = connection
        .prepare("SELECT id FROM attempt_logs ORDER BY id")
        .expect("prepare");
    let mut ids: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    ids.sort();
    ids
}

fn state_name(value: SchedulingState) -> &'static str {
    match value {
        SchedulingState::New => "new",
        SchedulingState::Learning => "learning",
        SchedulingState::Review => "review",
        SchedulingState::Relearning => "relearning",
    }
}

fn scheduling(database: &Arc<LocalDatabase>, local_user: &str, card: &str) -> CardSchedulingRecord {
    database
        .scheduling(local_user.to_owned(), card.to_owned())
        .expect("scheduling")
        .expect("scheduled card")
}

fn same_numbers(left: &CardSchedulingRecord, right: &CardSchedulingRecord) {
    assert_eq!(left.stability, right.stability);
    assert_eq!(left.difficulty, right.difficulty);
    assert_eq!(left.reps, right.reps);
    assert_eq!(left.lapses, right.lapses);
    assert_eq!(state_name(left.state), state_name(right.state));
    assert_eq!(left.last_review_at, right.last_review_at);
    assert_eq!(left.due_at, right.due_at);
    assert_eq!(left.desired_retention, right.desired_retention);
}

#[tokio::test]
async fn two_devices_converge_after_syncing_in_different_orders() {
    let directory = TestDir::new("converge");
    let (app, state) = test_state(&directory).await;
    let (cookie, remote_user) = learner_session(&app, &state).await;

    let device_one = device(&directory, "device-one");
    let device_two = device(&directory, "device-two");
    device_one
        .database
        .bind_remote_user(device_one.local_user.clone(), remote_user.clone())
        .expect("bind device one");
    device_two
        .database
        .bind_remote_user(device_two.local_user.clone(), remote_user.clone())
        .expect("bind device two");

    // Both devices answer offline; the timestamps interleave so the replay
    // order must come from the attempt log, not from upload order.
    let mut expected = vec![
        offline_attempt(
            &device_one.database,
            &device_one.local_user,
            CARD_ONE,
            "2026-01-02T10:00:00.000000Z",
            true,
        ),
        offline_attempt(
            &device_two.database,
            &device_two.local_user,
            CARD_ONE,
            "2026-01-02T10:05:00.000000Z",
            false,
        ),
        offline_attempt(
            &device_one.database,
            &device_one.local_user,
            CARD_ONE,
            "2026-01-02T10:10:00.000000Z",
            true,
        ),
        offline_attempt(
            &device_two.database,
            &device_two.local_user,
            CARD_TWO,
            "2026-01-02T10:15:00.000000Z",
            true,
        ),
    ];

    // Device two uploads first, device one second, then each pulls twice.
    let pushed = push(&app, &cookie, &device_two).await;
    assert_eq!(pushed["accepted"], 2);
    let pushed = push(&app, &cookie, &device_one).await;
    assert_eq!(pushed["accepted"], 2);

    pull(&app, &cookie, &device_two).await;
    pull(&app, &cookie, &device_one).await;
    pull(&app, &cookie, &device_two).await;
    pull(&app, &cookie, &device_one).await;

    expected.sort();
    assert_eq!(
        stored_attempt_ids(&device_one.database_path),
        expected,
        "device one holds every attempt once"
    );
    assert_eq!(
        stored_attempt_ids(&device_two.database_path),
        expected,
        "device two holds every attempt once"
    );

    // The server keeps the same set, and POSTing its own rows back is a no-op.
    let stored = expect_json(
        call(
            &app,
            Method::GET,
            "/api/study/attempts",
            None,
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let mut server_ids: Vec<String> = stored
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| attempt["id"].as_str().expect("id").to_owned())
        .collect();
    server_ids.sort();
    assert_eq!(server_ids, expected, "the server holds the same set");

    let duplicate = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(json!({"attempts": stored})),
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(duplicate["accepted"], 0);
    assert_eq!(duplicate["duplicates"], 4);

    // FSRS state is identical everywhere because it is replayed, not synced.
    let server_scheduling = expect_json(
        call(
            &app,
            Method::GET,
            &format!("/api/study/scheduling?card_id={CARD_ONE}"),
            None,
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let left = scheduling(&device_one.database, &device_one.local_user, CARD_ONE);
    let right = scheduling(&device_two.database, &device_two.local_user, CARD_ONE);
    same_numbers(&left, &right);
    assert_eq!(
        server_scheduling["stability"].as_f64().expect("stability"),
        left.stability
    );
    assert_eq!(
        server_scheduling["difficulty"]
            .as_f64()
            .expect("difficulty"),
        left.difficulty
    );
    assert_eq!(
        server_scheduling["reps"].as_u64().expect("reps"),
        u64::from(left.reps)
    );
    assert_eq!(
        server_scheduling["lapses"].as_u64().expect("lapses"),
        u64::from(left.lapses)
    );
    assert_eq!(
        server_scheduling["last_review_at"].as_str(),
        left.last_review_at.as_deref()
    );
    assert_eq!(left.reps, 3);
    assert_eq!(left.lapses, 1);
}

#[tokio::test]
async fn interrupted_upload_retries_without_duplicates() {
    let directory = TestDir::new("retry");
    let (app, state) = test_state(&directory).await;
    let (cookie, remote_user) = learner_session(&app, &state).await;
    let device = device(&directory, "device");
    device
        .database
        .bind_remote_user(device.local_user.clone(), remote_user.clone())
        .expect("bind");
    let attempt = offline_attempt(
        &device.database,
        &device.local_user,
        CARD_ONE,
        "2026-01-02T10:00:00.000000Z",
        true,
    );

    let pending = device
        .database
        .pending_sync(device.local_user.clone(), 200)
        .expect("pending");
    assert_eq!(pending.attempts.len(), 1);
    assert_eq!(pending.attempts[0].user_id, remote_user);

    // The server stores the batch, but the response never reaches the device,
    // so it never confirms and still reports the attempt as pending.
    let body = json!({"attempts": pending.attempts.iter().map(attempt_body).collect::<Vec<_>>()});
    let first = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(body.clone()),
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(first["accepted"], 1);
    assert_eq!(
        device
            .database
            .pending_sync(device.local_user.clone(), 200)
            .expect("pending")
            .attempts
            .len(),
        1
    );

    let retry = expect_json(
        call(
            &app,
            Method::POST,
            "/api/sync/attempts",
            Some(body),
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(retry["accepted"], 0);
    assert_eq!(retry["duplicates"], 1);
    assert_eq!(device.database.next_attempt_cursor().expect("cursor"), 2);

    acknowledge(&device.database, &device.local_user, &pending);
    assert!(
        device
            .database
            .pending_sync(device.local_user.clone(), 200)
            .expect("pending")
            .attempts
            .is_empty()
    );
    let stored = expect_json(
        call(
            &app,
            Method::GET,
            "/api/study/attempts",
            None,
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let attempts = stored.as_array().expect("attempts");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["id"].as_str(), Some(attempt.as_str()));
}

#[tokio::test]
async fn content_snapshot_imports_published_content_once() {
    let directory = TestDir::new("content");
    let (app, state) = test_state(&directory).await;
    let (cookie, _remote_user) = learner_session(&app, &state).await;

    let snapshot = expect_json(
        call(&app, Method::GET, "/api/sync/content", None, Some(&cookie)).await,
        StatusCode::OK,
    )
    .await;
    let bundle = snapshot["bundle_jsonl"]
        .as_str()
        .expect("bundle")
        .to_owned();
    let hash = snapshot["snapshot_sha256"]
        .as_str()
        .expect("hash")
        .to_owned();
    assert!(!bundle.contains(REDACTED_RESOURCE));
    let assets = snapshot["assets"].as_array().expect("assets");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["sha256"], ALLOWED_SHA256);

    let assets_dir = directory.join("downloaded");
    std::fs::create_dir_all(&assets_dir).expect("assets directory");
    std::fs::copy(
        demo_asset("phase-diagram.svg"),
        assets_dir.join("phase-diagram.svg"),
    )
    .expect("copy asset");

    let device_root = directory.join("fresh-device");
    std::fs::create_dir_all(&device_root).expect("device directory");
    let database = LocalDatabase::open(
        device_root
            .join("reinforce.db")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("open device");
    let summary = database
        .import_content_snapshot(
            bundle.clone(),
            Some(assets_dir.to_string_lossy().into_owned()),
        )
        .expect("import snapshot");
    assert_eq!(summary.blobs_written, 1);
    assert!(summary.applied >= 10, "unexpected summary {summary:?}");

    // Restricted media never reaches the device, published content does.
    assert!(database.question_card(QUESTION_ONE.to_owned()).is_ok());
    assert!(database.resource(REDACTED_RESOURCE.to_owned()).is_err());
    let blob = database
        .resource_blob_path(ALLOWED_RESOURCE.to_owned())
        .expect("allowed blob");
    assert!(Path::new(&blob).is_file());
    let pinned = database
        .knowledge_card(PINNED_CARD.to_owned())
        .expect("pinned card");
    assert!(
        pinned
            .blocks_json
            .contains("Content redacted for this viewer")
    );

    let again = database
        .import_content_snapshot(bundle, Some(assets_dir.to_string_lossy().into_owned()))
        .expect("re-import");
    assert_eq!(again.blobs_written, 0, "blobs are content addressed");

    let known = expect_json(
        call(
            &app,
            Method::GET,
            &format!("/api/sync/content?known={hash}"),
            None,
            Some(&cookie),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(known["unchanged"], true);
    assert!(known.get("bundle_jsonl").is_none());
}
