//! Behavioural tests for the desktop state layer.
//!
//! These exercise the same code paths the window drives — catalogue reads, the
//! five session builders, answer composition and the attempt-log commit — without
//! opening a window or touching a GPU.

use std::path::{Path, PathBuf};

use reinforce_core::CardId;
use reinforce_core::primitives::AttemptDirection;
use reinforce_core::timestamp::Timestamp;
use reinforce_desktop::model::{
    Library, PracticalCountdown, Quiz, QuizMode, QuizTargets, desktop_options,
};
use reinforce_desktop::store::Store;
use uuid::Uuid;

const NOTE_ID: &str = "01a21714-c98d-769f-be7a-f92b11e5cc60";
const FIRST_QUESTION_ID: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const EXAM_SET_ID: &str = "01a21714-c98d-7931-a48a-6a1d5cd348bd";
const KNOWLEDGE_CARD_ID: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("reinforce-desktop-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn database(&self) -> PathBuf {
        self.path.join("reinforce.db")
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

fn now() -> Timestamp {
    "2026-01-02T00:00:00.000000Z".parse().unwrap()
}

fn open_demo() -> (TempDir, Store) {
    let temp = TempDir::new();
    let mut store = Store::open(temp.database()).unwrap();
    let summary = store.import_bundle(bundle_path()).unwrap();
    assert!(summary.applied >= 10, "unexpected import: {summary:?}");
    assert_eq!(summary.blobs_written, 2);
    (temp, store)
}

fn targets() -> QuizTargets {
    QuizTargets {
        exam_set: Some(EXAM_SET_ID.parse().unwrap()),
        cards: vec![KNOWLEDGE_CARD_ID.parse().unwrap()],
        subjects: None,
    }
}

#[test]
fn demo_library_resolves_the_note_outline() {
    let (_temp, store) = open_demo();
    let mut library = Library::load(&store).unwrap();
    assert_eq!(library.subjects.len(), 1);
    assert_eq!(library.subjects[0].subject, "physics");
    assert_eq!(library.notes.len(), 1);
    assert_eq!(
        library.notes[0].id.as_str(),
        NOTE_ID,
        "the demo note should be listed"
    );
    assert_eq!(library.exam_sets.len(), 1);

    library.select(&store, 0).unwrap();
    let outline = library.outline().expect("the outline should resolve");
    assert_eq!(
        outline.cards.len(),
        2,
        "the outline transcludes the table card and the pin card"
    );
    assert!(outline.missing_card_ids.is_empty());
    assert_eq!(outline.note.title, "相位變化入門");

    let targets = library.targets();
    assert_eq!(targets.cards.len(), 2);
    assert_eq!(
        targets.exam_set.as_ref().map(|id| id.as_str()),
        Some(EXAM_SET_ID.to_owned())
    );
}

#[test]
fn full_exam_round_is_written_to_the_attempt_log() {
    let (_temp, mut store) = open_demo();
    let user = store.local_user().unwrap();
    let mut quiz = Quiz::start(
        &store,
        QuizMode::FullExam,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert_eq!(quiz.total(), 4);
    assert_eq!(quiz.questions()[0].id.as_str(), FIRST_QUESTION_ID);

    quiz.toggle_option("A");
    quiz.submit(now()).unwrap();
    quiz.toggle_option("C");
    quiz.toggle_option("A");
    quiz.submit(now()).unwrap();
    quiz.type_text("沸騰");
    quiz.submit(now()).unwrap();
    // The last question is a voided placeholder: submit records a timeout that
    // the core grades as voided and never schedules.
    assert!(quiz.submit(now()).unwrap().is_none());
    assert!(quiz.is_finished(), "the paper should be handed in");

    let score = quiz.score().unwrap();
    assert_eq!(score.correct, 3);
    assert_eq!(score.scoreable, 3);
    assert_eq!(score.voided, 1);

    let written = quiz.commit(&mut store, &user.id, now()).unwrap();
    assert_eq!(written, 4);
    assert_eq!(store.attempt_count(&user.id).unwrap(), 4);

    let scheduling = reinforce_core::db::study::get_scheduling(
        store.connection(),
        &user.id,
        &CardId::from(
            FIRST_QUESTION_ID
                .parse::<reinforce_core::QuestionCardId>()
                .unwrap(),
        ),
        AttemptDirection::Default,
    )
    .unwrap()
    .expect("graded answers should derive scheduling");
    assert_eq!(scheduling.reps, 1);
    assert!(scheduling.due_at.is_some());
}

#[test]
fn topic_drill_follows_tests_edges_and_grades_immediately() {
    let (_temp, store) = open_demo();
    let user = store.local_user().unwrap();
    let mut quiz = Quiz::start(
        &store,
        QuizMode::TopicDrill,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert!(!quiz.is_empty());
    assert!(
        quiz.questions()
            .iter()
            .any(|question| question.id.as_str() == FIRST_QUESTION_ID)
    );

    quiz.toggle_option("B");
    let outcome = quiz
        .submit(now())
        .unwrap()
        .expect("topic drills grade at once");
    assert!(!outcome.is_correct);
    assert!(!outcome.voided);
    assert!(quiz.feedback().is_some());
}

#[test]
fn daily_due_and_weakness_sessions_build_from_the_demo() {
    let (_temp, mut store) = open_demo();
    let user = store.local_user().unwrap();

    let due = Quiz::start(
        &store,
        QuizMode::DailyDue,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert!(due.total() > 0, "new cards should be due on a fresh store");

    // Weakness ranking needs history, so record one graded round first.
    let mut exam = Quiz::start(
        &store,
        QuizMode::FullExam,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    for key in ["A", "A", "C"] {
        exam.toggle_option(key);
        exam.submit(now()).unwrap();
    }
    exam.type_text("沸騰");
    exam.submit(now()).unwrap();
    exam.commit(&mut store, &user.id, now()).unwrap();

    let weakness = Quiz::start(
        &store,
        QuizMode::WeaknessDrill,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert!(
        weakness.total() > 0,
        "recorded mistakes should surface in the weakness drill"
    );
}

#[test]
fn practical_mode_reports_an_empty_bank_instead_of_a_blank_window() {
    let (_temp, store) = open_demo();
    let user = store.local_user().unwrap();
    let quiz = Quiz::start(
        &store,
        QuizMode::PracticalExam,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    // The demo fixture has no pin or image questions, so the mode has to be
    // honest about an empty bank rather than opening an unusable session.
    assert!(quiz.is_empty());
}

#[test]
fn practical_countdown_counts_whole_days() {
    let countdown = PracticalCountdown::parse("2026-01-12").unwrap();
    assert_eq!(countdown.days_left(now()), 10);
    assert_eq!(countdown.label(now()), "實務考倒數 10 天");

    let today = PracticalCountdown::parse("2026-01-02").unwrap();
    assert_eq!(today.days_left(now()), 0);
    assert_eq!(today.label(now()), "實務考：今天");

    let past = PracticalCountdown::parse("2025-12-01").unwrap();
    assert_eq!(past.days_left(now()), 0);
}

#[test]
fn exam_date_is_stored_in_shared_settings() {
    let (_temp, store) = open_demo();
    store
        .set_setting("desktop.practical_exam_at", "2026-02-01")
        .unwrap();
    assert_eq!(
        store
            .setting("desktop.practical_exam_at")
            .unwrap()
            .as_deref(),
        Some("2026-02-01")
    );
}

#[test]
fn tab_manager_opens_focuses_and_closes_falling_back_to_home() {
    use reinforce_desktop::model::{TabManager, WorkspaceTab, WorkspaceTabKind};

    let mut tabs = TabManager::default();
    assert_eq!(tabs.tabs().len(), 1);
    assert_eq!(tabs.active().kind, WorkspaceTabKind::Home);

    let note_id: reinforce_core::NoteId = NOTE_ID.parse().unwrap();
    tabs.open_or_focus(WorkspaceTab::note(note_id.clone(), "筆記 A"));
    tabs.open_or_focus(WorkspaceTab::quiz(1, "複習 1"));
    assert_eq!(tabs.tabs().len(), 3);
    assert_eq!(tabs.active().kind, WorkspaceTabKind::Quiz(1));

    // Re-opening existing note tab focuses without duplicating.
    tabs.open_or_focus(WorkspaceTab::note(note_id.clone(), "筆記 A（更新）"));
    assert_eq!(tabs.tabs().len(), 3);
    assert_eq!(tabs.active().title, "筆記 A（更新）");

    // Closing active tab focuses adjacent tab; closing last falls back to Home.
    tabs.close(&format!("note:{}", note_id.as_str()));
    assert_eq!(tabs.tabs().len(), 2);
    tabs.close("quiz:1");
    tabs.close("home");
    assert_eq!(tabs.tabs().len(), 1);
    assert_eq!(tabs.active().kind, WorkspaceTabKind::Home);
}

#[test]
fn note_check_state_animates_collapse_smoothly_and_resets() {
    use reinforce_desktop::model::NoteCheckState;

    let note_id: reinforce_core::NoteId = NOTE_ID.parse().unwrap();
    let card_id: reinforce_core::KnowledgeCardId = KNOWLEDGE_CARD_ID.parse().unwrap();
    let mut state = NoteCheckState::default();
    assert_eq!(state.factor(&note_id, &card_id), 1.0);

    assert!(state.toggle(&note_id, &card_id));
    assert!(state.is_checked(&note_id, &card_id));
    let mut steps = 0;
    while state.step_animations(0.016) && steps < 100 {
        steps += 1;
    }
    assert!(
        steps > 1 && steps < 100,
        "animation should take multiple frames"
    );
    assert_eq!(state.factor(&note_id, &card_id), 0.0);

    // Show-checked override reveals card immediately; reset clears checks.
    state.show_checked = true;
    assert_eq!(state.factor(&note_id, &card_id), 1.0);
    state.show_checked = false;
    state.reset_note(&note_id);
    assert_eq!(state.checked_count(&note_id), 0);
}

#[test]
fn nested_subject_groups_filter_library_and_build_scoped_exam() {
    use reinforce_desktop::model::{build_subject_group_tree, is_descendant_or_self};

    let (_temp, store) = open_demo();
    let root = store
        .create_subject_group(None, "自然科學", &[], 0, now())
        .unwrap();
    let child = store
        .create_subject_group(Some(&root.id), "物理", &["physics".to_owned()], 0, now())
        .unwrap();
    let other = store
        .create_subject_group(None, "化學", &["chemistry".to_owned()], 1, now())
        .unwrap();

    let mut library = Library::load(&store).unwrap();
    assert_eq!(library.subject_groups.len(), 3);
    assert!(is_descendant_or_self(
        &library.subject_groups,
        &child.id,
        &root.id
    ));
    let tree = build_subject_group_tree(&library.subject_groups);
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].resolved_subjects, vec!["physics".to_owned()]);

    // Selecting parent group recursively includes child's "physics" subject.
    library.selected_group_id = Some(root.id.clone());
    assert_eq!(library.filtered_notes().len(), 1);
    let user = store.local_user().unwrap();
    let quiz = Quiz::start(
        &store,
        QuizMode::FullExam,
        &library.targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert_eq!(quiz.total(), 4);

    // Selecting unrelated "chemistry" group filters out physics notes & yields empty quiz.
    library.selected_group_id = Some(other.id.clone());
    assert!(library.filtered_notes().is_empty());
    let empty_quiz = Quiz::start(
        &store,
        QuizMode::FullExam,
        &library.targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert!(empty_quiz.is_empty());

    // TopicDrill with a selected nested subject group uses that group's knowledge cards,
    // even if a note from another subject was previously opened in library.outline().
    library.select(&store, 0).unwrap();
    let chem_card = reinforce_core::KnowledgeCard {
        id: reinforce_core::KnowledgeCardId::generate(),
        subject: "chemistry".to_owned(),
        topics: vec!["acids".to_owned()],
        title: "酸鹼中和".to_owned(),
        blocks: vec![reinforce_core::ContentBlock::Markdown {
            text: "酸與鹼反應生成鹽與水".to_owned(),
        }],
        anchor_ids: Vec::new(),
        status: reinforce_core::primitives::CardStatus::Published,
        verification: reinforce_core::primitives::Verification::Verified,
        unsure_count: 0,
        metadata: serde_json::json!({}),
        created_at: now(),
        updated_at: now(),
    };
    let chem_question = reinforce_core::QuestionCard {
        id: reinforce_core::QuestionCardId::generate(),
        exam_set_id: None,
        question_group_id: None,
        question_no: Some(1),
        subject: "chemistry".to_owned(),
        topics: vec!["acids".to_owned()],
        stem_blocks: vec![reinforce_core::ContentBlock::Markdown {
            text: "HCl + NaOH 產物為？".to_owned(),
        }],
        anchor_ids: Vec::new(),
        question_type: reinforce_core::primitives::QuestionType::ShortAnswer,
        grading_mode: reinforce_core::primitives::GradingMode::Exact,
        options: Vec::new(),
        answer: vec!["NaCl".to_owned()],
        accepted_answers: Vec::new(),
        answer_source_kind: reinforce_core::primitives::AnswerSourceKind::Official,
        answer_basis: None,
        explanation_blocks: Vec::new(),
        status: reinforce_core::primitives::CardStatus::Published,
        verification: reinforce_core::primitives::Verification::Verified,
        duplicate_question_id: None,
        unsure_count: 0,
        metadata: serde_json::json!({}),
        created_at: now(),
        updated_at: now(),
    };
    let chem_edge = reinforce_core::Edge {
        id: reinforce_core::EdgeId::generate(),
        edge_type: reinforce_core::primitives::EdgeType::Tests,
        from: reinforce_core::EdgeEndpoint::Question {
            id: chem_question.id.clone(),
        },
        to: reinforce_core::EdgeEndpoint::Card {
            id: chem_card.id.clone(),
        },
        created_at: now(),
    };
    reinforce_core::db::content::insert_knowledge_card(store.connection(), &chem_card).unwrap();
    reinforce_core::db::content::insert_question_card(store.connection(), &chem_question).unwrap();
    reinforce_core::db::content::insert_edge(store.connection(), &chem_edge).unwrap();

    let mut reloaded = Library::load(&store).unwrap();
    reloaded.select(&store, 0).unwrap(); // open physics note
    reloaded.selected_group_id = Some(other.id.clone()); // select chemistry group
    let chem_targets = reloaded.targets();
    assert_eq!(chem_targets.cards, vec![chem_card.id.clone()]);
    let topic_quiz = Quiz::start(
        &store,
        QuizMode::TopicDrill,
        &chem_targets,
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert_eq!(topic_quiz.total(), 1);
    assert_eq!(topic_quiz.questions()[0].id, chem_question.id);
}

#[test]
fn grouped_quiz_supports_free_unit_navigation_and_five_level_confidence() {
    use reinforce_core::primitives::Confidence;
    use reinforce_desktop::model::ConfidenceLevel5;

    let (_temp, store) = open_demo();
    let user = store.local_user().unwrap();
    let mut quiz = Quiz::start(
        &store,
        QuizMode::FullExam,
        &targets(),
        &desktop_options(),
        &user.id,
        now(),
    )
    .unwrap();
    assert!(quiz.units().len() >= 2);
    assert!(!quiz.sidebar_buckets().is_empty());

    // Jump directly to unit 1, select an option with 5-level confidence, submit, then jump back to unit 0.
    quiz.select_unit(1, now()).unwrap();
    assert_eq!(quiz.active_unit_index(), 1);
    let unit1_idx = quiz.current_unit().unwrap().session_indices[0];
    quiz.set_confidence5(ConfidenceLevel5::Unfamiliar);
    assert_eq!(
        ConfidenceLevel5::Unfamiliar.to_core(),
        (Confidence::Split, Some("concept_gap"))
    );
    quiz.toggle_option_at(unit1_idx, "A");
    assert_eq!(quiz.submit_current_unit(now()).unwrap(), 1);
    assert!(quiz.answer_for(unit1_idx).is_some());

    quiz.prev_unit(now()).unwrap();
    assert_eq!(quiz.active_unit_index(), 0);
}
