use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::auth::set_setting;
use reinforce_core::content::{NormalizedRect, OcclusionMask, OutlineNode, Pin, QuestionOption};
use reinforce_core::content_store;
use reinforce_core::primitives::{
    AnswerSourceKind, AttemptDirection, AttemptId, CardStatus, Confidence, EdgeType, ExportPolicy,
    GradingMode, QuestionType, ResourceKind, Sensitivity, SessionMode, Verification,
};
use reinforce_core::{
    CardId, ContentBlock, ContentHash, Database, Edge, EdgeEndpoint, EdgeId, KnowledgeCard,
    KnowledgeCardId, Note, NoteId, QuestionCard, QuestionCardId, Resource, ResourceId, Timestamp,
};
use reinforce_export::import_markdown;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use zip::ZipArchive;

use reinforce_server::auth::hash_token;
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::state::AppState;

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
const SETUP_TOKEN: &str = "export-owner-token";
const SUBJECT: &str = "export-policy";

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "reinforce-export-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fonts(dir: &Path) -> PathBuf {
    let fonts = dir.join("fonts");
    std::fs::create_dir_all(&fonts).expect("fonts dir");
    for (index, font) in typst_assets::fonts().enumerate() {
        std::fs::write(fonts.join(format!("font-{index}.ttf")), font).expect("font file");
    }
    fonts
}

async fn state(dir: &Path, mode: AuthMode, fonts_dir: &Path) -> AppState {
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: dir.join("test.db"),
        blobs: dir.join("blobs"),
        fonts: fonts_dir.to_path_buf(),
        auth: mode,
        demo: false,
    };
    AppState::open(&config).await.expect("open state")
}

type TestApp = Router<()>;

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
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&value).expect("encode json"))
        }
        None => Body::empty(),
    };
    let request = builder.body(body).expect("request");
    app.clone().oneshot(request).await.expect("response")
}

async fn response_bytes(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body")
        .to_vec()
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = response_bytes(response).await;
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("cookie")
        .to_str()
        .expect("cookie text")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned()
}

async fn setup_owner(app: &TestApp, state: &AppState) -> String {
    state
        .with_db(move |database| {
            set_setting(
                database.connection(),
                "owner_setup_token_hash",
                &hash_token(SETUP_TOKEN),
            )?;
            Ok(())
        })
        .await
        .expect("setup token");
    let response = call(
        app,
        Method::POST,
        "/api/auth/owner/setup",
        Some(json!({
            "token": SETUP_TOKEN,
            "username": "owner",
            "password": "owner-password",
        })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = call(
        app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "owner", "password": "owner-password"})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    cookie(&response)
}

async fn register_learner(app: &TestApp, owner_cookie: &str) -> String {
    let response = call(
        app,
        Method::POST,
        "/api/auth/invites",
        Some(json!({"expires_in_secs": 3600})),
        Some(owner_cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let code = response_json(response).await["code"]
        .as_str()
        .expect("invite code")
        .to_owned();
    let response = call(
        app,
        Method::POST,
        "/api/auth/register",
        Some(json!({
            "invite_code": code,
            "username": "learner",
            "password": "learner-password",
        })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = call(
        app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "learner", "password": "learner-password"})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    cookie(&response)
}

#[derive(Clone)]
struct ContentFixture {
    published_card: KnowledgeCardId,
    draft_card: KnowledgeCardId,
    question: QuestionCardId,
    allow_hash: String,
    redact_hash: String,
    owner_hash: String,
}

fn resource(title: &str, bytes: &[u8], policy: ExportPolicy) -> (Resource, String) {
    let hash = ContentHash::from_bytes(bytes);
    let hash_text = hash.as_str().to_owned();
    (
        Resource {
            id: ResourceId::generate(),
            kind: ResourceKind::Image,
            title: title.to_owned(),
            sha256: hash,
            byte_size: bytes.len() as i64,
            page_count: None,
            width: Some(100),
            height: Some(80),
            license: None,
            attribution_url: None,
            attribution_notice: None,
            sensitivity: Sensitivity::Restricted,
            export_policy: policy,
            created_at: Timestamp::unix_epoch(),
        },
        hash_text,
    )
}

fn knowledge_card(
    id: KnowledgeCardId,
    title: &str,
    status: CardStatus,
    blocks: Vec<ContentBlock>,
) -> KnowledgeCard {
    let now = Timestamp::unix_epoch();
    KnowledgeCard {
        id,
        subject: SUBJECT.to_owned(),
        topics: vec![],
        title: title.to_owned(),
        blocks,
        anchor_ids: vec![],
        status,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn question_card(id: QuestionCardId) -> QuestionCard {
    let now = Timestamp::unix_epoch();
    QuestionCard {
        id,
        exam_set_id: None,
        question_group_id: None,
        question_no: Some(1),
        subject: SUBJECT.to_owned(),
        topics: vec![],
        stem_blocks: vec![ContentBlock::Markdown {
            text: "QUESTION_VISIBLE".to_owned(),
        }],
        explanation_blocks: Vec::new(),
        anchor_ids: Vec::new(),
        question_type: QuestionType::SingleChoice,
        grading_mode: GradingMode::Exact,
        options: vec![QuestionOption {
            key: "A".to_owned(),
            content: "answer".to_owned(),
            explanation: None,
            linked_card_ids: Vec::new(),
        }],
        answer: vec!["A".to_owned()],
        accepted_answers: Vec::new(),
        answer_source_kind: AnswerSourceKind::Official,
        answer_basis: None,
        duplicate_question_id: None,
        status: CardStatus::Published,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

async fn seed_content(state: &AppState) -> ContentFixture {
    let allow_bytes =
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><!--allow--></svg>"
            .to_vec();
    let redact_bytes =
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><!--redact--></svg>"
            .to_vec();
    let owner_bytes =
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><!--owner--></svg>"
            .to_vec();
    let (allow, allow_hash) = resource("allow", &allow_bytes, ExportPolicy::Allow);
    let (redact, redact_hash) = resource("redact", &redact_bytes, ExportPolicy::Redact);
    let (owner_only, owner_hash) = resource("owner", &owner_bytes, ExportPolicy::OwnerOnly);
    for bytes in [&allow_bytes, &redact_bytes, &owner_bytes] {
        let hash = ContentHash::from_bytes(bytes);
        let path = state.blob_path(hash.as_str());
        std::fs::create_dir_all(path.parent().expect("blob parent")).expect("blob dir");
        std::fs::write(path, bytes).expect("blob file");
    }

    let published_card = KnowledgeCardId::generate();
    let draft_card = KnowledgeCardId::generate();
    let question = QuestionCardId::generate();
    let note_id = NoteId::generate();
    let seed_published = published_card.clone();
    let seed_draft = draft_card.clone();
    let seed_question = question.clone();
    state
        .with_db(move |database| {
            let connection = database.connection();
            for item in [&allow, &redact, &owner_only] {
                content_store::store_resource(connection, item)?;
            }
            let pin = Pin {
                no: 1,
                x: 0.2,
                y: 0.3,
                label_x: 0.6,
                label_y: 0.7,
                term_primary: "ALLOW_TERM".to_owned(),
                term_secondary: None,
                detail: None,
                hint: None,
                linked_card_id: None,
            };
            let secret_pin = Pin {
                no: 2,
                term_primary: "OWNER_SECRET".to_owned(),
                ..pin.clone()
            };
            let mask = OcclusionMask {
                group_id: "mask".to_owned(),
                rect: NormalizedRect {
                    x: 0.1,
                    y: 0.1,
                    w: 0.2,
                    h: 0.2,
                },
            };
            let card = knowledge_card(
                seed_published.clone(),
                "published card",
                CardStatus::Published,
                vec![
                    ContentBlock::Markdown {
                        text: "PUBLISHED_VISIBLE".to_owned(),
                    },
                    ContentBlock::Math {
                        latex: "\\frac{1}{2}".to_owned(),
                        display: true,
                    },
                    ContentBlock::InteractivePins {
                        resource_id: allow.id.clone(),
                        overlay_resource_id: None,
                        pins: vec![pin],
                    },
                    ContentBlock::ImageOcclusion {
                        resource_id: redact.id.clone(),
                        masks: vec![mask],
                    },
                    ContentBlock::InteractivePins {
                        resource_id: owner_only.id.clone(),
                        overlay_resource_id: None,
                        pins: vec![secret_pin],
                    },
                ],
            );
            let draft = knowledge_card(
                seed_draft,
                "draft card",
                CardStatus::Draft,
                vec![ContentBlock::Markdown {
                    text: "DRAFT_SECRET".to_owned(),
                }],
            );
            let note = Note {
                id: note_id,
                subject: SUBJECT.to_owned(),
                title: "export note".to_owned(),
                outline: vec![OutlineNode::Section {
                    heading: "Section".to_owned(),
                    intro_markdown: None,
                    children: vec![OutlineNode::CardRef {
                        card_id: seed_published.clone(),
                    }],
                }],
                view_settings: json!({}),
                status: CardStatus::Published,
                created_at: Timestamp::unix_epoch(),
                updated_at: Timestamp::unix_epoch(),
            };
            let question = question_card(seed_question);
            content_store::insert_knowledge_card(connection, &card)?;
            content_store::insert_knowledge_card(connection, &draft)?;
            content_store::insert_note(connection, &note)?;
            content_store::insert_question_card(connection, &question)?;
            Ok(())
        })
        .await
        .expect("seed content");
    ContentFixture {
        published_card,
        draft_card,
        question,
        allow_hash,
        redact_hash,
        owner_hash,
    }
}

fn unzip(bytes: &[u8]) -> TempDir {
    let dir = TempDir::new().expect("unzip dir");
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("zip archive");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        let path = dir.path().join(entry.name());
        if entry.is_dir() {
            std::fs::create_dir_all(&path).expect("zip directory");
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("zip parent");
        }
        let mut file = std::fs::File::create(&path).expect("zip file");
        std::io::copy(&mut entry, &mut file).expect("write zip entry");
    }
    dir
}

fn read_entity(root: &Path, kind: &str, id: &str) -> String {
    let path = root.join(format!("{kind}s")).join(format!("{id}.md"));
    std::fs::read_to_string(path).unwrap_or_default()
}

type IdTitles = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String)>,
    BTreeSet<(String, String)>,
);

fn id_titles(input: &reinforce_export::ExportInput) -> IdTitles {
    (
        input
            .notes
            .iter()
            .map(|item| (item.id.to_string(), item.title.clone()))
            .collect(),
        input
            .cards
            .iter()
            .map(|item| (item.id.to_string(), item.title.clone()))
            .collect(),
        input
            .questions
            .iter()
            .map(|item| (item.id.to_string(), item.subject.clone()))
            .collect(),
    )
}

fn stored_titles(db: &Path) -> IdTitles {
    let database = Database::open(db).expect("database");
    let connection = database.connection();
    let mut statement = connection
        .prepare("SELECT id, title FROM notes")
        .expect("notes query");
    let notes = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("notes rows")
        .map(Result::unwrap)
        .collect();
    let mut statement = connection
        .prepare("SELECT id, title FROM knowledge_cards")
        .expect("cards query");
    let cards = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("card rows")
        .map(Result::unwrap)
        .collect();
    let mut statement = connection
        .prepare("SELECT id, subject FROM question_cards")
        .expect("questions query");
    let questions = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("question rows")
        .map(Result::unwrap)
        .collect();
    (notes, cards, questions)
}

fn subject_request(format: &str, preset: Option<&str>) -> Value {
    let mut body = json!({
        "format": format,
        "scope": { "kind": "subject", "subject": SUBJECT },
    });
    if let Some(preset) = preset {
        body["preset"] = json!(preset);
    }
    body
}

#[tokio::test]
async fn api_owner_and_learner_export_policies_are_enforced() {
    let dir = TestDir::new();
    let fonts_dir = fonts(&dir.0);
    let state = state(&dir.0, AuthMode::Invite, &fonts_dir).await;
    let app = reinforce_server::app(state.clone());
    let owner = setup_owner(&app, &state).await;
    let learner = register_learner(&app, &owner).await;
    let fixture = seed_content(&state).await;

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("markdown", None)),
        Some(&learner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/zip"
    );
    assert!(
        response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    let learner_zip = response_bytes(response).await;
    let learner_dir = unzip(&learner_zip);
    let imported = import_markdown(learner_dir.path()).expect("learner bundle imports");
    let published = read_entity(
        learner_dir.path(),
        "card",
        &fixture.published_card.to_string(),
    );
    let draft = read_entity(learner_dir.path(), "card", &fixture.draft_card.to_string());
    assert!(draft.is_empty(), "draft card leaked to learner");
    assert!(published.contains("PUBLISHED_VISIBLE"));
    assert!(published.contains("ALLOW_TERM"));
    assert!(published.contains("Content redacted for this viewer."));
    assert!(!published.contains("OWNER_SECRET"));
    assert!(
        learner_dir
            .path()
            .join("assets")
            .join(&fixture.allow_hash)
            .is_file()
    );
    assert!(
        !learner_dir
            .path()
            .join("assets")
            .join(&fixture.redact_hash)
            .is_file()
    );
    assert!(
        !learner_dir
            .path()
            .join("assets")
            .join(&fixture.owner_hash)
            .is_file()
    );
    let (_, cards, questions) = id_titles(&imported);
    assert!(
        cards
            .iter()
            .any(|(id, _)| id == &fixture.published_card.to_string())
    );
    assert!(
        questions
            .iter()
            .any(|(id, _)| id == &fixture.question.to_string())
    );

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("markdown", None)),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let owner_dir = unzip(&response_bytes(response).await);
    let imported = import_markdown(owner_dir.path()).expect("owner bundle imports");
    let owner_card = read_entity(
        owner_dir.path(),
        "card",
        &fixture.published_card.to_string(),
    );
    assert!(owner_card.contains("OWNER_SECRET"));
    assert!(
        owner_dir
            .path()
            .join("assets")
            .join(&fixture.owner_hash)
            .is_file()
    );
    let (notes, cards, questions) = id_titles(&imported);
    let (all_notes, all_cards, all_questions) = stored_titles(&dir.0.join("test.db"));
    assert_eq!(notes, all_notes);
    assert_eq!(cards, all_cards);
    assert_eq!(questions, all_questions);
}

#[tokio::test]
async fn api_rejects_invalid_empty_and_unauthenticated_exports() {
    let dir = TestDir::new();
    let fonts_dir = fonts(&dir.0);
    let state = state(&dir.0, AuthMode::Invite, &PathBuf::from("missing-fonts")).await;
    let app = reinforce_server::app(state.clone());
    let owner = setup_owner(&app, &state).await;
    seed_content(&state).await;

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("markdown", None)),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(json!({
            "format": "docx",
            "scope": { "kind": "subject", "subject": SUBJECT },
        })),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "invalid_request"
    );

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("pdf", None)),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(json!({
            "format": "markdown",
            "scope": { "kind": "subject", "subject": "no-such-subject" },
        })),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "empty_export"
    );

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("pdf", Some("worksheet"))),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "fonts_missing"
    );

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(json!({
            "format": "markdown",
            "scope": { "kind": "weakness", "threshold": 2.0 },
        })),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    drop(fonts_dir);
}

#[tokio::test]
async fn api_pdf_export_is_inline_and_valid() {
    let dir = TestDir::new();
    let fonts_dir = fonts(&dir.0);
    let state = state(&dir.0, AuthMode::Invite, &fonts_dir).await;
    let app = reinforce_server::app(state.clone());
    let owner = setup_owner(&app, &state).await;
    seed_content(&state).await;
    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("pdf", Some("standard"))),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    assert!(
        response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("inline;")
    );
    let bytes = response_bytes(response).await;
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(bytes.len() > 1000);
}

#[tokio::test]
async fn api_and_cli_report_missing_math_font() {
    let dir = TestDir::new();
    let fonts_dir = dir.0.join("fonts");
    std::fs::create_dir_all(&fonts_dir).expect("fonts dir");
    let text_font = typst_assets::fonts().next().expect("bundled text font");
    std::fs::write(fonts_dir.join("text-only.otf"), text_font).expect("font file");

    let state = state(&dir.0, AuthMode::Invite, &fonts_dir).await;
    let app = reinforce_server::app(state.clone());
    let owner = setup_owner(&app, &state).await;
    seed_content(&state).await;

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(subject_request("pdf", Some("standard"))),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], "fonts_missing");
    let message = body["error"]["message"].as_str().expect("message");
    assert!(message.contains("New Computer Modern Math"));
    assert!(message.contains("CJK"));
    assert!(!message.contains(dir.0.to_str().unwrap()));

    let db = dir.0.join("test.db");
    let output = Command::new(env!("CARGO_BIN_EXE_reinforce"))
        .args(["export", "--format", "pdf", "--preset", "standard"])
        .args(["--subject", SUBJECT])
        .args(["--db", db.to_str().unwrap()])
        .args(["--blobs", dir.0.join("blobs").to_str().unwrap()])
        .args(["--fonts", fonts_dir.to_str().unwrap()])
        .args(["--out", dir.0.join("missing-math.pdf").to_str().unwrap()])
        .output()
        .expect("run cli export");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MATH table"), "{stderr}");
    assert!(!stderr.contains("internal server error"), "{stderr}");
}

#[tokio::test]
async fn api_weakness_scope_includes_tests_linked_questions() {
    let dir = TestDir::new();
    let fonts_dir = fonts(&dir.0);
    let state = state(&dir.0, AuthMode::Invite, &fonts_dir).await;
    let app = reinforce_server::app(state.clone());
    let owner = setup_owner(&app, &state).await;
    let fixture = seed_content(&state).await;
    let user_id = state.owner_id.clone();
    let card_id = fixture.published_card.clone();
    let question_id = fixture.question.clone();
    state
        .with_db(move |database| {
            let connection = database.connection_mut();
            let edge = Edge {
                id: EdgeId::generate(),
                edge_type: EdgeType::Tests,
                from: EdgeEndpoint::Question {
                    id: question_id.clone(),
                },
                to: EdgeEndpoint::Card {
                    id: card_id.clone(),
                },
                created_at: Timestamp::unix_epoch(),
            };
            reinforce_core::content_store::insert_edge(connection, &edge)?;
            reinforce_core::db::study::append_attempt(
                connection,
                &reinforce_core::AttemptLog {
                    id: AttemptId::generate(),
                    user_id: user_id.clone(),
                    card_id: CardId::from(question_id),
                    direction: AttemptDirection::Default,
                    session_mode: SessionMode::DailyDue,
                    submitted_payload: json!({"answer": {"kind": "keys", "values": []}}),
                    is_correct: false,
                    duration_ms: 10_000,
                    confidence: Confidence::Certain,
                    error_tags: Vec::new(),
                    created_at: Timestamp::unix_epoch(),
                    sync_cursor: 1,
                    affects_scheduling: true,
                },
            )?;
            Ok(())
        })
        .await
        .expect("seed attempt");

    let response = call(
        &app,
        Method::POST,
        "/api/export",
        Some(json!({
            "format": "markdown",
            "scope": { "kind": "weakness", "threshold": 0.5 },
        })),
        Some(&owner),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let extracted = unzip(&response_bytes(response).await);
    let imported = import_markdown(extracted.path()).expect("weakness bundle imports");
    let (notes, cards, questions) = id_titles(&imported);
    assert!(notes.is_empty());
    assert!(
        cards
            .iter()
            .any(|(id, _)| id == &fixture.published_card.to_string())
    );
    assert!(
        questions
            .iter()
            .any(|(id, _)| id == &fixture.question.to_string())
    );
}

#[test]
fn cli_exports_pdf_and_round_trip_markdown_from_demo_bundle() {
    let dir = TestDir::new();
    let fonts_dir = fonts(&dir.0);
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo");
    let db = dir.0.join("demo.db");
    let blobs = dir.0.join("blobs");
    let import = Command::new(env!("CARGO_BIN_EXE_reinforce"))
        .args(["import", bundle.to_str().unwrap(), "--mode", "owner"])
        .args(["--db", db.to_str().unwrap()])
        .args(["--blobs", blobs.to_str().unwrap()])
        .output()
        .expect("run import");
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let import_report: Value = serde_json::from_slice(&import.stdout).expect("import JSON report");
    assert_eq!(import_report["blobs_written"], 2);

    let pdf = dir.0.join("out.pdf");
    let export_pdf = Command::new(env!("CARGO_BIN_EXE_reinforce"))
        .args(["export", "--format", "pdf", "--preset", "goodnotes"])
        .args(["--subject", "physics", "--out", pdf.to_str().unwrap()])
        .args(["--db", db.to_str().unwrap()])
        .args(["--blobs", blobs.to_str().unwrap()])
        .args(["--fonts", fonts_dir.to_str().unwrap()])
        .output()
        .expect("run pdf export");
    assert!(
        export_pdf.status.success(),
        "{}",
        String::from_utf8_lossy(&export_pdf.stderr)
    );
    let pdf_bytes = std::fs::read(&pdf).expect("pdf file");
    assert!(pdf_bytes.starts_with(b"%PDF-"));
    assert!(pdf_bytes.len() > 1000);

    let zip_path = dir.0.join("out.zip");
    let export_zip = Command::new(env!("CARGO_BIN_EXE_reinforce"))
        .args(["export", "--format", "markdown"])
        .args(["--subject", "physics", "--out", zip_path.to_str().unwrap()])
        .args(["--db", db.to_str().unwrap()])
        .args(["--blobs", blobs.to_str().unwrap()])
        .output()
        .expect("run markdown export");
    assert!(
        export_zip.status.success(),
        "{}",
        String::from_utf8_lossy(&export_zip.stderr)
    );
    let zip_bytes = std::fs::read(&zip_path).expect("zip file");
    let extracted = unzip(&zip_bytes);
    let imported = import_markdown(extracted.path()).expect("CLI bundle imports");
    let (notes, cards, questions) = id_titles(&imported);
    let (all_notes, all_cards, all_questions) = stored_titles(&db);
    assert_eq!(notes, all_notes);
    assert_eq!(cards, all_cards);
    assert_eq!(questions, all_questions);
    let mut any_markdown = false;
    let mut any_asset = false;
    for entry in std::fs::read_dir(extracted.path()).expect("extracted entries") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            for child in std::fs::read_dir(&path).expect("child dir") {
                let child = child.expect("child").path();
                if child.extension().and_then(|value| value.to_str()) == Some("md") {
                    any_markdown = true;
                }
                if child.is_file()
                    && child
                        .parent()
                        .and_then(Path::file_name)
                        .and_then(|v| v.to_str())
                        == Some("assets")
                {
                    any_asset = true;
                }
            }
        }
    }
    assert!(any_markdown, "markdown files missing");
    assert!(any_asset, "resource assets missing");
}
