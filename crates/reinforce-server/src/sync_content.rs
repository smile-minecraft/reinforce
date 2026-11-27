//! Published content snapshots for device sync.
//!
//! A snapshot is the same newline-delimited bundle the CLI importer consumes,
//! so a device applies it with the one importer that already validates and
//! stores content, and binary payloads keep travelling as content-addressed
//! blobs. Two rules shape the selection:
//!
//! * visibility follows the existing read routes: owners see everything that is
//!   not archived, learners only see `published` rows;
//! * `export_policy` follows the export crate: `allow` travels with its bytes,
//!   `redact` becomes a notice instead of the media, and `owner_only` media and
//!   the blocks that only exist for it are dropped.
//!
//! Nothing that references a dropped row is emitted, because the importer
//! validates referential integrity and a dangling anchor or linked card would
//! reject the whole snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use reinforce_core::content::{
    Anchor, ContentBlock, DiagramNodeBinding, Edge, EdgeEndpoint, ExamSet, KnowledgeCard, Note,
    OutlineNode, Pin, QuestionCard, QuestionGroup, Resource,
};
use reinforce_core::content_store;
use reinforce_core::primitives::{Error, ExportPolicy};
use reinforce_core::{
    AnchorId, BatchOperation, ContentHash, ExamSetId, KnowledgeCardId, NoteId, OperationKind,
    QuestionCardId, QuestionGroupId, ResourceId,
};
use rusqlite::Connection;

use crate::error::ApiResult;

/// One blob the device still has to download through `/api/resources/{id}/blob`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentAsset {
    pub resource_id: ResourceId,
    pub sha256: String,
    pub byte_size: i64,
}

/// A complete, deterministic view of the content a device may store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSnapshot {
    pub bundle_jsonl: String,
    pub assets: Vec<ContentAsset>,
    /// sha256 of `bundle_jsonl`; equal hashes mean the device is up to date.
    pub snapshot_sha256: String,
}

fn status_sql(owner: bool) -> &'static str {
    if owner {
        "status <> 'archived'"
    } else {
        "status = 'published'"
    }
}

fn query_ids<T: FromStr<Err = Error>>(connection: &Connection, sql: &str) -> ApiResult<Vec<T>> {
    let mut statement = connection.prepare(sql)?;
    let values = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut ids = Vec::with_capacity(values.len());
    for value in values {
        ids.push(value.parse::<T>()?);
    }
    Ok(ids)
}

fn list_notes(connection: &Connection, owner: bool) -> ApiResult<Vec<Note>> {
    let sql = format!(
        "SELECT id FROM notes WHERE {} ORDER BY id",
        status_sql(owner)
    );
    let mut notes = Vec::new();
    for id in query_ids::<NoteId>(connection, &sql)? {
        if let Some(note) = content_store::get_note(connection, &id)? {
            notes.push(note);
        }
    }
    Ok(notes)
}

fn list_knowledge_cards(connection: &Connection, owner: bool) -> ApiResult<Vec<KnowledgeCard>> {
    let sql = format!(
        "SELECT id FROM knowledge_cards WHERE {} ORDER BY id",
        status_sql(owner)
    );
    let mut cards = Vec::new();
    for id in query_ids::<KnowledgeCardId>(connection, &sql)? {
        if let Some(card) = content_store::get_knowledge_card(connection, &id)? {
            cards.push(card);
        }
    }
    Ok(cards)
}

fn list_question_cards(connection: &Connection, owner: bool) -> ApiResult<Vec<QuestionCard>> {
    let sql = format!(
        "SELECT id FROM question_cards WHERE {} ORDER BY id",
        status_sql(owner)
    );
    let mut questions = Vec::new();
    for id in query_ids::<QuestionCardId>(connection, &sql)? {
        if let Some(card) = content_store::get_question_card(connection, &id)? {
            questions.push(card);
        }
    }
    Ok(questions)
}

/// Owners receive every exam set; learners only sets that carry a published
/// question (mirroring `/api/exam_sets`) plus the sets referenced by the groups
/// that are pulled in.
fn select_exam_sets(
    connection: &Connection,
    owner: bool,
    referenced: &BTreeSet<ExamSetId>,
) -> ApiResult<Vec<ExamSet>> {
    let mut ids = if owner {
        query_ids::<ExamSetId>(connection, "SELECT id FROM exam_sets ORDER BY id")?
    } else {
        query_ids::<ExamSetId>(
            connection,
            "SELECT id FROM exam_sets WHERE EXISTS (
                 SELECT 1 FROM question_cards q
                 WHERE q.exam_set_id = exam_sets.id AND q.status = 'published'
             ) ORDER BY id",
        )?
    };
    for id in referenced {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids.sort();
    let mut sets = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(set) = content_store::get_exam_set(connection, &id)? {
            sets.push(set);
        }
    }
    Ok(sets)
}

fn select_question_groups(
    connection: &Connection,
    ids: &BTreeSet<QuestionGroupId>,
) -> ApiResult<Vec<QuestionGroup>> {
    let mut groups = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(group) = content_store::get_question_group(connection, id)? {
            groups.push(group);
        }
    }
    Ok(groups)
}

fn block_resource_ids(blocks: &[ContentBlock], ids: &mut BTreeSet<ResourceId>) {
    for block in blocks {
        match block {
            ContentBlock::ImageOcclusion { resource_id, .. } => {
                ids.insert(resource_id.clone());
            }
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                ..
            } => {
                ids.insert(resource_id.clone());
                if let Some(id) = overlay_resource_id {
                    ids.insert(id.clone());
                }
            }
            _ => {}
        }
    }
}

fn redaction_notice(title: &str) -> ContentBlock {
    ContentBlock::Markdown {
        text: format!("[Restricted resource: {title}. Content redacted for this viewer.]"),
    }
}

/// Mirrors `reinforce_export::model::visible_block` for one block.
fn restrict_block(
    block: &ContentBlock,
    restricted: &BTreeMap<ResourceId, (ExportPolicy, String)>,
) -> Option<ContentBlock> {
    let affected: Vec<&ResourceId> = match block {
        ContentBlock::ImageOcclusion { resource_id, .. } => vec![resource_id],
        ContentBlock::InteractivePins {
            resource_id,
            overlay_resource_id,
            ..
        } => {
            let mut ids = vec![resource_id];
            if let Some(id) = overlay_resource_id {
                ids.push(id);
            }
            ids
        }
        _ => return Some(block.clone()),
    };

    let mut redacted: Option<&str> = None;
    for id in affected {
        match restricted.get(id) {
            None => {}
            Some((ExportPolicy::OwnerOnly, _)) => return None,
            Some((ExportPolicy::Redact, title)) => redacted = Some(title.as_str()),
            Some((ExportPolicy::Allow, _)) => {}
        }
    }
    Some(redacted.map_or_else(|| block.clone(), redaction_notice))
}

fn restrict_blocks(
    blocks: &[ContentBlock],
    restricted: &BTreeMap<ResourceId, (ExportPolicy, String)>,
) -> Vec<ContentBlock> {
    blocks
        .iter()
        .filter_map(|block| restrict_block(block, restricted))
        .collect()
}

fn restrict_outline(
    nodes: &[OutlineNode],
    included: &BTreeSet<KnowledgeCardId>,
) -> Vec<OutlineNode> {
    nodes
        .iter()
        .filter_map(|node| match node {
            OutlineNode::Section {
                heading,
                intro_markdown,
                children,
            } => Some(OutlineNode::Section {
                heading: heading.clone(),
                intro_markdown: intro_markdown.clone(),
                children: restrict_outline(children, included),
            }),
            OutlineNode::CardRef { card_id } => {
                included.contains(card_id).then(|| OutlineNode::CardRef {
                    card_id: card_id.clone(),
                })
            }
        })
        .collect()
}

fn restrict_pin(pin: &Pin, included: &BTreeSet<KnowledgeCardId>) -> Pin {
    let mut pin = pin.clone();
    if pin
        .linked_card_id
        .as_ref()
        .is_some_and(|id| !included.contains(id))
    {
        pin.linked_card_id = None;
    }
    pin
}

fn restrict_diagram(
    bindings: &[DiagramNodeBinding],
    included: &BTreeSet<KnowledgeCardId>,
) -> Vec<DiagramNodeBinding> {
    bindings
        .iter()
        .filter(|binding| included.contains(&binding.card_id))
        .cloned()
        .collect()
}

/// Rewrites a block so it never points at a card that is not part of the
/// snapshot. Diagram bindings and pin hints are navigation aids, so they are
/// simply dropped instead of dropping the whole block.
fn restrict_linked_cards(
    blocks: &[ContentBlock],
    included: &BTreeSet<KnowledgeCardId>,
) -> Vec<ContentBlock> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Diagram {
                source,
                syntax,
                svg_cache,
                node_bindings,
            } => ContentBlock::Diagram {
                source: source.clone(),
                syntax: *syntax,
                svg_cache: svg_cache.clone(),
                node_bindings: restrict_diagram(node_bindings, included),
            },
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                pins,
            } => ContentBlock::InteractivePins {
                resource_id: resource_id.clone(),
                overlay_resource_id: overlay_resource_id.clone(),
                pins: pins.iter().map(|pin| restrict_pin(pin, included)).collect(),
            },
            other => other.clone(),
        })
        .collect()
}

fn endpoint_included(
    endpoint: &EdgeEndpoint,
    cards: &BTreeSet<KnowledgeCardId>,
    questions: &BTreeSet<QuestionCardId>,
) -> bool {
    match endpoint {
        EdgeEndpoint::Card { id } => cards.contains(id),
        EdgeEndpoint::Question { id } | EdgeEndpoint::Option { id, .. } => questions.contains(id),
    }
}

fn select_edges(
    connection: &Connection,
    cards: &BTreeSet<KnowledgeCardId>,
    questions: &BTreeSet<QuestionCardId>,
) -> ApiResult<Vec<Edge>> {
    let ids = query_ids::<reinforce_core::EdgeId>(connection, "SELECT id FROM edges ORDER BY id")?;
    let mut edges = Vec::new();
    for id in ids {
        let Some(edge) = content_store::get_edge(connection, &id)? else {
            continue;
        };
        if endpoint_included(&edge.from, cards, questions)
            && endpoint_included(&edge.to, cards, questions)
        {
            edges.push(edge);
        }
    }
    Ok(edges)
}

fn push_line(
    lines: &mut Vec<String>,
    kind: OperationKind,
    entity: &impl serde::Serialize,
) -> ApiResult<()> {
    let operation = BatchOperation::from_entity(kind, None, entity)?;
    lines.push(serde_json::to_string(&operation)?);
    Ok(())
}

fn blob_path(blobs: &Path, hash: &ContentHash) -> std::path::PathBuf {
    let prefix = hash.as_str().get(..2).unwrap_or("xx").to_owned();
    blobs.join(prefix).join(hash.as_str())
}

/// Builds the snapshot for one viewer. The same database state always produces
/// the same bytes, so the hash is a useful "already up to date" check.
pub fn snapshot(
    connection: &Connection,
    blobs: &Path,
    viewer_is_owner: bool,
) -> ApiResult<ContentSnapshot> {
    let notes = list_notes(connection, viewer_is_owner)?;
    let cards = list_knowledge_cards(connection, viewer_is_owner)?;
    let mut questions = list_question_cards(connection, viewer_is_owner)?;

    let group_ids: BTreeSet<QuestionGroupId> = questions
        .iter()
        .filter_map(|question| question.question_group_id.clone())
        .collect();
    let groups = select_question_groups(connection, &group_ids)?;

    let mut set_ids: BTreeSet<ExamSetId> = questions
        .iter()
        .filter_map(|question| question.exam_set_id.clone())
        .collect();
    set_ids.extend(groups.iter().filter_map(|group| group.exam_set_id.clone()));
    let mut sets = select_exam_sets(connection, viewer_is_owner, &set_ids)?;

    let mut resource_ids: BTreeSet<ResourceId> = BTreeSet::new();
    let mut anchor_ids: BTreeSet<AnchorId> = BTreeSet::new();
    for card in &cards {
        anchor_ids.extend(card.anchor_ids.iter().cloned());
        block_resource_ids(&card.blocks, &mut resource_ids);
    }
    for question in &questions {
        anchor_ids.extend(question.anchor_ids.iter().cloned());
        block_resource_ids(&question.stem_blocks, &mut resource_ids);
        block_resource_ids(&question.explanation_blocks, &mut resource_ids);
    }
    for group in &groups {
        block_resource_ids(&group.shared_stem_blocks, &mut resource_ids);
    }
    for set in &sets {
        if let Some(id) = &set.source_resource_id {
            resource_ids.insert(id.clone());
        }
    }

    let mut anchors: Vec<Anchor> = Vec::new();
    for id in &anchor_ids {
        if let Some(anchor) = content_store::get_anchor(connection, id)? {
            resource_ids.insert(anchor.resource_id.clone());
            anchors.push(anchor);
        }
    }

    let mut resources: Vec<Resource> = Vec::new();
    for id in &resource_ids {
        if let Some(resource) = content_store::get_resource(connection, id)? {
            resources.push(resource);
        }
    }

    let mut included_resources: BTreeSet<ResourceId> = BTreeSet::new();
    let mut restricted: BTreeMap<ResourceId, (ExportPolicy, String)> = BTreeMap::new();
    for resource in &resources {
        if viewer_is_owner || resource.export_policy == ExportPolicy::Allow {
            included_resources.insert(resource.id.clone());
        } else {
            restricted.insert(
                resource.id.clone(),
                (resource.export_policy, resource.title.clone()),
            );
        }
    }
    anchors.retain(|anchor| included_resources.contains(&anchor.resource_id));
    sets.iter_mut().for_each(|set| {
        if set
            .source_resource_id
            .as_ref()
            .is_some_and(|id| !included_resources.contains(id))
        {
            set.source_resource_id = None;
        }
    });

    let included_card_ids: BTreeSet<KnowledgeCardId> =
        cards.iter().map(|card| card.id.clone()).collect();
    let included_question_ids: BTreeSet<QuestionCardId> =
        questions.iter().map(|card| card.id.clone()).collect();

    let cards: Vec<KnowledgeCard> = cards
        .into_iter()
        .map(|card| KnowledgeCard {
            blocks: restrict_linked_cards(
                &restrict_blocks(&card.blocks, &restricted),
                &included_card_ids,
            ),
            anchor_ids: card
                .anchor_ids
                .into_iter()
                .filter(|id| anchors.iter().any(|anchor| anchor.id == *id))
                .collect(),
            ..card
        })
        .collect();

    questions = questions
        .into_iter()
        .map(|question| QuestionCard {
            stem_blocks: restrict_linked_cards(
                &restrict_blocks(&question.stem_blocks, &restricted),
                &included_card_ids,
            ),
            explanation_blocks: restrict_linked_cards(
                &restrict_blocks(&question.explanation_blocks, &restricted),
                &included_card_ids,
            ),
            anchor_ids: question
                .anchor_ids
                .into_iter()
                .filter(|id| anchors.iter().any(|anchor| anchor.id == *id))
                .collect(),
            options: question
                .options
                .into_iter()
                .map(|option| reinforce_core::content::QuestionOption {
                    linked_card_ids: option
                        .linked_card_ids
                        .into_iter()
                        .filter(|id| included_card_ids.contains(id))
                        .collect(),
                    ..option
                })
                .collect(),
            duplicate_question_id: question
                .duplicate_question_id
                .filter(|id| included_question_ids.contains(id)),
            ..question
        })
        .collect();

    // A learner only receives published questions, so a complete exam set may
    // arrive with fewer cards than it expects. Keeping the flag would make
    // count conservation reject the whole snapshot on import.
    if !viewer_is_owner {
        for set in &mut sets {
            let (Some(expected), true) = (set.expected_questions, set.is_complete) else {
                continue;
            };
            let included = questions
                .iter()
                .filter(|question| question.exam_set_id.as_ref() == Some(&set.id))
                .count();
            if included != expected as usize {
                set.is_complete = false;
            }
        }
    }

    let groups: Vec<QuestionGroup> = groups
        .into_iter()
        .map(|group| QuestionGroup {
            shared_stem_blocks: restrict_linked_cards(
                &restrict_blocks(&group.shared_stem_blocks, &restricted),
                &included_card_ids,
            ),
            ..group
        })
        .collect();

    let notes: Vec<Note> = notes
        .into_iter()
        .map(|note| Note {
            outline: restrict_outline(&note.outline, &included_card_ids),
            ..note
        })
        .collect();

    let edges = select_edges(connection, &included_card_ids, &included_question_ids)?;
    let resources: Vec<Resource> = resources
        .into_iter()
        .filter(|resource| included_resources.contains(&resource.id))
        .collect();

    let mut lines = Vec::new();
    for resource in &resources {
        push_line(&mut lines, OperationKind::Resource, resource)?;
    }
    for anchor in &anchors {
        push_line(&mut lines, OperationKind::Anchor, anchor)?;
    }
    for set in &sets {
        push_line(&mut lines, OperationKind::ExamSet, set)?;
    }
    for group in &groups {
        push_line(&mut lines, OperationKind::QuestionGroup, group)?;
    }
    for card in &cards {
        push_line(&mut lines, OperationKind::KnowledgeCard, card)?;
    }
    for note in &notes {
        push_line(&mut lines, OperationKind::Note, note)?;
    }
    for question in &questions {
        push_line(&mut lines, OperationKind::QuestionCard, question)?;
    }
    for edge in &edges {
        push_line(&mut lines, OperationKind::Edge, edge)?;
    }

    let mut bundle_jsonl = lines.join("\n");
    if !bundle_jsonl.is_empty() {
        bundle_jsonl.push('\n');
    }
    let snapshot_sha256 = ContentHash::from_bytes(bundle_jsonl.as_bytes())
        .as_str()
        .to_owned();

    let mut assets = Vec::new();
    for resource in &resources {
        if blob_path(blobs, &resource.sha256).is_file() {
            assets.push(ContentAsset {
                resource_id: resource.id.clone(),
                sha256: resource.sha256.as_str().to_owned(),
                byte_size: resource.byte_size,
            });
        }
    }

    Ok(ContentSnapshot {
        bundle_jsonl,
        assets,
        snapshot_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reinforce_core::primitives::{
        AnswerSourceKind, CardStatus, GradingMode, QuestionType, SourceKind, Verification,
    };
    use reinforce_core::proposal::{
        ApprovalOptions, BatchOperation, SubmitOutcome, approve_batch, submit_batch,
    };
    use reinforce_core::validation::Actor;
    use reinforce_core::{Database, ExamSet, QuestionCard, Timestamp};

    fn stamp(value: &str) -> Timestamp {
        value.parse().expect("timestamp")
    }

    fn exam_set() -> ExamSet {
        ExamSet {
            id: "01a21714-c98d-7931-a48a-6a1d5cd348bd".parse().expect("set"),
            subject: "physics".to_owned(),
            title: "partial set".to_owned(),
            source_kind: SourceKind::PastExam,
            exam_year: None,
            source_resource_id: None,
            expected_questions: Some(2),
            metadata: serde_json::json!({}),
            is_complete: true,
        }
    }

    fn question_card(set_id: &str) -> QuestionCard {
        QuestionCard {
            id: "01a21714-c98d-70f7-90ba-1bced11dd072"
                .parse()
                .expect("card"),
            exam_set_id: Some(set_id.parse().expect("set")),
            question_group_id: None,
            question_no: Some(1),
            subject: "physics".to_owned(),
            topics: Vec::new(),
            stem_blocks: vec![ContentBlock::Markdown {
                text: "Which phase?".to_owned(),
            }],
            explanation_blocks: Vec::new(),
            anchor_ids: Vec::new(),
            question_type: QuestionType::SingleChoice,
            grading_mode: GradingMode::Exact,
            options: vec![
                reinforce_core::content::QuestionOption {
                    key: "A".to_owned(),
                    content: "solid".to_owned(),
                    explanation: None,
                    linked_card_ids: Vec::new(),
                },
                reinforce_core::content::QuestionOption {
                    key: "B".to_owned(),
                    content: "liquid".to_owned(),
                    explanation: None,
                    linked_card_ids: Vec::new(),
                },
            ],
            answer: vec!["A".to_owned()],
            accepted_answers: Vec::new(),
            answer_source_kind: AnswerSourceKind::Official,
            answer_basis: None,
            duplicate_question_id: None,
            status: CardStatus::Published,
            verification: Verification::Unchecked,
            unsure_count: 0,
            metadata: serde_json::json!({}),
            created_at: stamp("2026-01-01T00:00:00.000000Z"),
            updated_at: stamp("2026-01-01T00:00:00.000000Z"),
        }
    }

    fn imported_snapshot(bundle: &str) -> Result<usize, String> {
        let mut database = Database::open_in_memory().expect("database");
        let operations: Vec<BatchOperation> = bundle
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("operation"))
            .collect();
        let proposal = match submit_batch(database.connection_mut(), Actor::Owner, operations)
            .expect("submit")
        {
            SubmitOutcome::Accepted { proposal } => proposal,
            SubmitOutcome::Rejected { errors } => return Err(format!("{errors:?}")),
        };
        match approve_batch(
            database.connection_mut(),
            &proposal.id,
            ApprovalOptions {
                reviewer: Actor::Owner,
                promote_verified: true,
            },
        )
        .expect("approve")
        {
            reinforce_core::proposal::ApprovalOutcome::Applied { applied, .. } => Ok(applied),
            reinforce_core::proposal::ApprovalOutcome::Rejected { errors, .. } => {
                Err(format!("{errors:?}"))
            }
        }
    }

    #[test]
    fn snapshot_of_an_empty_store_is_empty_and_stable() {
        let database = Database::open_in_memory().unwrap();
        let snapshot = snapshot(database.connection(), Path::new("."), false).unwrap();
        assert!(snapshot.bundle_jsonl.is_empty());
        assert!(snapshot.assets.is_empty());
        assert_eq!(
            snapshot.snapshot_sha256,
            ContentHash::from_bytes(b"").as_str()
        );
    }

    #[test]
    fn a_partial_exam_set_is_marked_incomplete_for_learners() {
        let mut database = Database::open_in_memory().expect("database");
        let connection = database.connection_mut();
        let set = exam_set();
        content_store::insert_exam_set(connection, &set).expect("set");
        content_store::insert_question_card(connection, &question_card(set.id.as_str().as_str()))
            .expect("question");

        let owner_snapshot = snapshot(database.connection(), Path::new("."), true).expect("owner");
        assert!(
            imported_snapshot(&owner_snapshot.bundle_jsonl).is_err(),
            "the owner sees the real, incomplete store"
        );

        let learner_snapshot =
            snapshot(database.connection(), Path::new("."), false).expect("learner");
        let set_line = learner_snapshot
            .bundle_jsonl
            .lines()
            .find(|line| line.contains("\"op\":\"exam_set\""))
            .expect("exam set line");
        assert!(set_line.contains("\"is_complete\":false"));
        assert_eq!(
            imported_snapshot(&learner_snapshot.bundle_jsonl).expect("learner import"),
            2
        );
    }
}
