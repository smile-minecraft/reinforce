//! View state for the desktop client.
//!
//! Everything in this module is plain Rust: it owns no window, no GPU context
//! and no GPUI types. The window layer drives it from keyboard and mouse events
//! and renders whatever it exposes, which keeps the behaviour that matters —
//! which cards a note shows, how an answer is turned into a submission, when a
//! session is written to the attempt log — covered by ordinary `cargo test`.

use reinforce_core::content::{KnowledgeCard, QuestionCard};
use reinforce_core::db::catalog::{ExamSetSummary, NoteOutline, SubjectSummary};
use reinforce_core::primitives::{Confidence, QuestionType, SessionMode};
use reinforce_core::quiz::build::{
    BuildOptions, build_daily_due, build_full_exam, build_practical_exam, build_topic_drill,
    build_weakness_drill,
};
use reinforce_core::quiz::{
    AnswerSubmission, GradingOutcome, NormalizedPoint, PinClick, QuizAnswer, QuizSession, Score,
    SessionState, TickOutcome,
};
use reinforce_core::{ExamSetId, KnowledgeCardId, Note, NoteId, QuestionCardId, Timestamp, UserId};

use crate::store::Store;
use crate::{Error, Result};

/// The five study modes the spec requires from every client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuizMode {
    DailyDue,
    FullExam,
    TopicDrill,
    PracticalExam,
    WeaknessDrill,
}

impl QuizMode {
    pub const ALL: [QuizMode; 5] = [
        QuizMode::DailyDue,
        QuizMode::FullExam,
        QuizMode::TopicDrill,
        QuizMode::PracticalExam,
        QuizMode::WeaknessDrill,
    ];

    pub fn title(self) -> &'static str {
        match self {
            QuizMode::DailyDue => "每日到期",
            QuizMode::FullExam => "整卷模擬",
            QuizMode::TopicDrill => "主題／卡片",
            QuizMode::PracticalExam => "實務考",
            QuizMode::WeaknessDrill => "弱點複習",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            QuizMode::DailyDue => "依 FSRS 到期日排序，含新卡",
            QuizMode::FullExam => "依題號作答，交卷後統一評分",
            QuizMode::TopicDrill => "以本筆記卡片的 tests 邊抽題",
            QuizMode::PracticalExam => "釘選與影像題，逐題倒數",
            QuizMode::WeaknessDrill => "錯誤率高與沒把握的題目優先",
        }
    }

    pub fn session_mode(self) -> SessionMode {
        match self {
            QuizMode::DailyDue => SessionMode::DailyDue,
            QuizMode::FullExam => SessionMode::FullExam,
            QuizMode::TopicDrill => SessionMode::TopicDrill,
            QuizMode::PracticalExam => SessionMode::PracticalExam,
            QuizMode::WeaknessDrill => SessionMode::WeaknessDrill,
        }
    }

    /// Whether grading is deferred until the paper is handed in.
    pub fn deferred(self) -> bool {
        matches!(self, QuizMode::FullExam)
    }
}

/// What a mode needs besides the database: the exam set for a full paper and
/// the knowledge cards shown in the open note for a targeted drill.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuizTargets {
    pub exam_set: Option<ExamSetId>,
    pub cards: Vec<KnowledgeCardId>,
}

impl QuizTargets {
    /// Everything the open note transcludes, which is what a targeted drill
    /// follows through `tests` edges.
    pub fn from_outline(outline: Option<&NoteOutline>) -> Self {
        Self {
            exam_set: None,
            cards: outline
                .map(|outline| outline.cards.iter().map(|card| card.id.clone()).collect())
                .unwrap_or_default(),
        }
    }
}

/// The library list and the currently open note.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub subjects: Vec<SubjectSummary>,
    pub notes: Vec<Note>,
    pub cards: Vec<KnowledgeCard>,
    pub exam_sets: Vec<ExamSetSummary>,
    outline: Option<NoteOutline>,
}

impl Library {
    pub fn load(store: &Store) -> Result<Self> {
        Ok(Self {
            subjects: store.subjects()?,
            notes: store.notes(None)?,
            cards: store.knowledge_cards(None)?,
            exam_sets: store.exam_sets(None)?,
            outline: None,
        })
    }

    /// Opens the note at `index` and resolves its transcluded cards.
    pub fn select(&mut self, store: &Store, index: usize) -> Result<()> {
        let Some(note) = self.notes.get(index) else {
            return Ok(());
        };
        let id: NoteId = note.id.clone();
        self.outline = store.note_outline(&id)?;
        Ok(())
    }

    pub fn select_id(&mut self, store: &Store, id: &NoteId) -> Result<()> {
        let index = self.notes.iter().position(|note| &note.id == id);
        match index {
            Some(index) => self.select(store, index),
            None => Ok(()),
        }
    }

    pub fn outline(&self) -> Option<&NoteOutline> {
        self.outline.as_ref()
    }

    pub fn note(&self) -> Option<&Note> {
        self.outline.as_ref().map(|outline| &outline.note)
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.exam_sets.is_empty()
    }

    pub fn targets(&self) -> QuizTargets {
        QuizTargets {
            exam_set: self.exam_sets.first().map(|set| set.exam_set.id.clone()),
            cards: self
                .outline()
                .map(|outline| outline.cards.iter().map(|card| card.id.clone()).collect())
                .unwrap_or_default(),
        }
    }
}

/// Countdown to a practical exam date, shown on every screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PracticalCountdown {
    target: Timestamp,
}

impl PracticalCountdown {
    /// Accepts a `YYYY-MM-DD` date and treats it as midnight UTC.
    pub fn parse(date: &str) -> Result<Self> {
        let date = date.trim();
        let format = time::macros::format_description!("[year]-[month]-[day]");
        let date = time::Date::parse(date, &format).map_err(|_| {
            Error::Invalid("practical exam date", format!("{date} is not YYYY-MM-DD"))
        })?;
        let datetime = date
            .with_hms(0, 0, 0)
            .map_err(|_| Error::Invalid("practical exam date", date.to_string()))?
            .assume_utc();
        Ok(Self {
            target: Timestamp::from_offset(datetime),
        })
    }

    pub fn target(&self) -> Timestamp {
        self.target
    }

    /// Whole days left, rounding up so the last day still reads "1 天".
    pub fn days_left(&self, now: Timestamp) -> i64 {
        let remaining = self.target.days_since(&now);
        if remaining <= 0.0 {
            0
        } else {
            remaining.ceil() as i64
        }
    }

    pub fn label(&self, now: Timestamp) -> String {
        let days = self.days_left(now);
        if days == 0 {
            "實務考：今天".to_owned()
        } else {
            format!("實務考倒數 {days} 天")
        }
    }
}

/// A running quiz session plus the answer being composed.
pub struct Quiz {
    mode: QuizMode,
    session: QuizSession,
    selection: Vec<String>,
    typed: String,
    feedback: Option<GradingOutcome>,
    submitted: usize,
    question_started_at: Timestamp,
    question_limit_secs: Option<u16>,
}

impl Quiz {
    pub fn start(
        store: &Store,
        mode: QuizMode,
        targets: &QuizTargets,
        options: &BuildOptions,
        user_id: &UserId,
        now: Timestamp,
    ) -> Result<Self> {
        let session = match mode {
            QuizMode::DailyDue => build_daily_due(store.connection(), user_id, now, options)?,
            QuizMode::FullExam => {
                let exam_set = targets.exam_set.clone().ok_or_else(|| {
                    Error::Invalid("exam set", "no exam set to simulate".to_owned())
                })?;
                build_full_exam(store.connection(), &exam_set, options)?
            }
            QuizMode::TopicDrill => {
                if targets.cards.is_empty() {
                    return Err(Error::Invalid(
                        "topic drill",
                        "open a note with cards first".to_owned(),
                    ));
                }
                build_topic_drill(store.connection(), &targets.cards, options)?
            }
            QuizMode::PracticalExam => build_practical_exam(store.connection(), &[], options)?,
            QuizMode::WeaknessDrill => build_weakness_drill(store.connection(), user_id, options)?,
        };
        let mut quiz = Self {
            mode,
            session,
            selection: Vec::new(),
            typed: String::new(),
            feedback: None,
            submitted: 0,
            question_started_at: now,
            question_limit_secs: (mode == QuizMode::PracticalExam)
                .then_some(options.seconds_per_question),
        };
        quiz.session.start(now);
        Ok(quiz)
    }

    pub fn mode(&self) -> QuizMode {
        self.mode
    }

    pub fn session(&self) -> &QuizSession {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut QuizSession {
        &mut self.session
    }

    pub fn questions(&self) -> &[QuestionCard] {
        self.session.questions()
    }

    pub fn is_empty(&self) -> bool {
        self.questions().is_empty()
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.session.state(),
            SessionState::Submitted | SessionState::Expired { .. }
        )
    }

    pub fn index(&self) -> usize {
        match self.session.state() {
            SessionState::NotStarted => 0,
            SessionState::InProgress { index }
            | SessionState::Paused { index }
            | SessionState::Expired { index } => {
                index.min(self.questions().len().saturating_sub(1))
            }
            SessionState::Submitted => self.questions().len(),
        }
    }

    pub fn total(&self) -> usize {
        self.questions().len()
    }

    pub fn current(&self) -> Option<&QuestionCard> {
        if self.is_finished() {
            return None;
        }
        self.questions().get(self.index())
    }

    pub fn current_question_id(&self) -> Option<QuestionCardId> {
        self.current().map(|question| question.id.clone())
    }

    pub fn selection(&self) -> &[String] {
        &self.selection
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn feedback(&self) -> Option<&GradingOutcome> {
        self.feedback.as_ref()
    }

    pub fn submitted(&self) -> usize {
        self.submitted
    }

    /// Toggles an option of the current question.
    ///
    /// Single-choice, vocabulary and pin questions keep exactly one key while a
    /// multiple-choice question accumulates them, matching the answer shape the
    /// core grader expects.
    pub fn toggle_option(&mut self, key: &str) {
        let Some(question) = self.current() else {
            return;
        };
        match question.question_type {
            QuestionType::MultipleChoice => {
                if let Some(position) = self.selection.iter().position(|value| value == key) {
                    self.selection.remove(position);
                } else {
                    self.selection.push(key.to_owned());
                    self.selection.sort();
                }
            }
            QuestionType::SingleChoice
            | QuestionType::Vocab
            | QuestionType::ImageQa
            | QuestionType::PinIdentify => {
                self.selection = vec![key.to_owned()];
            }
            _ => {}
        }
    }

    pub fn select_many(&mut self, keys: &[String]) {
        for key in keys {
            self.toggle_option(key);
        }
    }

    pub fn push_char(&mut self, character: char) {
        let Some(question) = self.current() else {
            return;
        };
        if accepts_text(question.question_type) && !character.is_control() {
            self.typed.push(character);
        }
    }

    pub fn backspace(&mut self) {
        self.typed.pop();
    }

    /// Replaces the typed answer, used by tests and scripted runs.
    pub fn type_text(&mut self, text: &str) {
        if self.accepts_text() {
            self.typed = text.to_owned();
        }
    }

    pub fn accepts_text(&self) -> bool {
        self.current()
            .is_some_and(|question| accepts_text(question.question_type))
    }

    /// Sends the composed answer to the core session.
    ///
    /// A voided placeholder question can be left blank: pressing submit then
    /// records a timed-out submission, which the core grades as voided and
    /// never feeds into scheduling.
    pub fn submit(&mut self, now: Timestamp) -> Result<Option<GradingOutcome>> {
        let Some(question) = self.current() else {
            return Ok(None);
        };
        let submission = if accepts_text(question.question_type) {
            if self.typed.trim().is_empty() {
                if question.grading_mode == reinforce_core::primitives::GradingMode::Voided {
                    AnswerSubmission::TimedOut
                } else {
                    return Ok(None);
                }
            } else {
                AnswerSubmission::Text {
                    value: self.typed.trim().to_owned(),
                }
            }
        } else {
            if self.selection.is_empty() {
                if question.grading_mode == reinforce_core::primitives::GradingMode::Voided {
                    AnswerSubmission::TimedOut
                } else {
                    return Ok(None);
                }
            } else {
                AnswerSubmission::Keys {
                    values: self.selection.clone(),
                }
            }
        };
        let answer = QuizAnswer {
            submission,
            confidence: Confidence::Certain,
            error_tags: Vec::new(),
        };
        let outcome = self.session.submit_answer(answer, now)?;
        self.submitted += 1;
        self.selection.clear();
        self.typed.clear();
        self.feedback = outcome.clone();
        self.question_started_at = now;
        Ok(outcome)
    }

    /// Records a click on a pin-locate question.
    pub fn submit_pin(&mut self, point: NormalizedPoint, now: Timestamp) -> Result<()> {
        let Some(question) = self.current() else {
            return Ok(());
        };
        if question.question_type != QuestionType::PinLocate {
            return Ok(());
        }
        let answer = QuizAnswer {
            submission: AnswerSubmission::PinClicks {
                clicks: vec![PinClick {
                    no: u32::try_from(self.submitted + 1).unwrap_or(1),
                    point,
                }],
            },
            confidence: Confidence::Certain,
            error_tags: Vec::new(),
        };
        let outcome = self.session.submit_answer(answer, now)?;
        self.submitted += 1;
        self.feedback = outcome;
        self.question_started_at = now;
        Ok(())
    }

    /// Drives the practical countdown; a timeout is graded by the core session.
    pub fn tick(&mut self, now: Timestamp) -> Result<TickOutcome> {
        let outcome = self.session.tick(now)?;
        if let TickOutcome::QuestionTimedOut { graded, .. } = &outcome {
            self.submitted += 1;
            self.feedback = Some(graded.outcome.clone());
            self.selection.clear();
            self.typed.clear();
            self.question_started_at = now;
        }
        Ok(outcome)
    }

    pub fn score(&self) -> Result<Score> {
        Ok(self.session.score()?)
    }

    /// Turns the finished session into attempt rows and derived scheduling.
    pub fn commit(&mut self, store: &mut Store, user_id: &UserId, now: Timestamp) -> Result<u32> {
        if !self.is_finished() {
            self.session.finish(now)?;
        }
        store.commit_session(&mut self.session, user_id, now)
    }

    /// Seconds left on a practical question, if the session is timed.
    pub fn seconds_left(&self, now: Timestamp) -> Option<u64> {
        let limit = self.question_limit_secs?;
        if self.is_finished() {
            return None;
        }
        let elapsed_ms = (now.days_since(&self.question_started_at) * 86_400_000.0)
            .round()
            .max(0.0) as u64;
        let limit_ms = u64::from(limit) * 1_000;
        Some(limit_ms.saturating_sub(elapsed_ms) / 1_000)
    }
}

fn accepts_text(question_type: QuestionType) -> bool {
    matches!(
        question_type,
        QuestionType::ShortAnswer | QuestionType::Vocab
    )
}

/// Everything the window needs after a session is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub mode: QuizMode,
    pub correct: usize,
    pub scoreable: usize,
    pub voided: usize,
    pub ratio: f64,
    pub written: u32,
}

impl Summary {
    pub fn new(mode: QuizMode, score: &Score, written: u32) -> Self {
        Self {
            mode,
            correct: score.correct,
            scoreable: score.scoreable,
            voided: score.voided,
            ratio: score.ratio,
            written,
        }
    }
}

impl std::fmt::Debug for Quiz {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Quiz")
            .field("mode", &self.mode)
            .field("index", &self.index())
            .field("total", &self.total())
            .field("submitted", &self.submitted)
            .finish()
    }
}

/// Default options for desktop sessions: untimed papers, practical questions
/// keep the spec's 30 second default countdown.
pub fn desktop_options() -> BuildOptions {
    BuildOptions::default()
}
