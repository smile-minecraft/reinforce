use std::collections::HashMap;

use rusqlite::types::Value;
use rusqlite::{Connection, params, params_from_iter};
use serde::de::DeserializeOwned;

use crate::primitives::{AttemptDirection, Error, Result};
use crate::study::AttemptLog;
use crate::timestamp::Timestamp;
use crate::{KnowledgeCardId, QuestionCardId, UserId};

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    Ok(serde_json::from_str(value)?)
}

fn bounded(limit: usize) -> i64 {
    i64::try_from(limit.clamp(1, 1_000)).unwrap_or(1_000)
}

/// Appends `AND q.subject IN (...)` to a question query. Callers must reject an
/// empty subject list first: an empty `IN ()` would be a SQL syntax error, and
/// an empty scope means "no questions", never "every question".
fn append_subject_filter(sql: &mut String, values: &mut Vec<Value>, subjects: &[String]) {
    let placeholders = subjects.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    sql.push_str(&format!(" AND q.subject IN ({placeholders})"));
    values.extend(subjects.iter().cloned().map(Value::Text));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueQuestion {
    pub id: QuestionCardId,
    pub is_new: bool,
}

pub fn due_questions(
    connection: &Connection,
    user_id: &UserId,
    now: Timestamp,
    new_card_limit: usize,
    question_limit: usize,
) -> Result<Vec<DueQuestion>> {
    due_questions_scoped(
        connection,
        user_id,
        now,
        new_card_limit,
        question_limit,
        None,
    )
}

/// Due questions narrowed to a subject scope, filtered in SQL so `question_limit`
/// counts only matching questions.
pub fn due_questions_scoped(
    connection: &Connection,
    user_id: &UserId,
    now: Timestamp,
    new_card_limit: usize,
    question_limit: usize,
    subjects: Option<&[String]>,
) -> Result<Vec<DueQuestion>> {
    if subjects.is_some_and(|subjects| subjects.is_empty()) {
        return Ok(Vec::new());
    }
    let mut sql = String::from(
        "SELECT q.id, s.due_at IS NULL AS is_new
         FROM question_cards q
         LEFT JOIN card_scheduling s
           ON s.user_id = ?1 AND s.card_id = q.id AND s.direction = 'default'
         WHERE q.status = 'published'
           AND ((s.due_at IS NOT NULL AND s.due_at <= ?2) OR s.due_at IS NULL)",
    );
    let mut values: Vec<Value> = vec![Value::Text(user_id.as_str()), Value::Text(now.as_str())];
    if let Some(subjects) = subjects {
        append_subject_filter(&mut sql, &mut values, subjects);
    }
    sql.push_str(" ORDER BY is_new, s.due_at, q.id");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get::<_, QuestionCardId>(0)?, row.get::<_, i64>(1)? != 0))
    })?;

    let maximum = bounded(question_limit);
    let new_maximum = i64::try_from(new_card_limit).unwrap_or(i64::MAX);
    let mut selected = Vec::new();
    let mut new_count = 0_i64;
    for row in rows {
        let (id, is_new) = row?;
        if is_new {
            if new_count >= new_maximum {
                continue;
            }
            new_count += 1;
        }
        selected.push(DueQuestion { id, is_new });
        if i64::try_from(selected.len()).unwrap_or(i64::MAX) >= maximum {
            break;
        }
    }
    Ok(selected)
}

pub fn exam_question_ids(
    connection: &Connection,
    exam_set_id: &crate::ExamSetId,
    limit: usize,
) -> Result<Vec<QuestionCardId>> {
    exam_question_ids_scoped(connection, exam_set_id, limit, None)
}

pub fn exam_question_ids_scoped(
    connection: &Connection,
    exam_set_id: &crate::ExamSetId,
    limit: usize,
    subjects: Option<&[String]>,
) -> Result<Vec<QuestionCardId>> {
    if subjects.is_some_and(|subjects| subjects.is_empty()) {
        return Ok(Vec::new());
    }
    let mut sql = String::from(
        "SELECT q.id FROM question_cards q
         WHERE q.exam_set_id = ?1 AND q.status = 'published'",
    );
    let mut values: Vec<Value> = vec![Value::Text(exam_set_id.as_str())];
    if let Some(subjects) = subjects {
        append_subject_filter(&mut sql, &mut values, subjects);
    }
    values.push(Value::Integer(bounded(limit)));
    sql.push_str(&format!(
        " ORDER BY CASE WHEN q.question_no IS NULL THEN 1 ELSE 0 END, q.question_no, q.id LIMIT ?{}",
        values.len()
    ));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

pub fn topic_question_ids(
    connection: &Connection,
    knowledge_card_ids: &[KnowledgeCardId],
    limit: usize,
) -> Result<Vec<QuestionCardId>> {
    topic_question_ids_scoped(connection, knowledge_card_ids, limit, None)
}

pub fn topic_question_ids_scoped(
    connection: &Connection,
    knowledge_card_ids: &[KnowledgeCardId],
    limit: usize,
    subjects: Option<&[String]>,
) -> Result<Vec<QuestionCardId>> {
    if knowledge_card_ids.is_empty() || subjects.is_some_and(|subjects| subjects.is_empty()) {
        return Ok(Vec::new());
    }
    let placeholders = knowledge_card_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "SELECT DISTINCT q.id
         FROM edges e
         JOIN knowledge_cards k ON k.id = e.to_id AND k.status = 'published'
         JOIN question_cards q ON q.id = e.from_id
         WHERE e.edge_type = 'tests'
           AND e.to_kind = 'card'
           AND e.from_kind IN ('question', 'option')
           AND e.to_id IN ({placeholders})
           AND q.status = 'published'"
    );
    let mut values: Vec<Value> = knowledge_card_ids
        .iter()
        .map(|id| Value::Text(id.as_str()))
        .collect();
    if let Some(subjects) = subjects {
        append_subject_filter(&mut sql, &mut values, subjects);
    }
    values.push(Value::Integer(bounded(limit)));
    sql.push_str(&format!(" ORDER BY q.id LIMIT ?{}", values.len()));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

pub fn practical_question_ids(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<QuestionCardId>> {
    practical_question_ids_scoped(connection, limit, None)
}

pub fn practical_question_ids_scoped(
    connection: &Connection,
    limit: usize,
    subjects: Option<&[String]>,
) -> Result<Vec<QuestionCardId>> {
    if subjects.is_some_and(|subjects| subjects.is_empty()) {
        return Ok(Vec::new());
    }
    let mut sql = String::from(
        "SELECT q.id FROM question_cards q
         WHERE q.status = 'published'
           AND q.question_type IN ('pin_identify', 'pin_locate', 'image_qa')",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(subjects) = subjects {
        append_subject_filter(&mut sql, &mut values, subjects);
    }
    values.push(Value::Integer(bounded(limit)));
    sql.push_str(&format!(" ORDER BY q.id LIMIT ?{}", values.len()));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

/// Every published question of the given subjects, ordered the way a combined
/// mock exam is printed: by subject, then exam set, then question number.
pub fn subject_question_ids(
    connection: &Connection,
    subjects: &[String],
    limit: usize,
) -> Result<Vec<QuestionCardId>> {
    if subjects.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = String::from("SELECT q.id FROM question_cards q WHERE q.status = 'published'");
    let mut values: Vec<Value> = Vec::new();
    append_subject_filter(&mut sql, &mut values, subjects);
    values.push(Value::Integer(bounded(limit)));
    sql.push_str(&format!(
        " ORDER BY q.subject, q.exam_set_id, q.question_no IS NULL, q.question_no, q.id LIMIT ?{}",
        values.len()
    ));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get(0))?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

pub fn user_attempts(connection: &Connection, user_id: &UserId) -> Result<Vec<AttemptLog>> {
    let mut statement = connection.prepare(
        "SELECT id, user_id, card_id, direction, session_mode, submitted_payload,
                is_correct, duration_ms, confidence, error_tags_json, created_at, sync_cursor,
                affects_scheduling
         FROM attempt_logs
         WHERE user_id = ?1
         ORDER BY created_at, id",
    )?;
    let rows = statement.query_map(params![user_id], |row| {
        Ok(AttemptLog {
            id: row.get(0)?,
            user_id: row.get(1)?,
            card_id: row.get(2)?,
            direction: row.get::<_, AttemptDirection>(3)?,
            session_mode: row.get(4)?,
            submitted_payload: from_json(&row.get::<_, String>(5)?).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            is_correct: row.get::<_, i64>(6)? != 0,
            duration_ms: row.get(7)?,
            confidence: row.get(8)?,
            error_tags: from_json(&row.get::<_, String>(9)?).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            created_at: row.get(10)?,
            sync_cursor: row.get(11)?,
            affects_scheduling: row.get::<_, i64>(12)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Error::from)
}

/// Question-to-knowledge-card links from both question-level and option-level
/// `tests` edges.
pub fn tests_links(
    connection: &Connection,
    question_ids: &[QuestionCardId],
) -> Result<HashMap<QuestionCardId, Vec<KnowledgeCardId>>> {
    let mut links: HashMap<QuestionCardId, Vec<KnowledgeCardId>> = HashMap::new();
    if question_ids.is_empty() {
        return Ok(links);
    }
    let placeholders = question_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT DISTINCT from_id, to_id FROM edges
         WHERE edge_type = 'tests' AND to_kind = 'card'
           AND from_kind IN ('question', 'option')
           AND from_id IN ({placeholders})"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(question_ids.iter().map(|id| id.as_str())),
        |row| {
            Ok((
                row.get::<_, QuestionCardId>(0)?,
                row.get::<_, KnowledgeCardId>(1)?,
            ))
        },
    )?;
    for row in rows {
        let (question_id, card_id) = row?;
        let cards = links.entry(question_id).or_default();
        if !cards.contains(&card_id) {
            cards.push(card_id);
        }
    }
    Ok(links)
}
