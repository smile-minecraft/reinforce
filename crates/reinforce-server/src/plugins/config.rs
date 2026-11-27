//! Owner-written plugin manifests.
//!
//! This module is the whole trust boundary for executable plugins: a plugin
//! exists only because the owner put a manifest in the file passed to
//! `reinforce serve --plugins`. Nothing in the database, the HTTP API or the
//! CLI can add, change or override a command, an argument, an environment
//! variable or a resource limit. Requests can only name an already configured
//! plugin and one of its capabilities.
//!
//! This release implements the `stdio` transport only. A `remote` transport is
//! recognised by the parser so the schema is stable, but loading such a file
//! fails with an explicit error instead of silently ignoring the entry.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};

/// Default wall-clock limit for one plugin run.
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;
/// Default cap for everything a plugin writes to stdout in one run.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Default number of runs a job may use before it is marked failed.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 1;

pub const MIN_TIMEOUT_SECS: u64 = 1;
pub const MAX_TIMEOUT_SECS: u64 = 3_600;
pub const MIN_MAX_OUTPUT_BYTES: usize = 1_024;
pub const MAX_MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ATTEMPTS: u32 = 5;
pub const MAX_ARGS: usize = 64;
pub const MAX_ARG_BYTES: usize = 4_096;
pub const MAX_PLUGINS: usize = 32;
pub const MAX_CAPABILITIES: usize = 16;

/// Environment variables a plugin process inherits. Everything else is
/// cleared, so a plugin cannot read server secrets through the environment.
pub const ENV_ALLOWLIST: [&str; 5] = ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"];

/// One plugin as written by the owner.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub name: String,
    pub capabilities: Vec<String>,
    pub transport: TransportManifest,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_output_bytes: Option<usize>,
    #[serde(default)]
    pub max_attempts: Option<u32>,
}

/// Transport declaration. `remote` parses but is rejected at load time.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportManifest {
    #[serde(rename = "type")]
    pub kind: TransportKind,
    #[serde(default)]
    pub command: Option<PathBuf>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Stdio,
    Remote,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginFile {
    #[serde(default)]
    pub plugins: Vec<PluginManifest>,
}

/// A validated, executable plugin definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginDefinition {
    pub name: String,
    pub capabilities: Vec<String>,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub max_attempts: u32,
}

impl PluginDefinition {
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate == capability)
    }
}

/// Owner-facing description of a configured plugin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PluginSummary {
    pub name: String,
    pub capabilities: Vec<String>,
    pub transport: &'static str,
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
    pub max_output_bytes: usize,
    pub max_attempts: u32,
}

impl From<&PluginDefinition> for PluginSummary {
    fn from(definition: &PluginDefinition) -> Self {
        Self {
            name: definition.name.clone(),
            capabilities: definition.capabilities.clone(),
            transport: "stdio",
            command: definition.command.display().to_string(),
            args: definition.args.clone(),
            timeout_secs: definition.timeout.as_secs(),
            max_output_bytes: definition.max_output_bytes,
            max_attempts: definition.max_attempts,
        }
    }
}

/// The validated plugin list of one server process.
#[derive(Debug, Clone, Default)]
pub struct PluginConfig {
    definitions: Vec<PluginDefinition>,
    /// True when the owner actually supplied a manifest file. Starting the
    /// server without one must not fail jobs that a previous run queued.
    from_file: bool,
}

fn config_error(message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_plugin_config",
        message.into(),
    )
}

fn valid_identifier(value: &str, max: usize) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    value.len() <= max
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
}

impl PluginConfig {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Reads and validates the manifest file. `None` means "no plugins".
    pub fn load(path: Option<&Path>) -> ApiResult<Self> {
        let Some(path) = path else {
            return Ok(Self::empty());
        };
        let text = std::fs::read_to_string(path).map_err(|error| {
            config_error(format!(
                "cannot read plugin configuration {}: {error}",
                path.display()
            ))
        })?;
        Self::parse(&text)
    }

    /// Parses manifest JSON. Every unknown field is an error so a typo cannot
    /// silently change what gets executed.
    pub fn parse(text: &str) -> ApiResult<Self> {
        let file: PluginFile = serde_json::from_str(text)
            .map_err(|error| config_error(format!("invalid plugin configuration: {error}")))?;
        if file.plugins.len() > MAX_PLUGINS {
            return Err(config_error(format!(
                "at most {MAX_PLUGINS} plugins can be configured"
            )));
        }
        let mut definitions = Vec::with_capacity(file.plugins.len());
        for manifest in file.plugins {
            definitions.push(resolve(manifest)?);
        }
        let mut names: Vec<&str> = definitions.iter().map(|item| item.name.as_str()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        if names.len() != total {
            return Err(config_error("plugin names must be unique"));
        }
        Ok(Self {
            definitions,
            from_file: true,
        })
    }

    pub fn get(&self, name: &str) -> Option<&PluginDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &PluginDefinition> {
        self.definitions.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Whether a manifest file was provided.
    pub fn from_file(&self) -> bool {
        self.from_file
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn summaries(&self) -> Vec<PluginSummary> {
        self.definitions.iter().map(PluginSummary::from).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.definitions
            .iter()
            .map(|definition| definition.name.clone())
            .collect()
    }
}

fn resolve(manifest: PluginManifest) -> ApiResult<PluginDefinition> {
    let name = manifest.name.trim().to_owned();
    if !valid_identifier(&name, 64) {
        return Err(config_error(format!(
            "invalid plugin name {name:?}: use lowercase letters, digits, '_' or '-'"
        )));
    }
    if manifest.capabilities.is_empty() {
        return Err(config_error(format!(
            "plugin {name:?} must declare at least one capability"
        )));
    }
    if manifest.capabilities.len() > MAX_CAPABILITIES {
        return Err(config_error(format!(
            "plugin {name:?} declares more than {MAX_CAPABILITIES} capabilities"
        )));
    }
    let mut capabilities = Vec::with_capacity(manifest.capabilities.len());
    for capability in &manifest.capabilities {
        if !valid_identifier(capability, 64) {
            return Err(config_error(format!(
                "plugin {name:?} has an invalid capability {capability:?}"
            )));
        }
        if capabilities.contains(capability) {
            return Err(config_error(format!(
                "plugin {name:?} declares capability {capability:?} twice"
            )));
        }
        capabilities.push(capability.clone());
    }

    let timeout_secs = manifest.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&timeout_secs) {
        return Err(config_error(format!(
            "plugin {name:?} timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
        )));
    }
    let max_output_bytes = manifest
        .max_output_bytes
        .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
    if !(MIN_MAX_OUTPUT_BYTES..=MAX_MAX_OUTPUT_BYTES).contains(&max_output_bytes) {
        return Err(config_error(format!(
            "plugin {name:?} max_output_bytes must be between {MIN_MAX_OUTPUT_BYTES} and {MAX_MAX_OUTPUT_BYTES}"
        )));
    }
    let max_attempts = manifest.max_attempts.unwrap_or(DEFAULT_MAX_ATTEMPTS);
    if !(1..=MAX_ATTEMPTS).contains(&max_attempts) {
        return Err(config_error(format!(
            "plugin {name:?} max_attempts must be between 1 and {MAX_ATTEMPTS}"
        )));
    }

    let transport = manifest.transport;
    match transport.kind {
        TransportKind::Remote => {
            if transport.command.is_some() || !transport.args.is_empty() {
                return Err(config_error(format!(
                    "plugin {name:?} declares a remote transport with stdio fields"
                )));
            }
            let url = transport.url.as_deref().unwrap_or_default();
            if url.is_empty() {
                return Err(config_error(format!(
                    "plugin {name:?} declares a remote transport without a url"
                )));
            }
            Err(config_error(format!(
                "plugin {name:?}: remote (WS/HTTP) transports are not implemented yet; \
                 configure a stdio command instead"
            )))
        }
        TransportKind::Stdio => {
            if transport.url.is_some() {
                return Err(config_error(format!(
                    "plugin {name:?} declares a stdio transport with a url"
                )));
            }
            let command = transport.command.ok_or_else(|| {
                config_error(format!(
                    "plugin {name:?} must declare an absolute stdio command"
                ))
            })?;
            if !command.is_absolute() {
                return Err(config_error(format!(
                    "plugin {name:?} command must be an absolute path, got {}",
                    command.display()
                )));
            }
            if transport.args.len() > MAX_ARGS {
                return Err(config_error(format!(
                    "plugin {name:?} declares more than {MAX_ARGS} arguments"
                )));
            }
            for argument in &transport.args {
                if argument.len() > MAX_ARG_BYTES || argument.contains('\0') {
                    return Err(config_error(format!(
                        "plugin {name:?} has an argument that is too long or contains a NUL byte"
                    )));
                }
            }
            Ok(PluginDefinition {
                name,
                capabilities,
                command,
                args: transport.args,
                timeout: Duration::from_secs(timeout_secs),
                max_output_bytes,
                max_attempts,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(value: serde_json::Value) -> String {
        json!({ "plugins": [value] }).to_string()
    }

    fn stdio_manifest(name: &str) -> serde_json::Value {
        json!({
            "name": name,
            "capabilities": ["ingest_pdf"],
            "transport": {"type": "stdio", "command": "/usr/local/bin/fake-plugin"}
        })
    }

    #[test]
    fn valid_manifest_resolves_with_defaults() {
        let config = PluginConfig::parse(&manifest(stdio_manifest("fake"))).expect("parse");
        let definition = config.get("fake").expect("configured");
        assert_eq!(
            definition.command,
            PathBuf::from("/usr/local/bin/fake-plugin")
        );
        assert!(definition.args.is_empty());
        assert_eq!(
            definition.timeout,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS)
        );
        assert_eq!(definition.max_output_bytes, DEFAULT_MAX_OUTPUT_BYTES);
        assert_eq!(definition.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert!(definition.supports("ingest_pdf"));
        assert!(!definition.supports("tikz_render"));
        assert_eq!(config.summaries()[0].transport, "stdio");
    }

    #[test]
    fn limits_and_args_are_taken_from_the_file() {
        let mut value = stdio_manifest("fake");
        value["transport"]["args"] = json!(["--mode", "fast"]);
        value["timeout_secs"] = json!(42);
        value["max_output_bytes"] = json!(4096);
        value["max_attempts"] = json!(3);
        let config = PluginConfig::parse(&manifest(value)).expect("parse");
        let definition = config.get("fake").expect("configured");
        assert_eq!(
            definition.args,
            vec!["--mode".to_owned(), "fast".to_owned()]
        );
        assert_eq!(definition.timeout, Duration::from_secs(42));
        assert_eq!(definition.max_output_bytes, 4096);
        assert_eq!(definition.max_attempts, 3);
    }

    #[test]
    fn remote_transports_are_refused_explicitly() {
        let value = json!({
            "name": "remote",
            "capabilities": ["ingest_pdf"],
            "transport": {"type": "remote", "url": "wss://example.test/plugin"}
        });
        let error = PluginConfig::parse(&manifest(value)).expect_err("remote is not supported");
        assert!(error.message.contains("not implemented"));
    }

    #[test]
    fn remote_transport_needs_a_url_and_no_stdio_fields() {
        let value = json!({
            "name": "remote",
            "capabilities": ["ingest_pdf"],
            "transport": {"type": "remote"}
        });
        let error = PluginConfig::parse(&manifest(value)).expect_err("url is required");
        assert!(error.message.contains("without a url"));

        let value = json!({
            "name": "remote",
            "capabilities": ["ingest_pdf"],
            "transport": {"type": "remote", "url": "wss://x", "command": "/bin/true"}
        });
        let error = PluginConfig::parse(&manifest(value)).expect_err("mixed transport");
        assert!(error.message.contains("stdio fields"));
    }

    #[test]
    fn commands_must_be_absolute_and_not_use_a_url() {
        let mut value = stdio_manifest("fake");
        value["transport"]["command"] = json!("fake-plugin");
        let error = PluginConfig::parse(&manifest(value)).expect_err("relative command");
        assert!(error.message.contains("absolute"));

        let mut value = stdio_manifest("fake");
        value["transport"]["url"] = json!("wss://x");
        let error = PluginConfig::parse(&manifest(value)).expect_err("url with stdio");
        assert!(error.message.contains("with a url"));
    }

    #[test]
    fn unknown_fields_are_rejected_so_typos_are_visible() {
        let mut value = stdio_manifest("fake");
        value["timeout_second"] = json!(10);
        let error = PluginConfig::parse(&manifest(value)).expect_err("unknown field");
        assert!(error.message.contains("invalid plugin configuration"));

        let mut value = stdio_manifest("fake");
        value["transport"]["shell"] = json!(true);
        let error = PluginConfig::parse(&manifest(value)).expect_err("unknown transport field");
        assert!(error.message.contains("invalid plugin configuration"));
    }

    #[test]
    fn names_capabilities_and_duplicates_are_validated() {
        for name in ["Fake", "../fake", "", "fake plugin", "fake/plugin"] {
            let error = PluginConfig::parse(&manifest(stdio_manifest(name)))
                .expect_err("invalid name must be rejected");
            assert!(error.message.contains("invalid plugin name"), "{name}");
        }

        let mut value = stdio_manifest("fake");
        value["capabilities"] = json!([]);
        assert!(PluginConfig::parse(&manifest(value)).is_err());

        let mut value = stdio_manifest("fake");
        value["capabilities"] = json!(["ingest_pdf", "ingest_pdf"]);
        let error = PluginConfig::parse(&manifest(value)).expect_err("duplicate capability");
        assert!(error.message.contains("twice"));

        let mut value = stdio_manifest("fake");
        value["capabilities"] = json!(["Ingest PDF"]);
        assert!(PluginConfig::parse(&manifest(value)).is_err());

        let two = json!({
            "plugins": [stdio_manifest("fake"), stdio_manifest("fake")]
        })
        .to_string();
        let error = PluginConfig::parse(&two).expect_err("duplicate names");
        assert!(error.message.contains("unique"));
    }

    #[test]
    fn numeric_limits_are_bounded() {
        for (field, value) in [
            ("timeout_secs", json!(0)),
            ("timeout_secs", json!(MAX_TIMEOUT_SECS + 1)),
            ("max_output_bytes", json!(1)),
            ("max_output_bytes", json!(MAX_MAX_OUTPUT_BYTES + 1)),
            ("max_attempts", json!(0)),
            ("max_attempts", json!(MAX_ATTEMPTS + 1)),
        ] {
            let mut manifest = stdio_manifest("fake");
            manifest[field] = value;
            let error = PluginConfig::parse(&manifest.to_string()).expect_err(field);
            assert!(!error.message.is_empty(), "{field}");
        }
    }

    #[test]
    fn an_empty_configuration_is_valid() {
        let config = PluginConfig::parse(r#"{"plugins":[]}"#).expect("empty config");
        assert!(config.is_empty());
        assert_eq!(config.len(), 0);
        assert!(config.from_file());
        let missing = PluginConfig::load(None).expect("no path");
        assert!(missing.is_empty());
        assert!(!missing.from_file());
    }

    #[test]
    fn a_missing_file_is_a_clear_error() {
        let error = PluginConfig::load(Some(Path::new("/nonexistent/plugins.json")))
            .expect_err("missing file");
        assert!(error.message.contains("cannot read plugin configuration"));
    }
}
