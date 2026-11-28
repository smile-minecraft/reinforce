use reinforce_core::content::{
    OutlineNode as CoreOutlineNode, QuestionOption as CoreQuestionOption,
};
use reinforce_core::primitives::{
    AnswerSourceKind as CoreAnswerSourceKind, AttemptDirection as CoreAttemptDirection, AttemptId,
    CardStatus as CoreCardStatus, Confidence as CoreConfidence, GradingMode as CoreGradingMode,
    QuestionType as CoreQuestionType, SchedulingState as CoreSchedulingState,
    SessionMode as CoreSessionMode, SourceKind as CoreSourceKind, Verification as CoreVerification,
};
use reinforce_core::quiz::build::BuildOptions as CoreBuildOptions;
use reinforce_core::quiz::grade::AnswerSubmission as CoreAnswerSubmission;
use reinforce_core::quiz::grade::GradingOutcome as CoreGradingOutcome;
use reinforce_core::quiz::grade::{
    NormalizedPoint as CoreNormalizedPoint, PinClick as CorePinClick,
};
use reinforce_core::quiz::session::{
    AnswerRecord as CoreAnswerRecord, GradedAnswer as CoreGradedAnswer,
    QuizAnswer as CoreQuizAnswer, Score as CoreScore, SessionState as CoreSessionState,
    TickOutcome as CoreTickOutcome,
};
use reinforce_core::study::AttemptLog as CoreAttemptLog;
use reinforce_core::study::CardScheduling as CoreCardScheduling;
use reinforce_core::weakness::{
    ConfusablePair as CoreConfusablePair, ErrorTagCount as CoreErrorTagCount,
    KnowledgeWeakness as CoreKnowledgeWeakness, QuestionWeakness as CoreQuestionWeakness,
    WeaknessReport as CoreWeaknessReport,
};
use reinforce_core::{
    AnchorId, CardId, ExamSet as CoreExamSet, KnowledgeCard as CoreKnowledgeCard, KnowledgeCardId,
    Note as CoreNote, QuestionCard as CoreQuestionCard, QuestionGroup as CoreQuestionGroup,
    SubjectGroup as CoreSubjectGroup, Timestamp, UserId,
};

use crate::error::{FfiError, finite, parse_id, parse_timestamp, u32_field, usize_field};

fn json(value: &serde_json::Value) -> String {
    value.to_string()
}

pub(crate) fn stamp(value: &Timestamp) -> String {
    value.as_str()
}

fn optional_stamp(value: &Option<Timestamp>) -> Option<String> {
    value.as_ref().map(stamp)
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum CardStatus {
    Draft,
    Staging,
    Published,
    Archived,
}

impl From<CoreCardStatus> for CardStatus {
    fn from(value: CoreCardStatus) -> Self {
        match value {
            CoreCardStatus::Draft => Self::Draft,
            CoreCardStatus::Staging => Self::Staging,
            CoreCardStatus::Published => Self::Published,
            CoreCardStatus::Archived => Self::Archived,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Verification {
    Unchecked,
    Verified,
}

impl From<CoreVerification> for Verification {
    fn from(value: CoreVerification) -> Self {
        match value {
            CoreVerification::Unchecked => Self::Unchecked,
            CoreVerification::Verified => Self::Verified,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum SourceKind {
    PastExam,
    QuestionBank,
    Original,
    Custom,
}

impl From<CoreSourceKind> for SourceKind {
    fn from(value: CoreSourceKind) -> Self {
        match value {
            CoreSourceKind::PastExam => Self::PastExam,
            CoreSourceKind::QuestionBank => Self::QuestionBank,
            CoreSourceKind::Original => Self::Original,
            CoreSourceKind::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum QuestionType {
    SingleChoice,
    MultipleChoice,
    ShortAnswer,
    Vocab,
    ImageQa,
    ImageOcclusion,
    PinIdentify,
    PinLocate,
}

impl From<CoreQuestionType> for QuestionType {
    fn from(value: CoreQuestionType) -> Self {
        match value {
            CoreQuestionType::SingleChoice => Self::SingleChoice,
            CoreQuestionType::MultipleChoice => Self::MultipleChoice,
            CoreQuestionType::ShortAnswer => Self::ShortAnswer,
            CoreQuestionType::Vocab => Self::Vocab,
            CoreQuestionType::ImageQa => Self::ImageQa,
            CoreQuestionType::ImageOcclusion => Self::ImageOcclusion,
            CoreQuestionType::PinIdentify => Self::PinIdentify,
            CoreQuestionType::PinLocate => Self::PinLocate,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum GradingMode {
    Exact,
    AnyAccepted,
    Voided,
}

impl From<CoreGradingMode> for GradingMode {
    fn from(value: CoreGradingMode) -> Self {
        match value {
            CoreGradingMode::Exact => Self::Exact,
            CoreGradingMode::AnyAccepted => Self::AnyAccepted,
            CoreGradingMode::Voided => Self::Voided,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AnswerSourceKind {
    Official,
    PeerNote,
    DuplicateQuestion,
    Inferred,
    OriginalAuthored,
    Unanswered,
}

impl From<CoreAnswerSourceKind> for AnswerSourceKind {
    fn from(value: CoreAnswerSourceKind) -> Self {
        match value {
            CoreAnswerSourceKind::Official => Self::Official,
            CoreAnswerSourceKind::PeerNote => Self::PeerNote,
            CoreAnswerSourceKind::DuplicateQuestion => Self::DuplicateQuestion,
            CoreAnswerSourceKind::Inferred => Self::Inferred,
            CoreAnswerSourceKind::OriginalAuthored => Self::OriginalAuthored,
            CoreAnswerSourceKind::Unanswered => Self::Unanswered,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SessionMode {
    DailyDue,
    FullExam,
    TopicDrill,
    PracticalExam,
    WeaknessDrill,
}

impl From<CoreSessionMode> for SessionMode {
    fn from(value: CoreSessionMode) -> Self {
        match value {
            CoreSessionMode::DailyDue => Self::DailyDue,
            CoreSessionMode::FullExam => Self::FullExam,
            CoreSessionMode::TopicDrill => Self::TopicDrill,
            CoreSessionMode::PracticalExam => Self::PracticalExam,
            CoreSessionMode::WeaknessDrill => Self::WeaknessDrill,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AttemptDirection {
    Default,
    Forward,
    Reverse,
}

impl From<CoreAttemptDirection> for AttemptDirection {
    fn from(value: CoreAttemptDirection) -> Self {
        match value {
            CoreAttemptDirection::Default => Self::Default,
            CoreAttemptDirection::Forward => Self::Forward,
            CoreAttemptDirection::Reverse => Self::Reverse,
        }
    }
}

impl From<AttemptDirection> for CoreAttemptDirection {
    fn from(value: AttemptDirection) -> Self {
        match value {
            AttemptDirection::Default => Self::Default,
            AttemptDirection::Forward => Self::Forward,
            AttemptDirection::Reverse => Self::Reverse,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Confidence {
    Certain,
    Split,
    Guessed,
}

impl From<CoreConfidence> for Confidence {
    fn from(value: CoreConfidence) -> Self {
        match value {
            CoreConfidence::Certain => Self::Certain,
            CoreConfidence::Split => Self::Split,
            CoreConfidence::Guessed => Self::Guessed,
        }
    }
}

impl From<Confidence> for CoreConfidence {
    fn from(value: Confidence) -> Self {
        match value {
            Confidence::Certain => Self::Certain,
            Confidence::Split => Self::Split,
            Confidence::Guessed => Self::Guessed,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum SchedulingState {
    New,
    Learning,
    Review,
    Relearning,
}

impl From<CoreSchedulingState> for SchedulingState {
    fn from(value: CoreSchedulingState) -> Self {
        match value {
            CoreSchedulingState::New => Self::New,
            CoreSchedulingState::Learning => Self::Learning,
            CoreSchedulingState::Review => Self::Review,
            CoreSchedulingState::Relearning => Self::Relearning,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum OutlineNode {
    Section {
        heading: String,
        intro_markdown: Option<String>,
        children: Vec<OutlineNode>,
    },
    CardRef {
        card_id: String,
    },
}

impl From<&CoreOutlineNode> for OutlineNode {
    fn from(value: &CoreOutlineNode) -> Self {
        match value {
            CoreOutlineNode::Section {
                heading,
                intro_markdown,
                children,
            } => Self::Section {
                heading: heading.clone(),
                intro_markdown: intro_markdown.clone(),
                children: children.iter().map(OutlineNode::from).collect(),
            },
            CoreOutlineNode::CardRef { card_id } => Self::CardRef {
                card_id: card_id.as_str(),
            },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Note {
    pub id: String,
    pub subject: String,
    pub title: String,
    pub outline: Vec<OutlineNode>,
    pub status: CardStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CoreNote> for Note {
    fn from(value: CoreNote) -> Self {
        Self {
            id: value.id.as_str(),
            subject: value.subject,
            title: value.title,
            outline: value.outline.iter().map(OutlineNode::from).collect(),
            status: value.status.into(),
            created_at: stamp(&value.created_at),
            updated_at: stamp(&value.updated_at),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct KnowledgeCard {
    pub id: String,
    pub subject: String,
    pub topics: Vec<String>,
    pub title: String,
    pub blocks_json: String,
    pub anchor_ids: Vec<String>,
    pub status: CardStatus,
    pub verification: Verification,
    pub unsure_count: u32,
    pub metadata_json: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CoreKnowledgeCard> for KnowledgeCard {
    fn from(value: CoreKnowledgeCard) -> Self {
        Self {
            id: value.id.as_str(),
            subject: value.subject,
            topics: value.topics,
            title: value.title,
            blocks_json: json(&serde_json::to_value(&value.blocks).unwrap_or_default()),
            anchor_ids: value.anchor_ids.iter().map(AnchorId::as_str).collect(),
            status: value.status.into(),
            verification: value.verification.into(),
            unsure_count: value.unsure_count,
            metadata_json: json(&value.metadata),
            created_at: stamp(&value.created_at),
            updated_at: stamp(&value.updated_at),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExamSet {
    pub id: String,
    pub subject: String,
    pub title: String,
    pub source_kind: SourceKind,
    pub exam_year: Option<i32>,
    pub source_resource_id: Option<String>,
    pub expected_questions: Option<u32>,
    pub metadata_json: String,
    pub is_complete: bool,
}

impl From<CoreExamSet> for ExamSet {
    fn from(value: CoreExamSet) -> Self {
        Self {
            id: value.id.as_str(),
            subject: value.subject,
            title: value.title,
            source_kind: value.source_kind.into(),
            exam_year: value.exam_year,
            source_resource_id: value.source_resource_id.map(|id| id.as_str()),
            expected_questions: value.expected_questions,
            metadata_json: json(&value.metadata),
            is_complete: value.is_complete,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct QuestionGroup {
    pub id: String,
    pub exam_set_id: Option<String>,
    pub title: Option<String>,
    pub shared_stem_blocks_json: String,
}

impl From<CoreQuestionGroup> for QuestionGroup {
    fn from(value: CoreQuestionGroup) -> Self {
        Self {
            id: value.id.as_str(),
            exam_set_id: value.exam_set_id.map(|id| id.as_str()),
            title: value.title,
            shared_stem_blocks_json: json(
                &serde_json::to_value(&value.shared_stem_blocks).unwrap_or_default(),
            ),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SubjectGroup {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub sort_order: i32,
    pub subjects: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CoreSubjectGroup> for SubjectGroup {
    fn from(value: CoreSubjectGroup) -> Self {
        Self {
            id: value.id.as_str(),
            parent_id: value.parent_id.map(|id| id.as_str()),
            name: value.name,
            sort_order: value.sort_order,
            subjects: value.subjects,
            created_at: stamp(&value.created_at),
            updated_at: stamp(&value.updated_at),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct QuestionOption {
    pub key: String,
    pub content: String,
    pub explanation: Option<String>,
    pub linked_card_ids: Vec<String>,
}

impl From<CoreQuestionOption> for QuestionOption {
    fn from(value: CoreQuestionOption) -> Self {
        Self {
            key: value.key,
            content: value.content,
            explanation: value.explanation,
            linked_card_ids: value
                .linked_card_ids
                .iter()
                .map(KnowledgeCardId::as_str)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct QuestionCard {
    pub id: String,
    pub exam_set_id: Option<String>,
    pub question_group_id: Option<String>,
    pub question_no: Option<u32>,
    pub subject: String,
    pub topics: Vec<String>,
    pub stem_blocks_json: String,
    pub explanation_blocks_json: String,
    pub anchor_ids: Vec<String>,
    pub question_type: QuestionType,
    pub grading_mode: GradingMode,
    pub options: Vec<QuestionOption>,
    pub answer: Vec<String>,
    pub accepted_answers: Vec<Vec<String>>,
    pub answer_source_kind: AnswerSourceKind,
    pub answer_basis: Option<String>,
    pub status: CardStatus,
    pub verification: Verification,
    pub unsure_count: u32,
    pub metadata_json: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CoreQuestionCard> for QuestionCard {
    fn from(value: CoreQuestionCard) -> Self {
        Self {
            id: value.id.as_str(),
            exam_set_id: value.exam_set_id.map(|id| id.as_str()),
            question_group_id: value.question_group_id.map(|id| id.as_str()),
            question_no: value.question_no,
            subject: value.subject,
            topics: value.topics,
            stem_blocks_json: json(&serde_json::to_value(&value.stem_blocks).unwrap_or_default()),
            explanation_blocks_json: json(
                &serde_json::to_value(&value.explanation_blocks).unwrap_or_default(),
            ),
            anchor_ids: value.anchor_ids.iter().map(AnchorId::as_str).collect(),
            question_type: value.question_type.into(),
            grading_mode: value.grading_mode.into(),
            options: value
                .options
                .into_iter()
                .map(QuestionOption::from)
                .collect(),
            answer: value.answer,
            accepted_answers: value.accepted_answers,
            answer_source_kind: value.answer_source_kind.into(),
            answer_basis: value.answer_basis,
            status: value.status.into(),
            verification: value.verification.into(),
            unsure_count: value.unsure_count,
            metadata_json: json(&value.metadata),
            created_at: stamp(&value.created_at),
            updated_at: stamp(&value.updated_at),
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum SearchHit {
    KnowledgeCard { id: String },
    QuestionCard { id: String },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportSummary {
    pub proposal_id: String,
    pub applied: u32,
    /// Assets newly copied into the blob directory by this import.
    pub blobs_written: u32,
}

// The literals mirror `reinforce_core::quiz::build::BuildOptions::default`.
// Bare `#[uniffi(default)]` would use the field type's default (0/false) in the
// generated Swift and Kotlin bindings, which silently disables scheduling and
// builds invalid sessions.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BuildOptions {
    #[uniffi(default = 50)]
    pub question_limit: u32,
    #[uniffi(default = 20)]
    pub new_card_limit: u32,
    #[uniffi(default = None)]
    pub total_time_secs: Option<u32>,
    #[uniffi(default = 30)]
    pub seconds_per_question: u32,
    #[uniffi(default = true)]
    pub affects_scheduling: bool,
    #[uniffi(default = 0.05)]
    pub pin_radius: f64,
    #[uniffi(default = None)]
    pub pin_aspect_ratio: Option<f64>,
    /// Restricts every session mode to the listed subjects; `None` keeps all.
    #[uniffi(default = None)]
    pub subjects: Option<Vec<String>>,
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
            subjects: None,
        }
    }
}

impl BuildOptions {
    pub(crate) fn into_core(self) -> Result<CoreBuildOptions, FfiError> {
        let seconds = u16::try_from(self.seconds_per_question).map_err(|_| {
            FfiError::invalid(
                "seconds per question",
                "must be between 1 and 300".to_owned(),
            )
        })?;
        if !(1..=300).contains(&seconds) {
            return Err(FfiError::invalid(
                "seconds per question",
                "must be between 1 and 300".to_owned(),
            ));
        }
        if self.total_time_secs == Some(0) {
            return Err(FfiError::invalid(
                "total time",
                "must be greater than zero".to_owned(),
            ));
        }
        Ok(CoreBuildOptions {
            question_limit: usize_field("question limit", self.question_limit)?,
            new_card_limit: usize_field("new card limit", self.new_card_limit)?,
            total_time_secs: self.total_time_secs,
            seconds_per_question: seconds,
            affects_scheduling: self.affects_scheduling,
            pin_radius: finite("pin radius", self.pin_radius)?,
            pin_aspect_ratio: self
                .pin_aspect_ratio
                .map(|value| finite("pin aspect ratio", value))
                .transpose()?,
            subjects: self.subjects,
        })
    }
}

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct NormalizedPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PinClick {
    pub no: u32,
    pub point: NormalizedPoint,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum AnswerSubmission {
    Keys { values: Vec<String> },
    Text { value: String },
    PinClicks { clicks: Vec<PinClick> },
    TimedOut,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct QuizAnswer {
    pub submission: AnswerSubmission,
    pub confidence: Confidence,
    #[uniffi(default)]
    pub error_tags: Vec<String>,
}

impl QuizAnswer {
    pub(crate) fn into_core(self) -> Result<CoreQuizAnswer, FfiError> {
        let submission = match self.submission {
            AnswerSubmission::Keys { values } => {
                if values.iter().all(|value| value.trim().is_empty()) {
                    return Err(FfiError::invalid(
                        "submission",
                        "keys must contain at least one value".to_owned(),
                    ));
                }
                CoreAnswerSubmission::Keys { values }
            }
            AnswerSubmission::Text { value } => {
                if value.trim().is_empty() {
                    return Err(FfiError::invalid(
                        "submission",
                        "text must not be empty".to_owned(),
                    ));
                }
                CoreAnswerSubmission::Text { value }
            }
            AnswerSubmission::PinClicks { clicks } => {
                let mut core_clicks = Vec::with_capacity(clicks.len());
                for click in clicks {
                    core_clicks.push(CorePinClick {
                        no: click.no,
                        point: CoreNormalizedPoint {
                            x: finite("pin x", click.point.x)?,
                            y: finite("pin y", click.point.y)?,
                        },
                    });
                }
                CoreAnswerSubmission::PinClicks {
                    clicks: core_clicks,
                }
            }
            AnswerSubmission::TimedOut => CoreAnswerSubmission::TimedOut,
        };
        Ok(CoreQuizAnswer {
            submission,
            confidence: self.confidence.into(),
            error_tags: self.error_tags,
        })
    }
}

impl From<CoreAnswerSubmission> for AnswerSubmission {
    fn from(value: CoreAnswerSubmission) -> Self {
        match value {
            CoreAnswerSubmission::Keys { values } => Self::Keys { values },
            CoreAnswerSubmission::Text { value } => Self::Text { value },
            CoreAnswerSubmission::PinClicks { clicks } => Self::PinClicks {
                clicks: clicks
                    .into_iter()
                    .map(|click| PinClick {
                        no: click.no,
                        point: NormalizedPoint {
                            x: click.point.x,
                            y: click.point.y,
                        },
                    })
                    .collect(),
            },
            CoreAnswerSubmission::TimedOut => Self::TimedOut,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AnswerRecord {
    pub index: u32,
    pub question_id: String,
    pub submission: AnswerSubmission,
    pub confidence: Confidence,
    pub error_tags: Vec<String>,
    pub duration_ms: i64,
    pub created_at: String,
    pub timed_out: bool,
}

impl From<CoreAnswerRecord> for AnswerRecord {
    fn from(value: CoreAnswerRecord) -> Self {
        Self {
            index: u32_field("index", value.index).unwrap_or(u32::MAX),
            question_id: value.question_id.as_str(),
            submission: value.submission.into(),
            confidence: value.confidence.into(),
            error_tags: value.error_tags,
            duration_ms: value.duration_ms,
            created_at: stamp(&value.created_at),
            timed_out: value.timed_out,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct GradingOutcome {
    pub is_correct: bool,
    pub scoreable: bool,
    pub voided: bool,
    pub affects_scheduling: bool,
    pub note: String,
}

impl From<CoreGradingOutcome> for GradingOutcome {
    fn from(value: CoreGradingOutcome) -> Self {
        Self {
            is_correct: value.is_correct,
            scoreable: value.scoreable,
            voided: value.voided,
            affects_scheduling: value.affects_scheduling,
            note: value.note,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct GradedAnswer {
    pub question_id: String,
    pub error_tags: Vec<String>,
    pub duration_ms: i64,
    pub is_correct: bool,
    pub scoreable: bool,
    pub voided: bool,
    pub note: String,
}

impl From<CoreGradedAnswer> for GradedAnswer {
    fn from(value: CoreGradedAnswer) -> Self {
        Self {
            question_id: value.record.question_id.as_str(),
            error_tags: error_tags(&value.record),
            duration_ms: value.record.duration_ms,
            is_correct: value.outcome.is_correct,
            scoreable: value.outcome.scoreable,
            voided: value.outcome.voided,
            note: value.outcome.note,
        }
    }
}

fn error_tags(record: &CoreAnswerRecord) -> Vec<String> {
    let mut tags = Vec::new();
    if record.timed_out {
        tags.push("timeout".to_owned());
    }
    for tag in &record.error_tags {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    tags
}

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Score {
    pub correct: u32,
    pub scoreable: u32,
    pub voided: u32,
    pub ratio: f64,
}

impl From<CoreScore> for Score {
    fn from(value: CoreScore) -> Self {
        Self {
            correct: u32_field("correct", value.correct).unwrap_or(u32::MAX),
            scoreable: u32_field("scoreable", value.scoreable).unwrap_or(u32::MAX),
            voided: u32_field("voided", value.voided).unwrap_or(u32::MAX),
            ratio: value.ratio,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum SessionState {
    NotStarted,
    InProgress { index: u32 },
    Paused { index: u32 },
    Expired { index: u32 },
    Submitted,
}

impl From<CoreSessionState> for SessionState {
    fn from(value: CoreSessionState) -> Self {
        match value {
            CoreSessionState::NotStarted => Self::NotStarted,
            CoreSessionState::InProgress { index } => Self::InProgress {
                index: u32_field("index", index).unwrap_or(u32::MAX),
            },
            CoreSessionState::Paused { index } => Self::Paused {
                index: u32_field("index", index).unwrap_or(u32::MAX),
            },
            CoreSessionState::Expired { index } => Self::Expired {
                index: u32_field("index", index).unwrap_or(u32::MAX),
            },
            CoreSessionState::Submitted => Self::Submitted,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum TickOutcome {
    None,
    QuestionTimedOut { graded: GradedAnswer },
    SessionExpired { index: u32 },
}

impl From<CoreTickOutcome> for TickOutcome {
    fn from(value: CoreTickOutcome) -> Self {
        match value {
            CoreTickOutcome::None => Self::None,
            CoreTickOutcome::QuestionTimedOut { graded, .. } => Self::QuestionTimedOut {
                graded: graded.into(),
            },
            CoreTickOutcome::SessionExpired { index } => Self::SessionExpired {
                index: u32_field("index", index).unwrap_or(u32::MAX),
            },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AttemptLogRecord {
    pub id: String,
    pub user_id: String,
    pub card_id: String,
    pub direction: AttemptDirection,
    pub session_mode: SessionMode,
    pub submitted_payload_json: String,
    pub is_correct: bool,
    pub duration_ms: i64,
    pub confidence: Confidence,
    pub error_tags: Vec<String>,
    pub created_at: String,
    pub sync_cursor: i64,
    pub affects_scheduling: bool,
}

impl TryFrom<AttemptLogRecord> for CoreAttemptLog {
    type Error = FfiError;

    fn try_from(value: AttemptLogRecord) -> Result<Self, Self::Error> {
        let payload: serde_json::Value = serde_json::from_str(&value.submitted_payload_json)?;
        Ok(Self {
            id: parse_id("attempt id", &value.id)?,
            user_id: parse_id("user id", &value.user_id)?,
            card_id: parse_id("card id", &value.card_id)?,
            direction: value.direction.into(),
            session_mode: match value.session_mode {
                SessionMode::DailyDue => CoreSessionMode::DailyDue,
                SessionMode::FullExam => CoreSessionMode::FullExam,
                SessionMode::TopicDrill => CoreSessionMode::TopicDrill,
                SessionMode::PracticalExam => CoreSessionMode::PracticalExam,
                SessionMode::WeaknessDrill => CoreSessionMode::WeaknessDrill,
            },
            submitted_payload: payload,
            is_correct: value.is_correct,
            duration_ms: value.duration_ms,
            confidence: value.confidence.into(),
            error_tags: value.error_tags,
            created_at: parse_timestamp("created at", &value.created_at)?,
            sync_cursor: value.sync_cursor,
            affects_scheduling: value.affects_scheduling,
        })
    }
}

impl From<CoreAttemptLog> for AttemptLogRecord {
    fn from(value: CoreAttemptLog) -> Self {
        Self {
            id: value.id.as_str(),
            user_id: value.user_id.as_str(),
            card_id: value.card_id.as_str(),
            direction: value.direction.into(),
            session_mode: value.session_mode.into(),
            submitted_payload_json: json(&value.submitted_payload),
            is_correct: value.is_correct,
            duration_ms: value.duration_ms,
            confidence: value.confidence.into(),
            error_tags: value.error_tags,
            created_at: stamp(&value.created_at),
            sync_cursor: value.sync_cursor,
            affects_scheduling: value.affects_scheduling,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CardSchedulingRecord {
    pub user_id: String,
    pub card_id: String,
    pub direction: AttemptDirection,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: SchedulingState,
    pub last_review_at: Option<String>,
    pub due_at: Option<String>,
    pub desired_retention: f64,
    pub updated_at: String,
}

impl From<CoreCardScheduling> for CardSchedulingRecord {
    fn from(value: CoreCardScheduling) -> Self {
        Self {
            user_id: value.user_id.as_str(),
            card_id: value.card_id.as_str(),
            direction: value.direction.into(),
            stability: value.stability,
            difficulty: value.difficulty,
            reps: value.reps,
            lapses: value.lapses,
            state: value.state.into(),
            last_review_at: optional_stamp(&value.last_review_at),
            due_at: optional_stamp(&value.due_at),
            desired_retention: value.desired_retention,
            updated_at: stamp(&value.updated_at),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DueQuestion {
    pub id: String,
    pub is_new: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ErrorTagCount {
    pub tag: String,
    pub count: u32,
}

impl From<CoreErrorTagCount> for ErrorTagCount {
    fn from(value: CoreErrorTagCount) -> Self {
        Self {
            tag: value.tag,
            count: value.count,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct KnowledgeWeakness {
    pub card_id: String,
    pub attempts: u32,
    pub error_rate: f64,
    pub hesitant_rate: f64,
    pub average_duration_ms: f64,
    pub score: f64,
}

impl From<CoreKnowledgeWeakness> for KnowledgeWeakness {
    fn from(value: CoreKnowledgeWeakness) -> Self {
        Self {
            card_id: value.card_id.as_str(),
            attempts: value.attempts,
            error_rate: value.error_rate,
            hesitant_rate: value.hesitant_rate,
            average_duration_ms: value.average_duration_ms,
            score: value.score,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct QuestionWeakness {
    pub question_id: String,
    pub attempts: u32,
    pub error_rate: f64,
    pub hesitant_rate: f64,
    pub average_duration_ms: f64,
    pub score: f64,
    pub linked_card_ids: Vec<String>,
    pub common_error_tags: Vec<ErrorTagCount>,
}

impl From<CoreQuestionWeakness> for QuestionWeakness {
    fn from(value: CoreQuestionWeakness) -> Self {
        Self {
            question_id: value.question_id.as_str(),
            attempts: value.attempts,
            error_rate: value.error_rate,
            hesitant_rate: value.hesitant_rate,
            average_duration_ms: value.average_duration_ms,
            score: value.score,
            linked_card_ids: value
                .linked_card_ids
                .iter()
                .map(KnowledgeCardId::as_str)
                .collect(),
            common_error_tags: value
                .common_error_tags
                .into_iter()
                .map(ErrorTagCount::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ConfusablePair {
    pub question_id: String,
    pub wrong_option: String,
    pub correct_option: String,
    pub count: u32,
}

impl From<CoreConfusablePair> for ConfusablePair {
    fn from(value: CoreConfusablePair) -> Self {
        Self {
            question_id: value.question_id.as_str(),
            wrong_option: value.wrong_option,
            correct_option: value.correct_option,
            count: value.count,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct WeaknessReport {
    pub knowledge_cards: Vec<KnowledgeWeakness>,
    pub questions: Vec<QuestionWeakness>,
    pub confusable_pairs: Vec<ConfusablePair>,
}

impl From<CoreWeaknessReport> for WeaknessReport {
    fn from(value: CoreWeaknessReport) -> Self {
        Self {
            knowledge_cards: value
                .knowledge_cards
                .into_iter()
                .map(KnowledgeWeakness::from)
                .collect(),
            questions: value
                .questions
                .into_iter()
                .map(QuestionWeakness::from)
                .collect(),
            confusable_pairs: value
                .confusable_pairs
                .into_iter()
                .map(ConfusablePair::from)
                .collect(),
        }
    }
}

pub(crate) fn parse_card_id(value: &str) -> Result<CardId, FfiError> {
    parse_id("card id", value)
}

pub(crate) fn parse_user_id(value: &str) -> Result<UserId, FfiError> {
    parse_id("user id", value)
}

pub(crate) fn generated_attempt_id() -> String {
    AttemptId::generate().as_str()
}

#[cfg(test)]
mod tests {
    use super::BuildOptions;
    use reinforce_core::quiz::build::BuildOptions as CoreBuildOptions;

    /// The `#[uniffi(default = ...)]` literals are what Swift and Kotlin see, so
    /// they must track the core defaults instead of drifting apart.
    #[test]
    fn build_options_defaults_match_core() {
        let ffi = BuildOptions::default();
        let core = CoreBuildOptions::default();
        assert_eq!(ffi.question_limit, core.question_limit as u32);
        assert_eq!(ffi.new_card_limit, core.new_card_limit as u32);
        assert_eq!(ffi.total_time_secs, core.total_time_secs);
        assert_eq!(ffi.seconds_per_question, core.seconds_per_question as u32);
        assert_eq!(ffi.affects_scheduling, core.affects_scheduling);
        assert_eq!(ffi.pin_radius, core.pin_radius);
        assert_eq!(ffi.pin_aspect_ratio, core.pin_aspect_ratio);
        assert_eq!(ffi.subjects, core.subjects);

        let converted = ffi.into_core().expect("defaults must be valid");
        assert_eq!(converted.question_limit, core.question_limit);
        assert_eq!(converted.new_card_limit, core.new_card_limit);
        assert_eq!(converted.seconds_per_question, core.seconds_per_question);
        assert_eq!(converted.affects_scheduling, core.affects_scheduling);
        assert_eq!(converted.pin_radius, core.pin_radius);
        assert_eq!(converted.subjects, core.subjects);
    }
}
