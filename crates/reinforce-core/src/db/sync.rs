//! Device ↔ server sync bookkeeping.
//!
//! The exchange itself is HTTP-free: native clients move JSON batches and the
//! server assigns cursors, while this module owns everything that must stay
//! consistent inside one database. Two invariants drive the design:
//!
//! * `attempt_logs` is append-only, so "already sent" can never be a column on
//!   an attempt. Upload progress lives in `sync_attempt_uploads` and a failed
//!   request only leaves the work pending, which makes retrying a batch safe.
//! * `CardScheduling` is a pure function of a user's attempts in chronological
//!   order, so a pulled batch is merged by id and the affected cards are
//!   replayed from the full local log instead of trusting a remote snapshot.

use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;

use crate::db::auth as auth_store;
use crate::db::study as study_store;
use crate::primitives::{AttemptDirection, AttemptId, CardId, Error, Result, UserId};
use crate::study::{AttemptLog, UserCardMeta};
use crate::timestamp::Timestamp;

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    Ok(serde_json::from_str(value)?)
}

/// The local identity mapping and pull progress of one device profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub local_user_id: UserId,
    pub remote_user_id: UserId,
    /// Highest server cursor this device has already merged.
    pub pull_cursor: i64,
}

/// What one pulled batch changed locally.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeOutcome {
    pub inserted_attempts: usize,
    pub skipped_attempts: usize,
    pub applied_meta: usize,
    pub skipped_meta: usize,
    pub pull_cursor: i64,
}

fn read_state(connection: &Connection, local_user_id: &UserId) -> Result<Option<SyncState>> {
    connection
        .query_row(
            "SELECT user_id, remote_user_id, pull_cursor FROM sync_state WHERE user_id = ?1",
            params![local_user_id],
            |row| {
                Ok(SyncState {
                    local_user_id: row.get(0)?,
                    remote_user_id: row.get(1)?,
                    pull_cursor: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn state(connection: &Connection, local_user_id: &UserId) -> Result<Option<SyncState>> {
    read_state(connection, local_user_id)
}

/// Binds a device profile to a server account.
///
/// Binding is idempotent. Binding the same profile to a different account
/// restarts the pull at cursor zero so the new account's history is fetched
/// instead of being skipped; the account already bound to another profile on
/// this device is unaffected.
pub fn bind_remote_user(
    connection: &Connection,
    local_user_id: &UserId,
    remote_user_id: &UserId,
    now: Timestamp,
) -> Result<SyncState> {
    if auth_store::user_by_id(connection, local_user_id)?.is_some() {
        // The profile is a real local user; the mapping can be (re)written.
    } else {
        return Err(Error::InvalidValue(
            "local user",
            "the local user does not exist on this device".to_owned(),
        ));
    }
    connection.execute(
        "INSERT INTO sync_state (user_id, remote_user_id, pull_cursor, updated_at)
         VALUES (?1, ?2, 0, ?3)
         ON CONFLICT(user_id) DO UPDATE SET
             pull_cursor = CASE
                 WHEN sync_state.remote_user_id = excluded.remote_user_id
                 THEN sync_state.pull_cursor
                 ELSE 0
             END,
             remote_user_id = excluded.remote_user_id,
             updated_at = excluded.updated_at",
        params![local_user_id, remote_user_id, now],
    )?;
    read_state(connection, local_user_id)?
        .ok_or_else(|| Error::InvalidValue("sync state", "cannot read the binding back".to_owned()))
}

/// Moves the pull cursor forward. It never moves backwards, so an out-of-order
/// response cannot make a device download the same history twice.
pub fn set_pull_cursor(
    connection: &Connection,
    local_user_id: &UserId,
    cursor: i64,
    now: Timestamp,
) -> Result<()> {
    if cursor < 0 {
        return Err(Error::InvalidValue(
            "sync cursor",
            "cursor must not be negative".to_owned(),
        ));
    }
    connection.execute(
        "UPDATE sync_state SET pull_cursor = MAX(pull_cursor, ?2), updated_at = ?3
         WHERE user_id = ?1",
        params![local_user_id, cursor, now],
    )?;
    Ok(())
}

/// Attempts of this user that were never confirmed as uploaded.
pub fn pending_attempts(
    connection: &Connection,
    user_id: &UserId,
    limit: usize,
) -> Result<Vec<AttemptLog>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(
        "SELECT id, user_id, card_id, direction, session_mode, submitted_payload,
                is_correct, duration_ms, confidence, error_tags_json, created_at,
                sync_cursor, affects_scheduling
         FROM attempt_logs a
         WHERE a.user_id = ?1
           AND NOT EXISTS (
               SELECT 1 FROM sync_attempt_uploads u WHERE u.attempt_id = a.id
           )
         ORDER BY a.sync_cursor, a.id LIMIT ?2",
    )?;
    let rows = statement.query_map(params![user_id, limit], |row| {
        Ok(AttemptLog {
            id: row.get(0)?,
            user_id: row.get(1)?,
            card_id: row.get(2)?,
            direction: row.get(3)?,
            session_mode: row.get(4)?,
            submitted_payload: from_json(&row.get::<_, String>(5)?)?,
            is_correct: row.get::<_, i64>(6)? != 0,
            duration_ms: row.get(7)?,
            confidence: row.get(8)?,
            error_tags: from_json(&row.get::<_, String>(9)?)?,
            created_at: row.get(10)?,
            sync_cursor: row.get(11)?,
            affects_scheduling: row.get::<_, i64>(12)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Error::from)
}

/// Personal card rows that changed since the last confirmed upload.
pub fn pending_user_card_meta(
    connection: &Connection,
    user_id: &UserId,
    limit: usize,
) -> Result<Vec<UserCardMeta>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(
        "SELECT m.user_id, m.card_id, m.starred, m.personal_tags_json, m.personal_note,
                m.updated_at
         FROM user_card_meta m
         LEFT JOIN sync_meta_uploads s
             ON s.user_id = m.user_id AND s.card_id = m.card_id
         WHERE m.user_id = ?1 AND (s.card_id IS NULL OR m.updated_at > s.updated_at)
         ORDER BY m.card_id LIMIT ?2",
    )?;
    let rows = statement.query_map(params![user_id, limit], |row| {
        Ok(UserCardMeta {
            user_id: row.get(0)?,
            card_id: row.get(1)?,
            starred: row.get::<_, i64>(2)? != 0,
            personal_tags: from_json(&row.get::<_, String>(3)?)?,
            personal_note: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Error::from)
}

/// Records that a server accepted these attempts. Unknown ids are ignored so a
/// stale acknowledgement cannot fail the whole batch.
pub fn mark_attempts_uploaded(
    connection: &Connection,
    attempt_ids: &[AttemptId],
    now: Timestamp,
) -> Result<()> {
    let mut statement = connection.prepare(
        "INSERT INTO sync_attempt_uploads (attempt_id, uploaded_at)
         SELECT id, ?2 FROM attempt_logs WHERE id = ?1
         ON CONFLICT(attempt_id) DO UPDATE SET uploaded_at = excluded.uploaded_at",
    )?;
    for id in attempt_ids {
        statement.execute(params![id, now])?;
    }
    Ok(())
}

/// Records the `updated_at` stamp a server accepted for each personal row.
/// Re-acknowledging an older stamp never rewinds the high-water mark.
pub fn mark_user_card_meta_uploaded(
    connection: &Connection,
    user_id: &UserId,
    stamps: &[(CardId, Timestamp)],
) -> Result<()> {
    let mut statement = connection.prepare(
        "INSERT INTO sync_meta_uploads (user_id, card_id, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id, card_id) DO UPDATE SET
             updated_at = MAX(excluded.updated_at, sync_meta_uploads.updated_at)",
    )?;
    for (card_id, updated_at) in stamps {
        statement.execute(params![user_id, card_id, updated_at])?;
    }
    Ok(())
}

/// Merges one pulled batch in a single transaction.
///
/// Attempts are deduplicated by id, stored under the local profile with a fresh
/// local cursor, and every affected card is replayed from the complete local
/// log. Personal rows are last-writer-wins on `updated_at`. The pull cursor
/// moves forward only after everything else succeeded, so an interrupted merge
/// leaves no half-applied state.
pub fn merge_remote_batch(
    connection: &mut Connection,
    local_user_id: &UserId,
    attempts: &[AttemptLog],
    metas: &[UserCardMeta],
    next_cursor: i64,
    desired_retention: f64,
) -> Result<MergeOutcome> {
    if next_cursor < 0 {
        return Err(Error::InvalidValue(
            "sync cursor",
            "cursor must not be negative".to_owned(),
        ));
    }
    let transaction = connection.transaction()?;
    let Some(bound) = read_state(&transaction, local_user_id)? else {
        return Err(Error::InvalidValue(
            "sync state",
            "the local user is not bound to a server account".to_owned(),
        ));
    };
    for attempt in attempts {
        if attempt.user_id != bound.remote_user_id {
            return Err(Error::InvalidValue(
                "attempt user_id",
                format!(
                    "attempt {} belongs to another server account",
                    attempt.id.as_str()
                ),
            ));
        }
    }
    for meta in metas {
        if meta.user_id != bound.remote_user_id {
            return Err(Error::InvalidValue(
                "card meta user_id",
                format!(
                    "card meta {} belongs to another server account",
                    meta.card_id.as_str()
                ),
            ));
        }
    }

    let mut outcome = MergeOutcome {
        pull_cursor: bound.pull_cursor.max(next_cursor),
        ..MergeOutcome::default()
    };
    let mut affected: Vec<(CardId, AttemptDirection)> = Vec::new();
    for attempt in attempts {
        if study_store::attempt_by_id(&transaction, &attempt.id)?.is_some() {
            outcome.skipped_attempts += 1;
            continue;
        }
        let local = AttemptLog {
            user_id: local_user_id.clone(),
            sync_cursor: study_store::next_attempt_cursor(&transaction)?,
            ..attempt.clone()
        };
        study_store::append_attempt(&transaction, &local)?;
        if local.affects_scheduling {
            let key = (local.card_id.clone(), local.direction);
            if !affected.contains(&key) {
                affected.push(key);
            }
        }
        outcome.inserted_attempts += 1;
    }
    for (card_id, direction) in affected {
        let scheduling = study_store::recompute_for_card(
            &transaction,
            local_user_id,
            &card_id,
            direction,
            desired_retention,
        )?;
        study_store::save_scheduling(&transaction, &scheduling)?;
    }
    // Everything in this batch came from the server, so it is already stored
    // there. Recording that avoids echoing pulled attempts straight back on the
    // next push (for example after an upload whose response was lost).
    let pulled: Vec<AttemptId> = attempts.iter().map(|attempt| attempt.id.clone()).collect();
    if !pulled.is_empty() {
        mark_attempts_uploaded(&transaction, &pulled, Timestamp::now())?;
    }
    for meta in metas {
        let local = UserCardMeta {
            user_id: local_user_id.clone(),
            ..meta.clone()
        };
        let existing =
            study_store::get_user_card_meta(&transaction, local_user_id, &local.card_id)?;
        match existing {
            Some(current) if current.updated_at >= local.updated_at => {
                outcome.skipped_meta += 1;
            }
            _ => {
                study_store::save_user_card_meta(&transaction, &local)?;
                outcome.applied_meta += 1;
            }
        }
    }
    if outcome.pull_cursor != bound.pull_cursor {
        set_pull_cursor(
            &transaction,
            local_user_id,
            outcome.pull_cursor,
            Timestamp::now(),
        )?;
    }
    transaction.commit()?;
    Ok(outcome)
}
