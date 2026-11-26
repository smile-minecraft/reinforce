//! Pure domain layer for Reinforce.
//!
//! This crate must stay free of HTTP and UI dependencies so that the server,
//! the native desktop client and the mobile bindings can all reuse it.

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
