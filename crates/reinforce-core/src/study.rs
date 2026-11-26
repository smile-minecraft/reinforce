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
