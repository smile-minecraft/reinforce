use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use reinforce_core::QuizSession as CoreQuizSession;

use crate::error::{FfiError, guard, locked, parse_timestamp};
use crate::types::{
    AttemptLogRecord, GradedAnswer, GradingOutcome, QuestionCard, QuizAnswer, Score, SessionMode,
    SessionState, TickOutcome,
};

#[derive(uniffi::Object)]
pub struct QuizSessionHandle {
    pub(crate) inner: Mutex<CoreQuizSession>,
    persisted: AtomicBool,
}

impl QuizSessionHandle {
    pub(crate) fn new(session: CoreQuizSession) -> Self {
        Self {
            inner: Mutex::new(session),
            persisted: AtomicBool::new(false),
        }
    }

    pub(crate) fn is_persisted(&self) -> bool {
        self.persisted.load(Ordering::SeqCst)
    }

    pub(crate) fn mark_persisted(&self) {
        self.persisted.store(true, Ordering::SeqCst);
    }
}

#[allow(unsafe_code)]
#[uniffi::export]
impl QuizSessionHandle {
    pub fn mode(&self) -> Result<SessionMode, FfiError> {
        guard("session mode", || {
            let session = locked("quiz session", &self.inner)?;
            Ok(session.mode().into())
        })
    }

    pub fn state(&self) -> Result<SessionState, FfiError> {
        guard("session state", || {
            let session = locked("quiz session", &self.inner)?;
            Ok(session.state().into())
        })
    }

    pub fn questions(&self) -> Result<Vec<QuestionCard>, FfiError> {
        guard("session questions", || {
            let session = locked("quiz session", &self.inner)?;
            Ok(session
                .questions()
                .iter()
                .cloned()
                .map(QuestionCard::from)
                .collect())
        })
    }

    pub fn current_question(&self) -> Result<QuestionCard, FfiError> {
        guard("current question", || {
            let session = locked("quiz session", &self.inner)?;
            let index = match session.state() {
                reinforce_core::SessionState::InProgress { index }
                | reinforce_core::SessionState::Paused { index }
                | reinforce_core::SessionState::Expired { index } => index,
                reinforce_core::SessionState::NotStarted
                | reinforce_core::SessionState::Submitted => {
                    return Err(FfiError::invalid(
                        "session state",
                        "no question is in progress".to_owned(),
                    ));
                }
            };
            session
                .questions()
                .get(index)
                .cloned()
                .map(QuestionCard::from)
                .ok_or_else(|| FfiError::missing("question index is out of range"))
        })
    }

    pub fn start(&self, now: String) -> Result<(), FfiError> {
        guard("start session", || {
            let now = parse_timestamp("now", &now)?;
            let mut session = locked("quiz session", &self.inner)?;
            session.start(now);
            Ok(())
        })
    }

    pub fn pause(&self, now: String) -> Result<(), FfiError> {
        guard("pause session", || {
            let now = parse_timestamp("now", &now)?;
            let mut session = locked("quiz session", &self.inner)?;
            session.pause(now)?;
            Ok(())
        })
    }

    pub fn resume(&self, now: String) -> Result<(), FfiError> {
        guard("resume session", || {
            let now = parse_timestamp("now", &now)?;
            let mut session = locked("quiz session", &self.inner)?;
            session.resume(now)?;
            Ok(())
        })
    }

    pub fn tick(&self, now: String) -> Result<TickOutcome, FfiError> {
        guard("tick session", || {
            let now = parse_timestamp("now", &now)?;
            let mut session = locked("quiz session", &self.inner)?;
            Ok(session.tick(now)?.into())
        })
    }

    pub fn submit_answer(
        &self,
        answer: QuizAnswer,
        now: String,
    ) -> Result<Option<GradingOutcome>, FfiError> {
        guard("submit answer", || {
            let now = parse_timestamp("now", &now)?;
            let answer = answer.into_core()?;
            let mut session = locked("quiz session", &self.inner)?;
            Ok(session
                .submit_answer(answer, now)?
                .map(GradingOutcome::from))
        })
    }

    pub fn finish(&self, now: String) -> Result<Vec<GradedAnswer>, FfiError> {
        guard("finish session", || {
            let now = parse_timestamp("now", &now)?;
            let mut session = locked("quiz session", &self.inner)?;
            Ok(session
                .finish(now)?
                .into_iter()
                .map(GradedAnswer::from)
                .collect())
        })
    }

    pub fn score(&self) -> Result<Score, FfiError> {
        guard("score session", || {
            let session = locked("quiz session", &self.inner)?;
            Ok(session.score()?.into())
        })
    }

    pub fn graded_answers(&self) -> Result<Vec<GradedAnswer>, FfiError> {
        guard("graded answers", || {
            let session = locked("quiz session", &self.inner)?;
            Ok(session
                .graded()
                .iter()
                .cloned()
                .map(GradedAnswer::from)
                .collect())
        })
    }

    /// Attempts carry the grading outcome, answer tags and time spent.
    pub fn attempts(
        &self,
        user_id: String,
        starting_cursor: i64,
    ) -> Result<Vec<AttemptLogRecord>, FfiError> {
        guard("session attempts", || {
            if starting_cursor < 0 {
                return Err(FfiError::invalid(
                    "starting cursor",
                    "starting cursor must not be negative",
                ));
            }
            let user_id = crate::error::parse_id("user id", &user_id)?;
            let session = locked("quiz session", &self.inner)?;
            Ok(session
                .attempts(&user_id, starting_cursor)
                .into_iter()
                .map(AttemptLogRecord::from)
                .collect())
        })
    }
}
