//! Offline sync between a device database and a `reinforce serve` instance.
//!
//! The transport stays in the native client (Swift `URLSession`, Kotlin HTTP):
//! this module only produces the batches to upload, merges the batches that
//! were pulled, and keeps the local bookkeeping. Every merge is one
//! transaction, so a crash between the HTTP call and the acknowledgement only
//! leaves work pending instead of producing partial state.
//!
//! Identity: a device profile created offline keeps its own `user_id` forever.
//! [`LocalDatabase::bind_remote_user`] records which server account it belongs
//! to; uploads are stamped with the server id and pulled rows are stored under
//! the local profile. Attempt ids are global, so both devices end up with the
//! same attempt set and the same deterministic FSRS state.

use std::path::Path;

use reinforce_core::db::sync as sync_store;
use reinforce_core::study::DEFAULT_DESIRED_RETENTION;
use reinforce_core::{
    AttemptLog, CardId, SyncState as CoreSyncState, Timestamp, UserCardMeta as CoreUserCardMeta,
};

use crate::database::LocalDatabase;
use crate::error::{FfiError, guard, locked, parse_id, parse_timestamp, u32_field, usize_field};
use crate::resources::UserCardMeta;
use crate::types::{AttemptLogRecord, ImportSummary, parse_card_id, parse_user_id};

/// How many records one page should carry when the caller does not say.
const DEFAULT_PAGE: usize = 200;

/// The binding of one device profile to a server account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncState {
    pub local_user_id: String,
    pub remote_user_id: String,
    /// Highest server cursor already merged into this device.
    pub pull_cursor: i64,
}

impl From<CoreSyncState> for SyncState {
    fn from(value: CoreSyncState) -> Self {
        Self {
            local_user_id: value.local_user_id.as_str(),
            remote_user_id: value.remote_user_id.as_str(),
            pull_cursor: value.pull_cursor,
        }
    }
}

/// Everything this device still has to push, and where to continue pulling.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PendingSync {
    /// `None` until the profile is bound to a server account.
    pub remote_user_id: Option<String>,
    /// Attempts whose `user_id` is already the server account id when bound.
    pub attempts: Vec<AttemptLogRecord>,
    /// `user_id` follows the same rule as `attempts`.
    pub user_card_meta: Vec<UserCardMeta>,
    /// Another page is waiting; call `pending_sync` again after acknowledging.
    pub has_more_attempts: bool,
    pub has_more_user_card_meta: bool,
    pub pull_cursor: i64,
}

/// The `updated_at` a server confirmed for one personal card row.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncMetaStamp {
    pub card_id: String,
    pub updated_at: String,
}

impl SyncMetaStamp {
    fn into_core(self) -> Result<(CardId, Timestamp), FfiError> {
        Ok((
            parse_card_id(&self.card_id)?,
            parse_timestamp("updated at", &self.updated_at)?,
        ))
    }
}

/// What one pulled batch changed locally.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncApplySummary {
    pub inserted_attempts: u32,
    pub skipped_attempts: u32,
    pub applied_user_card_meta: u32,
    pub skipped_user_card_meta: u32,
    pub pull_cursor: i64,
}

fn page_limit(limit: u32) -> Result<usize, FfiError> {
    let limit = if limit == 0 {
        DEFAULT_PAGE
    } else {
        usize_field("limit", limit)?
    };
    Ok(limit)
}

fn convert_attempts(records: Vec<AttemptLogRecord>) -> Result<Vec<AttemptLog>, FfiError> {
    records
        .into_iter()
        .map(AttemptLog::try_from)
        .collect::<Result<Vec<_>, FfiError>>()
}

fn convert_meta(records: Vec<UserCardMeta>) -> Result<Vec<CoreUserCardMeta>, FfiError> {
    let mut metas = Vec::with_capacity(records.len());
    for record in records {
        metas.push(CoreUserCardMeta {
            user_id: parse_user_id(&record.user_id)?,
            card_id: parse_card_id(&record.card_id)?,
            starred: record.starred,
            personal_tags: record.personal_tags,
            personal_note: record.personal_note,
            updated_at: parse_timestamp("updated at", &record.updated_at)?,
        });
    }
    Ok(metas)
}

#[allow(unsafe_code)]
#[uniffi::export]
impl LocalDatabase {
    /// Binds this device profile to a server account.
    ///
    /// Idempotent. Binding the same profile to a different account restarts the
    /// pull at cursor zero so the new account's history is fetched. Local
    /// attempts are never rewritten; only the uploaded `user_id` changes.
    pub fn bind_remote_user(
        &self,
        local_user_id: String,
        remote_user_id: String,
    ) -> Result<SyncState, FfiError> {
        guard("bind remote user", || {
            let local_user_id = parse_user_id(&local_user_id)?;
            let remote_user_id = parse_user_id(&remote_user_id)?;
            let database = locked("database", &self.inner)?;
            Ok(SyncState::from(sync_store::bind_remote_user(
                database.connection(),
                &local_user_id,
                &remote_user_id,
                Timestamp::now(),
            )?))
        })
    }

    /// Reads the binding of one device profile, if it was bound before.
    pub fn sync_state(&self, local_user_id: String) -> Result<Option<SyncState>, FfiError> {
        guard("read sync state", || {
            let local_user_id = parse_user_id(&local_user_id)?;
            let database = locked("database", &self.inner)?;
            Ok(sync_store::state(database.connection(), &local_user_id)?.map(SyncState::from))
        })
    }

    /// Returns the next upload page and the pull cursor in one call.
    ///
    /// `limit` caps both lists; zero uses the default of 200. `has_more_*`
    /// tells the caller whether to ask again after the server acknowledged the
    /// page with [`LocalDatabase::confirm_upload`].
    pub fn pending_sync(&self, local_user_id: String, limit: u32) -> Result<PendingSync, FfiError> {
        guard("read pending sync", || {
            let local_user_id = parse_user_id(&local_user_id)?;
            let limit = page_limit(limit)?;
            let database = locked("database", &self.inner)?;
            let connection = database.connection();
            let state = sync_store::state(connection, &local_user_id)?;
            let attempts = sync_store::pending_attempts(connection, &local_user_id, limit + 1)?;
            let mut meta =
                sync_store::pending_user_card_meta(connection, &local_user_id, limit + 1)?;
            let has_more_attempts = attempts.len() > limit;
            let has_more_user_card_meta = meta.len() > limit;
            let mut attempts = attempts;
            attempts.truncate(limit);
            meta.truncate(limit);
            let remote_user_id = state.as_ref().map(|value| value.remote_user_id.clone());
            let attempts = attempts
                .into_iter()
                .map(|mut attempt| {
                    if let Some(remote) = &remote_user_id {
                        attempt.user_id = remote.clone();
                    }
                    AttemptLogRecord::from(attempt)
                })
                .collect();
            let user_card_meta = meta
                .into_iter()
                .map(|mut record| {
                    if let Some(remote) = &remote_user_id {
                        record.user_id = remote.clone();
                    }
                    UserCardMeta::from(record)
                })
                .collect();
            Ok(PendingSync {
                remote_user_id: remote_user_id.map(|id| id.as_str()),
                attempts,
                user_card_meta,
                has_more_attempts,
                has_more_user_card_meta,
                pull_cursor: state.map_or(0, |value| value.pull_cursor),
            })
        })
    }

    /// Marks a page as accepted by the server after a successful response.
    ///
    /// Only call this once the HTTP request returned 2xx. Unknown ids are
    /// ignored and older meta stamps never rewind the high-water mark, so
    /// re-sending an acknowledgement is safe.
    pub fn confirm_upload(
        &self,
        local_user_id: String,
        attempt_ids: Vec<String>,
        meta_stamps: Vec<SyncMetaStamp>,
    ) -> Result<(), FfiError> {
        guard("confirm upload", || {
            let local_user_id = parse_user_id(&local_user_id)?;
            let mut ids = Vec::with_capacity(attempt_ids.len());
            for id in &attempt_ids {
                ids.push(parse_id("attempt id", id)?);
            }
            let stamps = meta_stamps
                .into_iter()
                .map(SyncMetaStamp::into_core)
                .collect::<Result<Vec<_>, FfiError>>()?;
            let mut database = locked("database", &self.inner)?;
            let transaction = database.transaction()?;
            sync_store::mark_attempts_uploaded(
                &transaction,
                &local_user_id,
                &ids,
                Timestamp::now(),
            )?;
            sync_store::mark_user_card_meta_uploaded(&transaction, &local_user_id, &stamps)?;
            transaction.commit().map_err(|error| FfiError::Core {
                message: error.to_string(),
            })?;
            Ok(())
        })
    }

    /// Merges one pulled batch and advances the pull cursor atomically.
    ///
    /// Attempts already known by id are skipped, the rest are stored under the
    /// local profile and every affected card is replayed from the full local
    /// log. Personal rows follow last-writer-wins on `updated_at`. Passing the
    /// same batch twice is therefore a no-op, and an interrupted merge leaves
    /// the previous cursor in place.
    pub fn apply_remote_batch(
        &self,
        local_user_id: String,
        attempts: Vec<AttemptLogRecord>,
        user_card_meta: Vec<UserCardMeta>,
        next_cursor: i64,
    ) -> Result<SyncApplySummary, FfiError> {
        guard("apply remote batch", || {
            let local_user_id = parse_user_id(&local_user_id)?;
            let attempts = convert_attempts(attempts)?;
            let metas = convert_meta(user_card_meta)?;
            let mut database = locked("database", &self.inner)?;
            let outcome = sync_store::merge_remote_batch(
                database.connection_mut(),
                &local_user_id,
                &attempts,
                &metas,
                next_cursor,
                DEFAULT_DESIRED_RETENTION,
            )?;
            Ok(SyncApplySummary {
                inserted_attempts: u32_field("inserted attempts", outcome.inserted_attempts)?,
                skipped_attempts: u32_field("skipped attempts", outcome.skipped_attempts)?,
                applied_user_card_meta: u32_field("applied card meta", outcome.applied_meta)?,
                skipped_user_card_meta: u32_field("skipped card meta", outcome.skipped_meta)?,
                pull_cursor: outcome.pull_cursor,
            })
        })
    }

    /// Applies a content snapshot produced by `GET /api/sync/content`.
    ///
    /// The document uses the same newline-delimited bundle format as
    /// [`LocalDatabase::import_bundle`], so the same validator and blob rules
    /// apply. `assets_dir` points at a directory the client already downloaded
    /// the referenced blobs into; the files whose sha256 a resource operation
    /// references are copied into the blob store under that hash. Passing the
    /// same snapshot again rewrites nothing.
    pub fn import_content_snapshot(
        &self,
        bundle_jsonl: String,
        assets_dir: Option<String>,
    ) -> Result<ImportSummary, FfiError> {
        guard("import content snapshot", || {
            if bundle_jsonl.trim().is_empty() {
                return Err(FfiError::invalid("snapshot", "bundle must not be empty"));
            }
            let operations = crate::bundle::read_operations_from_text(&bundle_jsonl)?;
            if operations.is_empty() {
                return Err(FfiError::invalid("snapshot", "no operations found"));
            }
            let blobs_written = match assets_dir.as_deref().map(str::trim) {
                Some(directory) if !directory.is_empty() => {
                    let assets = crate::bundle::assets_in(Path::new(directory), &operations)?;
                    crate::bundle::copy_assets(&self.blobs_dir, &assets)?
                }
                _ => 0,
            };
            // Server content keeps the verification state the server stored:
            // content the owner never checked stays unchecked on the device.
            self.publish_operations(operations, blobs_written, false)
        })
    }
}
