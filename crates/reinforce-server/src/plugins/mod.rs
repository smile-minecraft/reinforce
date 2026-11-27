//! Out-of-process plugin support: owner-written manifests, the stdio runner and
//! the durable work queue.
//!
//! Read `plugins/README.md` for the protocol and the trust boundary. The short
//! version: only the owner's configuration file can declare what executes, the
//! HTTP API can only queue work for those declared plugins, and every result
//! becomes a pending proposal.

pub mod config;
pub mod runner;
pub mod runtime;

pub use config::{PluginConfig, PluginDefinition, PluginSummary};
pub use runtime::{PluginRuntime, RecoveryReport, check_submission};
