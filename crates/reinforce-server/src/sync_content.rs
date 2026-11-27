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
use reinforce_core::primitives::{AnswerSourceKind, Error, ExportPolicy};
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

fn redaction_notice() -> ContentBlock {
    // The notice must not reveal which resource was withheld, so it never
    // carries the resource title.
    ContentBlock::Markdown {
        text: "[Restricted resource. Content redacted for this viewer.]".to_owned(),
    }
}

/// Mirrors `reinforce_export::model::visible_block` for one block.
fn restrict_block(
    block: &ContentBlock,
    restricted: &BTreeMap<ResourceId, ExportPolicy>,
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

    let mut redacted = false;
    for id in affected {
        match restricted.get(id) {
            None => {}
            Some(ExportPolicy::OwnerOnly) => return None,
            Some(ExportPolicy::Redact) => redacted = true,
            Some(ExportPolicy::Allow) => {}
        }
    }
    Some(if redacted {
        redaction_notice()
    } else {
        block.clone()
    })
}

fn restrict_blocks(
    blocks: &[ContentBlock],
    restricted: &BTreeMap<ResourceId, ExportPolicy>,
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

fn blob_path(blobs: &Path, hash: &str) -> std::path::PathBuf {
    let prefix = hash.get(..2).unwrap_or("xx").to_owned();
    blobs.join(prefix).join(hash)
}

/// Builds the snapshot for one viewer. The same database state always produces
/// the same bytes, so the hash is a useful "already up to date" check. Blobs
/// are not checked here: [`retain_stored_assets`] filters them after the
/// database lock is released.
pub fn snapshot(connection: &Connection, viewer_is_owner: bool) -> ApiResult<ContentSnapshot> {
    let notes = list_notes(connection, viewer_is_owner)?;
    let cards = list_knowledge_cards(connection, viewer_is_owner)?;
    let questions = list_question_cards(connection, viewer_is_owner)?;

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
    let mut restricted: BTreeMap<ResourceId, ExportPolicy> = BTreeMap::new();
    for resource in &resources {
        if viewer_is_owner || resource.export_policy == ExportPolicy::Allow {
            included_resources.insert(resource.id.clone());
        } else {
            restricted.insert(resource.id.clone(), resource.export_policy);
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

    let mut questions: Vec<QuestionCard> = questions
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
            ..question
        })
        .collect();

    // Trimming can remove what an answer provenance needs: an inferred answer
    // loses its anchors when every resource behind them is restricted, and a
    // duplicate loses its original when that question is not part of this
    // view. The importer rejects such a question, so it cannot travel at all;
    // dropping it keeps the rest of the snapshot importable. The fixpoint
    // handles chains where a duplicate points at a question that is itself
    // dropped.
    let mut kept_questions: BTreeSet<QuestionCardId> = questions
        .iter()
        .map(|question| question.id.clone())
        .collect();
    loop {
        let mut next = kept_questions.clone();
        for question in &questions {
            if next.contains(&question.id) && !provenance_survives(question, &next) {
                next.remove(&question.id);
            }
        }
        if next.len() == kept_questions.len() {
            break;
        }
        kept_questions = next;
    }
    questions.retain(|question| kept_questions.contains(&question.id));

    // Every viewer, including the owner, may receive fewer questions than a set
    // expects (archived or restricted rows). Count conservation would reject
    // the snapshot, so the flag has to follow the actual output.
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

    let included_question_ids: BTreeSet<QuestionCardId> = questions
        .iter()
        .map(|question| question.id.clone())
        .collect();
    let questions = order_questions_for_insert(questions);
    let kept_group_ids: BTreeSet<QuestionGroupId> = questions
        .iter()
        .filter_map(|question| question.question_group_id.clone())
        .collect();

    let groups: Vec<QuestionGroup> = groups
        .into_iter()
        .filter(|group| kept_group_ids.contains(&group.id))
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
            view_settings: restrict_canvas(&note.view_settings, &included_card_ids),
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

    let assets = resources
        .iter()
        .map(|resource| ContentAsset {
            resource_id: resource.id.clone(),
            sha256: resource.sha256.as_str().to_owned(),
            byte_size: resource.byte_size,
        })
        .collect();

    Ok(ContentSnapshot {
        bundle_jsonl,
        assets,
        snapshot_sha256,
    })
}

/// Drops assets whose blob is not on disk.
///
/// The caller runs this outside the database lock: a snapshot describes what
/// the database holds, and whether the bytes exist is a filesystem question
/// that should not block every other request.
pub fn retain_stored_assets(assets: &mut Vec<ContentAsset>, blobs: &Path) {
    assets.retain(|asset| blob_path(blobs, &asset.sha256).is_file());
}

/// Orders question cards so an original is inserted before the duplicates that
/// point at it. The importer writes rows with foreign keys enforced, so a
/// duplicate that precedes its original would reject the whole snapshot.
fn order_questions_for_insert(questions: Vec<QuestionCard>) -> Vec<QuestionCard> {
    let mut remaining = questions;
    let mut ordered = Vec::with_capacity(remaining.len());
    let mut emitted: BTreeSet<QuestionCardId> = BTreeSet::new();
    while !remaining.is_empty() {
        let mut progress = false;
        let mut index = 0;
        while index < remaining.len() {
            let ready = remaining[index]
                .duplicate_question_id
                .as_ref()
                .is_none_or(|id| emitted.contains(id));
            if ready {
                let question = remaining.remove(index);
                emitted.insert(question.id.clone());
                ordered.push(question);
                progress = true;
            } else {
                index += 1;
            }
        }
        if !progress {
            // Stored content cannot contain a duplicate cycle because it would
            // have failed the same foreign key when it was approved; keep the
            // remaining rows rather than dropping data silently.
            ordered.append(&mut remaining);
        }
    }
    ordered
}

/// Whether the answer provenance of a trimmed question still validates.
fn provenance_survives(question: &QuestionCard, kept: &BTreeSet<QuestionCardId>) -> bool {
    if !question.answer.iter().any(|value| !value.trim().is_empty()) {
        return true;
    }
    match question.answer_source_kind {
        AnswerSourceKind::Inferred => !question.anchor_ids.is_empty(),
        AnswerSourceKind::DuplicateQuestion => question
            .duplicate_question_id
            .as_ref()
            .is_some_and(|id| id != &question.id && kept.contains(id)),
        _ => true,
    }
}

/// Keeps canvas layout nodes only for cards that are part of the snapshot, so
/// the layout cannot disclose draft or archived card ids.
fn restrict_canvas(
    view_settings: &serde_json::Value,
    included: &BTreeSet<KnowledgeCardId>,
) -> serde_json::Value {
    let mut settings = view_settings.clone();
    let Some(nodes) = settings
        .get_mut("canvas")
        .and_then(|canvas| canvas.get_mut("nodes"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return settings;
    };
    nodes.retain(|node| {
        node.get("card_id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| included.iter().any(|card| card.as_str() == id))
    });
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use reinforce_core::content::{
        Anchor, KnowledgeCard, NormalizedRect, Note, OutlineNode, Resource,
    };
    use reinforce_core::primitives::{
        AnchorKind, AnswerSourceKind, CardStatus, GradingMode, QuestionType, ResourceKind,
        Sensitivity, SourceKind, Verification,
    };
    use reinforce_core::proposal::{ApprovalOptions, SubmitOutcome, approve_batch, submit_batch};
    use reinforce_core::validation::Actor;
    use reinforce_core::{Database, ExamSet, QuestionCard, Timestamp};
    use serde_json::json;

    const SET_ONE: &str = "01a21714-c98d-7931-a48a-000000000001";
    const SET_TWO: &str = "01a21714-c98d-7931-a48a-000000000002";
    const Q_INFERRED: &str = "01a21714-c98d-70f7-90ba-000000000001";
    const Q_DUPLICATE: &str = "01a21714-c98d-70f7-90ba-000000000002";
    const Q_DRAFT: &str = "01a21714-c98d-70f7-90ba-000000000003";
    const Q_ARCHIVED: &str = "01a21714-c98d-70f7-90ba-000000000004";
    const Q_PUBLISHED: &str = "01a21714-c98d-70f7-90ba-000000000005";
    const Q_DUPLICATE_ARCHIVED: &str = "01a21714-c98d-70f7-90ba-000000000006";
    const Q_ARCHIVED_ORIGINAL: &str = "01a21714-c98d-70f7-90ba-000000000007";
    const R_REDACT: &str = "01a21714-c98c-78d5-8550-000000000001";
    const R_OWNER: &str = "01a21714-c98c-78d5-8550-000000000002";
    const A_REDACT: &str = "01a21714-c98d-7814-a2c2-000000000001";
    const A_OWNER: &str = "01a21714-c98d-7814-a2c2-000000000002";
    const CARD_PUBLISHED: &str = "01a21714-c98d-716e-9b53-000000000001";
    const CARD_DRAFT: &str = "01a21714-c98d-716e-9b53-000000000002";
    const NOTE_ONE: &str = "01a21714-c98d-769f-be7a-000000000001";

    fn stamp(value: &str) -> Timestamp {
        value.parse().expect("timestamp")
    }

    fn exam_set(id: &str, expected: u32) -> ExamSet {
        ExamSet {
            id: id.parse().expect("set"),
            subject: "physics".to_owned(),
            title: "exam set".to_owned(),
            source_kind: SourceKind::PastExam,
            exam_year: None,
            source_resource_id: None,
            expected_questions: Some(expected),
            metadata: json!({}),
            is_complete: true,
        }
    }

    fn loose_exam_set(id: &str) -> ExamSet {
        ExamSet {
            expected_questions: None,
            is_complete: false,
            ..exam_set(id, 0)
        }
    }

    fn question(id: &str, set: &str, number: u32) -> QuestionCard {
        QuestionCard {
            id: id.parse().expect("question"),
            exam_set_id: Some(set.parse().expect("set")),
            question_group_id: None,
            question_no: Some(number),
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
            metadata: json!({}),
            created_at: stamp("2026-01-01T00:00:00.000000Z"),
            updated_at: stamp("2026-01-01T00:00:00.000000Z"),
        }
    }

    fn resource(id: &str, policy: ExportPolicy, title: &str) -> Resource {
        Resource {
            id: id.parse().expect("resource"),
            kind: ResourceKind::Image,
            title: title.to_owned(),
            sha256: ContentHash::from_bytes(id.as_bytes()),
            byte_size: 128,
            page_count: None,
            width: None,
            height: None,
            license: None,
            attribution_url: None,
            attribution_notice: None,
            sensitivity: Sensitivity::Restricted,
            export_policy: policy,
            created_at: stamp("2026-01-01T00:00:00.000000Z"),
        }
    }

    fn anchor(id: &str, resource_id: &str) -> Anchor {
        Anchor {
            id: id.parse().expect("anchor"),
            resource_id: resource_id.parse().expect("resource"),
            anchor_kind: AnchorKind::Rect,
            pdf_page: None,
            printed_page: None,
            legend_pdf_page: None,
            rect: Some(NormalizedRect {
                x: 0.1,
                y: 0.1,
                w: 0.2,
                h: 0.2,
            }),
            media_span: None,
        }
    }

    fn knowledge_card(id: &str, status: CardStatus, blocks: Vec<ContentBlock>) -> KnowledgeCard {
        KnowledgeCard {
            id: id.parse().expect("card"),
            subject: "physics".to_owned(),
            topics: Vec::new(),
            title: "card".to_owned(),
            blocks,
            anchor_ids: Vec::new(),
            status,
            verification: Verification::Unchecked,
            unsure_count: 0,
            metadata: json!({}),
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

    fn set_line(bundle: &str) -> &str {
        bundle
            .lines()
            .find(|line| line.contains("\"op\":\"exam_set\""))
            .expect("exam set line")
    }

    #[test]
    fn snapshot_of_an_empty_store_is_empty_and_stable() {
        let database = Database::open_in_memory().unwrap();
        let snapshot = snapshot(database.connection(), false).unwrap();
        assert!(snapshot.bundle_jsonl.is_empty());
        assert!(snapshot.assets.is_empty());
        assert_eq!(
            snapshot.snapshot_sha256,
            ContentHash::from_bytes(b"").as_str()
        );
    }

    #[test]
    fn a_partial_exam_set_is_marked_incomplete_for_every_viewer() {
        let mut database = Database::open_in_memory().expect("database");
        {
            let connection = database.connection_mut();
            content_store::insert_exam_set(connection, &exam_set(SET_ONE, 2)).expect("set");
            content_store::insert_question_card(connection, &question(Q_PUBLISHED, SET_ONE, 1))
                .expect("question");
            let mut archived = question(Q_ARCHIVED, SET_ONE, 2);
            archived.status = CardStatus::Archived;
            content_store::insert_question_card(connection, &archived).expect("archived");
        }

        // The owner loses the archived question too, so the flag has to follow
        // the actual output instead of the stored value.
        for owner in [true, false] {
            let snapshot = snapshot(database.connection(), owner).expect("snapshot");
            assert!(
                set_line(&snapshot.bundle_jsonl).contains("\"is_complete\":false"),
                "viewer owner={owner} must receive an incomplete set"
            );
            imported_snapshot(&snapshot.bundle_jsonl).expect("snapshot must import");
        }
    }

    #[test]
    fn inferred_questions_without_visible_anchors_are_dropped_for_learners() {
        let mut database = Database::open_in_memory().expect("database");
        {
            let connection = database.connection_mut();
            content_store::store_resource(
                connection,
                &resource(R_REDACT, ExportPolicy::Redact, "chapter-4-secret.pdf"),
            )
            .expect("redacted resource");
            content_store::store_resource(
                connection,
                &resource(R_OWNER, ExportPolicy::OwnerOnly, "owner-notes.pdf"),
            )
            .expect("owner resource");
            content_store::insert_anchor(connection, &anchor(A_REDACT, R_REDACT)).expect("anchor");
            content_store::insert_anchor(connection, &anchor(A_OWNER, R_OWNER)).expect("anchor");
            content_store::insert_exam_set(connection, &loose_exam_set(SET_ONE)).expect("set");
            let mut inferred = question(Q_INFERRED, SET_ONE, 1);
            inferred.answer_source_kind = AnswerSourceKind::Inferred;
            inferred.answer_basis = Some("read from the figure".to_owned());
            inferred.anchor_ids = vec![
                A_REDACT.parse().expect("anchor"),
                A_OWNER.parse().expect("anchor"),
            ];
            content_store::insert_question_card(connection, &inferred).expect("question");
        }

        let owner_snapshot = snapshot(database.connection(), true).expect("owner");
        assert!(owner_snapshot.bundle_jsonl.contains(Q_INFERRED));
        imported_snapshot(&owner_snapshot.bundle_jsonl).expect("owner import");

        let learner_snapshot = snapshot(database.connection(), false).expect("learner");
        assert!(
            !learner_snapshot.bundle_jsonl.contains(Q_INFERRED),
            "an inferred answer without anchors cannot be imported"
        );
        assert!(!learner_snapshot.bundle_jsonl.contains(A_REDACT));
        assert!(!learner_snapshot.bundle_jsonl.contains(A_OWNER));
        imported_snapshot(&learner_snapshot.bundle_jsonl).expect("learner import");
    }

    #[test]
    fn duplicate_questions_are_dropped_when_their_original_is_not_in_the_snapshot() {
        let mut database = Database::open_in_memory().expect("database");
        {
            let connection = database.connection_mut();
            content_store::insert_exam_set(connection, &loose_exam_set(SET_ONE)).expect("set");
            content_store::insert_exam_set(connection, &loose_exam_set(SET_TWO)).expect("set");

            // A draft original is invisible to learners but visible to the owner.
            let mut draft = question(Q_DRAFT, SET_ONE, 1);
            draft.status = CardStatus::Draft;
            content_store::insert_question_card(connection, &draft).expect("draft");
            let mut duplicate = question(Q_DUPLICATE, SET_ONE, 2);
            duplicate.answer_source_kind = AnswerSourceKind::DuplicateQuestion;
            duplicate.answer_basis = Some("same as the draft question".to_owned());
            duplicate.duplicate_question_id = Some(Q_DRAFT.parse().expect("question"));
            content_store::insert_question_card(connection, &duplicate).expect("duplicate");

            // An archived original is invisible to everyone.
            let mut archived = question(Q_ARCHIVED_ORIGINAL, SET_TWO, 1);
            archived.status = CardStatus::Archived;
            content_store::insert_question_card(connection, &archived).expect("archived");
            let mut duplicate = question(Q_DUPLICATE_ARCHIVED, SET_TWO, 2);
            duplicate.answer_source_kind = AnswerSourceKind::DuplicateQuestion;
            duplicate.answer_basis = Some("same as the archived question".to_owned());
            duplicate.duplicate_question_id = Some(Q_ARCHIVED_ORIGINAL.parse().expect("question"));
            content_store::insert_question_card(connection, &duplicate).expect("duplicate");
        }

        let owner_snapshot = snapshot(database.connection(), true).expect("owner");
        assert!(owner_snapshot.bundle_jsonl.contains(Q_DUPLICATE));
        assert!(
            !owner_snapshot.bundle_jsonl.contains(Q_DUPLICATE_ARCHIVED),
            "the archived original is invisible to the owner as well"
        );
        imported_snapshot(&owner_snapshot.bundle_jsonl).expect("owner import");

        let learner_snapshot = snapshot(database.connection(), false).expect("learner");
        assert!(!learner_snapshot.bundle_jsonl.contains(Q_DUPLICATE));
        assert!(!learner_snapshot.bundle_jsonl.contains(Q_DUPLICATE_ARCHIVED));
        imported_snapshot(&learner_snapshot.bundle_jsonl).expect("learner import");
    }

    #[test]
    fn canvas_layout_only_keeps_cards_that_are_part_of_the_snapshot() {
        let mut database = Database::open_in_memory().expect("database");
        {
            let connection = database.connection_mut();
            content_store::insert_knowledge_card(
                connection,
                &knowledge_card(CARD_PUBLISHED, CardStatus::Published, Vec::new()),
            )
            .expect("published card");
            content_store::insert_knowledge_card(
                connection,
                &knowledge_card(CARD_DRAFT, CardStatus::Draft, Vec::new()),
            )
            .expect("draft card");
            let note = Note {
                id: NOTE_ONE.parse().expect("note"),
                subject: "physics".to_owned(),
                title: "canvas note".to_owned(),
                outline: vec![OutlineNode::Section {
                    heading: "layout".to_owned(),
                    intro_markdown: None,
                    children: vec![
                        OutlineNode::CardRef {
                            card_id: CARD_PUBLISHED.parse().expect("card"),
                        },
                        OutlineNode::CardRef {
                            card_id: CARD_DRAFT.parse().expect("card"),
                        },
                    ],
                }],
                view_settings: json!({
                    "canvas": {
                        "version": 1,
                        "viewport": {"x": 0.0, "y": 0.0, "zoom": 1.0},
                        "nodes": [
                            {
                                "card_id": CARD_PUBLISHED,
                                "x": 10.0, "y": 20.0, "w": 100.0, "h": 80.0
                            },
                            {
                                "card_id": CARD_DRAFT,
                                "x": 30.0, "y": 40.0, "w": 100.0, "h": 80.0
                            }
                        ]
                    }
                }),
                status: CardStatus::Published,
                created_at: stamp("2026-01-01T00:00:00.000000Z"),
                updated_at: stamp("2026-01-01T00:00:00.000000Z"),
            };
            content_store::insert_note(connection, &note).expect("note");
        }

        let learner_snapshot = snapshot(database.connection(), false).expect("learner");
        assert!(
            !learner_snapshot.bundle_jsonl.contains(CARD_DRAFT),
            "a draft card id must not travel through the canvas layout"
        );
        assert!(learner_snapshot.bundle_jsonl.contains(CARD_PUBLISHED));
        let note_line = learner_snapshot
            .bundle_jsonl
            .lines()
            .find(|line| line.contains("\"op\":\"note\""))
            .expect("note line");
        let operation: BatchOperation = serde_json::from_str(note_line).expect("operation");
        let settings = operation.data().clone();
        let nodes = settings["view_settings"]["canvas"]["nodes"]
            .as_array()
            .expect("canvas nodes");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["card_id"], CARD_PUBLISHED);
        imported_snapshot(&learner_snapshot.bundle_jsonl).expect("learner import");

        let owner_snapshot = snapshot(database.connection(), true).expect("owner");
        let note_line = owner_snapshot
            .bundle_jsonl
            .lines()
            .find(|line| line.contains("\"op\":\"note\""))
            .expect("note line");
        let operation: BatchOperation = serde_json::from_str(note_line).expect("operation");
        let settings = operation.data().clone();
        assert_eq!(
            settings["view_settings"]["canvas"]["nodes"]
                .as_array()
                .expect("canvas nodes")
                .len(),
            2,
            "the owner keeps the full layout"
        );
    }

    #[test]
    fn redacted_media_does_not_reveal_its_title() {
        let mut database = Database::open_in_memory().expect("database");
        {
            let connection = database.connection_mut();
            content_store::store_resource(
                connection,
                &resource(R_REDACT, ExportPolicy::Redact, "chapter-4-secret.pdf"),
            )
            .expect("redacted resource");
            let card = knowledge_card(
                CARD_PUBLISHED,
                CardStatus::Published,
                vec![ContentBlock::InteractivePins {
                    resource_id: R_REDACT.parse().expect("resource"),
                    overlay_resource_id: None,
                    pins: Vec::new(),
                }],
            );
            content_store::insert_knowledge_card(connection, &card).expect("card");
        }

        let learner_snapshot = snapshot(database.connection(), false).expect("learner");
        assert!(
            learner_snapshot
                .bundle_jsonl
                .contains("Content redacted for this viewer")
        );
        assert!(
            !learner_snapshot
                .bundle_jsonl
                .contains("chapter-4-secret.pdf"),
            "the notice must not name the withheld resource"
        );
        assert!(!learner_snapshot.bundle_jsonl.contains(R_REDACT));
        imported_snapshot(&learner_snapshot.bundle_jsonl).expect("learner import");

        let owner_snapshot = snapshot(database.connection(), true).expect("owner");
        assert!(owner_snapshot.bundle_jsonl.contains("chapter-4-secret.pdf"));
    }
}
