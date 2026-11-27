use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::primitives::{
    AttemptDirection, AttemptId, CardId, Confidence, SchedulingState, SessionMode, UserId,
};
use crate::timestamp::Timestamp;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub display_name: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptLog {
    pub id: AttemptId,
    pub user_id: UserId,
    pub card_id: CardId,
    pub direction: AttemptDirection,
    pub session_mode: SessionMode,
    pub submitted_payload: Value,
    pub is_correct: bool,
    pub duration_ms: i64,
    pub confidence: Confidence,
    pub error_tags: Vec<String>,
    pub created_at: Timestamp,
    pub sync_cursor: i64,
    pub affects_scheduling: bool,
}

/// The retention every local and server-side replay uses, so both ends derive
/// the same FSRS state from the same attempt log.
pub const DEFAULT_DESIRED_RETENTION: f64 = 0.9;

/// Voided questions stay in the append-only log for auditability but must never
/// influence grading totals or FSRS. The marker is carried in the payload so no
/// second column is needed.
pub fn is_voided_attempt(attempt: &AttemptLog) -> bool {
    attempt
        .submitted_payload
        .get("voided")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || attempt
            .submitted_payload
            .get("grading_mode")
            .and_then(Value::as_str)
            == Some("voided")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardScheduling {
    pub user_id: UserId,
    pub card_id: CardId,
    pub direction: AttemptDirection,
    pub stability: f64,
    pub difficulty: f64,
    pub reps: u32,
    pub lapses: u32,
    pub state: SchedulingState,
    pub last_review_at: Option<Timestamp>,
    pub due_at: Option<Timestamp>,
    pub desired_retention: f64,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserCardMeta {
    pub user_id: UserId,
    pub card_id: CardId,
    pub starred: bool,
    pub personal_tags: Vec<String>,
    pub personal_note: Option<String>,
    pub updated_at: Timestamp,
}
