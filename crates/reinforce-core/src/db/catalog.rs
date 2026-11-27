//! Catalogue queries for clients that browse the store without the HTTP API.
//!
//! These read-only helpers answer the questions a native or offline client asks
//! when rendering a library view: which subjects exist, what each subject
//! contains, how a note outline resolves to cards, and what an exam set holds.
//! Ordering is deterministic (`id` order, question cards by number) so clients
//! can page through results without duplicates.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::content::{ExamSet, KnowledgeCard, Note, OutlineNode, QuestionCard, QuestionGroup};
use crate::db::content::{
    get_exam_set, get_knowledge_card, get_note, get_question_card, get_question_group,
};
use crate::primitives::{CardStatus, Result};
use crate::{ExamSetId, KnowledgeCardId, NoteId, QuestionCardId, QuestionGroupId};

const MAX_LIST_LIMIT: usize = 1000;

fn limit_of(limit: usize) -> i64 {
    i64::try_from(limit.clamp(1, MAX_LIST_LIMIT)).unwrap_or(1000)
}

fn count_of(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// Per-subject totals for a library landing view.
///
/// Note, knowledge card and question card counts only include `published`
/// rows; task drills and quizzes cannot reach unpublished content. Exam sets
/// carry no status of their own, so `exam_set_count` counts every set of the
/// subject and [`ExamSetSummary`] reports how many of its questions are
/// published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectSummary {
    pub subject: String,
    pub note_count: u32,
    pub knowledge_card_count: u32,
    pub exam_set_count: u32,
    pub question_card_count: u32,
}

/// An exam set plus the size of its question bank.
#[derive(Debug, Clone, PartialEq)]
pub struct ExamSetSummary {
    pub exam_set: ExamSet,
    pub question_count: u32,
    pub published_question_count: u32,
}

/// A note with its outline resolved to stored cards.
///
/// `cards` holds the referenced cards in first-appearance order without
/// duplicates. `missing_card_ids` lists references whose card is absent so the
/// caller can decide whether that is a repair or an error.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteOutline {
    pub note: Note,
    pub cards: Vec<KnowledgeCard>,
    pub missing_card_ids: Vec<KnowledgeCardId>,
}

/// Counts every subject that owns at least one row in any content table.
pub fn list_subjects(connection: &Connection) -> Result<Vec<SubjectSummary>> {
    let mut statement = connection.prepare(
        "WITH subjects(subject) AS (
             SELECT subject FROM notes
             UNION SELECT subject FROM knowledge_cards
             UNION SELECT subject FROM exam_sets
             UNION SELECT subject FROM question_cards
         )
         SELECT subjects.subject,
             (SELECT COUNT(*) FROM notes n
                 WHERE n.subject = subjects.subject AND n.status = 'published'),
             (SELECT COUNT(*) FROM knowledge_cards k
                 WHERE k.subject = subjects.subject AND k.status = 'published'),
             (SELECT COUNT(*) FROM exam_sets e WHERE e.subject = subjects.subject),
             (SELECT COUNT(*) FROM question_cards q
                 WHERE q.subject = subjects.subject AND q.status = 'published')
         FROM subjects
         ORDER BY subjects.subject",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SubjectSummary {
            subject: row.get(0)?,
            note_count: count_of(row.get(1)?),
            knowledge_card_count: count_of(row.get(2)?),
            exam_set_count: count_of(row.get(3)?),
            question_card_count: count_of(row.get(4)?),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

/// Lists notes, optionally narrowed to one subject and one status.
pub fn list_notes(
    connection: &Connection,
    subject: Option<&str>,
    status: Option<CardStatus>,
    limit: usize,
) -> Result<Vec<Note>> {
    let mut statement = connection.prepare(
        "SELECT id FROM notes
         WHERE (?1 IS NULL OR subject = ?1) AND (?2 IS NULL OR status = ?2)
         ORDER BY id LIMIT ?3",
    )?;
    let ids = statement
        .query_map(params![subject, status, limit_of(limit)], |row| {
            row.get::<_, NoteId>(0)
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    let mut notes = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(note) = get_note(connection, &id)? {
            notes.push(note);
        }
    }
    Ok(notes)
}

/// Lists knowledge cards, optionally narrowed to one subject and one status.
pub fn list_knowledge_cards(
    connection: &Connection,
    subject: Option<&str>,
    status: Option<CardStatus>,
    limit: usize,
) -> Result<Vec<KnowledgeCard>> {
    let mut statement = connection.prepare(
        "SELECT id FROM knowledge_cards
         WHERE (?1 IS NULL OR subject = ?1) AND (?2 IS NULL OR status = ?2)
         ORDER BY id LIMIT ?3",
    )?;
    let ids = statement
        .query_map(params![subject, status, limit_of(limit)], |row| {
            row.get::<_, KnowledgeCardId>(0)
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    let mut cards = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(card) = get_knowledge_card(connection, &id)? {
            cards.push(card);
        }
    }
    Ok(cards)
}

/// Lists exam sets with the size of their question banks.
pub fn list_exam_sets(
    connection: &Connection,
    subject: Option<&str>,
    limit: usize,
) -> Result<Vec<ExamSetSummary>> {
    let mut statement = connection.prepare(
        "SELECT exam_sets.id,
             (SELECT COUNT(*) FROM question_cards q WHERE q.exam_set_id = exam_sets.id),
             (SELECT COUNT(*) FROM question_cards q
                 WHERE q.exam_set_id = exam_sets.id AND q.status = 'published')
         FROM exam_sets
         WHERE (?1 IS NULL OR exam_sets.subject = ?1)
         ORDER BY exam_sets.id LIMIT ?2",
    )?;
    let rows = statement.query_map(params![subject, limit_of(limit)], |row| {
        Ok((
            row.get::<_, ExamSetId>(0)?,
            count_of(row.get(1)?),
            count_of(row.get(2)?),
        ))
    })?;
    let mut summaries = Vec::new();
    for row in rows {
        let (id, question_count, published_question_count) = row?;
        if let Some(exam_set) = get_exam_set(connection, &id)? {
            summaries.push(ExamSetSummary {
                exam_set,
                question_count,
                published_question_count,
            });
        }
    }
    Ok(summaries)
}

/// Lists the question groups of one exam set in id order.
pub fn list_question_groups(
    connection: &Connection,
    exam_set_id: &ExamSetId,
    limit: usize,
) -> Result<Vec<QuestionGroup>> {
    let mut statement = connection.prepare(
        "SELECT id FROM question_groups WHERE exam_set_id = ?1
         ORDER BY id LIMIT ?2",
    )?;
    let ids = statement
        .query_map(params![exam_set_id, limit_of(limit)], |row| {
            row.get::<_, QuestionGroupId>(0)
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    let mut groups = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(group) = get_question_group(connection, &id)? {
            groups.push(group);
        }
    }
    Ok(groups)
}

/// Lists the question cards of one exam set by question number.
///
/// Cards without a number come last so a printed exam keeps its order even
/// when placeholders lack numbering.
pub fn list_question_cards(
    connection: &Connection,
    exam_set_id: &ExamSetId,
    status: Option<CardStatus>,
    limit: usize,
) -> Result<Vec<QuestionCard>> {
    let mut statement = connection.prepare(
        "SELECT id FROM question_cards
         WHERE exam_set_id = ?1 AND (?2 IS NULL OR status = ?2)
         ORDER BY question_no IS NULL, question_no, id LIMIT ?3",
    )?;
    let ids = statement
        .query_map(params![exam_set_id, status, limit_of(limit)], |row| {
            row.get::<_, QuestionCardId>(0)
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    let mut cards = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(card) = get_question_card(connection, &id)? {
            cards.push(card);
        }
    }
    Ok(cards)
}

/// Collects the card ids an outline references, depth-first and deduplicated.
pub fn outline_card_ids(outline: &[OutlineNode]) -> Vec<KnowledgeCardId> {
    fn walk(
        nodes: &[OutlineNode],
        seen: &mut HashSet<KnowledgeCardId>,
        ids: &mut Vec<KnowledgeCardId>,
    ) {
        for node in nodes {
            match node {
                OutlineNode::CardRef { card_id } => {
                    if seen.insert(card_id.clone()) {
                        ids.push(card_id.clone());
                    }
                }
                OutlineNode::Section { children, .. } => walk(children, seen, ids),
            }
        }
    }

    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    walk(outline, &mut seen, &mut ids);
    ids
}

/// Loads a note together with every card its outline references.
pub fn note_outline(connection: &Connection, id: &NoteId) -> Result<Option<NoteOutline>> {
    let Some(note) = get_note(connection, id)? else {
        return Ok(None);
    };
    let mut cards = Vec::new();
    let mut missing_card_ids = Vec::new();
    for card_id in outline_card_ids(&note.outline) {
        match get_knowledge_card(connection, &card_id)? {
            Some(card) => cards.push(card),
            None => missing_card_ids.push(card_id),
        }
    }
    Ok(Some(NoteOutline {
        note,
        cards,
        missing_card_ids,
    }))
}
