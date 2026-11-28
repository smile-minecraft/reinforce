//! Native GPUI desktop client for Reinforce.
//!
//! The binary opens a local `reinforce.db` and its `blobs` directory and renders
//! it without any HTTP hop. The crate is split so the behaviour is testable
//! without a GPU:
//!
//! - [`store`] owns SQLite access, offline imports and attempt commits.
//! - [`model`] holds the library, quiz and countdown view state.
//! - [`markdown`] turns card text into display blocks.
//! - [`render`] pre-renders math and diagrams to cached SVG files.
//! - [`app`] is the GPUI window that draws those pieces.

pub mod app;
pub mod markdown;
pub mod model;
pub mod render;
pub mod store;

pub use app::{DesktopApp, LaunchOptions, StartScreen};
pub use model::{
    ConfidenceLevel5, Library, NoteCheckState, PracticalCountdown, QuestionUnit, Quiz, QuizMode,
    QuizTargets, SubjectGroupNode, Summary, TabManager, WorkspaceTab, WorkspaceTabKind,
    build_question_units, build_sidebar_buckets, build_subject_group_tree, desktop_options,
    flatten_subject_group_tree, is_descendant_or_self,
};
pub use render::{Figure, Renderer};
pub use store::{ImportSummary, Store};

/// Errors surfaced to the window as a status banner.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("core error: {0}")]
    Core(#[from] reinforce_core::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid {0}: {1}")]
    Invalid(&'static str, String),
    #[error("render error: {0}")]
    Render(String),
}

pub type Result<T> = std::result::Result<T, Error>;
