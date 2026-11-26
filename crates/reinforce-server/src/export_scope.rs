//! Selection of exportable content shared by the CLI and the HTTP API.
//!
//! The export crate is deliberately unaware of the database. This module is
//! the only place that turns a user-facing scope into an [`ExportInput`],
//! including visibility filtering and inline blobs from the sharded store.

use std::collections::BTreeSet;
use std::path::Path;
use std::str::FromStr;

use reinforce_core::content::OutlineNode;
use reinforce_core::content_store;
use reinforce_core::primitives::{CardStatus, EdgeType};
use reinforce_core::weakness;
use reinforce_core::{
    ContentBlock, ContentHash, KnowledgeCard, KnowledgeCardId, Note, NoteId, QuestionCard,
    QuestionCardId, ResourceId, UserId,
};
use reinforce_export::{ExportInput, ResourceBlob};
use rusqlite::Connection;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone)]
pub enum ExportScope {
    Note { note_id: NoteId },
    Subject { subject: String },
    Weakness { threshold: f64, user_id: UserId },
}

fn status_is_visible(status: CardStatus, viewer_is_owner: bool) -> bool {
    if viewer_is_owner {
        status != CardStatus::Archived
    } else {
        status == CardStatus::Published
    }
}

fn status_sql(viewer_is_owner: bool) -> &'static str {
    if viewer_is_owner {
        "status <> 'archived'"
    } else {
        "status = 'published'"
    }
}

fn parse_id<T: FromStr>(value: &str, field: &'static str) -> ApiResult<T>
where
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error: T::Err| ApiError::bad_request(field, error.to_string()))
}

fn collect_outline_cards(nodes: &[OutlineNode], ids: &mut BTreeSet<String>) {
    for node in nodes {
        match node {
            OutlineNode::Section { children, .. } => collect_outline_cards(children, ids),
            OutlineNode::CardRef { card_id } => {
                ids.insert(card_id.to_string());
            }
        }
    }
}

fn select_notes(connection: &Connection, scope: &ExportScope, owner: bool) -> ApiResult<Vec<Note>> {
    let subject = match scope {
        ExportScope::Subject { subject } => subject.clone(),
        _ => return Ok(Vec::new()),
    };
    let sql = format!(
        "SELECT id FROM notes WHERE subject = ?1 AND {} ORDER BY id",
        status_sql(owner)
    );
    let mut statement = connection.prepare(&sql)?;
    let ids = statement
        .query_map([&subject], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut notes = Vec::new();
    for id in ids {
        let id = parse_id::<NoteId>(&id, "note_id")?;
        if let Some(note) = content_store::get_note(connection, &id)? {
            notes.push(note);
        }
    }
    Ok(notes)
}

fn select_note_scope(
    connection: &Connection,
    note_id: &NoteId,
    owner: bool,
) -> ApiResult<Vec<Note>> {
    let Some(note) = content_store::get_note(connection, note_id)? else {
        return Ok(Vec::new());
    };
    if !status_is_visible(note.status, owner) {
        return Ok(Vec::new());
    }
    Ok(vec![note])
}

fn load_cards(
    connection: &Connection,
    ids: impl IntoIterator<Item = String>,
    owner: bool,
) -> ApiResult<Vec<KnowledgeCard>> {
    let mut cards = Vec::new();
    for id in ids {
        let id = parse_id::<KnowledgeCardId>(&id, "card_id")?;
        let Some(card) = content_store::get_knowledge_card(connection, &id)? else {
            continue;
        };
        if status_is_visible(card.status, owner) {
            cards.push(card);
        }
    }
    Ok(cards)
}

fn select_subject_cards(
    connection: &Connection,
    subject: &str,
    owner: bool,
) -> ApiResult<Vec<KnowledgeCard>> {
    let sql = format!(
        "SELECT id FROM knowledge_cards WHERE subject = ?1 AND {} ORDER BY id",
        status_sql(owner)
    );
    let mut statement = connection.prepare(&sql)?;
    let ids = statement
        .query_map([subject], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    load_cards(connection, ids, owner)
}

fn select_subject_questions(
    connection: &Connection,
    subject: &str,
    owner: bool,
) -> ApiResult<Vec<QuestionCard>> {
    let sql = format!(
        "SELECT id FROM question_cards WHERE subject = ?1 AND {} ORDER BY id",
        status_sql(owner)
    );
    let mut statement = connection.prepare(&sql)?;
    let ids = statement
        .query_map([subject], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    load_questions(connection, ids, owner)
}

fn load_questions(
    connection: &Connection,
    ids: impl IntoIterator<Item = String>,
    owner: bool,
) -> ApiResult<Vec<QuestionCard>> {
    let mut questions = Vec::new();
    for id in ids {
        let id = parse_id::<QuestionCardId>(&id, "question_id")?;
        let Some(question) = content_store::get_question_card(connection, &id)? else {
            continue;
        };
        if status_is_visible(question.status, owner) {
            questions.push(question);
        }
    }
    Ok(questions)
}

fn select_weakness_cards(
    connection: &Connection,
    threshold: f64,
    user_id: &UserId,
    owner: bool,
) -> ApiResult<Vec<KnowledgeCard>> {
    let report = weakness::build_report(connection, user_id, 10_000)?;
    let ids = report
        .knowledge_cards
        .into_iter()
        .filter(|entry| entry.error_rate >= threshold)
        .map(|entry| entry.card_id.to_string())
        .collect::<BTreeSet<_>>();
    load_cards(connection, ids, owner)
}

fn tests_question_ids(
    connection: &Connection,
    card_ids: &BTreeSet<String>,
) -> ApiResult<BTreeSet<String>> {
    let mut statement = connection.prepare(
        "SELECT from_id, to_id FROM edges
         WHERE edge_type = ?1 AND from_kind = 'question' AND to_kind = 'card'",
    )?;
    let rows = statement.query_map([EdgeType::Tests.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut ids = BTreeSet::new();
    for row in rows {
        let (question, card) = row?;
        if card_ids.contains(&card) {
            ids.insert(question);
        }
    }
    Ok(ids)
}

fn blocks_of<'a>(
    cards: &'a [KnowledgeCard],
    questions: &'a [QuestionCard],
) -> Vec<&'a ContentBlock> {
    let mut blocks = Vec::new();
    for card in cards {
        blocks.extend(card.blocks.iter());
    }
    for question in questions {
        blocks.extend(question.stem_blocks.iter());
        blocks.extend(question.explanation_blocks.iter());
    }
    blocks
}

fn referenced_resource_ids(blocks: &[&ContentBlock]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for block in blocks {
        match block {
            ContentBlock::ImageOcclusion { resource_id, .. } => {
                ids.insert(resource_id.to_string());
            }
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                ..
            } => {
                ids.insert(resource_id.to_string());
                if let Some(id) = overlay_resource_id {
                    ids.insert(id.to_string());
                }
            }
            _ => {}
        }
    }
    ids
}

pub fn select(
    connection: &Connection,
    scope: &ExportScope,
    viewer_is_owner: bool,
    blobs: &Path,
) -> ApiResult<ExportInput> {
    let (notes, cards, questions) = match scope {
        ExportScope::Note { note_id } => {
            let notes = select_note_scope(connection, note_id, viewer_is_owner)?;
            let mut card_ids = BTreeSet::new();
            for note in &notes {
                collect_outline_cards(&note.outline, &mut card_ids);
            }
            let cards = load_cards(connection, card_ids, viewer_is_owner)?;
            (notes, cards, Vec::new())
        }
        ExportScope::Subject { subject } => (
            select_notes(connection, scope, viewer_is_owner)?,
            select_subject_cards(connection, subject, viewer_is_owner)?,
            select_subject_questions(connection, subject, viewer_is_owner)?,
        ),
        ExportScope::Weakness { threshold, user_id } => {
            let cards = select_weakness_cards(connection, *threshold, user_id, viewer_is_owner)?;
            let card_ids = cards
                .iter()
                .map(|card| card.id.to_string())
                .collect::<BTreeSet<_>>();
            let questions = load_questions(
                connection,
                tests_question_ids(connection, &card_ids)?,
                viewer_is_owner,
            )?;
            (Vec::new(), cards, questions)
        }
    };

    let blocks = blocks_of(&cards, &questions);
    let resource_ids = referenced_resource_ids(&blocks);
    let mut input = ExportInput {
        notes,
        cards,
        questions,
        blob_dir: None,
        ..ExportInput::default()
    };
    let mut seen_hashes = BTreeSet::new();
    for id in resource_ids {
        let id = parse_id::<ResourceId>(&id, "resource_id")?;
        let Some(resource) = content_store::get_resource(connection, &id)? else {
            continue;
        };
        let hash = resource.sha256.as_str();
        if !seen_hashes.contains(hash) {
            let prefix = hash.get(..2).unwrap_or("xx");
            let path = blobs.join(prefix).join(hash);
            if let Ok(bytes) = std::fs::read(&path) {
                if ContentHash::from_bytes(&bytes) == resource.sha256 {
                    seen_hashes.insert(hash.to_owned());
                    input.blobs.push(ResourceBlob {
                        sha256: resource.sha256.clone(),
                        bytes,
                    });
                }
            }
        }
        input.resources.push(resource);
    }
    Ok(input)
}
