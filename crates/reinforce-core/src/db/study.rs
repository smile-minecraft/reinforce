use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;

use crate::fsrs::recompute_scheduling;
use crate::primitives::{AttemptDirection, CardId, Error, Result, SessionMode, UserId};
use crate::study::{AttemptLog, CardScheduling, User, UserCardMeta};
use crate::timestamp::Timestamp;

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    Ok(serde_json::from_str(value)?)
}

pub fn insert_user(connection: &Connection, user: &User) -> Result<()> {
    connection.execute(
        "INSERT INTO users (id, display_name, created_at) VALUES (?1, ?2, ?3)",
        params![user.id, user.display_name, user.created_at],
    )?;
    Ok(())
}

pub fn next_attempt_cursor(connection: &Connection) -> Result<i64> {
    let cursor: i64 = connection.query_row(
        "SELECT COALESCE(MAX(sync_cursor), 0) + 1 FROM attempt_logs",
        [],
        |row| row.get(0),
    )?;
    Ok(cursor)
}

/// The only write path for attempt logs. The schema has triggers that reject
/// UPDATE and DELETE, so this module deliberately exposes no such functions.
pub fn append_attempt(connection: &Connection, attempt: &AttemptLog) -> Result<()> {
    connection.execute(
        "INSERT INTO attempt_logs (
            id, user_id, card_id, direction, session_mode, submitted_payload,
            is_correct, duration_ms, confidence, error_tags_json, created_at, sync_cursor
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            attempt.id,
            attempt.user_id,
            attempt.card_id,
            attempt.direction,
            attempt.session_mode,
            serde_json::to_string(&attempt.submitted_payload)?,
            i64::from(attempt.is_correct),
            attempt.duration_ms,
            attempt.confidence,
            serde_json::to_string(&attempt.error_tags)?,
            attempt.created_at,
            attempt.sync_cursor,
        ],
    )?;
    Ok(())
}

pub fn list_attempts(
    connection: &Connection,
    user_id: &UserId,
    card_id: &CardId,
    direction: AttemptDirection,
) -> Result<Vec<AttemptLog>> {
    let mut statement = connection.prepare(
        "SELECT id, user_id, card_id, direction, session_mode, submitted_payload,
                is_correct, duration_ms, confidence, error_tags_json, created_at, sync_cursor
         FROM attempt_logs
         WHERE user_id = ?1 AND card_id = ?2 AND direction = ?3
         ORDER BY created_at, id",
    )?;
    let rows = statement.query_map(params![user_id, card_id, direction], |row| {
        Ok(AttemptLog {
            id: row.get(0)?,
            user_id: row.get(1)?,
            card_id: row.get(2)?,
            direction: row.get(3)?,
            session_mode: row.get::<_, SessionMode>(4)?,
            submitted_payload: from_json(&row.get::<_, String>(5)?)?,
            is_correct: row.get::<_, i64>(6)? != 0,
            duration_ms: row.get(7)?,
            confidence: row.get(8)?,
            error_tags: from_json(&row.get::<_, String>(9)?)?,
            created_at: row.get(10)?,
            sync_cursor: row.get(11)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Error::from)
}

pub fn recompute_for_card(
    connection: &Connection,
    user_id: &UserId,
    card_id: &CardId,
    direction: AttemptDirection,
    desired_retention: f64,
) -> Result<CardScheduling> {
    let attempts = list_attempts(connection, user_id, card_id, direction)?;
    recompute_scheduling(
        user_id.clone(),
        card_id.clone(),
        direction,
        &attempts,
        desired_retention,
    )
}

pub fn save_scheduling(connection: &Connection, scheduling: &CardScheduling) -> Result<()> {
    connection.execute(
        "INSERT INTO card_scheduling (
            user_id, card_id, direction, stability, difficulty, reps, lapses, state,
            last_review_at, due_at, desired_retention, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(user_id, card_id, direction) DO UPDATE SET
            stability = excluded.stability,
            difficulty = excluded.difficulty,
            reps = excluded.reps,
            lapses = excluded.lapses,
            state = excluded.state,
            last_review_at = excluded.last_review_at,
            due_at = excluded.due_at,
            desired_retention = excluded.desired_retention,
            updated_at = excluded.updated_at",
        params![
            scheduling.user_id,
            scheduling.card_id,
            scheduling.direction,
            scheduling.stability,
            scheduling.difficulty,
            scheduling.reps,
            scheduling.lapses,
            scheduling.state,
            scheduling.last_review_at,
            scheduling.due_at,
            scheduling.desired_retention,
            scheduling.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_scheduling(
    connection: &Connection,
    user_id: &UserId,
    card_id: &CardId,
    direction: AttemptDirection,
) -> Result<Option<CardScheduling>> {
    connection
        .query_row(
            "SELECT user_id, card_id, direction, stability, difficulty, reps, lapses, state,
                    last_review_at, due_at, desired_retention, updated_at
             FROM card_scheduling WHERE user_id = ?1 AND card_id = ?2 AND direction = ?3",
            params![user_id, card_id, direction],
            |row| {
                Ok(CardScheduling {
                    user_id: row.get(0)?,
                    card_id: row.get(1)?,
                    direction: row.get(2)?,
                    stability: row.get(3)?,
                    difficulty: row.get(4)?,
                    reps: row.get::<_, i64>(5)? as u32,
                    lapses: row.get::<_, i64>(6)? as u32,
                    state: row.get(7)?,
                    last_review_at: row.get::<_, Option<Timestamp>>(8)?,
                    due_at: row.get::<_, Option<Timestamp>>(9)?,
                    desired_retention: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn save_user_card_meta(connection: &Connection, meta: &UserCardMeta) -> Result<()> {
    connection.execute(
        "INSERT INTO user_card_meta (
            user_id, card_id, starred, personal_tags_json, personal_note, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(user_id, card_id) DO UPDATE SET
            starred = excluded.starred,
            personal_tags_json = excluded.personal_tags_json,
            personal_note = excluded.personal_note,
            updated_at = excluded.updated_at",
        params![
            meta.user_id,
            meta.card_id,
            i64::from(meta.starred),
            serde_json::to_string(&meta.personal_tags)?,
            meta.personal_note,
            meta.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_user_card_meta(
    connection: &Connection,
    user_id: &UserId,
    card_id: &CardId,
) -> Result<Option<UserCardMeta>> {
    connection
        .query_row(
            "SELECT user_id, card_id, starred, personal_tags_json, personal_note, updated_at
             FROM user_card_meta WHERE user_id = ?1 AND card_id = ?2",
            params![user_id, card_id],
            |row| {
                Ok(UserCardMeta {
                    user_id: row.get(0)?,
                    card_id: row.get(1)?,
                    starred: row.get::<_, i64>(2)? != 0,
                    personal_tags: from_json(&row.get::<_, String>(3)?)?,
                    personal_note: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}
