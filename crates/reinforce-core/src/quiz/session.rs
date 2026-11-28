use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::content::QuestionCard;
use crate::primitives::{
    AttemptDirection, AttemptId, CardId, Confidence, Error, QuestionType, Result, SessionMode,
    UserId,
};
use crate::quiz::grade::{
    AnswerSubmission, GradingConfig, GradingOutcome, grade_question, validate_config,
};
use crate::study::AttemptLog;
use crate::timestamp::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    NotStarted,
    InProgress { index: usize },
    Paused { index: usize },
    Expired { index: usize },
    Submitted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum TimerState {
    Idle,
    Running {
        elapsed_before: u64,
        started_at: Timestamp,
    },
    Paused {
        elapsed_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Countdown {
    limit_secs: Option<u32>,
    state: TimerState,
}

impl Countdown {
    fn new(limit_secs: Option<u32>) -> Self {
        Self {
            limit_secs,
            state: TimerState::Idle,
        }
    }

    fn limit_ms(&self) -> u64 {
        self.limit_secs
            .map_or(u64::MAX, |seconds| u64::from(seconds) * 1_000)
    }

    fn elapsed_ms(&self, now: Timestamp) -> u64 {
        match self.state {
            TimerState::Idle => 0,
            TimerState::Paused { elapsed_ms } => elapsed_ms,
            TimerState::Running {
                elapsed_before,
                started_at,
            } => {
                let since = (now.days_since(&started_at) * 86_400_000.0).max(0.0);
                elapsed_before.saturating_add(since.round() as u64)
            }
        }
    }

    fn start(&mut self, now: Timestamp) {
        self.state = TimerState::Running {
            elapsed_before: 0,
            started_at: now,
        };
    }

    fn pause(&mut self, now: Timestamp) {
        if let TimerState::Running { .. } = self.state {
            self.state = TimerState::Paused {
                elapsed_ms: self.elapsed_ms(now),
            };
        }
    }

    fn resume(&mut self, now: Timestamp) {
        if let TimerState::Paused { elapsed_ms } = self.state {
            self.state = TimerState::Running {
                elapsed_before: elapsed_ms,
                started_at: now,
            };
        }
    }

    fn is_expired(&self, now: Timestamp) -> bool {
        self.limit_secs.is_some() && self.elapsed_ms(now) >= self.limit_ms()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuizAnswer {
    pub submission: AnswerSubmission,
    pub confidence: Confidence,
    #[serde(default)]
    pub error_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub index: usize,
    pub question_id: crate::QuestionCardId,
    pub submission: AnswerSubmission,
    pub confidence: Confidence,
    #[serde(default)]
    pub error_tags: Vec<String>,
    pub duration_ms: i64,
    pub created_at: Timestamp,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradedAnswer {
    pub record: AnswerRecord,
    pub outcome: GradingOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub correct: usize,
    pub scoreable: usize,
    pub voided: usize,
    pub ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TickOutcome {
    None,
    QuestionTimedOut { index: usize, graded: GradedAnswer },
    SessionExpired { index: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuizSession {
    mode: SessionMode,
    questions: Vec<QuestionCard>,
    state: SessionState,
    total_timer: Option<Countdown>,
    question_timer: Countdown,
    grading: GradingConfig,
    affects_scheduling: bool,
    defer_grading: bool,
    answers: Vec<AnswerRecord>,
    graded: Vec<GradedAnswer>,
}

impl QuizSession {
    pub fn new(
        mode: SessionMode,
        questions: Vec<QuestionCard>,
        total_time_secs: Option<u32>,
        seconds_per_question: Option<u16>,
        grading: GradingConfig,
        affects_scheduling: bool,
        defer_grading: bool,
    ) -> Result<Self> {
        validate_config(&grading)?;
        if total_time_secs == Some(0) {
            return Err(Error::InvalidValue(
                "total time",
                "must be greater than zero".to_owned(),
            ));
        }
        if let Some(seconds) = seconds_per_question {
            if !(1..=300).contains(&seconds) {
                return Err(Error::InvalidValue("question time", seconds.to_string()));
            }
        }
        let limit_secs = match mode {
            SessionMode::FullExam => total_time_secs,
            SessionMode::PracticalExam => seconds_per_question.map(u32::from),
            _ => None,
        };
        let total_timer = if mode == SessionMode::FullExam {
            total_time_secs.map(|seconds| Countdown::new(Some(seconds)))
        } else {
            None
        };
        Ok(Self {
            mode,
            questions,
            state: SessionState::NotStarted,
            total_timer,
            question_timer: Countdown::new(limit_secs),
            grading,
            affects_scheduling,
            defer_grading,
            answers: Vec::new(),
            graded: Vec::new(),
        })
    }

    pub fn mode(&self) -> SessionMode {
        self.mode
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn questions(&self) -> &[QuestionCard] {
        &self.questions
    }

    pub fn answers(&self) -> &[AnswerRecord] {
        &self.answers
    }

    pub fn graded(&self) -> &[GradedAnswer] {
        &self.graded
    }

    /// Returns the stored answer for one question index, if it was answered.
    pub fn answer_for(&self, index: usize) -> Option<&AnswerRecord> {
        self.answers.iter().find(|record| record.index == index)
    }

    /// Returns the grading result for one question index, if it is known yet.
    pub fn graded_for(&self, index: usize) -> Option<&GradedAnswer> {
        self.graded
            .iter()
            .find(|graded| graded.record.index == index)
    }

    pub fn grading(&self) -> GradingConfig {
        self.grading
    }

    pub fn affects_scheduling(&self) -> bool {
        self.affects_scheduling
    }

    pub fn start(&mut self, now: Timestamp) {
        if self.state != SessionState::NotStarted {
            return;
        }
        self.state = if self.questions.is_empty() {
            SessionState::Submitted
        } else {
            SessionState::InProgress { index: 0 }
        };
        self.question_timer.start(now);
        if let Some(timer) = &mut self.total_timer {
            timer.start(now);
        }
    }

    pub fn pause(&mut self, now: Timestamp) -> Result<()> {
        match self.state {
            SessionState::InProgress { index } => {
                self.question_timer.pause(now);
                if let Some(timer) = &mut self.total_timer {
                    timer.pause(now);
                }
                self.state = SessionState::Paused { index };
                Ok(())
            }
            SessionState::Paused { .. } => Ok(()),
            _ => Err(Error::InvalidValue(
                "session state",
                "only a running session can pause".to_owned(),
            )),
        }
    }

    pub fn resume(&mut self, now: Timestamp) -> Result<()> {
        match self.state {
            SessionState::Paused { index } => {
                self.question_timer.resume(now);
                if let Some(timer) = &mut self.total_timer {
                    timer.resume(now);
                }
                self.state = SessionState::InProgress { index };
                Ok(())
            }
            SessionState::InProgress { .. } => Ok(()),
            _ => Err(Error::InvalidValue(
                "session state",
                "only a paused session can resume".to_owned(),
            )),
        }
    }

    pub fn tick(&mut self, now: Timestamp) -> Result<TickOutcome> {
        let SessionState::InProgress { index } = self.state else {
            return Ok(TickOutcome::None);
        };
        if self
            .total_timer
            .as_ref()
            .is_some_and(|timer| timer.is_expired(now))
        {
            self.state = SessionState::Expired { index };
            return Ok(TickOutcome::SessionExpired { index });
        }
        if self.question_timer.is_expired(now) {
            if self.answer_for(index).is_some() {
                return Ok(TickOutcome::None);
            }
            let graded = self.record_timeout(now)?;
            return Ok(TickOutcome::QuestionTimedOut { index, graded });
        }
        Ok(TickOutcome::None)
    }

    pub fn submit_answer(
        &mut self,
        answer: QuizAnswer,
        now: Timestamp,
    ) -> Result<Option<GradingOutcome>> {
        let SessionState::InProgress { index } = self.state else {
            return Err(Error::InvalidValue(
                "session state",
                "no question is in progress".to_owned(),
            ));
        };
        let duration_ms = self
            .question_timer
            .elapsed_ms(now)
            .min(self.question_timer.limit_ms()) as i64;
        let record = AnswerRecord {
            index,
            question_id: self.questions[index].id.clone(),
            submission: answer.submission,
            confidence: answer.confidence,
            error_tags: answer.error_tags,
            duration_ms,
            created_at: now,
            timed_out: false,
        };
        let graded = self.grade_record(&record)?;
        self.store_answer(record, &graded);
        self.advance(now);
        Ok(if self.defer_grading {
            None
        } else {
            Some(graded.outcome)
        })
    }

    /// Switches the active question without submitting or advancing.
    ///
    /// A running session restarts the per-question timer on the new index; a
    /// paused session only moves the paused index and stays paused.
    pub fn select_index(&mut self, index: usize, now: Timestamp) -> Result<()> {
        if index >= self.questions.len() {
            return Err(Error::InvalidValue("question index", index.to_string()));
        }
        match self.state {
            SessionState::InProgress { .. } => {
                self.state = SessionState::InProgress { index };
                self.question_timer.start(now);
                Ok(())
            }
            SessionState::Paused { .. } => {
                self.state = SessionState::Paused { index };
                Ok(())
            }
            _ => Err(Error::InvalidValue(
                "session state",
                "only a running or paused session can select a question".to_owned(),
            )),
        }
    }

    /// Grades and stores an answer for any question index without advancing.
    ///
    /// Re-answering the same index replaces the stored answer (and its immediate
    /// grading) instead of appending, so `score` and `attempts` never count the
    /// same question twice. Completion always stays with `finish`.
    pub fn submit_answer_at(
        &mut self,
        index: usize,
        answer: QuizAnswer,
        now: Timestamp,
    ) -> Result<Option<GradingOutcome>> {
        if !matches!(self.state, SessionState::InProgress { .. }) {
            return Err(Error::InvalidValue(
                "session state",
                "no question is in progress".to_owned(),
            ));
        }
        let question_id = self
            .questions
            .get(index)
            .ok_or_else(|| Error::InvalidValue("question index", index.to_string()))?
            .id
            .clone();
        let duration_ms = self
            .question_timer
            .elapsed_ms(now)
            .min(self.question_timer.limit_ms()) as i64;
        let record = AnswerRecord {
            index,
            question_id,
            submission: answer.submission,
            confidence: answer.confidence,
            error_tags: answer.error_tags,
            duration_ms,
            created_at: now,
            timed_out: false,
        };
        let graded = self.grade_record(&record)?;
        self.store_answer(record, &graded);
        Ok(if self.defer_grading {
            None
        } else {
            Some(graded.outcome)
        })
    }

    pub fn finish(&mut self, _now: Timestamp) -> Result<Vec<GradedAnswer>> {
        if self.state == SessionState::NotStarted {
            return Err(Error::InvalidValue(
                "session state",
                "session has not started".to_owned(),
            ));
        }
        if self.defer_grading && !self.graded_matches_answers() {
            self.graded = self
                .answers
                .iter()
                .map(|record| self.grade_record(record))
                .collect::<Result<Vec<_>>>()?;
        }
        if matches!(
            self.state,
            SessionState::InProgress { .. } | SessionState::Paused { .. }
        ) {
            self.state = SessionState::Submitted;
        }
        Ok(self.graded.clone())
    }

    pub fn score(&self) -> Result<Score> {
        let mut correct = 0;
        let mut scoreable = 0;
        let mut voided = 0;
        for record in &self.answers {
            let outcome = self.grade_record(record)?.outcome;
            if outcome.voided {
                voided += 1;
                continue;
            }
            if outcome.scoreable {
                scoreable += 1;
                if outcome.is_correct {
                    correct += 1;
                }
            }
        }
        let ratio = if scoreable == 0 {
            0.0
        } else {
            correct as f64 / scoreable as f64
        };
        Ok(Score {
            correct,
            scoreable,
            voided,
            ratio,
        })
    }

    pub fn attempts(&self, user_id: &UserId, starting_cursor: i64) -> Vec<AttemptLog> {
        self.graded
            .iter()
            .enumerate()
            .map(|(offset, graded)| {
                let questions = &self.questions[graded.record.index];
                let mut error_tags = Vec::new();
                if graded.record.timed_out {
                    error_tags.push("timeout".to_owned());
                } else if !graded.outcome.is_correct {
                    error_tags.push("incorrect".to_owned());
                }
                for tag in &graded.record.error_tags {
                    if !error_tags.contains(tag) {
                        error_tags.push(tag.clone());
                    }
                }
                AttemptLog {
                    id: AttemptId::generate(),
                    user_id: user_id.clone(),
                    card_id: CardId::from(graded.record.question_id.clone()),
                    direction: AttemptDirection::Default,
                    session_mode: self.mode,
                    submitted_payload: json!({
                        "answer": graded.record.submission,
                        "confidence": graded.record.confidence,
                        "grading": graded.outcome,
                        "grading_mode": questions.grading_mode,
                        "voided": graded.outcome.voided,
                        "timed_out": graded.record.timed_out,
                    }),
                    is_correct: graded.outcome.is_correct,
                    duration_ms: graded.record.duration_ms,
                    confidence: graded.record.confidence,
                    error_tags,
                    created_at: graded.record.created_at,
                    sync_cursor: starting_cursor + i64::try_from(offset).unwrap_or(i64::MAX),
                    affects_scheduling: graded.outcome.affects_scheduling,
                }
            })
            .collect()
    }

    fn grade_record(&self, record: &AnswerRecord) -> Result<GradedAnswer> {
        let question = self
            .questions
            .get(record.index)
            .ok_or_else(|| Error::InvalidValue("answer index", record.index.to_string()))?;
        Ok(GradedAnswer {
            record: record.clone(),
            outcome: grade_question(
                question,
                &record.submission,
                &self.grading,
                self.affects_scheduling,
            )?,
        })
    }

    /// A deferred session only skips re-grading when its graded list is exactly
    /// the stored answers in the same order.
    fn graded_matches_answers(&self) -> bool {
        self.graded.len() == self.answers.len()
            && self
                .graded
                .iter()
                .zip(self.answers.iter())
                .all(|(graded, record)| &graded.record == record)
    }

    fn store_answer(&mut self, record: AnswerRecord, graded: &GradedAnswer) {
        let index = record.index;
        let timed_out = record.timed_out;
        match self
            .answers
            .iter_mut()
            .find(|existing| existing.index == index)
        {
            Some(existing) => *existing = record,
            None => self.answers.push(record),
        }
        if self.defer_grading && !timed_out {
            // A timeout recorded for this index before the answer is stale now;
            // dropping it lets `finish` re-grade the whole paper from answers.
            self.graded
                .retain(|existing| existing.record.index != index);
        } else {
            match self
                .graded
                .iter_mut()
                .find(|existing| existing.record.index == index)
            {
                Some(existing) => *existing = graded.clone(),
                None => self.graded.push(graded.clone()),
            }
        }
    }

    fn record_timeout(&mut self, now: Timestamp) -> Result<GradedAnswer> {
        let SessionState::InProgress { index } = self.state else {
            return Err(Error::InvalidValue(
                "session state",
                "no question is in progress".to_owned(),
            ));
        };
        let record = AnswerRecord {
            index,
            question_id: self.questions[index].id.clone(),
            submission: AnswerSubmission::TimedOut,
            confidence: Confidence::Guessed,
            error_tags: Vec::new(),
            duration_ms: self.question_timer.limit_ms() as i64,
            created_at: now,
            timed_out: true,
        };
        let graded = self.grade_record(&record)?;
        self.store_answer(record, &graded);
        self.advance(now);
        Ok(graded)
    }

    fn advance(&mut self, now: Timestamp) {
        let SessionState::InProgress { index } = self.state else {
            return;
        };
        let next = index + 1;
        if next < self.questions.len() {
            self.state = SessionState::InProgress { index: next };
            self.question_timer.start(now);
        } else {
            self.state = SessionState::Submitted;
        }
    }
}

/// Practical sessions only accept these question types.
pub fn is_practical_question(question: &QuestionCard) -> bool {
    matches!(
        question.question_type,
        QuestionType::PinIdentify | QuestionType::PinLocate | QuestionType::ImageQa
    )
}
