use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use reinforce_core::db::{auth, study as study_store};
use reinforce_core::db::{
    content as content_store, quiz as quiz_store, search as search_store,
    subject_groups as subject_groups_store,
};
use reinforce_core::primitives::AttemptDirection;
use reinforce_core::proposal::{
    ApprovalOptions, ApprovalOutcome, BatchOperation, SubmitOutcome, approve_batch, submit_batch,
};
use reinforce_core::quiz::build::{
    build_daily_due, build_full_exam, build_practical_exam, build_subject_exam, build_topic_drill,
    build_weakness_drill,
};
use reinforce_core::validation::Actor;
use reinforce_core::weakness;
use reinforce_core::{CardId, Database as CoreDatabase, SubjectGroupId, Timestamp};

use crate::error::{FfiError, guard, locked, parse_id, parse_timestamp, usize_field};
use crate::session::QuizSessionHandle;
use crate::types::{
    AttemptLogRecord, BuildOptions, CardSchedulingRecord, DueQuestion, ExamSet, ImportSummary,
    KnowledgeCard, Note, QuestionCard, QuestionGroup, SearchHit, SubjectGroup, WeaknessReport,
    generated_attempt_id, parse_card_id, parse_user_id,
};

#[derive(uniffi::Object)]
pub struct LocalDatabase {
    pub(crate) inner: Mutex<CoreDatabase>,
    /// Content-addressed directory holding imported `assets/` blobs.
    pub(crate) blobs_dir: PathBuf,
}

impl LocalDatabase {
    fn open_at(path: &Path, blobs_dir: PathBuf) -> Result<Arc<Self>, FfiError> {
        let database = CoreDatabase::open(path)?;
        Ok(Arc::new(Self {
            inner: Mutex::new(database),
            blobs_dir,
        }))
    }

    fn default_blobs_dir(database_path: &Path) -> PathBuf {
        database_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join("blobs")
    }

    /// Submits validated bundle operations and applies them as the owner.
    ///
    /// `promote_verified` is the approval switch: a user importing their own
    /// bundle promotes its cards to verified, while content pulled from a
    /// server keeps the verification state the server stored.
    pub(crate) fn publish_operations(
        &self,
        operations: Vec<BatchOperation>,
        blobs_written: u32,
        promote_verified: bool,
    ) -> Result<ImportSummary, FfiError> {
        if operations.is_empty() {
            return Err(FfiError::invalid("bundle", "no operations found"));
        }
        let mut database = locked("database", &self.inner)?;
        let proposal = match submit_batch(database.connection_mut(), Actor::Owner, operations)? {
            SubmitOutcome::Accepted { proposal } => proposal,
            SubmitOutcome::Rejected { errors } => {
                return Err(FfiError::invalid("bundle", serde_json::to_string(&errors)?));
            }
        };
        match approve_batch(
            database.connection_mut(),
            &proposal.id,
            ApprovalOptions {
                reviewer: Actor::Owner,
                promote_verified,
            },
        )? {
            ApprovalOutcome::Applied { applied, .. } => Ok(ImportSummary {
                proposal_id: proposal.id.as_str(),
                applied: u32::try_from(applied).unwrap_or(u32::MAX),
                blobs_written,
            }),
            ApprovalOutcome::Rejected { errors, .. } => {
                Err(FfiError::invalid("bundle", serde_json::to_string(&errors)?))
            }
        }
    }
}

#[allow(unsafe_code)]
#[uniffi::export]
impl LocalDatabase {
    /// Opens a database whose blobs live in a `blobs` directory beside it.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, FfiError> {
        guard("open database", || {
            if path.trim().is_empty() {
                return Err(FfiError::invalid("database path", "path must not be empty"));
            }
            let database_path = Path::new(&path);
            Self::open_at(database_path, Self::default_blobs_dir(database_path))
        })
    }

    /// Opens a database with an explicit blob directory, for app groups and
    /// sandboxes that keep media outside the database folder.
    #[uniffi::constructor]
    pub fn open_with_blobs(
        database_path: String,
        blobs_dir: String,
    ) -> Result<Arc<Self>, FfiError> {
        guard("open database with blobs", || {
            if database_path.trim().is_empty() {
                return Err(FfiError::invalid("database path", "path must not be empty"));
            }
            if blobs_dir.trim().is_empty() {
                return Err(FfiError::invalid(
                    "blobs directory",
                    "path must not be empty",
                ));
            }
            Self::open_at(Path::new(&database_path), PathBuf::from(&blobs_dir))
        })
    }

    pub fn schema_version(&self) -> Result<u32, FfiError> {
        guard("schema version", || {
            let database = locked("database", &self.inner)?;
            Ok(database.schema_version()?)
        })
    }

    /// Reads a standard bundle (a `bundle.jsonl` file or a directory holding
    /// it) and publishes it locally.
    ///
    /// Referenced files under `assets/` are copied into the blob directory
    /// before the proposal is submitted, so a published resource always has its
    /// bytes. Blobs are content-addressed: re-importing the same bundle rewrites
    /// nothing and reports `blobs_written == 0`. A bundle that fails validation
    /// may still leave newly copied blobs behind; they are unreferenced and
    /// harmless.
    pub fn import_bundle(&self, bundle_path: String) -> Result<ImportSummary, FfiError> {
        guard("import bundle", || {
            if bundle_path.trim().is_empty() {
                return Err(FfiError::invalid("bundle path", "path must not be empty"));
            }
            let path = Path::new(&bundle_path);
            let operations = crate::bundle::read_operations(path)?;
            let blobs_written =
                crate::bundle::copy_referenced_assets(&self.blobs_dir, path, &operations)?;
            self.publish_operations(operations, blobs_written, true)
        })
    }

    pub fn note(&self, id: String) -> Result<Note, FfiError> {
        guard("read note", || {
            let id: reinforce_core::NoteId = crate::error::parse_id("note id", &id)?;
            let database = locked("database", &self.inner)?;
            content_store::get_note(database.connection(), &id)?
                .map(Note::from)
                .ok_or_else(|| FfiError::missing("note does not exist"))
        })
    }

    pub fn knowledge_card(&self, id: String) -> Result<KnowledgeCard, FfiError> {
        guard("read knowledge card", || {
            let id = parse_card_id(&id)?;
            let database = locked("database", &self.inner)?;
            let id = reinforce_core::KnowledgeCardId::from_uuid(id.as_uuid())?;
            content_store::get_knowledge_card(database.connection(), &id)?
                .map(KnowledgeCard::from)
                .ok_or_else(|| FfiError::missing("knowledge card does not exist"))
        })
    }

    pub fn question_card(&self, id: String) -> Result<QuestionCard, FfiError> {
        guard("read question card", || {
            let id = parse_card_id(&id)?;
            let database = locked("database", &self.inner)?;
            let id = reinforce_core::QuestionCardId::from_uuid(id.as_uuid())?;
            content_store::get_question_card(database.connection(), &id)?
                .map(QuestionCard::from)
                .ok_or_else(|| FfiError::missing("question card does not exist"))
        })
    }

    pub fn question_group(&self, id: String) -> Result<QuestionGroup, FfiError> {
        guard("read question group", || {
            let id: reinforce_core::QuestionGroupId =
                crate::error::parse_id("question group id", &id)?;
            let database = locked("database", &self.inner)?;
            content_store::get_question_group(database.connection(), &id)?
                .map(QuestionGroup::from)
                .ok_or_else(|| FfiError::missing("question group does not exist"))
        })
    }

    pub fn exam_set(&self, id: String) -> Result<ExamSet, FfiError> {
        guard("read exam set", || {
            let id: reinforce_core::ExamSetId = crate::error::parse_id("exam set id", &id)?;
            let database = locked("database", &self.inner)?;
            content_store::get_exam_set(database.connection(), &id)?
                .map(ExamSet::from)
                .ok_or_else(|| FfiError::missing("exam set does not exist"))
        })
    }

    pub fn search(&self, query: String, limit: u32) -> Result<Vec<SearchHit>, FfiError> {
        guard("search", || {
            if query.trim().is_empty() {
                return Err(FfiError::invalid("query", "query must not be empty"));
            }
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(search_store::search_cards_visible(
                database.connection(),
                query.trim(),
                limit,
                true,
            )?
            .into_iter()
            .map(|hit| match hit {
                search_store::SearchHit::KnowledgeCard(id) => {
                    SearchHit::KnowledgeCard { id: id.as_str() }
                }
                search_store::SearchHit::QuestionCard(id) => {
                    SearchHit::QuestionCard { id: id.as_str() }
                }
            })
            .collect())
        })
    }

    pub fn create_daily_due_session(
        &self,
        user_id: String,
        now: String,
        options: BuildOptions,
    ) -> Result<Arc<QuizSessionHandle>, FfiError> {
        guard("build daily due session", || {
            let user_id = parse_user_id(&user_id)?;
            let now = parse_timestamp("now", &now)?;
            let options = options.into_core()?;
            let database = locked("database", &self.inner)?;
            let session = build_daily_due(database.connection(), &user_id, now, &options)?;
            Ok(Arc::new(QuizSessionHandle::new(session)))
        })
    }

    pub fn create_full_exam_session(
        &self,
        exam_set_id: String,
        options: BuildOptions,
    ) -> Result<Arc<QuizSessionHandle>, FfiError> {
        guard("build full exam session", || {
            let exam_set_id: reinforce_core::ExamSetId =
                crate::error::parse_id("exam set id", &exam_set_id)?;
            let options = options.into_core()?;
            let database = locked("database", &self.inner)?;
            let session = build_full_exam(database.connection(), &exam_set_id, &options)?;
            Ok(Arc::new(QuizSessionHandle::new(session)))
        })
    }

    /// Builds one combined mock exam over every published question of the given
    /// subjects, ordered by subject, exam set and question number.
    pub fn create_subject_exam_session(
        &self,
        subjects: Vec<String>,
        options: BuildOptions,
    ) -> Result<Arc<QuizSessionHandle>, FfiError> {
        guard("build subject exam session", || {
            let options = options.into_core()?;
            let database = locked("database", &self.inner)?;
            let session = build_subject_exam(database.connection(), &subjects, &options)?;
            Ok(Arc::new(QuizSessionHandle::new(session)))
        })
    }

    pub fn create_topic_drill_session(
        &self,
        card_ids: Vec<String>,
        options: BuildOptions,
    ) -> Result<Arc<QuizSessionHandle>, FfiError> {
        guard("build topic drill session", || {
            let mut ids = Vec::with_capacity(card_ids.len());
            for id in &card_ids {
                ids.push(crate::error::parse_id::<reinforce_core::KnowledgeCardId>(
                    "knowledge card id",
                    id,
                )?);
            }
            let options = options.into_core()?;
            let database = locked("database", &self.inner)?;
            let session = build_topic_drill(database.connection(), &ids, &options)?;
            Ok(Arc::new(QuizSessionHandle::new(session)))
        })
    }

    pub fn create_practical_exam_session(
        &self,
        question_ids: Vec<String>,
        options: BuildOptions,
    ) -> Result<Arc<QuizSessionHandle>, FfiError> {
        guard("build practical exam session", || {
            let mut ids = Vec::with_capacity(question_ids.len());
            for id in &question_ids {
                ids.push(crate::error::parse_id::<reinforce_core::QuestionCardId>(
                    "question id",
                    id,
                )?);
            }
            let options = options.into_core()?;
            let database = locked("database", &self.inner)?;
            let session = build_practical_exam(database.connection(), &ids, &options)?;
            Ok(Arc::new(QuizSessionHandle::new(session)))
        })
    }

    pub fn create_weakness_drill_session(
        &self,
        user_id: String,
        options: BuildOptions,
    ) -> Result<Arc<QuizSessionHandle>, FfiError> {
        guard("build weakness drill session", || {
            let user_id = parse_user_id(&user_id)?;
            let options = options.into_core()?;
            let database = locked("database", &self.inner)?;
            let session = build_weakness_drill(database.connection(), &user_id, &options)?;
            Ok(Arc::new(QuizSessionHandle::new(session)))
        })
    }

    /// Creates a nested subject folder. `parent_id` must reference an existing
    /// group when given.
    pub fn create_subject_group(
        &self,
        parent_id: Option<String>,
        name: String,
        subjects: Vec<String>,
        sort_order: i32,
    ) -> Result<SubjectGroup, FfiError> {
        guard("create subject group", || {
            let parent_id = parent_id
                .as_deref()
                .map(|value| parse_id::<SubjectGroupId>("subject group id", value))
                .transpose()?;
            let database = locked("database", &self.inner)?;
            Ok(subject_groups_store::create_subject_group(
                database.connection(),
                parent_id.as_ref(),
                &name,
                &subjects,
                sort_order,
                Timestamp::now(),
            )?
            .into())
        })
    }

    /// Replaces a group's parent, name, subjects and sort order. A group can
    /// never be moved below itself or one of its descendants.
    pub fn update_subject_group(
        &self,
        id: String,
        parent_id: Option<String>,
        name: String,
        subjects: Vec<String>,
        sort_order: i32,
    ) -> Result<SubjectGroup, FfiError> {
        guard("update subject group", || {
            let id: SubjectGroupId = parse_id("subject group id", &id)?;
            let parent_id = parent_id
                .as_deref()
                .map(|value| parse_id::<SubjectGroupId>("subject group id", value))
                .transpose()?;
            let database = locked("database", &self.inner)?;
            Ok(subject_groups_store::update_subject_group(
                database.connection(),
                &id,
                parent_id.as_ref(),
                &name,
                &subjects,
                sort_order,
                Timestamp::now(),
            )?
            .into())
        })
    }

    /// Deletes a group; descendants and subject links cascade with it.
    pub fn delete_subject_group(&self, id: String) -> Result<bool, FfiError> {
        guard("delete subject group", || {
            let id: SubjectGroupId = parse_id("subject group id", &id)?;
            let database = locked("database", &self.inner)?;
            Ok(subject_groups_store::delete_subject_group(
                database.connection(),
                &id,
            )?)
        })
    }

    pub fn create_user(&self, display_name: String) -> Result<String, FfiError> {
        guard("create user", || {
            if display_name.trim().is_empty() {
                return Err(FfiError::invalid(
                    "display name",
                    "display name must not be empty",
                ));
            }
            let mut database = locked("database", &self.inner)?;
            let user_id = auth::create_user(
                database.connection_mut(),
                display_name.trim(),
                Timestamp::now(),
            )?;
            Ok(user_id.as_str())
        })
    }

    /// Writes a graded session and derives FSRS scheduling in one transaction.
    pub fn commit_session(
        &self,
        session: Arc<QuizSessionHandle>,
        user_id: String,
        now: String,
    ) -> Result<u32, FfiError> {
        guard("commit session", || {
            let user_id = parse_user_id(&user_id)?;
            let now = parse_timestamp("now", &now)?;
            let mut database = locked("database", &self.inner)?;
            let transaction = database.transaction()?;
            let cursor = study_store::next_attempt_cursor(&transaction)?;
            if session.is_persisted() {
                return Err(FfiError::invalid(
                    "quiz session",
                    "session has already been persisted",
                ));
            }
            let attempts = {
                let mut session = locked("quiz session", &session.inner)?;
                session.finish(now)?;
                session.attempts(&user_id, cursor)
            };
            let written = u32::try_from(attempts.len()).unwrap_or(u32::MAX);
            for attempt in &attempts {
                study_store::append_attempt(&transaction, attempt)?;
                if reinforce_core::study::contributes_to_scheduling(attempt) {
                    let scheduling = study_store::recompute_for_card(
                        &transaction,
                        &attempt.user_id,
                        &attempt.card_id,
                        attempt.direction,
                        0.9,
                    )?;
                    study_store::save_scheduling_if_effective(&transaction, &scheduling)?;
                }
            }
            transaction.commit().map_err(|error| FfiError::Core {
                message: error.to_string(),
            })?;
            session.mark_persisted();
            Ok(written)
        })
    }

    pub fn append_attempt(
        &self,
        record: AttemptLogRecord,
    ) -> Result<Option<CardSchedulingRecord>, FfiError> {
        guard("append attempt", || {
            let attempt: reinforce_core::AttemptLog = record.try_into()?;
            if attempt.sync_cursor < 0 {
                return Err(FfiError::invalid(
                    "sync cursor",
                    "sync cursor must not be negative",
                ));
            }
            let mut database = locked("database", &self.inner)?;
            let transaction = database.transaction()?;
            study_store::append_attempt(&transaction, &attempt)?;
            let scheduling = if reinforce_core::study::contributes_to_scheduling(&attempt) {
                let value = study_store::recompute_for_card(
                    &transaction,
                    &attempt.user_id,
                    &attempt.card_id,
                    attempt.direction,
                    0.9,
                )?;
                study_store::save_scheduling_if_effective(&transaction, &value)?
                    .then(|| CardSchedulingRecord::from(value))
            } else {
                None
            };
            transaction.commit().map_err(|error| FfiError::Core {
                message: error.to_string(),
            })?;
            Ok(scheduling)
        })
    }

    pub fn next_attempt_cursor(&self) -> Result<i64, FfiError> {
        guard("next attempt cursor", || {
            let database = locked("database", &self.inner)?;
            Ok(study_store::next_attempt_cursor(database.connection())?)
        })
    }

    pub fn scheduling(
        &self,
        user_id: String,
        card_id: String,
    ) -> Result<Option<CardSchedulingRecord>, FfiError> {
        guard("read scheduling", || {
            let user_id = parse_user_id(&user_id)?;
            let card_id = parse_card_id(&card_id)?;
            let database = locked("database", &self.inner)?;
            Ok(study_store::get_scheduling(
                database.connection(),
                &user_id,
                &card_id,
                AttemptDirection::Default,
            )?
            .map(CardSchedulingRecord::from))
        })
    }

    pub fn due_questions(
        &self,
        user_id: String,
        now: String,
        new_card_limit: u32,
        limit: u32,
    ) -> Result<Vec<DueQuestion>, FfiError> {
        guard("list due questions", || {
            let user_id = parse_user_id(&user_id)?;
            let now = parse_timestamp("now", &now)?;
            let new_card_limit = usize_field("new card limit", new_card_limit)?;
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(quiz_store::due_questions(
                database.connection(),
                &user_id,
                now,
                new_card_limit,
                limit,
            )?
            .into_iter()
            .map(|due| DueQuestion {
                id: due.id.as_str(),
                is_new: due.is_new,
            })
            .collect())
        })
    }

    pub fn due_schedules(
        &self,
        user_id: String,
        now: String,
        limit: u32,
    ) -> Result<Vec<CardSchedulingRecord>, FfiError> {
        guard("list due schedules", || {
            let user_id = parse_user_id(&user_id)?;
            let now = parse_timestamp("now", &now)?;
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            let due =
                quiz_store::due_questions(database.connection(), &user_id, now, limit, limit)?;
            let mut records = Vec::new();
            for entry in due {
                let card_id: CardId = crate::error::parse_id("card id", &entry.id.as_str())?;
                if let Some(scheduling) = study_store::get_scheduling(
                    database.connection(),
                    &user_id,
                    &card_id,
                    AttemptDirection::Default,
                )? {
                    records.push(CardSchedulingRecord::from(scheduling));
                }
            }
            Ok(records)
        })
    }

    pub fn weakness_report(&self, user_id: String, limit: u32) -> Result<WeaknessReport, FfiError> {
        guard("build weakness report", || {
            let user_id = parse_user_id(&user_id)?;
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(weakness::build_report(database.connection(), &user_id, limit)?.into())
        })
    }
}

#[allow(unsafe_code)]
#[uniffi::export]
pub fn new_attempt_id() -> String {
    generated_attempt_id()
}
