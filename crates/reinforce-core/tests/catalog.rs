use reinforce_core::content::{ContentBlock, OutlineNode, QuestionOption};
use reinforce_core::db::catalog as catalog_store;
use reinforce_core::db::content as store;
use reinforce_core::primitives::{
    AnswerSourceKind, CardStatus, GradingMode, QuestionType, SourceKind, Verification,
};
use reinforce_core::{
    Database, ExamSet, KnowledgeCard, Note, QuestionCard, QuestionGroup, Timestamp,
};
use serde_json::json;

fn knowledge_card(subject: &str, title: &str, status: CardStatus) -> KnowledgeCard {
    let now = Timestamp::now();
    KnowledgeCard {
        id: reinforce_core::KnowledgeCardId::generate(),
        subject: subject.to_owned(),
        topics: vec!["topic".to_owned()],
        title: title.to_owned(),
        blocks: vec![ContentBlock::Markdown {
            text: format!("{title} body"),
        }],
        anchor_ids: vec![],
        status,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn note(subject: &str, title: &str, outline: Vec<OutlineNode>, status: CardStatus) -> Note {
    let now = Timestamp::now();
    Note {
        id: reinforce_core::NoteId::generate(),
        subject: subject.to_owned(),
        title: title.to_owned(),
        outline,
        view_settings: json!({}),
        status,
        created_at: now,
        updated_at: now,
    }
}

fn exam_set(subject: &str, title: &str) -> ExamSet {
    ExamSet {
        id: reinforce_core::ExamSetId::generate(),
        subject: subject.to_owned(),
        title: title.to_owned(),
        source_kind: SourceKind::Original,
        exam_year: None,
        source_resource_id: None,
        expected_questions: None,
        metadata: json!({}),
        is_complete: true,
    }
}

fn question_group(exam_set_id: reinforce_core::ExamSetId, title: &str) -> QuestionGroup {
    QuestionGroup {
        id: reinforce_core::QuestionGroupId::generate(),
        exam_set_id: Some(exam_set_id),
        title: Some(title.to_owned()),
        shared_stem_blocks: vec![ContentBlock::Markdown {
            text: format!("{title} vignette"),
        }],
    }
}

fn question_card(
    exam_set_id: Option<reinforce_core::ExamSetId>,
    question_group_id: Option<reinforce_core::QuestionGroupId>,
    question_no: Option<u32>,
    subject: &str,
    status: CardStatus,
) -> QuestionCard {
    let now = Timestamp::now();
    QuestionCard {
        id: reinforce_core::QuestionCardId::generate(),
        exam_set_id,
        question_group_id,
        question_no,
        subject: subject.to_owned(),
        topics: vec![],
        stem_blocks: vec![ContentBlock::Markdown {
            text: "stem".to_owned(),
        }],
        explanation_blocks: vec![],
        anchor_ids: vec![],
        question_type: QuestionType::SingleChoice,
        grading_mode: GradingMode::Exact,
        options: vec![QuestionOption {
            key: "A".to_owned(),
            content: "yes".to_owned(),
            explanation: None,
            linked_card_ids: vec![],
        }],
        answer: vec!["A".to_owned()],
        accepted_answers: vec![],
        answer_source_kind: AnswerSourceKind::Official,
        answer_basis: None,
        duplicate_question_id: None,
        status,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

struct Fixture {
    database: Database,
    note_published: Note,
    note_draft: Note,
    card_published: KnowledgeCard,
    card_draft: KnowledgeCard,
    exam_set: ExamSet,
    group: QuestionGroup,
}

/// Builds a two-subject store: physics owns published and draft content plus a
/// scored exam set, chemistry owns a single published card.
fn fixture() -> Fixture {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();

    let card_published = knowledge_card("physics", "published card", CardStatus::Published);
    let card_draft = knowledge_card("physics", "draft card", CardStatus::Draft);
    let chemistry_card = knowledge_card("chemistry", "molecule basics", CardStatus::Published);
    for card in [&card_published, &card_draft, &chemistry_card] {
        store::insert_knowledge_card(connection, card).unwrap();
    }

    let midterm = exam_set("physics", "midterm");
    store::insert_exam_set(connection, &midterm).unwrap();
    let group = question_group(midterm.id.clone(), "shared passage");
    store::insert_question_group(connection, &group).unwrap();
    let other_set = exam_set("physics", "final");
    store::insert_exam_set(connection, &other_set).unwrap();
    let other_group = question_group(other_set.id.clone(), "unrelated passage");
    store::insert_question_group(connection, &other_group).unwrap();
    let first = question_card(
        Some(midterm.id.clone()),
        Some(group.id.clone()),
        Some(2),
        "physics",
        CardStatus::Published,
    );
    let second = question_card(
        Some(midterm.id.clone()),
        None,
        Some(1),
        "physics",
        CardStatus::Published,
    );
    let third = question_card(
        Some(midterm.id.clone()),
        None,
        Some(3),
        "physics",
        CardStatus::Published,
    );
    let draft = question_card(
        Some(midterm.id.clone()),
        None,
        Some(4),
        "physics",
        CardStatus::Draft,
    );
    let unnumbered = question_card(
        Some(midterm.id.clone()),
        None,
        None,
        "physics",
        CardStatus::Published,
    );
    let outside = question_card(None, None, Some(9), "physics", CardStatus::Published);
    for card in [&first, &second, &third, &draft, &unnumbered, &outside] {
        store::insert_question_card(connection, card).unwrap();
    }

    let missing_card = reinforce_core::KnowledgeCardId::generate();
    let outline = vec![
        OutlineNode::Section {
            heading: "Warm up".to_owned(),
            intro_markdown: Some("start here".to_owned()),
            children: vec![
                OutlineNode::CardRef {
                    card_id: card_published.id.clone(),
                },
                OutlineNode::Section {
                    heading: "Deeper".to_owned(),
                    intro_markdown: None,
                    children: vec![
                        OutlineNode::CardRef {
                            card_id: card_draft.id.clone(),
                        },
                        OutlineNode::CardRef {
                            card_id: card_published.id.clone(),
                        },
                    ],
                },
            ],
        },
        OutlineNode::CardRef {
            card_id: missing_card,
        },
    ];
    let note_published = note("physics", "phase changes", outline, CardStatus::Published);
    store::insert_note(connection, &note_published).unwrap();
    let note_draft = note(
        "physics",
        "draft note",
        vec![OutlineNode::CardRef {
            card_id: card_published.id.clone(),
        }],
        CardStatus::Draft,
    );
    store::insert_note(connection, &note_draft).unwrap();

    Fixture {
        database,
        note_published,
        note_draft,
        card_published,
        card_draft,
        exam_set: midterm,
        group,
    }
}

#[test]
fn subjects_aggregate_published_counts_across_content_tables() {
    let fixture = fixture();
    let subjects = catalog_store::list_subjects(fixture.database.connection()).unwrap();
    let names: Vec<&str> = subjects
        .iter()
        .map(|summary| summary.subject.as_str())
        .collect();
    assert_eq!(names, vec!["chemistry", "physics"]);

    let physics = &subjects[1];
    assert_eq!(physics.note_count, 1, "only the published note counts");
    assert_eq!(physics.knowledge_card_count, 1);
    assert_eq!(physics.exam_set_count, 2);
    assert_eq!(physics.question_card_count, 5);

    let chemistry = &subjects[0];
    assert_eq!(chemistry.note_count, 0);
    assert_eq!(chemistry.knowledge_card_count, 1);
    assert_eq!(chemistry.exam_set_count, 0);
    assert_eq!(chemistry.question_card_count, 0);
}

#[test]
fn notes_and_cards_filter_by_subject_status_and_limit() {
    let fixture = fixture();
    let connection = fixture.database.connection();

    let published =
        catalog_store::list_notes(connection, None, Some(CardStatus::Published), 50).unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].id, fixture.note_published.id);

    assert!(
        !published
            .iter()
            .any(|note| note.id == fixture.note_draft.id)
    );
    let every_note = catalog_store::list_notes(connection, None, None, 50).unwrap();
    assert_eq!(every_note.len(), 2);
    assert!(
        every_note
            .iter()
            .any(|note| note.id == fixture.note_draft.id)
    );
    assert_eq!(
        catalog_store::list_notes(connection, Some("chemistry"), None, 50)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        catalog_store::list_notes(connection, Some("physics"), None, 1)
            .unwrap()
            .len(),
        1
    );

    let cards = catalog_store::list_knowledge_cards(connection, Some("physics"), None, 50).unwrap();
    assert_eq!(cards.len(), 2);
    let published_cards = catalog_store::list_knowledge_cards(
        connection,
        Some("physics"),
        Some(CardStatus::Published),
        50,
    )
    .unwrap();
    assert_eq!(published_cards.len(), 1);
    assert_eq!(published_cards[0].id, fixture.card_published.id);
    assert_eq!(
        catalog_store::list_knowledge_cards(connection, Some("biology"), None, 50)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn exam_sets_report_question_bank_sizes() {
    let fixture = fixture();
    let connection = fixture.database.connection();

    let summaries = catalog_store::list_exam_sets(connection, Some("physics"), 50).unwrap();
    assert_eq!(summaries.len(), 2);
    let midterm = summaries
        .iter()
        .find(|summary| summary.exam_set.id == fixture.exam_set.id)
        .expect("the scored set is listed");
    assert_eq!(midterm.question_count, 5);
    assert_eq!(midterm.published_question_count, 4);
    assert_eq!(midterm.exam_set.title, "midterm");

    let final_round = summaries
        .iter()
        .find(|summary| summary.exam_set.id != fixture.exam_set.id)
        .expect("the empty set is listed");
    assert_eq!(final_round.question_count, 0);

    assert!(
        catalog_store::list_exam_sets(connection, Some("chemistry"), 50)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        catalog_store::list_exam_sets(connection, None, 1)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn note_outline_resolves_cards_in_order_and_reports_missing_references() {
    let fixture = fixture();
    let connection = fixture.database.connection();

    let outline = catalog_store::note_outline(connection, &fixture.note_published.id)
        .unwrap()
        .expect("the note exists");
    let ids: Vec<&reinforce_core::KnowledgeCardId> =
        outline.cards.iter().map(|card| &card.id).collect();
    assert_eq!(
        ids,
        vec![&fixture.card_published.id, &fixture.card_draft.id],
        "cards keep first-appearance order and appear once"
    );
    assert_eq!(outline.missing_card_ids.len(), 1);
    assert_eq!(outline.note.title, "phase changes");

    let missing = reinforce_core::NoteId::generate();
    assert!(
        catalog_store::note_outline(connection, &missing)
            .unwrap()
            .is_none()
    );
}

#[test]
fn outline_card_ids_walks_nested_sections() {
    let first = reinforce_core::KnowledgeCardId::generate();
    let second = reinforce_core::KnowledgeCardId::generate();
    let outline = vec![
        OutlineNode::CardRef {
            card_id: first.clone(),
        },
        OutlineNode::Section {
            heading: "section".to_owned(),
            intro_markdown: None,
            children: vec![
                OutlineNode::CardRef {
                    card_id: second.clone(),
                },
                OutlineNode::CardRef {
                    card_id: first.clone(),
                },
            ],
        },
    ];
    let ids = catalog_store::outline_card_ids(&outline);
    assert_eq!(ids, vec![first, second]);
}

#[test]
fn exam_set_questions_group_and_order_by_number() {
    let fixture = fixture();
    let connection = fixture.database.connection();

    let groups = catalog_store::list_question_groups(connection, &fixture.exam_set.id, 50).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, fixture.group.id);

    let cards =
        catalog_store::list_question_cards(connection, &fixture.exam_set.id, None, 50).unwrap();
    let numbers: Vec<Option<u32>> = cards.iter().map(|card| card.question_no).collect();
    assert_eq!(numbers, vec![Some(1), Some(2), Some(3), Some(4), None]);

    let published = catalog_store::list_question_cards(
        connection,
        &fixture.exam_set.id,
        Some(CardStatus::Published),
        50,
    )
    .unwrap();
    assert_eq!(published.len(), 4);
    assert!(
        published
            .iter()
            .all(|card| card.status == CardStatus::Published)
    );

    let limited =
        catalog_store::list_question_cards(connection, &fixture.exam_set.id, None, 2).unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].question_no, Some(1));

    let empty = reinforce_core::ExamSetId::generate();
    assert!(
        catalog_store::list_question_cards(connection, &empty, None, 50)
            .unwrap()
            .is_empty()
    );
}
