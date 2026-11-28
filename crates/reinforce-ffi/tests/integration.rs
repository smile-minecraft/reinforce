use std::path::{Path, PathBuf};

use reinforce_ffi::catalog::{GraphEndpoint, NoteOutlineNode};
use reinforce_ffi::error::FfiError;
use reinforce_ffi::resources::{AnchorKind, ExportPolicy, ResourceKind, Sensitivity};
use reinforce_ffi::types::{
    AnswerSubmission, AttemptDirection, AttemptLogRecord, BuildOptions, CardStatus, Confidence,
    QuizAnswer, SessionMode, SessionState,
};
use reinforce_ffi::{LocalDatabase, new_attempt_id};
use serde_json::json;
use uuid::Uuid;

const NOTE_ID: &str = "01a21714-c98d-769f-be7a-f92b11e5cc60";
const KNOWLEDGE_CARD_ID: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";
const CONFUSABLE_CARD_ID: &str = "01a21714-c98d-7210-a469-b56decf9f22b";
const PHASE_FEATURES_CARD_ID: &str = "01a21714-c98d-7b1d-92e4-b2d361f54d7f";
const EXAM_SET_ID: &str = "01a21714-c98d-7931-a48a-6a1d5cd348bd";
const QUESTION_ID: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const PHASE_DIAGRAM_ID: &str = "01a21714-c98c-7bfe-8cf8-a60b84573f56";
const PHASE_LABELS_ID: &str = "01a21714-c98c-78d5-8550-3d0928b9c9d1";
const PHASE_DIAGRAM_SHA256: &str =
    "22df5f119108ae55882dc11a03d09ba426cf4771ef92eaa418457f8db40cbe4a";
const ANCHOR_ID: &str = "01a21714-c98d-7814-a2c2-b8efc8433c2c";
const MISSING_ID: &str = "00000000-0000-7000-8000-000000000000";
const NOW: &str = "2026-01-02T00:00:00.000000Z";

/// A private directory so every test owns its database and blob store.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("reinforce-ffi-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn database(&self) -> PathBuf {
        self.path.join("reinforce.db")
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn open(&self) -> std::sync::Arc<LocalDatabase> {
        LocalDatabase::open(self.database().to_string_lossy().into_owned()).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn bundle_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo/bundle.jsonl")
}

fn demo_asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/demo/assets")
        .join(name)
}

fn options() -> BuildOptions {
    BuildOptions::default()
}

fn keys(values: &[&str]) -> QuizAnswer {
    QuizAnswer {
        submission: AnswerSubmission::Keys {
            values: values.iter().map(|value| (*value).to_owned()).collect(),
        },
        confidence: Confidence::Certain,
        error_tags: Vec::new(),
    }
}

fn text(value: &str) -> QuizAnswer {
    QuizAnswer {
        submission: AnswerSubmission::Text {
            value: value.to_owned(),
        },
        confidence: Confidence::Certain,
        error_tags: Vec::new(),
    }
}

fn blob_files(blobs: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(blobs)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn imported_bundle_supports_reading_quizzing_scheduling_and_weakness() {
    let temp = TempDir::new();
    let database = temp.open();
    let summary = database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();
    assert!(summary.applied >= 10, "unexpected import: {summary:?}");
    assert_eq!(summary.blobs_written, 2);
    assert!(database.schema_version().unwrap() > 0);

    let note = database.note(NOTE_ID.to_owned()).unwrap();
    assert_eq!(note.title, "相位變化入門");
    assert!(matches!(
        note.outline.first(),
        Some(reinforce_ffi::types::OutlineNode::Section { .. })
    ));
    let card = database
        .knowledge_card(KNOWLEDGE_CARD_ID.to_owned())
        .unwrap();
    assert_eq!(card.title, "水的三態與相變");
    assert!(card.blocks_json.contains("熔化"));
    let question = database.question_card(QUESTION_ID.to_owned()).unwrap();
    assert_eq!(question.answer, vec!["A"]);
    assert_eq!(question.options.len(), 3);
    let exam_set = database.exam_set(EXAM_SET_ID.to_owned()).unwrap();
    assert_eq!(exam_set.title, "相位變化小測");
    let hits = database.search("熔化".to_owned(), 20).unwrap();
    assert!(!hits.is_empty(), "search should find the imported card");

    let user_id = database.create_user("Offline Learner".to_owned()).unwrap();

    let due = database
        .create_daily_due_session(user_id.clone(), NOW.to_owned(), options())
        .unwrap();
    assert_eq!(
        due.mode().unwrap(),
        reinforce_ffi::types::SessionMode::DailyDue
    );
    let full = database
        .create_full_exam_session(EXAM_SET_ID.to_owned(), options())
        .unwrap();
    let topic = database
        .create_topic_drill_session(vec![KNOWLEDGE_CARD_ID.to_owned()], options())
        .unwrap();
    let practical = database
        .create_practical_exam_session(Vec::new(), options())
        .unwrap();
    let weakness_session = database
        .create_weakness_drill_session(user_id.clone(), options())
        .unwrap();
    assert_eq!(
        weakness_session.mode().unwrap(),
        reinforce_ffi::types::SessionMode::WeaknessDrill
    );
    assert!(practical.questions().unwrap().is_empty());
    assert!(!topic.questions().unwrap().is_empty());

    full.start(NOW.to_owned()).unwrap();
    assert!(matches!(
        full.state().unwrap(),
        SessionState::InProgress { index: 0 }
    ));
    let current = full.current_question().unwrap();
    assert_eq!(current.id, QUESTION_ID);
    full.submit_answer(keys(&["A"]), NOW.to_owned()).unwrap();
    full.submit_answer(keys(&["A", "C"]), NOW.to_owned())
        .unwrap();
    full.submit_answer(text("沸騰"), NOW.to_owned()).unwrap();
    full.submit_answer(text("x"), NOW.to_owned()).unwrap();
    let graded = full.finish(NOW.to_owned()).unwrap();
    assert_eq!(graded.len(), 4);
    assert!(graded.iter().any(|answer| answer.voided));
    let score = full.score().unwrap();
    assert_eq!(score.correct, 3);
    assert_eq!(score.voided, 1);
    assert_eq!(score.scoreable, 3);

    let written = database
        .commit_session(full, user_id.clone(), NOW.to_owned())
        .unwrap();
    assert_eq!(written, 4);
    let scheduling = database
        .scheduling(user_id.clone(), QUESTION_ID.to_owned())
        .unwrap()
        .expect("scheduling should be derived from the committed attempt");
    assert_eq!(scheduling.reps, 1);
    assert!(scheduling.due_at.is_some());
    let due_questions = database
        .due_questions(user_id.clone(), NOW.to_owned(), 10, 10)
        .unwrap();
    assert!(!due_questions.is_empty());
    let report = database.weakness_report(user_id.clone(), 20).unwrap();
    assert!(!report.questions.is_empty());

    let attempt = AttemptLogRecord {
        id: new_attempt_id(),
        user_id: user_id.clone(),
        card_id: QUESTION_ID.to_owned(),
        direction: AttemptDirection::Default,
        session_mode: reinforce_ffi::types::SessionMode::TopicDrill,
        submitted_payload_json: r#"{"answer":{"kind":"keys","values":["B"]}}"#.to_owned(),
        is_correct: false,
        duration_ms: 42_000,
        confidence: Confidence::Guessed,
        error_tags: vec!["incorrect".to_owned(), "misread".to_owned()],
        created_at: NOW.to_owned(),
        sync_cursor: database.next_attempt_cursor().unwrap(),
        affects_scheduling: true,
    };
    let updated = database.append_attempt(attempt).unwrap().unwrap();
    assert_eq!(updated.reps, 2);
    let report = database.weakness_report(user_id, 20).unwrap();
    assert!(report.questions.iter().any(|question| {
        question
            .common_error_tags
            .iter()
            .any(|tag| tag.tag == "misread")
    }));
}

#[test]
fn catalog_lists_imported_content_and_resolves_the_note_outline() {
    let temp = TempDir::new();
    let database = temp.open();
    database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();

    let subjects = database.subjects().unwrap();
    assert_eq!(subjects.len(), 1);
    let physics = &subjects[0];
    assert_eq!(physics.subject, "physics");
    assert_eq!(physics.note_count, 1);
    assert_eq!(physics.knowledge_card_count, 3);
    assert_eq!(physics.exam_set_count, 1);
    assert_eq!(physics.question_card_count, 4);

    let notes = database.notes(None, 50).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, NOTE_ID);
    assert_eq!(
        database
            .notes(Some("chemistry".to_owned()), 50)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        database.notes(Some("physics".to_owned()), 1).unwrap().len(),
        1
    );

    let cards = database
        .knowledge_cards(Some("physics".to_owned()), 50)
        .unwrap();
    assert_eq!(cards.len(), 3);
    assert!(cards.iter().any(|card| card.id == KNOWLEDGE_CARD_ID));

    let exam_sets = database.exam_sets(Some("physics".to_owned()), 50).unwrap();
    assert_eq!(exam_sets.len(), 1);
    assert_eq!(exam_sets[0].exam_set.id, EXAM_SET_ID);
    assert_eq!(exam_sets[0].exam_set.title, "相位變化小測");
    assert_eq!(exam_sets[0].question_count, 4);
    assert_eq!(exam_sets[0].published_question_count, 4);

    let outline = database.note_outline(NOTE_ID.to_owned()).unwrap();
    assert_eq!(outline.len(), 1);
    let NoteOutlineNode::Section {
        heading, children, ..
    } = &outline[0]
    else {
        panic!("expected a section, got {:?}", outline[0]);
    };
    assert_eq!(heading, "讀圖與相變");
    assert_eq!(children.len(), 2);
    let mut resolved_ids = Vec::new();
    for child in children {
        let NoteOutlineNode::Card { card } = child else {
            panic!("expected a resolved card");
        };
        resolved_ids.push(card.id.clone());
        assert!(matches!(card.status, CardStatus::Published));
        assert!(!card.title.is_empty());
    }
    assert_eq!(
        resolved_ids,
        vec![KNOWLEDGE_CARD_ID, PHASE_FEATURES_CARD_ID],
        "cards follow the outline order"
    );

    let questions = database.question_cards(EXAM_SET_ID.to_owned(), 50).unwrap();
    assert_eq!(questions.len(), 4);
    let numbers: Vec<Option<u32>> = questions.iter().map(|card| card.question_no).collect();
    assert_eq!(numbers, vec![Some(1), Some(2), Some(3), Some(4)]);
    assert_eq!(questions[0].id, QUESTION_ID);
    assert!(
        database
            .question_groups(EXAM_SET_ID.to_owned(), 50)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn bundle_assets_become_content_addressed_blobs() {
    let temp = TempDir::new();
    let blobs = temp.join("media");
    let database = LocalDatabase::open_with_blobs(
        temp.database().to_string_lossy().into_owned(),
        blobs.to_string_lossy().into_owned(),
    )
    .unwrap();
    let bundle = bundle_path().to_string_lossy().into_owned();
    let first = database.import_bundle(bundle.clone()).unwrap();
    assert_eq!(first.blobs_written, 2, "both demo images are referenced");

    let resource = database.resource(PHASE_DIAGRAM_ID.to_owned()).unwrap();
    assert!(matches!(resource.kind, ResourceKind::Image));
    assert_eq!(resource.title, "phase-diagram.svg");
    assert_eq!(resource.sha256, PHASE_DIAGRAM_SHA256);
    assert_eq!(resource.byte_size, 983);
    assert_eq!(resource.width, Some(800));
    assert!(matches!(resource.sensitivity, Sensitivity::Normal));
    assert!(matches!(resource.export_policy, ExportPolicy::Allow));

    let labels = database.resource(PHASE_LABELS_ID.to_owned()).unwrap();
    assert!(matches!(labels.sensitivity, Sensitivity::Restricted));
    assert!(matches!(labels.export_policy, ExportPolicy::Redact));

    let blob_path = database
        .resource_blob_path(PHASE_DIAGRAM_ID.to_owned())
        .unwrap();
    let blob = Path::new(&blob_path);
    assert!(blob.is_absolute(), "the client gets an absolute path");
    assert!(blob.is_file(), "the blob exists at {blob_path}");
    assert_eq!(
        blob.file_name().unwrap().to_string_lossy(),
        PHASE_DIAGRAM_SHA256
    );
    assert_eq!(blob.parent().unwrap(), blobs.as_path());
    let bytes = std::fs::read(blob).unwrap();
    assert_eq!(
        bytes,
        std::fs::read(demo_asset("phase-diagram.svg")).unwrap(),
        "the blob is byte-identical to the bundle asset"
    );
    assert_eq!(
        reinforce_core::ContentHash::from_bytes(&bytes).as_str(),
        PHASE_DIAGRAM_SHA256
    );

    let anchor = database.anchor(ANCHOR_ID.to_owned()).unwrap();
    assert_eq!(anchor.resource_id, PHASE_DIAGRAM_ID);
    assert!(matches!(anchor.anchor_kind, AnchorKind::Rect));
    let rect = anchor.rect.expect("a rect anchor keeps its rectangle");
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0.0, 0.0, 1.0, 1.0));
    assert!(anchor.pdf_page.is_none());

    let second = database.import_bundle(bundle).unwrap();
    assert_eq!(second.blobs_written, 0, "a replay rewrites no blob");
    assert_eq!(
        blob_files(&blobs),
        vec![
            "22df5f119108ae55882dc11a03d09ba426cf4771ef92eaa418457f8db40cbe4a".to_owned(),
            "d5115bdba60ace7b71df3758d174dec49382a539f34fd007ee263692f49e5179".to_owned(),
        ],
        "no duplicate, temporary or stray files"
    );
    assert!(second.applied >= 1);
}

#[test]
fn graph_neighbors_follow_typed_edges_in_both_directions() {
    let temp = TempDir::new();
    let database = temp.open();
    database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();

    let from_card = database
        .neighbors(
            GraphEndpoint::Card {
                id: KNOWLEDGE_CARD_ID.to_owned(),
            },
            1,
        )
        .unwrap();
    assert!(
        from_card.iter().any(|neighbor| matches!(
            &neighbor.endpoint,
            GraphEndpoint::Question { id } if id.as_str() == QUESTION_ID
        )),
        "the tests edge is followed from card to question: {from_card:?}"
    );
    assert!(from_card.iter().any(|neighbor| matches!(
        &neighbor.endpoint,
        GraphEndpoint::Card { id } if id.as_str() == CONFUSABLE_CARD_ID
    )));
    assert!(from_card.iter().all(|neighbor| neighbor.depth == 1));

    let from_question = database
        .neighbors(
            GraphEndpoint::Question {
                id: QUESTION_ID.to_owned(),
            },
            2,
        )
        .unwrap();
    assert!(
        matches!(
            &from_question[0].endpoint,
            GraphEndpoint::Card { id } if id.as_str() == KNOWLEDGE_CARD_ID
        ),
        "the tests question reaches its card first"
    );
    assert_eq!(from_question[0].depth, 1);
    assert!(
        from_question.iter().any(|neighbor| matches!(
            &neighbor.endpoint,
            GraphEndpoint::Card { id } if id.as_str() == CONFUSABLE_CARD_ID && neighbor.depth == 2
        )),
        "depth 2 reaches the confusable neighbour: {from_question:?}"
    );

    let options = database
        .neighbors(
            GraphEndpoint::Option {
                question_id: QUESTION_ID.to_owned(),
                key: "A".to_owned(),
            },
            1,
        )
        .unwrap();
    assert!(
        options.is_empty(),
        "no option edge exists in the demo bundle"
    );

    let unknown = database
        .neighbors(
            GraphEndpoint::Card {
                id: MISSING_ID.to_owned(),
            },
            1,
        )
        .unwrap();
    assert!(unknown.is_empty());
}

#[test]
fn user_card_meta_round_trips() {
    let temp = TempDir::new();
    let database = temp.open();
    database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();
    let user_id = database.create_user("Learner".to_owned()).unwrap();

    assert!(
        database
            .user_card_meta(user_id.clone(), KNOWLEDGE_CARD_ID.to_owned())
            .unwrap()
            .is_none()
    );

    let saved = database
        .save_user_card_meta(
            user_id.clone(),
            KNOWLEDGE_CARD_ID.to_owned(),
            true,
            vec!["midterm".to_owned(), "hard".to_owned()],
            Some("review before Friday".to_owned()),
        )
        .unwrap();
    assert!(saved.starred);
    assert_eq!(saved.personal_tags, vec!["midterm", "hard"]);
    assert_eq!(saved.personal_note.as_deref(), Some("review before Friday"));

    let read = database
        .user_card_meta(user_id.clone(), KNOWLEDGE_CARD_ID.to_owned())
        .unwrap()
        .expect("the saved row is readable");
    assert_eq!(read.user_id, user_id);
    assert_eq!(read.card_id, KNOWLEDGE_CARD_ID);
    assert_eq!(read.personal_tags, saved.personal_tags);
    assert_eq!(read.updated_at, saved.updated_at);

    let cleared = database
        .save_user_card_meta(
            user_id.clone(),
            KNOWLEDGE_CARD_ID.to_owned(),
            false,
            Vec::new(),
            None,
        )
        .unwrap();
    assert!(!cleared.starred);
    assert!(cleared.personal_tags.is_empty());
    assert!(cleared.personal_note.is_none());
    let read = database
        .user_card_meta(user_id, KNOWLEDGE_CARD_ID.to_owned())
        .unwrap()
        .unwrap();
    assert!(!read.starred);
    assert!(read.personal_note.is_none());
}

/// A second bundle exercises the paths the demo dataset does not carry:
/// question groups, unnumbered questions and content that stays unpublished.
#[test]
fn question_groups_and_unpublished_content_survive_import() {
    let temp = TempDir::new();
    let database = temp.open();

    let exam_set_id = Uuid::now_v7().to_string();
    let group_id = Uuid::now_v7().to_string();
    let draft_card_id = Uuid::now_v7().to_string();
    let published_note_id = Uuid::now_v7().to_string();
    let draft_note_id = Uuid::now_v7().to_string();
    let numbered_id = Uuid::now_v7().to_string();
    let unnumbered_id = Uuid::now_v7().to_string();

    let knowledge_card = json!({
        "op": "knowledge_card",
        "data": {
            "id": draft_card_id,
            "subject": "biology",
            "topics": ["cells"],
            "title": "draft card",
            "blocks": [{"type": "markdown", "text": "still being written"}],
            "anchor_ids": [],
            "status": "draft",
            "verification": "unchecked",
            "unsure_count": 0,
            "metadata": {},
            "created_at": NOW,
            "updated_at": NOW
        }
    });
    let note = |id: &str, title: &str, status: &str| {
        json!({
            "op": "note",
            "data": {
                "id": id,
                "subject": "biology",
                "title": title,
                "outline": [{
                    "type": "section",
                    "heading": "Cells",
                    "intro_markdown": null,
                    "children": [{"type": "card_ref", "card_id": draft_card_id.clone()}]
                }],
                "view_settings": {},
                "status": status,
                "created_at": NOW,
                "updated_at": NOW
            }
        })
    };
    let exam_set = json!({
        "op": "exam_set",
        "data": {
            "id": exam_set_id,
            "subject": "biology",
            "title": "cell quiz",
            "source_kind": "original",
            "exam_year": null,
            "source_resource_id": null,
            "expected_questions": null,
            "metadata": {},
            "is_complete": true
        }
    });
    let group = json!({
        "op": "question_group",
        "data": {
            "id": group_id.clone(),
            "exam_set_id": exam_set_id.clone(),
            "title": "shared vignette",
            "shared_stem_blocks": [{"type": "markdown", "text": "one passage"}]
        }
    });
    let question = |id: &str, number: serde_json::Value| {
        json!({
            "op": "question_card",
            "data": {
                "id": id,
                "exam_set_id": exam_set_id.clone(),
                "question_group_id": group_id.clone(),
                "question_no": number,
                "subject": "biology",
                "topics": [],
                "stem_blocks": [{"type": "markdown", "text": "which organelle?"}],
                "explanation_blocks": [],
                "anchor_ids": [],
                "question_type": "single_choice",
                "grading_mode": "exact",
                "options": [{"key": "A", "content": "mitochondrion", "explanation": null, "linked_card_ids": []}],
                "answer": ["A"],
                "accepted_answers": [],
                "answer_source_kind": "official",
                "answer_basis": null,
                "duplicate_question_id": null,
                "status": "published",
                "verification": "verified",
                "unsure_count": 0,
                "metadata": {},
                "created_at": NOW,
                "updated_at": NOW
            }
        })
    };

    let lines = [
        knowledge_card.to_string(),
        note(&published_note_id, "published note", "published").to_string(),
        note(&draft_note_id, "draft note", "draft").to_string(),
        exam_set.to_string(),
        group.to_string(),
        question(&numbered_id, json!(1)).to_string(),
        question(&unnumbered_id, json!(null)).to_string(),
    ];
    let custom = temp.join("custom");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(custom.join("bundle.jsonl"), lines.join("\n")).unwrap();

    let summary = database
        .import_bundle(custom.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(summary.blobs_written, 0, "this bundle carries no assets");

    let subjects = database.subjects().unwrap();
    let biology = subjects
        .iter()
        .find(|summary| summary.subject == "biology")
        .expect("biology is listed");
    assert_eq!(biology.note_count, 1, "the draft note is not listed");
    assert_eq!(
        biology.knowledge_card_count, 0,
        "the draft card is not browsable"
    );
    assert_eq!(biology.exam_set_count, 1);
    assert_eq!(biology.question_card_count, 2);

    let notes = database.notes(Some("biology".to_owned()), 50).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, published_note_id);
    assert!(matches!(notes[0].status, CardStatus::Published));

    let outline = database.note_outline(published_note_id).unwrap();
    let NoteOutlineNode::Section { children, .. } = &outline[0] else {
        panic!("expected a section");
    };
    let NoteOutlineNode::Card { card } = &children[0] else {
        panic!("expected a resolved card");
    };
    assert_eq!(card.id, draft_card_id);
    assert!(
        matches!(card.status, CardStatus::Draft),
        "a referenced card is resolved even when it is not published"
    );

    let groups = database.question_groups(exam_set_id.clone(), 50).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, group_id);
    assert_eq!(groups[0].title.as_deref(), Some("shared vignette"));

    let questions = database.question_cards(exam_set_id.clone(), 50).unwrap();
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[0].id, numbered_id);
    assert_eq!(questions[0].question_no, Some(1));
    assert_eq!(
        questions[0].question_group_id.as_deref(),
        Some(group_id.as_str())
    );
    assert_eq!(
        questions[1].question_no, None,
        "unnumbered questions sort last"
    );

    let summary = database.exam_sets(Some("biology".to_owned()), 50).unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].question_count, 2);
    assert_eq!(summary[0].published_question_count, 2);
}

#[test]
fn invalid_boundary_inputs_become_errors_instead_of_panics() {
    assert!(matches!(
        LocalDatabase::open(String::new()),
        Err(FfiError::InvalidInput { .. })
    ));
    let temp = TempDir::new();
    assert!(matches!(
        LocalDatabase::open_with_blobs(
            temp.database().to_string_lossy().into_owned(),
            String::new()
        ),
        Err(FfiError::InvalidInput { .. })
    ));

    let database = temp.open();
    assert!(matches!(
        database.import_bundle("does-not-exist.jsonl".to_owned()),
        Err(FfiError::Io { .. })
    ));
    assert!(matches!(
        database.question_card("not-a-uuid".to_owned()),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.question_card(MISSING_ID.to_owned()),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        database.search(String::new(), 10),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.weakness_report(MISSING_ID.to_owned(), 0),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.note_outline("not-a-uuid".to_owned()),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.note_outline(MISSING_ID.to_owned()),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        database.resource("not-a-uuid".to_owned()),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.resource(MISSING_ID.to_owned()),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        database.anchor(MISSING_ID.to_owned()),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        database.resource_blob_path(MISSING_ID.to_owned()),
        Err(FfiError::NotFound { .. })
    ));
    assert!(matches!(
        database.neighbors(
            GraphEndpoint::Card {
                id: MISSING_ID.to_owned()
            },
            0
        ),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.neighbors(
            GraphEndpoint::Card {
                id: MISSING_ID.to_owned()
            },
            17
        ),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.neighbors(
            GraphEndpoint::Option {
                question_id: MISSING_ID.to_owned(),
                key: " ".to_owned()
            },
            1
        ),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.save_user_card_meta(
            "not-a-user".to_owned(),
            KNOWLEDGE_CARD_ID.to_owned(),
            true,
            Vec::new(),
            None
        ),
        Err(FfiError::InvalidInput { .. })
    ));

    let mut invalid_options = options();
    invalid_options.question_limit = 0;
    assert!(matches!(
        database.create_full_exam_session(EXAM_SET_ID.to_owned(), invalid_options),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        database.create_daily_due_session(MISSING_ID.to_owned(), "yesterday".to_owned(), options()),
        Err(FfiError::InvalidInput { .. })
    ));

    let session = database
        .create_full_exam_session(EXAM_SET_ID.to_owned(), options())
        .unwrap();
    assert!(matches!(
        session.submit_answer(keys(&["A"]), NOW.to_owned()),
        Err(FfiError::InvalidInput { .. })
    ));
    assert!(matches!(
        session.start("not-a-timestamp".to_owned()),
        Err(FfiError::InvalidInput { .. })
    ));
}

/// The sync helpers paginate, keep the local profile id private to the device,
/// and refuse batches that belong to another server account.
#[test]
fn sync_helpers_paginate_and_guard_identity() {
    let temp = TempDir::new();
    let database = temp.open();
    database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();
    let local_user = database.create_user("Learner".to_owned()).unwrap();
    let remote_user = Uuid::now_v7().to_string();
    let stranger = Uuid::now_v7().to_string();

    assert!(database.sync_state(local_user.clone()).unwrap().is_none());
    let state = database
        .bind_remote_user(local_user.clone(), remote_user.clone())
        .unwrap();
    assert_eq!(state.remote_user_id, remote_user);
    assert_eq!(state.pull_cursor, 0);
    assert_eq!(
        database
            .bind_remote_user(local_user.clone(), remote_user.clone())
            .unwrap()
            .pull_cursor,
        0
    );

    let mut ids = Vec::new();
    for (index, timestamp) in [
        "2026-01-02T10:00:00.000000Z",
        "2026-01-02T10:05:00.000000Z",
        "2026-01-02T10:10:00.000000Z",
    ]
    .iter()
    .enumerate()
    {
        let id = new_attempt_id();
        database
            .append_attempt(AttemptLogRecord {
                id: id.clone(),
                user_id: local_user.clone(),
                card_id: KNOWLEDGE_CARD_ID.to_owned(),
                direction: AttemptDirection::Default,
                session_mode: SessionMode::DailyDue,
                submitted_payload_json: json!({"keys": ["A"]}).to_string(),
                is_correct: index != 1,
                duration_ms: 900,
                confidence: Confidence::Certain,
                error_tags: Vec::new(),
                created_at: (*timestamp).to_owned(),
                sync_cursor: database.next_attempt_cursor().unwrap(),
                affects_scheduling: true,
            })
            .unwrap();
        ids.push(id);
    }

    let page = database.pending_sync(local_user.clone(), 2).unwrap();
    assert_eq!(page.attempts.len(), 2);
    assert!(page.has_more_attempts);
    assert_eq!(
        page.attempts[0].user_id, remote_user,
        "uploads are stamped with the server account id"
    );
    assert_eq!(page.remote_user_id.as_deref(), Some(remote_user.as_str()));

    // Foreign rows are rejected before anything is written.
    let mut foreign = page.attempts[0].clone();
    foreign.id = new_attempt_id();
    foreign.user_id = stranger.clone();
    assert!(matches!(
        database.apply_remote_batch(local_user.clone(), vec![foreign], Vec::new(), 1),
        Err(FfiError::InvalidInput { .. })
    ));

    database
        .confirm_upload(
            local_user.clone(),
            page.attempts.iter().map(|a| a.id.clone()).collect(),
            Vec::new(),
        )
        .unwrap();
    let page = database.pending_sync(local_user.clone(), 2).unwrap();
    assert_eq!(page.attempts.len(), 1);
    assert!(!page.has_more_attempts);
    assert_eq!(page.attempts[0].id, ids[2]);
}

#[test]
fn subject_groups_topic_links_and_indexed_answers_round_trip() {
    let temp = TempDir::new();
    let database = temp.open();
    database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();

    let root = database
        .create_subject_group(None, "自然科".to_owned(), vec!["physics".to_owned()], 0)
        .unwrap();
    let child = database
        .create_subject_group(
            Some(root.id.clone()),
            "力學".to_owned(),
            vec!["physics".to_owned(), "mechanics".to_owned()],
            1,
        )
        .unwrap();
    assert_eq!(
        child.subjects,
        vec!["mechanics".to_owned(), "physics".to_owned()]
    );
    assert_eq!(database.subject_groups().unwrap().len(), 2);
    assert_eq!(
        database.resolve_group_subjects(root.id.clone()).unwrap(),
        vec!["mechanics".to_owned(), "physics".to_owned()]
    );

    let detached = database
        .update_subject_group(
            child.id.clone(),
            None,
            "力學".to_owned(),
            vec!["mechanics".to_owned()],
            2,
        )
        .unwrap();
    assert!(detached.parent_id.is_none());
    assert_eq!(detached.sort_order, 2);
    assert_eq!(
        database.resolve_group_subjects(root.id.clone()).unwrap(),
        vec!["physics".to_owned()]
    );
    assert!(database.delete_subject_group(child.id.clone()).unwrap());
    assert!(!database.delete_subject_group(child.id.clone()).unwrap());
    assert!(database.resolve_group_subjects(child.id).is_err());

    let linked = database
        .topic_question_ids(vec![KNOWLEDGE_CARD_ID.to_owned()], 10)
        .unwrap();
    assert_eq!(linked, vec![QUESTION_ID.to_owned()]);

    let exam = database
        .create_subject_exam_session(vec!["physics".to_owned()], options())
        .unwrap();
    assert_eq!(exam.questions().unwrap().len(), 4);
    exam.start(NOW.to_owned()).unwrap();
    let state = exam.select_index(1, NOW.to_owned()).unwrap();
    assert!(matches!(state, SessionState::InProgress { index: 1 }));
    assert!(
        exam.submit_answer_at(1, keys(&["A"]), NOW.to_owned())
            .unwrap()
            .is_none(),
        "subject exams defer grading"
    );
    assert!(
        exam.submit_answer_at(1, keys(&["C"]), NOW.to_owned())
            .unwrap()
            .is_none()
    );
    let answers = exam.answers().unwrap();
    assert_eq!(answers.len(), 1, "re-answering must replace");
    assert_eq!(answers[0].index, 1);
    match &answers[0].submission {
        AnswerSubmission::Keys { values } => assert_eq!(values, &vec!["C".to_owned()]),
        other => panic!("unexpected submission: {other:?}"),
    }
    assert_eq!(exam.finish(NOW.to_owned()).unwrap().len(), 1);
    assert_eq!(exam.score().unwrap().scoreable, 1);
    assert!(matches!(
        exam.select_index(0, NOW.to_owned()),
        Err(FfiError::InvalidInput { .. })
    ));
}
