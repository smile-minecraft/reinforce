use reinforce_core::content::{ContentBlock, QuestionOption};
use reinforce_core::primitives::{
    AnswerSourceKind, CardStatus, EdgeType, ExportPolicy, GradingMode, ProposalStatus,
    QuestionType, ResourceKind, Sensitivity, Verification,
};
use reinforce_core::proposal::{
    ApprovalOptions, ApprovalOutcome, BatchOperation, OperationKind, SubmitOutcome, approve_batch,
    batch_from_proposal, reject_batch, rejected_batches, submit_batch,
};
use reinforce_core::validation::{Actor, ValidationRule};
use reinforce_core::{
    Anchor, ContentHash, Database, EdgeEndpoint, KnowledgeCard, QuestionCard, Resource, Timestamp,
};
use serde_json::{Value, json};

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

fn knowledge_card(title: &str) -> KnowledgeCard {
    let now = Timestamp::now();
    KnowledgeCard {
        id: reinforce_core::KnowledgeCardId::generate(),
        subject: "biology".to_owned(),
        topics: vec![],
        title: title.to_owned(),
        blocks: vec![ContentBlock::Markdown {
            text: "oldblock".to_owned(),
        }],
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
        stem_blocks: vec![ContentBlock::Markdown {
            text: "oldstem".to_owned(),
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
        status: CardStatus::Draft,
        verification: Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn operation(kind: OperationKind, temp_ref: Option<&str>, value: Value) -> BatchOperation {
    BatchOperation::new(kind, temp_ref.map(str::to_owned), value)
}

fn entity<T: serde::Serialize>(
    kind: OperationKind,
    temp_ref: Option<&str>,
    value: &T,
) -> BatchOperation {
    BatchOperation::from_entity(kind, temp_ref.map(str::to_owned), value).unwrap()
}

fn temp_id(payload: &reinforce_core::BatchPayload, temp_ref: &str) -> String {
    payload
        .operations
        .iter()
        .find(|operation| operation.temp_ref.as_deref() == Some(temp_ref))
        .unwrap()
        .id
        .clone()
}

#[test]
fn submit_resolves_temp_refs_and_approve_applies_every_entity() {
    let mut database = Database::open_in_memory().unwrap();
    let anchor = Anchor {
        id: reinforce_core::AnchorId::generate(),
        resource_id: reinforce_core::ResourceId::generate(),
        anchor_kind: reinforce_core::primitives::AnchorKind::Rect,
        pdf_page: None,
        printed_page: None,
        legend_pdf_page: None,
        rect: Some(reinforce_core::content::NormalizedRect {
            x: 0.0,
            y: 0.0,
            w: 0.5,
            h: 0.5,
        }),
        media_span: None,
    };

    let mut card = knowledge_card("draft");
    card.verification = Verification::Verified;
    let mut question = question_card();
    question.verification = Verification::Verified;

    let operations = vec![
        entity(
            OperationKind::Resource,
            Some("$res_1"),
            &resource("image", Some((1200, 800))),
        ),
        operation(
            OperationKind::Anchor,
            Some("$anchor_1"),
            json!({
                "id": anchor.id,
                "resource_id": "$res_1",
                "anchor_kind": "rect",
                "rect": {"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5}
            }),
        ),
        operation(
            OperationKind::KnowledgeCard,
            Some("$card_1"),
            json!({
                "id": card.id,
                "subject": "biology",
                "topics": [],
                "title": "draft",
                "blocks": [],
                "anchor_ids": ["$anchor_1"],
                "status": "draft",
                "verification": "verified",
                "unsure_count": 0,
                "metadata": {},
                "created_at": "2026-11-26T00:00:00.000000Z",
                "updated_at": "2026-11-26T00:00:00.000000Z"
            }),
        ),
        entity(OperationKind::QuestionCard, Some("$q_1"), &question),
        operation(
            OperationKind::Edge,
            None,
            json!({
                "id": reinforce_core::EdgeId::generate(),
                "edge_type": "tests",
                "from": {"kind": "card", "id": "$card_1"},
                "to": {"kind": "question", "id": "$q_1"},
                "created_at": "2026-11-26T00:00:00.000000Z"
            }),
        ),
    ];

    let submission = submit_batch(
        database.connection_mut(),
        Actor::Agent {
            name: "agent".to_owned(),
        },
        operations,
    )
    .unwrap();
    let SubmitOutcome::Accepted { proposal } = submission else {
        panic!("submission must be accepted");
    };
    assert_eq!(proposal.status, ProposalStatus::Pending);
    assert!(
        reinforce_core::content_store::get_knowledge_card(database.connection(), &card.id)
            .unwrap()
            .is_none()
    );

    let payload = batch_from_proposal(&proposal).unwrap();
    assert_ne!(temp_id(&payload, "$card_1"), "$card_1");
    assert!(
        temp_id(&payload, "$card_1")
            .parse::<reinforce_core::KnowledgeCardId>()
            .is_ok()
    );
    let stored_card = reinforce_core::content_store::get_knowledge_card(
        database.connection(),
        &temp_id(&payload, "$card_1").parse().unwrap(),
    )
    .unwrap();
    assert!(stored_card.is_none());

    let ApprovalOutcome::Applied {
        applied,
        proposal: approved,
    } = approve_batch(
        database.connection_mut(),
        &proposal.id,
        ApprovalOptions {
            reviewer: Actor::Owner,
            promote_verified: false,
        },
    )
    .unwrap()
    else {
        panic!("approval must apply the batch");
    };
    assert_eq!(applied, 5);
    assert_eq!(approved.status, ProposalStatus::Approved);

    let saved = reinforce_core::content_store::get_knowledge_card(
        database.connection(),
        &temp_id(&payload, "$card_1").parse().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(saved.verification, Verification::Unchecked);
    assert_eq!(
        saved.anchor_ids,
        vec![
            temp_id(&payload, "$anchor_1")
                .parse::<reinforce_core::AnchorId>()
                .unwrap()
        ]
    );
    let saved_anchor = reinforce_core::content_store::get_anchor(
        database.connection(),
        &temp_id(&payload, "$anchor_1").parse().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        saved_anchor.resource_id.as_str(),
        temp_id(&payload, "$res_1")
    );
    let saved_resource = reinforce_core::content_store::get_resource(
        database.connection(),
        &temp_id(&payload, "$res_1").parse().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        (saved_resource.width, saved_resource.height),
        (Some(1200), Some(800))
    );

    let stored_question = reinforce_core::content_store::get_question_card(
        database.connection(),
        &temp_id(&payload, "$q_1").parse().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(stored_question.verification, Verification::Unchecked);
    let edge = reinforce_core::content_store::get_edge(
        database.connection(),
        &payload.operations[4].id.parse().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(edge.edge_type, EdgeType::Tests);
    assert_eq!(
        edge.from,
        EdgeEndpoint::Card {
            id: saved.id.clone()
        }
    );
    assert_eq!(
        edge.to,
        EdgeEndpoint::Question {
            id: stored_question.id
        }
    );
}

#[test]
fn validation_failure_returns_all_fields_and_writes_nothing() {
    let mut database = Database::open_in_memory().unwrap();
    let mut missing_source = question_card();
    missing_source.answer_source_kind = AnswerSourceKind::Unanswered;
    let mut unknown_key = question_card();
    unknown_key.answer = vec!["Z".to_owned()];

    let operations = vec![
        entity(OperationKind::QuestionCard, None, &missing_source),
        entity(OperationKind::QuestionCard, None, &unknown_key),
    ];
    let SubmitOutcome::Rejected { errors } =
        submit_batch(database.connection_mut(), Actor::Owner, operations).unwrap()
    else {
        panic!("invalid batch must be rejected");
    };
    assert!(errors.errors.len() >= 2);
    for error in &errors.errors {
        assert!(error.op_index.is_some());
        assert!(error.field_path.starts_with("answer"));
    }
    let json: Value = serde_json::from_str(&errors.to_json().unwrap()).unwrap();
    assert!(json["errors"].as_array().is_some());
    assert!(
        reinforce_core::content_store::list_proposals(
            database.connection(),
            ProposalStatus::Pending
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn approval_recomputes_search_text_and_can_promote_verified() {
    let mut database = Database::open_in_memory().unwrap();
    let card = knowledge_card("oldtitle");
    let question = question_card();
    reinforce_core::content_store::insert_knowledge_card(database.connection(), &card).unwrap();
    reinforce_core::content_store::insert_question_card(database.connection(), &question).unwrap();

    let mut updated_card = card.clone();
    updated_card.title = "neotitle".to_owned();
    updated_card.verification = Verification::Verified;
    let mut updated_question = question.clone();
    updated_question.stem_blocks = vec![ContentBlock::Markdown {
        text: "neostem".to_owned(),
    }];
    updated_question.verification = Verification::Verified;
    let operations = vec![
        entity(OperationKind::KnowledgeCard, None, &updated_card),
        entity(OperationKind::QuestionCard, None, &updated_question),
    ];
    let SubmitOutcome::Accepted { proposal } = submit_batch(
        database.connection_mut(),
        Actor::Agent {
            name: "a".to_owned(),
        },
        operations,
    )
    .unwrap() else {
        panic!("submission must be accepted");
    };
    approve_batch(
        database.connection_mut(),
        &proposal.id,
        ApprovalOptions {
            reviewer: Actor::Owner,
            promote_verified: true,
        },
    )
    .unwrap();

    let saved_card =
        reinforce_core::content_store::get_knowledge_card(database.connection(), &card.id)
            .unwrap()
            .unwrap();
    let saved_question =
        reinforce_core::content_store::get_question_card(database.connection(), &question.id)
            .unwrap()
            .unwrap();
    assert_eq!(saved_card.title, "neotitle");
    assert_eq!(saved_card.verification, Verification::Verified);
    assert_eq!(saved_question.verification, Verification::Verified);
    assert!(
        reinforce_core::search::search_knowledge_cards(database.connection(), "neotitle", 10)
            .unwrap()
            .contains(&card.id)
    );
    assert!(
        reinforce_core::search::search_knowledge_cards(database.connection(), "oldtitle", 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        reinforce_core::search::search_question_cards(database.connection(), "neostem", 10)
            .unwrap()
            .contains(&question.id)
    );
    assert!(
        reinforce_core::search::search_question_cards(database.connection(), "oldstem", 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejected_proposal_keeps_review_note_and_resubmission_works() {
    let mut database = Database::open_in_memory().unwrap();
    let card = question_card();
    let operations = vec![entity(OperationKind::QuestionCard, None, &card)];
    let SubmitOutcome::Accepted { proposal } = submit_batch(
        database.connection_mut(),
        Actor::Agent {
            name: "agent".to_owned(),
        },
        operations,
    )
    .unwrap() else {
        panic!("submission must be accepted");
    };

    assert!(reject_batch(database.connection(), &proposal.id, "   ").is_err());
    let rejected = reject_batch(database.connection(), &proposal.id, "add a real answer").unwrap();
    assert_eq!(rejected.status, ProposalStatus::Rejected);
    assert_eq!(rejected.review_note.as_deref(), Some("add a real answer"));
    assert!(
        approve_batch(
            database.connection_mut(),
            &proposal.id,
            ApprovalOptions {
                reviewer: Actor::Owner,
                promote_verified: false,
            },
        )
        .is_err()
    );
    assert_eq!(rejected_batches(database.connection()).unwrap().len(), 1);

    let mut revised = question_card();
    revised.id = card.id.clone();
    let SubmitOutcome::Accepted { proposal: retry } = submit_batch(
        database.connection_mut(),
        Actor::Agent {
            name: "agent".to_owned(),
        },
        vec![entity(OperationKind::QuestionCard, None, &revised)],
    )
    .unwrap() else {
        panic!("revised submission must be accepted");
    };
    let ApprovalOutcome::Applied { applied, .. } = approve_batch(
        database.connection_mut(),
        &retry.id,
        ApprovalOptions {
            reviewer: Actor::Owner,
            promote_verified: false,
        },
    )
    .unwrap() else {
        panic!("revised approval must apply");
    };
    assert_eq!(applied, 1);
}

#[test]
fn approval_revalidates_against_the_reviewer_and_keeps_pending_on_failure() {
    let mut database = Database::open_in_memory().unwrap();
    let mut card = knowledge_card("owner verified");
    card.verification = Verification::Verified;
    let SubmitOutcome::Accepted { proposal } = submit_batch(
        database.connection_mut(),
        Actor::Owner,
        vec![entity(OperationKind::KnowledgeCard, None, &card)],
    )
    .unwrap() else {
        panic!("submission must be accepted");
    };

    let ApprovalOutcome::Rejected {
        errors,
        proposal: unchanged,
    } = approve_batch(
        database.connection_mut(),
        &proposal.id,
        ApprovalOptions {
            reviewer: Actor::Agent {
                name: "reviewer".to_owned(),
            },
            promote_verified: false,
        },
    )
    .unwrap()
    else {
        panic!("non-owner reviewer must not apply verified content");
    };
    assert!(
        errors
            .errors
            .iter()
            .any(|error| error.rule == ValidationRule::VerificationLock)
    );
    assert_eq!(unchanged.status, ProposalStatus::Pending);
    assert!(
        reinforce_core::content_store::get_knowledge_card(database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn temp_ref_shape_errors_are_structured() {
    let mut database = Database::open_in_memory().unwrap();
    let card = knowledge_card("dup");
    let SubmitOutcome::Rejected { errors } = submit_batch(
        database.connection_mut(),
        Actor::Owner,
        vec![
            entity(OperationKind::KnowledgeCard, Some("$same"), &card),
            entity(OperationKind::KnowledgeCard, Some("$same"), &card),
        ],
    )
    .unwrap() else {
        panic!("duplicate temp_ref must be rejected");
    };
    assert!(
        errors
            .errors
            .iter()
            .any(|error| error.rule == ValidationRule::TempRef
                && error.message.contains("duplicate"))
    );

    let SubmitOutcome::Rejected { errors } = submit_batch(
        database.connection_mut(),
        Actor::Owner,
        vec![operation(
            OperationKind::KnowledgeCard,
            None,
            json!({
                "id": card.id,
                "subject": "biology",
                "title": "missing anchor",
                "blocks": [{"type": "image_occlusion", "resource_id": "$missing", "masks": []}],
                "anchor_ids": [],
                "status": "draft",
                "verification": "unchecked",
                "metadata": {}
            }),
        )],
    )
    .unwrap() else {
        panic!("unknown temp_ref must be rejected");
    };
    assert!(errors
        .errors
        .iter()
        .any(|error| error.rule == ValidationRule::TempRef && error.message.contains("unknown")));
}

#[test]
fn database_failure_during_approval_rolls_back_the_whole_batch() {
    let mut database = Database::open_in_memory().unwrap();
    let first = resource("duplicate hash", Some((100, 100)));
    let mut second = first.clone();
    second.id = reinforce_core::ResourceId::generate();
    let operations = vec![
        entity(OperationKind::Resource, None, &first),
        entity(OperationKind::Resource, None, &second),
    ];
    let SubmitOutcome::Accepted { proposal } =
        submit_batch(database.connection_mut(), Actor::Owner, operations).unwrap()
    else {
        panic!("submission must be accepted");
    };
    assert!(
        approve_batch(
            database.connection_mut(),
            &proposal.id,
            ApprovalOptions {
                reviewer: Actor::Owner,
                promote_verified: false,
            },
        )
        .is_err()
    );
    let count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM resources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0, "no entity may survive a failed apply");
    let stored = reinforce_core::content_store::get_proposal(database.connection(), &proposal.id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, ProposalStatus::Pending);
}
