use std::collections::HashSet;
use std::time::{Duration, Instant};

use reinforce_core::content::{ContentBlock, OutlineNode, QuestionOption};
use reinforce_core::db::{content as store, search, study as study_store};
use reinforce_core::fsrs::recompute_scheduling;
use reinforce_core::primitives::{
    AnchorKind, AnswerSourceKind, AttemptDirection, AttemptId, CardStatus, Confidence, EdgeType,
    ExportPolicy, GradingMode, ProposalStatus, QuestionType, ResourceKind, Sensitivity,
    SessionMode, SourceKind, Verification,
};
use reinforce_core::{
    Anchor, AttemptLog, CardId, ContentHash, Database, Edge, EdgeEndpoint, ExamSet, KnowledgeCard,
    Note, Proposal, ProposalTarget, QuestionCard, QuestionGroup, Resource, Timestamp, User, UserId,
};
use serde_json::json;

fn knowledge_card(title: &str) -> KnowledgeCard {
    let now = Timestamp::now();
    KnowledgeCard {
        id: reinforce_core::KnowledgeCardId::generate(),
        subject: "biology".to_owned(),
        topics: vec!["transport".to_owned()],
        title: title.to_owned(),
        blocks: vec![ContentBlock::Markdown {
            text: format!("{title} details"),
        }],
        anchor_ids: vec![],
        status: CardStatus::Published,
        verification: Verification::Unchecked,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn question_card(stem: &str) -> QuestionCard {
    let now = Timestamp::now();
    QuestionCard {
        id: reinforce_core::QuestionCardId::generate(),
        exam_set_id: None,
        question_group_id: None,
        question_no: None,
        subject: "biology".to_owned(),
        topics: vec!["transport".to_owned()],
        stem_blocks: vec![ContentBlock::Markdown {
            text: stem.to_owned(),
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
        status: CardStatus::Published,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn edge(edge_type: EdgeType, from: EdgeEndpoint, to: EdgeEndpoint) -> Edge {
    Edge {
        id: reinforce_core::EdgeId::generate(),
        edge_type,
        from,
        to,
        created_at: Timestamp::now(),
    }
}

fn attempt_at(
    user_id: &UserId,
    card_id: &CardId,
    timestamp: &str,
    is_correct: bool,
    confidence: Confidence,
) -> AttemptLog {
    AttemptLog {
        id: AttemptId::generate(),
        user_id: user_id.clone(),
        card_id: card_id.clone(),
        direction: AttemptDirection::Default,
        session_mode: SessionMode::DailyDue,
        submitted_payload: json!({}),
        is_correct,
        duration_ms: 1_000,
        confidence,
        error_tags: vec![],
        created_at: timestamp.parse().unwrap(),
        sync_cursor: 1,
        affects_scheduling: true,
    }
}

#[test]
fn migrations_create_both_layers_and_converge_on_rerun() {
    let mut database = Database::open_in_memory().unwrap();
    database.migrate().unwrap();
    assert_eq!(database.schema_version().unwrap(), 1);

    let tables: Vec<String> = database
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    for table in [
        "resources",
        "anchors",
        "knowledge_cards",
        "notes",
        "exam_sets",
        "question_groups",
        "question_cards",
        "edges",
        "proposals",
        "users",
        "attempt_logs",
        "card_scheduling",
        "user_card_meta",
    ] {
        assert!(tables.iter().any(|name| name == table), "missing {table}");
    }

    let foreign_keys: i64 = database
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(foreign_keys, 1);
}

#[test]
fn typed_entities_round_trip_through_both_layers() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let now = Timestamp::now();

    let resource = Resource {
        id: reinforce_core::ResourceId::generate(),
        kind: ResourceKind::Pdf,
        title: "source".to_owned(),
        sha256: ContentHash::from_bytes(b"source bytes"),
        byte_size: 12,
        page_count: Some(2),
        license: Some("CC-BY".to_owned()),
        attribution_url: None,
        attribution_notice: None,
        sensitivity: Sensitivity::Restricted,
        export_policy: ExportPolicy::Redact,
        created_at: now,
    };
    store::store_resource(connection, &resource).unwrap();
    assert_eq!(
        store::get_resource(connection, &resource.id)
            .unwrap()
            .unwrap(),
        resource
    );

    let anchor = Anchor {
        id: reinforce_core::AnchorId::generate(),
        resource_id: resource.id.clone(),
        anchor_kind: AnchorKind::Rect,
        pdf_page: Some(1),
        printed_page: None,
        legend_pdf_page: None,
        rect: Some(reinforce_core::content::NormalizedRect {
            x: 0.1,
            y: 0.2,
            w: 0.3,
            h: 0.4,
        }),
        media_span: None,
    };
    store::insert_anchor(connection, &anchor).unwrap();
    assert_eq!(
        store::get_anchor(connection, &anchor.id).unwrap().unwrap(),
        anchor
    );

    let card = knowledge_card("card");
    store::insert_knowledge_card(connection, &card).unwrap();
    assert_eq!(
        store::get_knowledge_card(connection, &card.id)
            .unwrap()
            .unwrap(),
        card
    );

    let note = Note {
        id: reinforce_core::NoteId::generate(),
        subject: "biology".to_owned(),
        title: "note".to_owned(),
        outline: vec![OutlineNode::CardRef {
            card_id: card.id.clone(),
        }],
        view_settings: json!({}),
        status: CardStatus::Draft,
        created_at: now,
        updated_at: now,
    };
    store::insert_note(connection, &note).unwrap();
    assert_eq!(
        store::get_note(connection, &note.id).unwrap().unwrap(),
        note
    );

    let exam_set = ExamSet {
        id: reinforce_core::ExamSetId::generate(),
        subject: "biology".to_owned(),
        title: "exam".to_owned(),
        source_kind: SourceKind::Original,
        exam_year: None,
        source_resource_id: None,
        expected_questions: Some(1),
        metadata: json!({}),
        is_complete: true,
    };
    store::insert_exam_set(connection, &exam_set).unwrap();
    assert_eq!(
        store::get_exam_set(connection, &exam_set.id)
            .unwrap()
            .unwrap(),
        exam_set
    );

    let group = QuestionGroup {
        id: reinforce_core::QuestionGroupId::generate(),
        exam_set_id: Some(exam_set.id.clone()),
        title: Some("group".to_owned()),
        shared_stem_blocks: vec![],
    };
    store::insert_question_group(connection, &group).unwrap();
    assert_eq!(
        store::get_question_group(connection, &group.id)
            .unwrap()
            .unwrap(),
        group
    );

    let question = question_card("stem");
    store::insert_question_card(connection, &question).unwrap();
    assert_eq!(
        store::get_question_card(connection, &question.id)
            .unwrap()
            .unwrap(),
        question
    );

    let edge = edge(
        EdgeType::Tests,
        EdgeEndpoint::Question {
            id: question.id.clone(),
        },
        EdgeEndpoint::Card {
            id: card.id.clone(),
        },
    );
    store::insert_edge(connection, &edge).unwrap();
    assert_eq!(
        store::get_edge(connection, &edge.id).unwrap().unwrap(),
        edge
    );

    let proposal = Proposal {
        id: reinforce_core::ProposalId::generate(),
        target: ProposalTarget::KnowledgeCard(card.id.clone()),
        payload: json!({"title": "draft"}),
        submitted_by: "agent".to_owned(),
        status: ProposalStatus::Pending,
        review_note: None,
        created_at: now,
        updated_at: now,
        sync_cursor: 1,
    };
    store::insert_proposal(connection, &proposal).unwrap();
    assert_eq!(
        store::get_proposal(connection, &proposal.id)
            .unwrap()
            .unwrap(),
        proposal
    );
    assert_eq!(
        store::list_proposals(connection, ProposalStatus::Pending).unwrap(),
        vec![proposal]
    );
}

#[test]
fn resource_hash_deduplicates_content() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let make = |title: &str| Resource {
        id: reinforce_core::ResourceId::generate(),
        kind: ResourceKind::Web,
        title: title.to_owned(),
        sha256: ContentHash::from_bytes(b"same bytes"),
        byte_size: 10,
        page_count: None,
        license: None,
        attribution_url: None,
        attribution_notice: None,
        sensitivity: Sensitivity::Normal,
        export_policy: ExportPolicy::Allow,
        created_at: Timestamp::now(),
    };
    let first = make("first");
    let second = make("second");
    let returned = store::store_resource(connection, &first).unwrap();
    assert_eq!(returned, first.id);
    assert_eq!(
        store::store_resource(connection, &second).unwrap(),
        first.id,
        "the same sha256 must resolve to the original resource"
    );
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM resources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn fts5_finds_chinese_and_english_and_short_queries_fall_back_to_like() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let mut chinese = knowledge_card("線粒體運輸機制");
    chinese.title = "線粒體運輸機制".to_owned();
    let english = knowledge_card("mitochondrion transport");
    store::insert_knowledge_card(connection, &chinese).unwrap();
    store::insert_knowledge_card(connection, &english).unwrap();

    let mut question = question_card("線粒體膜電位如何維持");
    question.stem_blocks = vec![ContentBlock::Markdown {
        text: "線粒體膜電位如何維持".to_owned(),
    }];
    store::insert_question_card(connection, &question).unwrap();

    assert!(
        search::search_knowledge_cards(connection, "線粒體", 10)
            .unwrap()
            .contains(&chinese.id)
    );
    assert!(
        search::search_knowledge_cards(connection, "mitochondrion", 10)
            .unwrap()
            .contains(&english.id)
    );
    assert!(
        search::search_question_cards(connection, "膜電位", 10)
            .unwrap()
            .contains(&question.id)
    );
    assert!(
        search::search_knowledge_cards(connection, "粒", 10)
            .unwrap()
            .contains(&chinese.id),
        "single-character queries use LIKE"
    );
    assert!(search::search_knowledge_cards(connection, "\" OR ", 10).is_ok());
}

#[test]
fn graph_walks_bounded_depth_and_stops_at_cycles() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let cards: Vec<_> = (0..4)
        .map(|index| knowledge_card(&format!("card {index}")))
        .collect();
    for card in &cards {
        store::insert_knowledge_card(connection, card).unwrap();
    }
    let endpoint = |card: &KnowledgeCard| EdgeEndpoint::Card {
        id: card.id.clone(),
    };
    for pair in [(0, 1), (1, 2), (2, 3)] {
        store::insert_edge(
            connection,
            &edge(
                EdgeType::Prerequisite,
                endpoint(&cards[pair.0]),
                endpoint(&cards[pair.1]),
            ),
        )
        .unwrap();
    }
    let shallow = search::graph_neighbors(connection, &endpoint(&cards[0]), 2).unwrap();
    let shallow_ids: Vec<_> = shallow.iter().map(|node| node.endpoint.id()).collect();
    assert!(shallow_ids.contains(&cards[1].id.as_str()));
    assert!(shallow_ids.contains(&cards[2].id.as_str()));
    assert!(!shallow_ids.contains(&cards[3].id.as_str()));
    assert!(
        !shallow_ids.contains(&cards[0].id.as_str()),
        "cycles are not revisited"
    );
    assert!(shallow.iter().all(|node| node.depth <= 2));

    store::insert_edge(
        connection,
        &edge(
            EdgeType::Prerequisite,
            endpoint(&cards[2]),
            endpoint(&cards[0]),
        ),
    )
    .unwrap();

    let full = search::graph_neighbors(connection, &endpoint(&cards[0]), 8).unwrap();
    let full_ids: Vec<_> = full.iter().map(|node| node.endpoint.id()).collect();
    assert!(full_ids.contains(&cards[3].id.as_str()));
    assert!(!full_ids.contains(&cards[0].id.as_str()));
    assert!(search::graph_neighbors(connection, &endpoint(&cards[0]), 17).is_err());
}

#[test]
fn attempt_logs_are_append_only_and_scheduling_is_deterministic() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let now = Timestamp::now();
    let user = User {
        id: UserId::generate(),
        display_name: "learner".to_owned(),
        created_at: now,
    };
    study_store::insert_user(connection, &user).unwrap();

    let card_id = CardId::from(knowledge_card("scheduled").id);
    let mut logs = Vec::new();
    for (days, correct) in [(0.0, true), (3.0, false), (6.0, true)] {
        let created_at = Timestamp::unix_epoch().plus_days(days);
        let cursor = study_store::next_attempt_cursor(connection).unwrap();
        logs.push(AttemptLog {
            id: AttemptId::generate(),
            user_id: user.id.clone(),
            card_id: card_id.clone(),
            direction: AttemptDirection::Default,
            session_mode: SessionMode::DailyDue,
            submitted_payload: json!({ "answer": "A" }),
            is_correct: correct,
            duration_ms: 1_000,
            confidence: Confidence::Split,
            error_tags: vec![],
            created_at,
            sync_cursor: cursor,
            affects_scheduling: true,
        });
    }
    for log in &logs {
        study_store::append_attempt(connection, log).unwrap();
    }
    assert_eq!(
        study_store::list_attempts(connection, &user.id, &card_id, AttemptDirection::Default)
            .unwrap()
            .len(),
        3
    );

    let first = study_store::recompute_for_card(
        connection,
        &user.id,
        &card_id,
        AttemptDirection::Default,
        0.9,
    )
    .unwrap();
    let second = study_store::recompute_for_card(
        connection,
        &user.id,
        &card_id,
        AttemptDirection::Default,
        0.9,
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.reps, 3);
    assert_eq!(first.lapses, 1);
    assert!(first.due_at.is_some());

    let reversed: Vec<AttemptLog> = logs.into_iter().rev().collect();
    let from_reversed = recompute_scheduling(
        user.id.clone(),
        card_id.clone(),
        AttemptDirection::Default,
        &reversed,
        0.9,
    )
    .unwrap();
    assert_eq!(
        first, from_reversed,
        "ordering comes from timestamps, not input order"
    );

    study_store::save_scheduling(connection, &first).unwrap();
    assert_eq!(
        study_store::get_scheduling(connection, &user.id, &card_id, AttemptDirection::Default)
            .unwrap()
            .unwrap(),
        first
    );

    assert!(
        connection
            .execute("UPDATE attempt_logs SET duration_ms = 1", [])
            .is_err()
    );
    assert!(connection.execute("DELETE FROM attempt_logs", []).is_err());
}

#[test]
fn invalid_enum_values_are_rejected() {
    assert!("not_a_status".parse::<CardStatus>().is_err());
    assert!("published".parse::<CardStatus>().is_ok());

    let database = Database::open_in_memory().unwrap();
    let now = Timestamp::now().as_str();
    assert!(database
        .connection()
        .execute(
            "INSERT INTO knowledge_cards (id, subject, title, status, verification, created_at, updated_at)
             VALUES ('card', 's', 't', 'bogus', 'unchecked', ?1, ?1)",
            [now],
        )
        .is_err());
}

#[test]
fn timestamp_text_order_matches_chronological_order_in_sql_and_rust() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let user = User {
        id: UserId::generate(),
        display_name: "learner".to_owned(),
        created_at: Timestamp::now(),
    };
    study_store::insert_user(connection, &user).unwrap();
    let card_id = CardId::from(knowledge_card("ordered").id);
    let logs = [
        attempt_at(
            &user.id,
            &card_id,
            "2026-11-26T15:19:13.25+08:00",
            true,
            Confidence::Certain,
        ),
        attempt_at(
            &user.id,
            &card_id,
            "2026-11-26T07:19:13.5Z",
            false,
            Confidence::Certain,
        ),
        attempt_at(
            &user.id,
            &card_id,
            "2026-11-26T07:19:14Z",
            true,
            Confidence::Split,
        ),
    ];
    let expected: Vec<AttemptId> = logs.iter().map(|log| log.id.clone()).collect();

    for index in [2, 0, 1] {
        study_store::append_attempt(connection, &logs[index]).unwrap();
    }
    let listed =
        study_store::list_attempts(connection, &user.id, &card_id, AttemptDirection::Default)
            .unwrap();
    let sql_order: Vec<AttemptId> = listed.iter().map(|log| log.id.clone()).collect();
    assert_eq!(
        sql_order, expected,
        "SQL ORDER BY created_at must follow time"
    );

    let mut rust_order = listed.clone();
    rust_order.reverse();
    rust_order.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.as_str().cmp(&right.id.as_str()))
    });
    let rust_ids: Vec<AttemptId> = rust_order.iter().map(|log| log.id.clone()).collect();
    assert_eq!(rust_ids, expected, "Rust ordering must follow time");
}

#[test]
fn fsrs_replay_ignores_input_order_with_fractional_timestamps() {
    let user_id = UserId::generate();
    let card_id = CardId::from(knowledge_card("fractional replay").id);
    let logs = vec![
        attempt_at(
            &user_id,
            &card_id,
            "2026-11-26T07:19:13Z",
            true,
            Confidence::Certain,
        ),
        attempt_at(
            &user_id,
            &card_id,
            "2026-11-26T07:19:13.5Z",
            true,
            Confidence::Split,
        ),
        attempt_at(
            &user_id,
            &card_id,
            "2026-11-26T07:19:14.000001Z",
            false,
            Confidence::Certain,
        ),
    ];

    let baseline = recompute_scheduling(
        user_id.clone(),
        card_id.clone(),
        AttemptDirection::Default,
        &logs,
        0.9,
    )
    .unwrap();
    let shuffled = vec![logs[2].clone(), logs[0].clone(), logs[1].clone()];
    let replayed =
        recompute_scheduling(user_id, card_id, AttemptDirection::Default, &shuffled, 0.9).unwrap();

    assert_eq!(replayed, baseline);
    assert_eq!(baseline.last_review_at, Some(logs[2].created_at));
}

#[test]
fn dense_graph_bfs_finishes_bounded_and_visits_each_node_once() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let cards: Vec<_> = (0..30)
        .map(|index| knowledge_card(&format!("dense {index}")))
        .collect();
    for card in &cards {
        store::insert_knowledge_card(connection, card).unwrap();
    }
    let endpoint = |card: &KnowledgeCard| EdgeEndpoint::Card {
        id: card.id.clone(),
    };
    for first in 0..cards.len() {
        for second in (first + 1)..cards.len() {
            store::insert_edge(
                connection,
                &edge(
                    EdgeType::ConfusableWith,
                    endpoint(&cards[first]),
                    endpoint(&cards[second]),
                ),
            )
            .unwrap();
        }
    }

    let started = Instant::now();
    let neighbors = search::graph_neighbors(connection, &endpoint(&cards[0]), 8).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "BFS on 30 fully connected nodes took {elapsed:?}"
    );
    assert_eq!(neighbors.len(), cards.len() - 1);
    let unique: HashSet<String> = neighbors.iter().map(|node| node.endpoint.id()).collect();
    assert_eq!(unique.len(), cards.len() - 1);
    assert!(neighbors.iter().all(|node| node.depth == 1));
    assert!(!unique.contains(&cards[0].id.as_str()));
}

#[test]
fn fts_indexes_readable_text_not_json_keys() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let mut card = knowledge_card("過氧化氫酶");
    card.title = "過氧化氫酶".to_owned();
    card.blocks = vec![ContentBlock::Markdown {
        text: "分解活性氧的酵素".to_owned(),
    }];
    store::insert_knowledge_card(connection, &card).unwrap();

    assert!(
        search::search_knowledge_cards(connection, "markdown", 10)
            .unwrap()
            .is_empty(),
        "the JSON type tag must not be searchable"
    );
    assert!(
        search::search_knowledge_cards(connection, "活性氧", 10)
            .unwrap()
            .contains(&card.id)
    );

    let search_text: String = connection
        .query_row(
            "SELECT search_text FROM knowledge_cards WHERE id = ?1",
            [&card.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(search_text.contains("transport"));
    assert!(!search_text.contains('['));
    assert!(!search_text.contains('"'));
}

#[test]
fn question_search_includes_option_and_explanation_text() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let mut question = question_card("選出正確位置");
    question.options[0].content = "線粒體內膜".to_owned();
    question.options[0].explanation = Some("電子傳遞鏈位於此處".to_owned());
    store::insert_question_card(connection, &question).unwrap();

    assert!(
        search::search_question_cards(connection, "線粒體內膜", 10)
            .unwrap()
            .contains(&question.id)
    );
    assert!(
        search::search_question_cards(connection, "電子傳遞鏈", 10)
            .unwrap()
            .contains(&question.id)
    );
}
