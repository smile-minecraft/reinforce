use std::path::{Path, PathBuf};

use reinforce_ffi::error::FfiError;
use reinforce_ffi::types::{
    AnswerSubmission, AttemptDirection, AttemptLogRecord, BuildOptions, Confidence, QuizAnswer,
    SessionState,
};
use reinforce_ffi::{LocalDatabase, new_attempt_id};
use uuid::Uuid;

const NOTE_ID: &str = "01a21714-c98d-769f-be7a-f92b11e5cc60";
const KNOWLEDGE_CARD_ID: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";
const EXAM_SET_ID: &str = "01a21714-c98d-7931-a48a-6a1d5cd348bd";
const QUESTION_ID: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const MISSING_ID: &str = "00000000-0000-7000-8000-000000000000";
const NOW: &str = "2026-01-02T00:00:00.000000Z";

struct TempDb {
    path: PathBuf,
}

impl TempDb {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("reinforce-ffi-{}.db", Uuid::now_v7())),
        }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.clone().into_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}

fn bundle_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/demo/bundle.jsonl")
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

#[test]
fn imported_bundle_supports_reading_quizzing_scheduling_and_weakness() {
    let temp = TempDb::new();
    let database = LocalDatabase::open(temp.path.to_string_lossy().into_owned()).unwrap();
    let summary = database
        .import_bundle(bundle_path().to_string_lossy().into_owned())
        .unwrap();
    assert!(summary.applied >= 10, "unexpected import: {summary:?}");
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
fn invalid_boundary_inputs_become_errors_instead_of_panics() {
    assert!(matches!(
        LocalDatabase::open(String::new()),
        Err(FfiError::InvalidInput { .. })
    ));

    let temp = TempDb::new();
    let database = LocalDatabase::open(temp.path.to_string_lossy().into_owned()).unwrap();
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
