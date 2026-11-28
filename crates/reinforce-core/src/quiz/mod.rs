//! Synchronous quiz assembly, session state and grading primitives.

pub mod build;
pub mod grade;
pub mod session;

pub use build::{
    BuildOptions, build_daily_due, build_full_exam, build_practical_exam, build_subject_exam,
    build_topic_drill, build_weakness_drill,
};
pub use grade::{
    AnswerSubmission, GradingConfig, GradingOutcome, NormalizedPoint, PinClick, grade_question,
    normalize_term,
};
pub use session::{
    AnswerRecord, GradedAnswer, QuizAnswer, QuizSession, Score, SessionState, TickOutcome,
    is_practical_question,
};
