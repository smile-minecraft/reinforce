use std::path::Path;
use std::sync::{Arc, Mutex};

use reinforce_core::db::{auth, study as study_store};
use reinforce_core::db::{content as content_store, quiz as quiz_store, search as search_store};
use reinforce_core::primitives::AttemptDirection;
use reinforce_core::proposal::{
    ApprovalOptions, ApprovalOutcome, BatchOperation, SubmitOutcome, approve_batch, submit_batch,
};
use reinforce_core::quiz::build::{
    build_daily_due, build_full_exam, build_practical_exam, build_topic_drill, build_weakness_drill,
};
use reinforce_core::validation::Actor;
use reinforce_core::weakness;
use reinforce_core::{CardId, Database as CoreDatabase, Timestamp};

use crate::error::{FfiError, guard, locked, parse_timestamp, usize_field};
use crate::session::QuizSessionHandle;
use crate::types::{
    AttemptLogRecord, BuildOptions, CardSchedulingRecord, DueQuestion, ExamSet, ImportSummary,
    KnowledgeCard, Note, QuestionCard, QuestionGroup, SearchHit, WeaknessReport,
    generated_attempt_id, parse_card_id, parse_user_id,
};

#[derive(uniffi::Object)]
pub struct LocalDatabase {
    inner: Mutex<CoreDatabase>,
}

#[allow(unsafe_code)]
#[uniffi::export]
impl LocalDatabase {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, FfiError> {
        guard("open database", || {
            if path.trim().is_empty() {
                return Err(FfiError::invalid("database path", "path must not be empty"));
            }
            let database = CoreDatabase::open(Path::new(&path))?;
            Ok(Arc::new(Self {
                inner: Mutex::new(database),
            }))
        })
    }

    pub fn schema_version(&self) -> Result<u32, FfiError> {
        guard("schema version", || {
            let database = locked("database", &self.inner)?;
            Ok(database.schema_version()?)
        })
    }

    /// Reads a newline-delimited standard bundle and publishes it locally.
    pub fn import_bundle(&self, bundle_path: String) -> Result<ImportSummary, FfiError> {
        guard("import bundle", || {
            if bundle_path.trim().is_empty() {
                return Err(FfiError::invalid("bundle path", "path must not be empty"));
            }
            let content = std::fs::read_to_string(&bundle_path)?;
            let mut operations = Vec::new();
            for (index, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let operation =
                    serde_json::from_str::<BatchOperation>(line.trim()).map_err(|error| {
                        FfiError::invalid(format!("bundle line {}", index + 1), error.to_string())
                    })?;
                operations.push(operation);
            }
            if operations.is_empty() {
                return Err(FfiError::invalid("bundle", "no operations found"));
            }

            let mut database = locked("database", &self.inner)?;
            let proposal = match submit_batch(database.connection_mut(), Actor::Owner, operations)?
            {
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
                    promote_verified: true,
                },
            )? {
                ApprovalOutcome::Applied { applied, .. } => Ok(ImportSummary {
                    proposal_id: proposal.id.as_str(),
                    applied: u32::try_from(applied).unwrap_or(u32::MAX),
                }),
                ApprovalOutcome::Rejected { errors, .. } => {
                    Err(FfiError::invalid("bundle", serde_json::to_string(&errors)?))
                }
            }
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
            let scheduling = if attempt.affects_scheduling {
                let value = study_store::recompute_for_card(
                    &transaction,
                    &attempt.user_id,
                    &attempt.card_id,
                    attempt.direction,
                    0.9,
                )?;
                study_store::save_scheduling(&transaction, &value)?;
                Some(CardSchedulingRecord::from(value))
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
