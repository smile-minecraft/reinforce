//! Pure domain layer for Reinforce.
//!
//! This crate stays free of HTTP, async runtimes and UI dependencies so the
//! server, the native desktop client and the mobile bindings can all reuse it.

pub mod content;
pub mod db;
pub mod fsrs;
pub mod hash;
pub mod primitives;
pub mod proposal;
pub mod quiz;
pub mod study;
pub mod timestamp;
pub mod validation;
pub mod weakness;

pub use content::{
    Anchor, ContentBlock, Edge, EdgeEndpoint, ExamSet, GraphNode, KnowledgeCard, Note, Proposal,
    ProposalTarget, QuestionCard, QuestionGroup, Resource,
};
pub use hash::ContentHash;
pub use primitives::{
    AnchorId, CardId, CardStatus, Confidence, EdgeId, Error, ExamSetId, KnowledgeCardId, NoteId,
    ProposalId, QuestionCardId, QuestionGroupId, ResourceId, Result, UserId, Verification,
};
pub use proposal::{
    ApprovalOptions, ApprovalOutcome, BatchOperation, BatchPayload, OperationKind, SubmitOutcome,
    approve_batch, batch_from_proposal, reject_batch, rejected_batches, submit_batch,
};
pub use timestamp::Timestamp;
pub use validation::{
    Actor, Snapshot, Staged, StoreView, ValidationError, ValidationReport, ValidationRule,
    validate_snapshot,
};

pub use db::Database;
pub use db::auth;
pub use db::catalog as catalog_store;
pub use db::content as content_store;
pub use db::quiz as quiz_store;
pub use db::search;
pub use db::study as study_store;
pub use db::sync as sync_store;
pub use db::sync::{MergeOutcome, SyncState};
pub use quiz::{QuizSession, SessionState};
pub use study::{AttemptLog, CardScheduling, User, UserCardMeta};
pub use weakness::WeaknessReport;

/// The Reinforce release version, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_follows_workspace() {
        assert_eq!(VERSION, "0.1.0");
    }
}
