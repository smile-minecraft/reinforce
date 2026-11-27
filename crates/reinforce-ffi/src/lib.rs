//! Synchronous, proc-macro UniFFI bindings for `reinforce-core`.
//!
//! All exported types live in this crate. Core types are converted at the
//! boundary so `reinforce-core` never depends on UniFFI.

uniffi::setup_scaffolding!();

mod bundle;

pub mod catalog;
pub mod database;
pub mod error;
pub mod resources;
pub mod session;
pub mod sync;
pub mod types;

pub use database::{LocalDatabase, new_attempt_id};
pub use error::FfiError;
pub use session::QuizSessionHandle;
