//! Local database access for the desktop client.
//!
//! The desktop build links the pure `reinforce-core` crate directly, so this
//! module is a thin convenience layer over the same catalogue, content and study
//! queries the HTTP API and the UniFFI bindings use. Nothing here re-implements
//! domain logic: sessions are built by `reinforce_core::quiz`, attempts are
//! appended through `reinforce_core::db::study` and scheduling is recomputed by
//! the core FSRS implementation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use reinforce_core::content::{Note, QuestionGroup};
use reinforce_core::db::catalog::{ExamSetSummary, NoteOutline, SubjectSummary};
use reinforce_core::db::{
    auth, catalog, content as content_store, quiz as quiz_store, study as study_store,
    subject_groups as subject_groups_store,
};
use reinforce_core::primitives::CardStatus;
use reinforce_core::proposal::{
    ApprovalOptions, ApprovalOutcome, BatchOperation, SubmitOutcome, approve_batch, submit_batch,
};
use reinforce_core::quiz::QuizSession;
use reinforce_core::study::User;
use reinforce_core::validation::Actor;
use reinforce_core::weakness::{WeaknessReport, build_report_scoped};
use reinforce_core::{
    ContentHash, Database, ExamSetId, KnowledgeCardId, NoteId, QuestionCardId, QuestionGroupId,
    ResourceId, SubjectGroup, SubjectGroupId, Timestamp, UserId,
};
use serde_json::Value;

use crate::{Error, Result};

pub const BUNDLE_FILE: &str = "bundle.jsonl";
pub const ASSETS_DIR: &str = "assets";
pub const DEMO_SEEDED_SETTING: &str = "desktop_demo_seeded";

/// Summary of one offline import, mirroring the CLI and FFI importers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub applied: u32,
    pub blobs_written: u32,
}

/// A local Reinforce database plus the blob directory that holds its media.
pub struct Store {
    database: Database,
    db_path: PathBuf,
    blobs_dir: PathBuf,
}

impl Store {
    /// Opens `db_path`, creating the file and schema when it is missing.
    ///
    /// Media blobs live in a `blobs` directory beside the database so a folder
    /// can be copied between machines, which is the same layout the CLI and the
    /// mobile bindings use.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let blobs_dir = db_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join("blobs");
        Self::open_with_blobs(&db_path, blobs_dir)
    }

    pub fn open_with_blobs(db_path: impl AsRef<Path>, blobs_dir: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let blobs_dir = blobs_dir.as_ref().to_path_buf();
        let database = Database::open(&db_path)?;
        Ok(Self {
            database,
            db_path,
            blobs_dir,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn blobs_dir(&self) -> &Path {
        &self.blobs_dir
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self.database.schema_version()?)
    }

    pub fn connection(&self) -> &rusqlite::Connection {
        self.database.connection()
    }

    /// Published notes, optionally narrowed to one subject.
    pub fn notes(&self, subject: Option<&str>) -> Result<Vec<Note>> {
        Ok(catalog::list_notes(
            self.connection(),
            subject,
            Some(CardStatus::Published),
            500,
        )?)
    }

    /// Published knowledge cards, optionally narrowed to one subject.
    pub fn knowledge_cards(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<reinforce_core::KnowledgeCard>> {
        Ok(catalog::list_knowledge_cards(
            self.connection(),
            subject,
            Some(CardStatus::Published),
            500,
        )?)
    }

    pub fn subjects(&self) -> Result<Vec<SubjectSummary>> {
        Ok(catalog::list_subjects(self.connection())?)
    }

    pub fn exam_sets(&self, subject: Option<&str>) -> Result<Vec<ExamSetSummary>> {
        Ok(catalog::list_exam_sets(self.connection(), subject, 200)?)
    }

    pub fn note_outline(&self, id: &NoteId) -> Result<Option<NoteOutline>> {
        Ok(catalog::note_outline(self.connection(), id)?)
    }

    pub fn exam_set(&self, id: &ExamSetId) -> Result<Option<reinforce_core::content::ExamSet>> {
        Ok(content_store::get_exam_set(self.connection(), id)?)
    }

    pub fn question_group(&self, id: &QuestionGroupId) -> Result<Option<QuestionGroup>> {
        Ok(content_store::get_question_group(self.connection(), id)?)
    }

    pub fn topic_question_ids(
        &self,
        card_ids: &[KnowledgeCardId],
        limit: usize,
    ) -> Result<Vec<QuestionCardId>> {
        Ok(quiz_store::topic_question_ids(
            self.connection(),
            card_ids,
            limit,
        )?)
    }

    pub fn subject_groups(&self) -> Result<Vec<SubjectGroup>> {
        Ok(subject_groups_store::list_subject_groups(
            self.connection(),
        )?)
    }

    pub fn create_subject_group(
        &self,
        parent_id: Option<&SubjectGroupId>,
        name: &str,
        subjects: &[String],
        sort_order: i32,
        now: Timestamp,
    ) -> Result<SubjectGroup> {
        Ok(subject_groups_store::create_subject_group(
            self.connection(),
            parent_id,
            name,
            subjects,
            sort_order,
            now,
        )?)
    }

    pub fn update_subject_group(
        &self,
        id: &SubjectGroupId,
        parent_id: Option<&SubjectGroupId>,
        name: &str,
        subjects: &[String],
        sort_order: i32,
        now: Timestamp,
    ) -> Result<SubjectGroup> {
        Ok(subject_groups_store::update_subject_group(
            self.connection(),
            id,
            parent_id,
            name,
            subjects,
            sort_order,
            now,
        )?)
    }

    pub fn delete_subject_group(&self, id: &SubjectGroupId) -> Result<bool> {
        Ok(subject_groups_store::delete_subject_group(
            self.connection(),
            id,
        )?)
    }

    pub fn resolve_group_subjects(&self, group_id: &SubjectGroupId) -> Result<Vec<String>> {
        Ok(subject_groups_store::resolve_group_subjects(
            self.connection(),
            group_id,
        )?)
    }

    pub fn weakness_report(
        &self,
        user_id: &UserId,
        limit: usize,
        subjects: Option<&[String]>,
    ) -> Result<WeaknessReport> {
        Ok(build_report_scoped(
            self.connection(),
            user_id,
            limit,
            subjects,
        )?)
    }

    /// Returns the first known user, creating a local learner on first launch.
    pub fn local_user(&self) -> Result<User> {
        let existing = self.connection().query_row(
            "SELECT id, display_name, created_at FROM users ORDER BY created_at, id LIMIT 1",
            [],
            |row| {
                Ok(User {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        );
        match existing {
            Ok(user) => Ok(user),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let user = User {
                    id: UserId::generate(),
                    display_name: "Local Learner".to_owned(),
                    created_at: Timestamp::now(),
                };
                study_store::insert_user(self.connection(), &user)?;
                Ok(user)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Number of attempt rows this store has recorded for one user.
    pub fn attempt_count(&self, user_id: &UserId) -> Result<usize> {
        Ok(quiz_store::user_attempts(self.connection(), user_id)?.len())
    }

    /// Appends every graded answer of `session` and refreshes FSRS scheduling.
    ///
    /// This is the same transaction boundary the FFI `commit_session` uses: the
    /// append-only log is written first, then scheduling is recomputed per card
    /// from the full attempt history so a partial write never leaves a card in a
    /// half-updated state.
    pub fn commit_session(
        &mut self,
        session: &mut QuizSession,
        user_id: &UserId,
        now: Timestamp,
    ) -> Result<u32> {
        let transaction = self.database.transaction()?;
        let cursor = study_store::next_attempt_cursor(&transaction)?;
        session.finish(now)?;
        let attempts = session.attempts(user_id, cursor);
        let written = u32::try_from(attempts.len()).unwrap_or(u32::MAX);
        for attempt in &attempts {
            study_store::append_attempt(&transaction, attempt)?;
            if attempt.affects_scheduling {
                let scheduling = study_store::recompute_for_card(
                    &transaction,
                    &attempt.user_id,
                    &attempt.card_id,
                    attempt.direction,
                    0.9,
                )?;
                study_store::save_scheduling(&transaction, &scheduling)?;
            }
        }
        transaction.commit()?;
        Ok(written)
    }

    /// Reads a reader setting from the shared `app_settings` table.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(auth::get_setting(self.connection(), key)?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        auth::set_setting(self.connection(), key, value)?;
        Ok(())
    }

    /// Path of a content-addressed blob, if this store has the bytes.
    pub fn blob_path(&self, sha256: &str) -> Option<PathBuf> {
        let path = self.blobs_dir.join(sha256);
        path.is_file().then_some(path)
    }

    /// Resolves the blob backing a resource, if the resource and its bytes exist.
    pub fn resource_blob(&self, resource_id: &ResourceId) -> Result<Option<PathBuf>> {
        let Some(resource) = content_store::get_resource(self.connection(), resource_id)? else {
            return Ok(None);
        };
        Ok(self.blob_path(resource.sha256.as_str()))
    }

    /// Publishes a standard bundle (a `bundle.jsonl` file or a directory that
    /// holds one) and copies its referenced assets into the blob directory.
    pub fn import_bundle(&mut self, path: impl AsRef<Path>) -> Result<ImportSummary> {
        let path = path.as_ref();
        let operations = read_operations(path)?;
        if operations.is_empty() {
            return Err(Error::Invalid("bundle", "no operations found".to_owned()));
        }
        let blobs_written = copy_referenced_assets(&self.blobs_dir, path, &operations)?;
        let proposal = match submit_batch(self.database.connection_mut(), Actor::Owner, operations)?
        {
            SubmitOutcome::Accepted { proposal } => proposal,
            SubmitOutcome::Rejected { errors } => {
                return Err(Error::Invalid("bundle", serde_json::to_string(&errors)?));
            }
        };
        match approve_batch(
            self.database.connection_mut(),
            &proposal.id,
            ApprovalOptions {
                reviewer: Actor::Owner,
                promote_verified: true,
            },
        )? {
            ApprovalOutcome::Applied { applied, .. } => Ok(ImportSummary {
                applied: u32::try_from(applied).unwrap_or(u32::MAX),
                blobs_written,
            }),
            ApprovalOutcome::Rejected { errors, .. } => {
                Err(Error::Invalid("bundle", serde_json::to_string(&errors)?))
            }
        }
    }
}

/// Reads every non-empty JSONL line as one batch operation.
pub fn read_operations(path: &Path) -> Result<Vec<BatchOperation>> {
    let file = bundle_file(path);
    let content = std::fs::read_to_string(&file)?;
    let mut operations = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let operation = serde_json::from_str::<BatchOperation>(line.trim())
            .map_err(|error| Error::Invalid("bundle", format!("line {}: {error}", index + 1)))?;
        operations.push(operation);
    }
    Ok(operations)
}

fn bundle_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(BUNDLE_FILE)
    } else {
        path.to_path_buf()
    }
}

fn assets_dir(path: &Path) -> Option<PathBuf> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let directory = root.join(ASSETS_DIR);
    directory.is_dir().then_some(directory)
}

/// Copies the bytes of every resource operation's `sha256` into the blob store.
pub fn copy_referenced_assets(
    blobs_dir: &Path,
    bundle_path: &Path,
    operations: &[BatchOperation],
) -> Result<u32> {
    let Some(directory) = assets_dir(bundle_path) else {
        return Ok(0);
    };
    let required: BTreeSet<String> = operations
        .iter()
        .filter_map(|operation| match operation {
            BatchOperation::Resource { data, .. } => data
                .get("sha256")
                .and_then(Value::as_str)
                .map(str::to_owned),
            _ => None,
        })
        .collect();
    if required.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(blobs_dir)?;
    let mut written = 0;
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path())?;
        let hash = ContentHash::from_bytes(&bytes);
        if !required.contains(hash.as_str()) {
            continue;
        }
        let target = blobs_dir.join(hash.as_str());
        if target.exists() {
            continue;
        }
        // Write to a hidden temporary file first so an interrupted copy never
        // becomes visible under its content hash.
        let temporary = blobs_dir.join(format!(".{}.tmp", hash.as_str()));
        std::fs::write(&temporary, &bytes)?;
        std::fs::rename(&temporary, &target)?;
        written += 1;
    }
    Ok(written)
}
