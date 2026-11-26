use reinforce_core::content::{
    ContentBlock, NormalizedRect, OcclusionMask, OutlineNode, QuestionOption,
};
use reinforce_core::content_store;
use reinforce_core::primitives::{
    AnchorKind, AnswerSourceKind, CardStatus, EdgeType, EndpointKind, ExportPolicy, GradingMode,
    QuestionType, ResourceKind, Sensitivity, SourceKind, Verification,
};
use reinforce_core::validation::{
    Actor, Snapshot, Staged, StoreView, ValidationError, ValidationReport, ValidationRule,
    validate_snapshot,
};
use reinforce_core::{
    Anchor, ContentHash, Database, Edge, EdgeEndpoint, ExamSet, KnowledgeCard, Note, QuestionCard,
    QuestionCardId, Resource, Timestamp, UserId,
};
use serde_json::json;

fn stage<T>(op_index: usize, value: T) -> Staged<T> {
    Staged {
        op_index,
        temp_ref: Some(format!("$item_{op_index}")),
        value,
    }
}

fn resource(tag: &str, dimensions: Option<(i64, i64)>) -> Resource {
    Resource {
        id: reinforce_core::ResourceId::generate(),
        kind: ResourceKind::Image,
        title: tag.to_owned(),
        sha256: ContentHash::from_bytes(tag.as_bytes()),
        byte_size: 100,
        page_count: None,
        width: dimensions.map(|value| value.0),
        height: dimensions.map(|value| value.1),
        license: None,
        attribution_url: None,
        attribution_notice: None,
        sensitivity: Sensitivity::Normal,
        export_policy: ExportPolicy::Allow,
        created_at: Timestamp::now(),
    }
}

fn anchor(resource_id: &reinforce_core::ResourceId) -> Anchor {
    Anchor {
        id: reinforce_core::AnchorId::generate(),
        resource_id: resource_id.clone(),
        anchor_kind: AnchorKind::Rect,
        pdf_page: None,
        printed_page: None,
        legend_pdf_page: None,
        rect: Some(NormalizedRect {
            x: 0.0,
            y: 0.0,
            w: 0.5,
            h: 0.5,
        }),
        media_span: None,
    }
}

fn knowledge_card(title: &str) -> KnowledgeCard {
    let now = Timestamp::now();
    KnowledgeCard {
        id: reinforce_core::KnowledgeCardId::generate(),
        subject: "biology".to_owned(),
        topics: vec![],
        title: title.to_owned(),
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

fn question_card() -> QuestionCard {
    let now = Timestamp::now();
    QuestionCard {
        id: reinforce_core::QuestionCardId::generate(),
        exam_set_id: None,
        question_group_id: None,
        question_no: None,
        subject: "biology".to_owned(),
        topics: vec![],
        stem_blocks: vec![],
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
        status: CardStatus::Draft,
        verification: Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn exam_set(source_kind: SourceKind, expected: Option<u32>, complete: bool) -> ExamSet {
    ExamSet {
        id: reinforce_core::ExamSetId::generate(),
        subject: "biology".to_owned(),
        title: "exam".to_owned(),
        source_kind,
        exam_year: None,
        source_resource_id: None,
        expected_questions: expected,
        metadata: json!({}),
        is_complete: complete,
    }
}

fn field_paths(errors: &[ValidationError], rule: ValidationRule) -> Vec<String> {
    errors
        .iter()
        .filter(|error| error.rule == rule)
        .map(|error| error.field_path.clone())
        .collect()
}

fn assert_json_shape(report: &ValidationReport) {
    let json: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    for error in json["errors"].as_array().unwrap() {
        for field in ["op_index", "temp_ref", "field_path", "rule", "message"] {
            assert!(error.get(field).is_some(), "missing {field} in {error}");
        }
    }
}

#[test]
fn answer_provenance_reports_every_violation_in_one_pass() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);

    let mut unanswered = question_card();
    unanswered.answer_source_kind = AnswerSourceKind::Unanswered;
    snapshot.push_question_card(stage(0, unanswered));

    let mut inferred = question_card();
    inferred.answer_source_kind = AnswerSourceKind::Inferred;
    inferred.answer_basis = None;
    snapshot.push_question_card(stage(1, inferred));

    let mut authored = question_card();
    authored.answer_source_kind = AnswerSourceKind::OriginalAuthored;
    snapshot.push_question_card(stage(2, authored));

    let mut duplicate = question_card();
    duplicate.answer_source_kind = AnswerSourceKind::DuplicateQuestion;
    duplicate.answer_basis = Some("same stem".to_owned());
    snapshot.push_question_card(stage(3, duplicate));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::AnswerProvenance);
    for expected in [
        "answer_source_kind",
        "answer_basis",
        "anchor_ids",
        "duplicate_question_id",
    ] {
        assert!(
            fields.iter().any(|field| field == expected),
            "missing {expected} in {fields:?}"
        );
    }
    assert_json_shape(&report);
}

#[test]
fn inferred_anchor_and_duplicate_reference_resolve_inside_the_snapshot() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);

    let base = resource("base", Some((1200, 800)));
    let base_id = base.id.clone();
    let point = anchor(&base_id);
    let anchor_id = point.id.clone();
    snapshot.push_resource(stage(0, base));
    snapshot.push_anchor(stage(1, point));

    let referenced = question_card();
    let referenced_id = referenced.id.clone();
    snapshot.push_question_card(stage(2, referenced));

    let mut inferred = question_card();
    inferred.answer_source_kind = AnswerSourceKind::Inferred;
    inferred.answer_basis = Some("anchor shows the pump".to_owned());
    inferred.anchor_ids.push(anchor_id);
    snapshot.push_question_card(stage(3, inferred));

    let mut duplicate = question_card();
    duplicate.answer_source_kind = AnswerSourceKind::DuplicateQuestion;
    duplicate.answer_basis = Some("same answer as prior exam".to_owned());
    duplicate.duplicate_question_id = Some(referenced_id);
    snapshot.push_question_card(stage(4, duplicate));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    assert!(report.is_ok(), "unexpected errors: {:?}", report.errors);
}

#[test]
fn original_anti_fabrication_flags_year_past_exam_and_question_number() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);

    let mut original = exam_set(SourceKind::Original, None, false);
    original.exam_year = Some(2026);
    snapshot.push_exam_set(stage(0, original));

    let past = exam_set(SourceKind::PastExam, None, false);
    let past_id = past.id.clone();
    snapshot.push_exam_set(stage(1, past));

    let mut fabricated = question_card();
    fabricated.question_type = QuestionType::ShortAnswer;
    fabricated.answer.clear();
    fabricated.answer_source_kind = AnswerSourceKind::OriginalAuthored;
    fabricated.exam_set_id = Some(past_id.clone());
    fabricated.question_no = Some(1);
    snapshot.push_question_card(stage(2, fabricated));

    let mut legitimate = question_card();
    legitimate.exam_set_id = Some(past_id);
    legitimate.question_no = Some(1);
    snapshot.push_question_card(stage(3, legitimate));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::OriginalAntiFabrication);
    assert!(fields.contains(&"exam_year".to_owned()), "{fields:?}");
    assert!(fields.contains(&"exam_set_id".to_owned()), "{fields:?}");
    assert!(fields.contains(&"question_no".to_owned()), "{fields:?}");
}

#[test]
fn grading_consistency_checks_keys_modes_and_voided_items() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);

    let mut multi_key = question_card();
    multi_key.answer = vec!["A".to_owned(), "B".to_owned()];
    multi_key.options.push(QuestionOption {
        key: "B".to_owned(),
        content: "no".to_owned(),
        explanation: None,
        linked_card_ids: vec![],
    });
    snapshot.push_question_card(stage(0, multi_key));

    let mut unknown_key = question_card();
    unknown_key.answer = vec!["C".to_owned()];
    snapshot.push_question_card(stage(1, unknown_key));

    let mut alternative = question_card();
    alternative.grading_mode = GradingMode::AnyAccepted;
    alternative.accepted_answers = vec![vec!["D".to_owned()]];
    snapshot.push_question_card(stage(2, alternative));

    let mut exact_with_alternatives = question_card();
    exact_with_alternatives.accepted_answers = vec![vec!["A".to_owned()]];
    snapshot.push_question_card(stage(3, exact_with_alternatives));

    let mut voided = question_card();
    voided.grading_mode = GradingMode::Voided;
    voided.answer_source_kind = AnswerSourceKind::Official;
    snapshot.push_question_card(stage(4, voided));

    let mut valid_alternative = question_card();
    valid_alternative.grading_mode = GradingMode::AnyAccepted;
    valid_alternative.options.push(QuestionOption {
        key: "B".to_owned(),
        content: "no".to_owned(),
        explanation: None,
        linked_card_ids: vec![],
    });
    valid_alternative.accepted_answers = vec![vec!["B".to_owned()]];
    snapshot.push_question_card(stage(5, valid_alternative));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::TypeGradingConsistency);
    for expected in [
        "answer",
        "answer[0]",
        "accepted_answers[0][0]",
        "accepted_answers",
        "answer_source_kind",
    ] {
        assert!(
            fields.iter().any(|field| field == expected),
            "missing {expected} in {fields:?}"
        );
    }
    assert!(
        !report.errors.iter().any(|error| error.op_index == Some(5)),
        "the valid any_accepted item must not fail: {:?}",
        report.errors
    );
}

#[test]
fn count_conservation_requires_contiguous_unique_placeholders() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);
    let set = exam_set(SourceKind::PastExam, Some(3), true);
    let set_id = set.id.clone();
    snapshot.push_exam_set(stage(0, set));

    for (index, question_no) in [Some(1), Some(2), Some(2), Some(5), None]
        .into_iter()
        .enumerate()
    {
        let mut card = question_card();
        card.question_type = QuestionType::ShortAnswer;
        card.grading_mode = GradingMode::Voided;
        card.answer.clear();
        card.answer_source_kind = AnswerSourceKind::Unanswered;
        card.exam_set_id = Some(set_id.clone());
        card.question_no = question_no;
        snapshot.push_question_card(stage(index + 1, card));
    }

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::CountConservation);
    for expected in [
        "expected_questions",
        "question_cards.question_no[2]",
        "question_cards.question_no",
    ] {
        assert!(
            fields.iter().any(|field| field == expected),
            "missing {expected} in {fields:?}"
        );
    }
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("missing placeholder")),
        "gap must be reported"
    );
}

#[test]
fn complete_exam_set_with_unanswered_placeholders_passes() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);
    let set = exam_set(SourceKind::PastExam, Some(2), true);
    let set_id = set.id.clone();
    snapshot.push_exam_set(stage(0, set));
    for (index, number) in [1, 2].into_iter().enumerate() {
        let mut card = question_card();
        card.question_type = QuestionType::ShortAnswer;
        card.grading_mode = GradingMode::Voided;
        card.answer.clear();
        card.answer_source_kind = AnswerSourceKind::Unanswered;
        card.exam_set_id = Some(set_id.clone());
        card.question_no = Some(number);
        snapshot.push_question_card(stage(index + 1, card));
    }
    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    assert!(report.is_ok(), "unexpected errors: {:?}", report.errors);
}

#[test]
fn referential_integrity_checks_links_media_aspect_and_edges() {
    let database = Database::open_in_memory().unwrap();
    let base = resource("base", Some((100, 50)));
    let base_id = base.id.clone();
    let overlay = resource("overlay", Some((200, 200)));
    let overlay_id = overlay.id.clone();
    let missing_dimensions = resource("no-dimensions", None);
    let missing_dimensions_id = missing_dimensions.id.clone();
    let target_card = knowledge_card("target");
    for item in [&base, &overlay, &missing_dimensions] {
        reinforce_core::content_store::store_resource(database.connection(), item).unwrap();
    }
    reinforce_core::content_store::insert_knowledge_card(database.connection(), &target_card)
        .unwrap();

    let mut snapshot = Snapshot::new(Actor::Owner);
    let mut card = knowledge_card("pins");
    card.blocks = vec![ContentBlock::InteractivePins {
        resource_id: base_id.clone(),
        overlay_resource_id: Some(overlay_id.clone()),
        pins: vec![reinforce_core::content::Pin {
            no: 1,
            x: 0.1,
            y: 0.2,
            label_x: 0.3,
            label_y: 0.4,
            term_primary: "term".to_owned(),
            term_secondary: None,
            detail: None,
            hint: None,
            linked_card_id: Some(reinforce_core::KnowledgeCardId::generate()),
        }],
    }];
    card.blocks.push(ContentBlock::ImageOcclusion {
        resource_id: reinforce_core::ResourceId::generate(),
        masks: vec![OcclusionMask {
            group_id: "mask".to_owned(),
            rect: NormalizedRect {
                x: 0.0,
                y: 0.0,
                w: 0.2,
                h: 0.2,
            },
        }],
    });
    card.anchor_ids.push(reinforce_core::AnchorId::generate());
    snapshot.push_knowledge_card(stage(0, card));

    let mut card_without_dimensions = knowledge_card("no dimensions");
    card_without_dimensions.blocks = vec![ContentBlock::InteractivePins {
        resource_id: missing_dimensions_id,
        overlay_resource_id: Some(overlay_id),
        pins: vec![],
    }];
    snapshot.push_knowledge_card(stage(1, card_without_dimensions));

    let valid_linked = knowledge_card("linked");
    let valid_linked_id = valid_linked.id.clone();
    snapshot.push_knowledge_card(stage(2, valid_linked));

    let mut note = Note {
        id: reinforce_core::NoteId::generate(),
        subject: "biology".to_owned(),
        title: "note".to_owned(),
        outline: vec![OutlineNode::CardRef {
            card_id: reinforce_core::KnowledgeCardId::generate(),
        }],
        view_settings: json!({}),
        status: CardStatus::Draft,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    };
    note.outline.push(OutlineNode::CardRef {
        card_id: valid_linked_id.clone(),
    });
    snapshot.push_note(stage(3, note));

    let mut question = question_card();
    question.options.push(QuestionOption {
        key: "B".to_owned(),
        content: "other".to_owned(),
        explanation: None,
        linked_card_ids: vec![reinforce_core::KnowledgeCardId::generate()],
    });
    let question_id = question.id.clone();
    snapshot.push_question_card(stage(4, question));
    let mut edge = Edge {
        id: reinforce_core::EdgeId::generate(),
        edge_type: EdgeType::Tests,
        from: EdgeEndpoint::Option {
            id: question_id.clone(),
            key: "Z".to_owned(),
        },
        to: EdgeEndpoint::Card {
            id: valid_linked_id,
        },
        created_at: Timestamp::now(),
    };
    edge.from = EdgeEndpoint::Option {
        id: question_id,
        key: "A".to_owned(),
    };
    snapshot.push_edge(stage(5, edge));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::ReferentialIntegrity);
    for expected in [
        "blocks[0].pins[0].linked_card_id",
        "blocks[1].resource_id",
        "anchor_ids[0]",
        "blocks[0].overlay_resource_id",
        "blocks[0].resource_id",
        "outline[0].card_id",
        "options[1].linked_card_ids[0]",
    ] {
        assert!(
            fields.iter().any(|field| field == expected),
            "missing {expected} in {fields:?}"
        );
    }
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("aspect ratio")),
        "overlay aspect mismatch must be reported"
    );
    assert!(
        !report.errors.iter().any(|error| error.op_index == Some(5)),
        "the valid option endpoint must not fail: {:?}",
        report.errors
    );
    assert!(database.connection().resource(&base_id).unwrap().is_some());
}

#[test]
fn verification_lock_rejects_non_owner_verified_cards() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Agent {
        name: "assistant".to_owned(),
    });
    let mut card = knowledge_card("verified by agent");
    card.verification = Verification::Verified;
    snapshot.push_knowledge_card(stage(0, card));
    let mut question = question_card();
    question.verification = Verification::Verified;
    snapshot.push_question_card(stage(1, question));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::VerificationLock);
    assert_eq!(
        fields,
        vec!["verification".to_owned(), "verification".to_owned()]
    );

    let mut member_snapshot = Snapshot::new(Actor::Member {
        user_id: UserId::generate(),
    });
    let mut member_card = knowledge_card("verified by member");
    member_card.verification = Verification::Verified;
    member_snapshot.push_knowledge_card(stage(0, member_card));
    let member_report = validate_snapshot(&member_snapshot, database.connection()).unwrap();
    assert_eq!(
        field_paths(&member_report.errors, ValidationRule::VerificationLock).len(),
        1
    );

    let mut owner_snapshot = Snapshot::new(Actor::Owner);
    let mut owner_card = knowledge_card("verified by owner");
    owner_card.verification = Verification::Verified;
    owner_snapshot.push_knowledge_card(stage(0, owner_card));
    let owner_report = validate_snapshot(&owner_snapshot, database.connection()).unwrap();
    assert!(
        owner_report.is_ok(),
        "unexpected errors: {:?}",
        owner_report.errors
    );
}

#[test]
fn matching_aspect_ratio_within_tolerance_passes() {
    let database = Database::open_in_memory().unwrap();
    let mut snapshot = Snapshot::new(Actor::Owner);
    let base = resource("base", Some((1000, 500)));
    let base_id = base.id.clone();
    let overlay = resource("overlay", Some((1005, 500)));
    let overlay_id = overlay.id.clone();
    snapshot.push_resource(stage(0, base));
    snapshot.push_resource(stage(1, overlay));
    let mut card = knowledge_card("aligned");
    card.blocks.push(ContentBlock::InteractivePins {
        resource_id: base_id,
        overlay_resource_id: Some(overlay_id),
        pins: vec![],
    });
    snapshot.push_knowledge_card(stage(2, card));
    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    assert!(report.is_ok(), "unexpected errors: {:?}", report.errors);
}

#[test]
fn endpoint_kind_is_reported_for_option_references() {
    let endpoint = EdgeEndpoint::Option {
        id: QuestionCardId::generate(),
        key: "A".to_owned(),
    };
    assert_eq!(endpoint.kind(), EndpointKind::Option);
}

fn placeholder(set_id: &reinforce_core::ExamSetId, number: u32) -> QuestionCard {
    let mut card = question_card();
    card.question_type = QuestionType::ShortAnswer;
    card.grading_mode = GradingMode::Voided;
    card.answer.clear();
    card.answer_source_kind = AnswerSourceKind::Unanswered;
    card.exam_set_id = Some(set_id.clone());
    card.question_no = Some(number);
    card
}

fn stored_complete_exam_set(database: &Database, expected: u32) -> (ExamSet, Vec<QuestionCard>) {
    let set = exam_set(SourceKind::PastExam, Some(expected), true);
    content_store::insert_exam_set(database.connection(), &set).unwrap();
    let cards: Vec<QuestionCard> = (1..=expected)
        .map(|number| placeholder(&set.id, number))
        .collect();
    for card in &cards {
        content_store::insert_question_card(database.connection(), card).unwrap();
    }
    (set, cards)
}

#[test]
fn count_conservation_covers_stored_sets_that_a_batch_adds_questions_to() {
    let database = Database::open_in_memory().unwrap();
    let (set, _) = stored_complete_exam_set(&database, 2);

    let mut snapshot = Snapshot::new(Actor::Owner);
    snapshot.push_question_card(stage(0, placeholder(&set.id, 3)));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::CountConservation);
    assert!(
        fields.iter().any(|field| field == "expected_questions"),
        "adding a third question to a complete two-question set must fail: {fields:?}"
    );
    assert!(
        report.errors.iter().all(|error| error.op_index == Some(0)),
        "errors point at the staged card"
    );
}

#[test]
fn count_conservation_covers_questions_moved_out_of_a_stored_set() {
    let database = Database::open_in_memory().unwrap();
    let (_, cards) = stored_complete_exam_set(&database, 2);

    let mut moved = cards[1].clone();
    moved.exam_set_id = None;
    moved.question_no = None;
    let mut snapshot = Snapshot::new(Actor::Owner);
    snapshot.push_question_card(stage(0, moved));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    let fields = field_paths(&report.errors, ValidationRule::CountConservation);
    assert!(
        fields.iter().any(|field| field == "expected_questions"),
        "moving a question out of a complete set must fail: {fields:?}"
    );
}

#[test]
fn replacing_a_question_inside_a_stored_set_keeps_the_count() {
    let database = Database::open_in_memory().unwrap();
    let (_, cards) = stored_complete_exam_set(&database, 2);

    let mut revised = cards[0].clone();
    revised.subject = "revised".to_owned();
    let mut snapshot = Snapshot::new(Actor::Owner);
    snapshot.push_question_card(stage(0, revised));

    let report = validate_snapshot(&snapshot, database.connection()).unwrap();
    assert!(report.is_ok(), "unexpected errors: {:?}", report.errors);
}
