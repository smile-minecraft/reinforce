use rusqlite::Connection;

use crate::db::{content as store, quiz as quiz_store};
use crate::primitives::{Error, Result, SessionMode};
use crate::quiz::grade::{GradingConfig, validate_config};
use crate::quiz::session::{QuizSession, is_practical_question};
use crate::weakness;
use crate::{ExamSetId, KnowledgeCardId, QuestionCard, QuestionCardId, Timestamp, UserId};

#[derive(Debug, Clone, PartialEq)]
pub struct BuildOptions {
    pub question_limit: usize,
    pub new_card_limit: usize,
    pub total_time_secs: Option<u32>,
    pub seconds_per_question: u16,
    pub affects_scheduling: bool,
    pub pin_radius: f64,
    pub pin_aspect_ratio: Option<f64>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            question_limit: 50,
            new_card_limit: 20,
            total_time_secs: None,
            seconds_per_question: 30,
            affects_scheduling: true,
            pin_radius: 0.05,
            pin_aspect_ratio: None,
        }
    }
}

impl BuildOptions {
    fn grading(&self) -> GradingConfig {
        GradingConfig {
            pin_radius: self.pin_radius,
            pin_aspect_ratio: self.pin_aspect_ratio,
        }
    }
}

fn validate(options: &BuildOptions) -> Result<()> {
    if options.question_limit == 0 {
        return Err(Error::InvalidValue(
            "question limit",
            "must be greater than zero".to_owned(),
        ));
    }
    validate_config(&options.grading())
}

fn session(
    mode: SessionMode,
    questions: Vec<QuestionCard>,
    options: &BuildOptions,
    total_time_secs: Option<u32>,
    seconds_per_question: Option<u16>,
    defer_grading: bool,
) -> Result<QuizSession> {
    QuizSession::new(
        mode,
        questions,
        total_time_secs,
        seconds_per_question,
        options.grading(),
        options.affects_scheduling,
        defer_grading,
    )
}

fn load(connection: &Connection, ids: Vec<QuestionCardId>) -> Result<Vec<QuestionCard>> {
    ids.into_iter()
        .map(|id| {
            store::get_question_card(connection, &id)?
                .ok_or_else(|| Error::InvalidValue("question id", id.as_str()))
        })
        .collect()
}

/// Overdue cards first (oldest due date first), then up to `new_card_limit`
/// cards without any scheduling row.
pub fn build_daily_due(
    connection: &Connection,
    user_id: &UserId,
    now: Timestamp,
    options: &BuildOptions,
) -> Result<QuizSession> {
    validate(options)?;
    let ids = quiz_store::due_questions(
        connection,
        user_id,
        now,
        options.new_card_limit,
        options.question_limit,
    )?
    .into_iter()
    .map(|due| due.id)
    .collect();
    session(
        SessionMode::DailyDue,
        load(connection, ids)?,
        options,
        None,
        None,
        false,
    )
}

/// Questions are ordered by `question_no`; grading is deferred until finish.
pub fn build_full_exam(
    connection: &Connection,
    exam_set_id: &ExamSetId,
    options: &BuildOptions,
) -> Result<QuizSession> {
    validate(options)?;
    let ids = quiz_store::exam_question_ids(connection, exam_set_id, options.question_limit)?;
    session(
        SessionMode::FullExam,
        load(connection, ids)?,
        options,
        options.total_time_secs,
        None,
        true,
    )
}

/// Follows `tests` edges from a question or one of its options to knowledge cards.
pub fn build_topic_drill(
    connection: &Connection,
    knowledge_card_ids: &[KnowledgeCardId],
    options: &BuildOptions,
) -> Result<QuizSession> {
    validate(options)?;
    let ids =
        quiz_store::topic_question_ids(connection, knowledge_card_ids, options.question_limit)?;
    session(
        SessionMode::TopicDrill,
        load(connection, ids)?,
        options,
        None,
        None,
        false,
    )
}

/// Only image and pin questions are eligible; per-question time is 1–300 seconds.
pub fn build_practical_exam(
    connection: &Connection,
    question_ids: &[QuestionCardId],
    options: &BuildOptions,
) -> Result<QuizSession> {
    validate(options)?;
    let ids = if question_ids.is_empty() {
        quiz_store::practical_question_ids(connection, options.question_limit)?
    } else {
        question_ids.to_vec()
    };
    let questions = load(connection, ids)?
        .into_iter()
        .filter(is_practical_question)
        .take(options.question_limit)
        .collect();
    session(
        SessionMode::PracticalExam,
        questions,
        options,
        None,
        Some(options.seconds_per_question),
        false,
    )
}

/// Highest-scoring weakness questions first, then questions linked to weak
/// knowledge cards through `tests` edges.
pub fn build_weakness_drill(
    connection: &Connection,
    user_id: &UserId,
    options: &BuildOptions,
) -> Result<QuizSession> {
    validate(options)?;
    let report = weakness::build_report(connection, user_id, options.question_limit)?;
    let mut ids: Vec<QuestionCardId> = report
        .questions
        .iter()
        .map(|entry| entry.question_id.clone())
        .collect();
    if ids.len() < options.question_limit {
        for card in report.knowledge_cards {
            let linked = quiz_store::topic_question_ids(
                connection,
                std::slice::from_ref(&card.card_id),
                options.question_limit,
            )?;
            for id in linked {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    ids.truncate(options.question_limit);
    session(
        SessionMode::WeaknessDrill,
        load(connection, ids)?,
        options,
        None,
        None,
        false,
    )
}
