use std::collections::HashSet;

use reinforce_core::content::{ContentBlock, Pin, QuestionOption};
use reinforce_core::db::{content as store, study as study_store};
use reinforce_core::fsrs::recompute_scheduling;
use reinforce_core::primitives::{
    AnswerSourceKind, AttemptDirection, AttemptId, CardStatus, Confidence, EdgeId, EdgeType,
    ExamSetId, GradingMode, KnowledgeCardId, QuestionCardId, QuestionType, ResourceId,
    SchedulingState, SessionMode, SourceKind, Verification,
};
use reinforce_core::quiz::{
    AnswerSubmission, BuildOptions, GradingConfig, NormalizedPoint, PinClick, QuizAnswer,
    SessionState, TickOutcome, build_daily_due, build_full_exam, build_practical_exam,
    build_topic_drill, build_weakness_drill, grade_question, normalize_term,
};
use reinforce_core::study::{AttemptLog, is_voided_attempt};
use reinforce_core::weakness::build_report;
use reinforce_core::{
    CardId, CardScheduling, Database, Edge, EdgeEndpoint, ExamSet, KnowledgeCard, QuestionCard,
    Timestamp, User, UserId,
};
use serde_json::{Value, json};

fn base() -> Timestamp {
    "2026-01-01T00:00:00.000000Z".parse().unwrap()
}

fn at(seconds: i64) -> Timestamp {
    base().plus_days(seconds as f64 / 86_400.0)
}

fn insert_user(connection: &rusqlite::Connection) -> User {
    let user = User {
        id: UserId::generate(),
        display_name: "learner".to_owned(),
        created_at: base(),
    };
    study_store::insert_user(connection, &user).unwrap();
    user
}

fn option(key: &str) -> QuestionOption {
    QuestionOption {
        key: key.to_owned(),
        content: format!("option {key}"),
        explanation: None,
        linked_card_ids: vec![],
    }
}

struct QuestionSpec {
    label: String,
    exam_set_id: Option<ExamSetId>,
    question_no: Option<u32>,
    question_type: QuestionType,
    grading_mode: GradingMode,
    answer: Vec<String>,
    accepted_answers: Vec<Vec<String>>,
    options: Vec<QuestionOption>,
    stem_blocks: Vec<ContentBlock>,
}

impl QuestionSpec {
    fn new(
        label: &str,
        question_type: QuestionType,
        grading_mode: GradingMode,
        answer: Vec<String>,
    ) -> Self {
        Self {
            label: label.to_owned(),
            exam_set_id: None,
            question_no: None,
            question_type,
            grading_mode,
            answer,
            accepted_answers: vec![],
            options: vec![],
            stem_blocks: vec![],
        }
    }

    fn exam(mut self, exam_set_id: ExamSetId, question_no: u32) -> Self {
        self.exam_set_id = Some(exam_set_id);
        self.question_no = Some(question_no);
        self
    }

    fn accepted(mut self, accepted_answers: Vec<Vec<String>>) -> Self {
        self.accepted_answers = accepted_answers;
        self
    }

    fn options(mut self, options: Vec<QuestionOption>) -> Self {
        self.options = options;
        self
    }

    fn stem(mut self, stem_blocks: Vec<ContentBlock>) -> Self {
        self.stem_blocks = stem_blocks;
        self
    }

    fn build(self) -> QuestionCard {
        let stem_blocks = if self.stem_blocks.is_empty() {
            vec![ContentBlock::Markdown {
                text: self.label.clone(),
            }]
        } else {
            self.stem_blocks
        };
        QuestionCard {
            id: QuestionCardId::generate(),
            exam_set_id: self.exam_set_id,
            question_group_id: None,
            question_no: self.question_no,
            subject: "biology".to_owned(),
            topics: vec![self.label],
            stem_blocks,
            explanation_blocks: vec![],
            anchor_ids: vec![],
            question_type: self.question_type,
            grading_mode: self.grading_mode,
            options: self.options,
            answer: self.answer,
            accepted_answers: self.accepted_answers,
            answer_source_kind: AnswerSourceKind::Official,
            answer_basis: None,
            duplicate_question_id: None,
            status: CardStatus::Published,
            verification: Verification::Verified,
            unsure_count: 0,
            metadata: json!({}),
            created_at: base(),
            updated_at: base(),
        }
    }
}

fn choice(label: &str, answer: &str) -> QuestionCard {
    QuestionSpec::new(
        label,
        QuestionType::SingleChoice,
        GradingMode::Exact,
        vec![answer.to_owned()],
    )
    .options(vec![option("A"), option("B"), option("C")])
    .build()
}

fn knowledge(title: &str) -> KnowledgeCard {
    KnowledgeCard {
        id: KnowledgeCardId::generate(),
        subject: "biology".to_owned(),
        topics: vec![],
        title: title.to_owned(),
        blocks: vec![],
        anchor_ids: vec![],
        status: CardStatus::Published,
        verification: Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: base(),
        updated_at: base(),
    }
}

fn tests_edge(from: EdgeEndpoint, to: KnowledgeCardId) -> Edge {
    Edge {
        id: EdgeId::generate(),
        edge_type: EdgeType::Tests,
        from,
        to: EdgeEndpoint::Card { id: to },
        created_at: base(),
    }
}

fn attempt(
    user: &User,
    card_id: CardId,
    correct: bool,
    confidence: Confidence,
    duration_ms: i64,
    created_at: Timestamp,
    payload: Value,
) -> AttemptLog {
    AttemptLog {
        id: AttemptId::generate(),
        user_id: user.id.clone(),
        card_id,
        direction: AttemptDirection::Default,
        session_mode: SessionMode::DailyDue,
        submitted_payload: payload,
        is_correct: correct,
        duration_ms,
        confidence,
        error_tags: vec![],
        created_at,
        sync_cursor: 1,
        affects_scheduling: true,
    }
}

fn save_due(
    connection: &rusqlite::Connection,
    user: &User,
    card_id: CardId,
    due_at: Option<Timestamp>,
) {
    let scheduling = CardScheduling {
        user_id: user.id.clone(),
        card_id,
        direction: AttemptDirection::Default,
        stability: 1.0,
        difficulty: 5.0,
        reps: 1,
        lapses: 0,
        state: SchedulingState::Review,
        last_review_at: Some(base()),
        due_at,
        desired_retention: 0.9,
        updated_at: base(),
    };
    study_store::save_scheduling(connection, &scheduling).unwrap();
}

fn pin(term_primary: &str, term_secondary: Option<&str>) -> Pin {
    Pin {
        no: 1,
        x: 0.5,
        y: 0.5,
        label_x: 0.6,
        label_y: 0.6,
        term_primary: term_primary.to_owned(),
        term_secondary: term_secondary.map(str::to_owned),
        detail: None,
        hint: None,
        linked_card_id: None,
    }
}

fn pin_stem(pin: Pin) -> Vec<ContentBlock> {
    vec![ContentBlock::InteractivePins {
        resource_id: ResourceId::generate(),
        overlay_resource_id: None,
        pins: vec![pin],
    }]
}

#[test]
fn daily_due_orders_overdue_cards_and_caps_new_cards() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let user = insert_user(connection);
    let overdue = choice("overdue", "A");
    let future = choice("future", "A");
    let first_new = choice("new-first", "A");
    let second_new = choice("new-second", "A");
    for question in [&overdue, &future, &first_new, &second_new] {
        store::insert_question_card(connection, question).unwrap();
    }
    save_due(
        connection,
        &user,
        CardId::from(overdue.id.clone()),
        Some(at(-86_400)),
    );
    save_due(
        connection,
        &user,
        CardId::from(future.id.clone()),
        Some(at(86_400)),
    );
    let options = BuildOptions {
        new_card_limit: 1,
        question_limit: 10,
        ..BuildOptions::default()
    };

    let session = build_daily_due(connection, &user.id, base(), &options).unwrap();
    let ids: Vec<QuestionCardId> = session.questions().iter().map(|q| q.id.clone()).collect();
    assert_eq!(ids.first(), Some(&overdue.id));
    assert!(!ids.contains(&future.id));
    assert!(ids.contains(&first_new.id) || ids.contains(&second_new.id));
    assert_eq!(ids.len(), 2);
}

#[test]
fn full_exam_orders_by_number_defers_grading_and_excludes_voided_from_score() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let user = insert_user(connection);
    let exam_set = ExamSet {
        id: ExamSetId::generate(),
        subject: "biology".to_owned(),
        title: "mock".to_owned(),
        source_kind: SourceKind::PastExam,
        exam_year: Some(2025),
        source_resource_id: None,
        expected_questions: None,
        metadata: json!({}),
        is_complete: true,
    };
    store::insert_exam_set(connection, &exam_set).unwrap();
    let third = QuestionSpec::new(
        "third",
        QuestionType::SingleChoice,
        GradingMode::Exact,
        vec!["A".to_owned()],
    )
    .exam(exam_set.id.clone(), 3)
    .options(vec![option("A"), option("B")])
    .build();
    let first = QuestionSpec::new(
        "first",
        QuestionType::SingleChoice,
        GradingMode::Exact,
        vec!["A".to_owned()],
    )
    .exam(exam_set.id.clone(), 1)
    .options(vec![option("A"), option("B")])
    .build();
    let second = QuestionSpec::new(
        "second",
        QuestionType::SingleChoice,
        GradingMode::Exact,
        vec!["B".to_owned()],
    )
    .exam(exam_set.id.clone(), 2)
    .options(vec![option("A"), option("B")])
    .build();
    let voided = QuestionSpec::new(
        "voided",
        QuestionType::SingleChoice,
        GradingMode::Voided,
        vec![],
    )
    .exam(exam_set.id.clone(), 4)
    .options(vec![option("A")])
    .build();
    for question in [&third, &first, &second, &voided] {
        store::insert_question_card(connection, question).unwrap();
    }
    let options = BuildOptions {
        total_time_secs: Some(120),
        ..BuildOptions::default()
    };
    let mut session = build_full_exam(connection, &exam_set.id, &options).unwrap();
    assert_eq!(
        session
            .questions()
            .iter()
            .map(|q| q.id.clone())
            .collect::<Vec<_>>(),
        vec![
            first.id.clone(),
            second.id.clone(),
            third.id.clone(),
            voided.id.clone()
        ]
    );

    session.start(base());
    for keys in [vec!["A"], vec!["A"], vec!["A"], vec!["A"]] {
        let outcome = session
            .submit_answer(
                QuizAnswer {
                    submission: AnswerSubmission::Keys {
                        values: keys.into_iter().map(str::to_owned).collect(),
                    },
                    confidence: Confidence::Certain,
                    error_tags: vec![],
                },
                base(),
            )
            .unwrap();
        assert!(outcome.is_none());
    }
    assert!(session.graded().is_empty());
    assert!(session.attempts(&user.id, 1).is_empty());
    let graded = session.finish(base()).unwrap();
    assert_eq!(graded.len(), 4);
    assert_eq!(session.state(), SessionState::Submitted);
    assert!(graded[3].outcome.voided);
    let score = session.score().unwrap();
    assert_eq!((score.correct, score.scoreable, score.voided), (2, 3, 1));
    assert!((score.ratio - 2.0 / 3.0).abs() < f64::EPSILON);
}

#[test]
fn full_exam_total_timer_expires() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let exam_set = ExamSet {
        id: ExamSetId::generate(),
        subject: "biology".to_owned(),
        title: "short".to_owned(),
        source_kind: SourceKind::Custom,
        exam_year: None,
        source_resource_id: None,
        expected_questions: None,
        metadata: json!({}),
        is_complete: false,
    };
    store::insert_exam_set(connection, &exam_set).unwrap();
    let question = QuestionSpec::new(
        "timed",
        QuestionType::SingleChoice,
        GradingMode::Exact,
        vec!["A".to_owned()],
    )
    .exam(exam_set.id.clone(), 1)
    .options(vec![option("A")])
    .build();
    store::insert_question_card(connection, &question).unwrap();
    let options = BuildOptions {
        total_time_secs: Some(10),
        ..BuildOptions::default()
    };
    let mut session = build_full_exam(connection, &exam_set.id, &options).unwrap();
    session.start(base());
    assert_eq!(
        session.tick(at(11)).unwrap(),
        TickOutcome::SessionExpired { index: 0 }
    );
    assert_eq!(session.state(), SessionState::Expired { index: 0 });
}

#[test]
fn topic_drill_follows_question_and_option_tests_edges() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let direct = knowledge("direct");
    let via_option = knowledge("option");
    let question = choice("linked", "A");
    let unrelated = choice("unrelated", "A");
    for card in [&direct, &via_option] {
        store::insert_knowledge_card(connection, card).unwrap();
    }
    for item in [&question, &unrelated] {
        store::insert_question_card(connection, item).unwrap();
    }
    store::insert_edge(
        connection,
        &tests_edge(
            EdgeEndpoint::Question {
                id: question.id.clone(),
            },
            direct.id.clone(),
        ),
    )
    .unwrap();
    store::insert_edge(
        connection,
        &tests_edge(
            EdgeEndpoint::Option {
                id: question.id.clone(),
                key: "A".to_owned(),
            },
            via_option.id.clone(),
        ),
    )
    .unwrap();

    let options = BuildOptions::default();
    let from_question =
        build_topic_drill(connection, std::slice::from_ref(&direct.id), &options).unwrap();
    let ids: HashSet<QuestionCardId> = from_question
        .questions()
        .iter()
        .map(|q| q.id.clone())
        .collect();
    assert_eq!(ids, HashSet::from([question.id.clone()]));
    let from_option = build_topic_drill(connection, &[via_option.id], &options).unwrap();
    assert_eq!(from_option.questions()[0].id, question.id);
}

#[test]
fn practical_exam_filters_question_types_and_pauses_countdown() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let identify = QuestionSpec::new(
        "identify",
        QuestionType::PinIdentify,
        GradingMode::Exact,
        vec!["1".to_owned()],
    )
    .stem(pin_stem(pin("Na/K ATPase", Some("鈉幫浦"))))
    .build();
    let locate = QuestionSpec::new(
        "locate",
        QuestionType::PinLocate,
        GradingMode::Exact,
        vec!["1".to_owned()],
    )
    .stem(pin_stem(pin("target", None)))
    .build();
    let image = QuestionSpec::new(
        "image",
        QuestionType::ImageQa,
        GradingMode::Exact,
        vec!["A".to_owned()],
    )
    .options(vec![option("A")])
    .build();
    let ordinary = choice("ordinary", "A");
    let ids = [
        identify.id.clone(),
        locate.id.clone(),
        image.id.clone(),
        ordinary.id.clone(),
    ];
    for question in [&identify, &locate, &image, &ordinary] {
        store::insert_question_card(connection, question).unwrap();
    }
    let options = BuildOptions::default();
    assert_eq!(options.seconds_per_question, 30);
    let mut session = build_practical_exam(connection, &ids, &options).unwrap();
    assert_eq!(session.questions().len(), 3);
    assert!(!session.questions().iter().any(|q| q.id == ordinary.id));

    let mut invalid = options.clone();
    invalid.seconds_per_question = 0;
    assert!(build_practical_exam(connection, &ids, &invalid).is_err());
    invalid.seconds_per_question = 301;
    assert!(build_practical_exam(connection, &ids, &invalid).is_err());

    session.start(base());
    session.pause(at(10)).unwrap();
    session.resume(at(40)).unwrap();
    match session.tick(at(70)).unwrap() {
        TickOutcome::QuestionTimedOut { index, graded } => {
            assert_eq!(index, 0);
            assert!(graded.record.timed_out);
            assert_eq!(graded.record.duration_ms, 30_000);
        }
        other => panic!("unexpected tick outcome: {other:?}"),
    }
    assert_eq!(session.state(), SessionState::InProgress { index: 1 });
    assert_eq!(session.graded().len(), 1);
}

#[test]
fn weakness_drill_ranks_slow_hesitant_errors_first() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let user = insert_user(connection);
    let weak = choice("weak", "A");
    let strong = choice("strong", "A");
    for question in [&weak, &strong] {
        store::insert_question_card(connection, question).unwrap();
    }
    let logs = [
        attempt(
            &user,
            CardId::from(weak.id.clone()),
            false,
            Confidence::Guessed,
            60_000,
            base(),
            json!({"answer": {"kind": "keys", "values": ["B"]}}),
        ),
        attempt(
            &user,
            CardId::from(strong.id.clone()),
            true,
            Confidence::Certain,
            500,
            base(),
            json!({"answer": {"kind": "keys", "values": ["A"]}}),
        ),
    ];
    for log in &logs {
        study_store::append_attempt(connection, log).unwrap();
    }

    let session = build_weakness_drill(connection, &user.id, &BuildOptions::default()).unwrap();
    assert_eq!(session.questions()[0].id, weak.id);
    assert_eq!(session.questions()[1].id, strong.id);
    assert_eq!(session.state(), SessionState::NotStarted);
}

#[test]
fn accepted_answers_are_order_independent_and_exact_stays_strict() {
    let disputed = QuestionSpec::new(
        "disputed",
        QuestionType::SingleChoice,
        GradingMode::AnyAccepted,
        vec!["A".to_owned()],
    )
    .accepted(vec![
        vec!["B".to_owned()],
        vec!["A".to_owned(), "C".to_owned()],
    ])
    .options(vec![option("A"), option("B"), option("C")])
    .build();
    let config = GradingConfig::default();
    for keys in [vec!["A"], vec!["B"], vec!["C", "A"]] {
        let outcome = grade_question(
            &disputed,
            &AnswerSubmission::Keys {
                values: keys.iter().map(|key| (*key).to_owned()).collect(),
            },
            &config,
            true,
        )
        .unwrap();
        assert!(outcome.is_correct, "{keys:?} should be accepted");
    }
    let wrong = grade_question(
        &disputed,
        &AnswerSubmission::Keys {
            values: vec!["A".to_owned(), "B".to_owned()],
        },
        &config,
        true,
    )
    .unwrap();
    assert!(!wrong.is_correct);
    let exact = choice("exact", "A");
    let outcome = grade_question(
        &exact,
        &AnswerSubmission::Keys {
            values: vec!["B".to_owned()],
        },
        &config,
        true,
    )
    .unwrap();
    assert!(!outcome.is_correct);
}

#[test]
fn pin_terms_normalize_case_space_and_full_width_characters() {
    assert_eq!(normalize_term(" Ｎａ／Ｋ　ＡＴＰase "), "na/k atpase");
    let question = QuestionSpec::new(
        "identify",
        QuestionType::PinIdentify,
        GradingMode::Exact,
        vec!["1".to_owned()],
    )
    .stem(pin_stem(pin("Na/K ATPase", Some("鈉幫浦"))))
    .build();
    let config = GradingConfig::default();
    let full_width = grade_question(
        &question,
        &AnswerSubmission::Text {
            value: " Ｎａ／Ｋ　ＡＴＰase ".to_owned(),
        },
        &config,
        true,
    )
    .unwrap();
    assert!(full_width.is_correct);
    let secondary = grade_question(
        &question,
        &AnswerSubmission::Text {
            value: "鈉幫浦".to_owned(),
        },
        &config,
        true,
    )
    .unwrap();
    assert!(secondary.is_correct);
    let wrong = grade_question(
        &question,
        &AnswerSubmission::Text {
            value: "wrong".to_owned(),
        },
        &config,
        true,
    )
    .unwrap();
    assert!(!wrong.is_correct);
}

#[test]
fn pin_locate_distance_accounts_for_aspect_ratio() {
    let question = QuestionSpec::new(
        "locate",
        QuestionType::PinLocate,
        GradingMode::Exact,
        vec!["1".to_owned()],
    )
    .stem(pin_stem(pin("target", None)))
    .build();
    let click = |x: f64| AnswerSubmission::PinClicks {
        clicks: vec![PinClick {
            no: 1,
            point: NormalizedPoint { x, y: 0.5 },
        }],
    };
    let wide = GradingConfig {
        pin_radius: 0.1,
        pin_aspect_ratio: Some(2.0),
    };
    assert!(
        grade_question(&question, &click(0.54), &wide, true)
            .unwrap()
            .is_correct
    );
    assert!(
        !grade_question(&question, &click(0.60), &wide, true)
            .unwrap()
            .is_correct
    );
    let square = GradingConfig {
        pin_radius: 0.1,
        pin_aspect_ratio: None,
    };
    assert!(
        grade_question(&question, &click(0.60), &square, true)
            .unwrap()
            .is_correct
    );
    assert!(
        !grade_question(&question, &click(1.20), &square, true)
            .unwrap()
            .is_correct
    );
}

#[test]
fn voided_and_practice_attempts_are_kept_but_do_not_update_fsrs() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let user = insert_user(connection);
    let voided = QuestionSpec::new(
        "voided",
        QuestionType::SingleChoice,
        GradingMode::Voided,
        vec![],
    )
    .options(vec![option("A")])
    .build();
    store::insert_question_card(connection, &voided).unwrap();

    let mut session = reinforce_core::quiz::QuizSession::new(
        SessionMode::DailyDue,
        vec![voided.clone()],
        None,
        None,
        GradingConfig::default(),
        true,
        false,
    )
    .unwrap();
    session.start(base());
    let outcome = session
        .submit_answer(
            QuizAnswer {
                submission: AnswerSubmission::Keys {
                    values: vec!["A".to_owned()],
                },
                confidence: Confidence::Certain,
                error_tags: vec![],
            },
            base(),
        )
        .unwrap()
        .unwrap();
    assert!(outcome.voided);
    assert!(!outcome.scoreable);
    assert!(!outcome.affects_scheduling);
    let attempts = session.attempts(&user.id, 1);
    assert_eq!(attempts.len(), 1);
    assert!(!attempts[0].affects_scheduling);
    assert!(is_voided_attempt(&attempts[0]));
    study_store::append_attempt(connection, &attempts[0]).unwrap();
    let replayed = study_store::list_attempts(
        connection,
        &user.id,
        &CardId::from(voided.id.clone()),
        AttemptDirection::Default,
    )
    .unwrap();
    assert_eq!(replayed.len(), 1);
    let scheduling = recompute_scheduling(
        user.id.clone(),
        CardId::from(voided.id.clone()),
        AttemptDirection::Default,
        &replayed,
        0.9,
    )
    .unwrap();
    assert_eq!(scheduling.reps, 0);
    let mut forced_voided = attempt(
        &user,
        CardId::from(voided.id.clone()),
        true,
        Confidence::Certain,
        1_000,
        base(),
        json!({"grading_mode": "voided"}),
    );
    forced_voided.affects_scheduling = true;
    let forced = recompute_scheduling(
        user.id.clone(),
        CardId::from(voided.id.clone()),
        AttemptDirection::Default,
        &[forced_voided],
        0.9,
    )
    .unwrap();
    assert_eq!(forced.reps, 0);

    let practice = choice("practice", "A");
    let mut practice_session = reinforce_core::quiz::QuizSession::new(
        SessionMode::DailyDue,
        vec![practice.clone()],
        None,
        None,
        GradingConfig::default(),
        false,
        false,
    )
    .unwrap();
    practice_session.start(base());
    practice_session
        .submit_answer(
            QuizAnswer {
                submission: AnswerSubmission::Keys {
                    values: vec!["A".to_owned()],
                },
                confidence: Confidence::Certain,
                error_tags: vec![],
            },
            base(),
        )
        .unwrap();
    let practice_attempts = practice_session.attempts(&user.id, 1);
    assert!(!practice_attempts[0].affects_scheduling);
    let mut with_practice = vec![attempt(
        &user,
        CardId::from(practice.id.clone()),
        true,
        Confidence::Certain,
        1_000,
        at(-1),
        json!({"answer": "A"}),
    )];
    with_practice.extend(practice_attempts);
    let derived = recompute_scheduling(
        user.id,
        CardId::from(practice.id),
        AttemptDirection::Default,
        &with_practice,
        0.9,
    )
    .unwrap();
    assert_eq!(derived.reps, 1);
}

#[test]
fn weakness_report_serializes_knowledge_question_and_option_pairs() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let user = insert_user(connection);
    let card = knowledge("transport");
    let question = QuestionSpec::new(
        "question",
        QuestionType::SingleChoice,
        GradingMode::Exact,
        vec!["B".to_owned()],
    )
    .options(vec![option("A"), option("B")])
    .build();
    store::insert_knowledge_card(connection, &card).unwrap();
    store::insert_question_card(connection, &question).unwrap();
    store::insert_edge(
        connection,
        &tests_edge(
            EdgeEndpoint::Question {
                id: question.id.clone(),
            },
            card.id.clone(),
        ),
    )
    .unwrap();
    let logs = [
        attempt(
            &user,
            CardId::from(question.id.clone()),
            false,
            Confidence::Guessed,
            30_000,
            base(),
            json!({"answer": {"kind": "keys", "values": ["A"]}}),
        ),
        attempt(
            &user,
            CardId::from(question.id.clone()),
            true,
            Confidence::Certain,
            1_000,
            at(1),
            json!({"answer": {"kind": "keys", "values": ["B"]}}),
        ),
    ];
    for log in &logs {
        study_store::append_attempt(connection, log).unwrap();
    }

    let report = build_report(connection, &user.id, 10).unwrap();
    assert_eq!(report.questions[0].question_id, question.id);
    assert!((report.questions[0].error_rate - 0.5).abs() < f64::EPSILON);
    assert!((report.questions[0].hesitant_rate - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.questions[0].average_duration_ms, 15_500.0);
    assert_eq!(report.knowledge_cards[0].card_id, card.id);
    assert_eq!(report.knowledge_cards[0].attempts, 2);
    assert_eq!(report.confusable_pairs[0].wrong_option, "A");
    assert_eq!(report.confusable_pairs[0].correct_option, "B");
    assert_eq!(report.confusable_pairs[0].count, 1);
    assert!(
        serde_json::to_string(&report)
            .unwrap()
            .contains("confusable_pairs")
    );
}

#[test]
fn short_answers_ignore_case_space_and_full_width_forms() {
    let question = QuestionSpec::new(
        "organelle",
        QuestionType::ShortAnswer,
        GradingMode::AnyAccepted,
        vec!["Mitochondria".to_owned()],
    )
    .accepted(vec![vec!["粒線體".to_owned()]])
    .build();
    let config = GradingConfig::default();
    let grade = |value: &str| {
        grade_question(
            &question,
            &AnswerSubmission::Text {
                value: value.to_owned(),
            },
            &config,
            true,
        )
        .unwrap()
        .is_correct
    };

    assert!(grade("mitochondria"));
    assert!(grade("  MITOCHONDRIA "));
    assert!(grade("ｍｉｔｏｃｈｏｎｄｒｉａ"));
    assert!(grade("粒線體"));
    assert!(!grade("ribosome"));
    assert!(!grade("   "));
}

#[test]
fn submitted_error_tags_reach_attempt_logs() {
    let question = choice("tagged", "A");
    let mut session = reinforce_core::quiz::QuizSession::new(
        SessionMode::DailyDue,
        vec![question],
        None,
        None,
        GradingConfig::default(),
        false,
        false,
    )
    .unwrap();
    session.start(base());
    session
        .submit_answer(
            QuizAnswer {
                submission: AnswerSubmission::Keys {
                    values: vec!["B".to_owned()],
                },
                confidence: Confidence::Split,
                error_tags: vec!["概念混淆".to_owned(), "incorrect".to_owned()],
            },
            base(),
        )
        .unwrap();

    let attempts = session.attempts(&UserId::generate(), 1);
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].error_tags,
        vec!["incorrect".to_owned(), "概念混淆".to_owned()]
    );
}
