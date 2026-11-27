//! Validation and resolution of the `hello.input` payload.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::proposal::{
    MAX_PROPOSED_CARDS, MAX_SUBJECT_BYTES, MAX_TOPIC_BYTES, MAX_TOPICS_PER_CARD,
};

/// Maximum UTF-8 resource text size handed to the ACP agent.
pub const MAX_RESOURCE_CONTENT_BYTES: usize = 256 * 1024;
/// Maximum byte size for optional extra instructions in `hello.input`.
pub const MAX_INSTRUCTIONS_BYTES: usize = 4 * 1024;
/// Maximum byte size for optional resource title or identifier fields.
pub const MAX_LABEL_BYTES: usize = 256;
/// Maximum number of pre-existing anchor IDs attached from job input.
pub const MAX_ANCHOR_IDS: usize = 32;

/// Structured input failure returned as a plugin `error` message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InputError {
    pub code: &'static str,
    pub message: String,
}

impl InputError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_input",
            message: message.into(),
        }
    }

    fn denied(message: impl Into<String>) -> Self {
        Self {
            code: "resource_access_denied",
            message: message.into(),
        }
    }
}

/// Raw JSON schema accepted in `hello.input`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJobInput {
    #[serde(default)]
    resource_id: Option<String>,
    #[serde(default)]
    resource_title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    resource_path: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    anchor_ids: Vec<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    max_cards: Option<usize>,
}

/// Validated job input ready to be turned into an ACP prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedJobInput {
    pub resource_id: Option<String>,
    pub resource_title: Option<String>,
    pub content: String,
    pub subject: Option<String>,
    pub topics: Vec<String>,
    pub anchor_ids: Vec<String>,
    pub instructions: Option<String>,
    pub max_cards: usize,
}

impl ResolvedJobInput {
    /// Parses and validates `hello.input`, optionally loading a text resource
    /// confined to `resource_dir`.
    pub fn parse(value: Value, resource_dir: Option<&Path>) -> Result<Self, InputError> {
        let raw: RawJobInput = serde_json::from_value(value)
            .map_err(|error| InputError::invalid(format!("invalid job input: {error}")))?;

        let content = resolve_content(raw.content, raw.resource_path, resource_dir)?;

        let resource_id = normalize_optional(raw.resource_id, "resource_id", MAX_LABEL_BYTES)?;
        let resource_title =
            normalize_optional(raw.resource_title, "resource_title", MAX_LABEL_BYTES)?;
        let subject = normalize_optional(raw.subject, "subject", MAX_SUBJECT_BYTES)?;
        let instructions =
            normalize_optional(raw.instructions, "instructions", MAX_INSTRUCTIONS_BYTES)?;

        if raw.topics.len() > MAX_TOPICS_PER_CARD {
            return Err(InputError::invalid(format!(
                "input.topics may contain at most {MAX_TOPICS_PER_CARD} items"
            )));
        }
        let mut topics = Vec::with_capacity(raw.topics.len());
        for (index, topic) in raw.topics.into_iter().enumerate() {
            let trimmed = topic.trim();
            if trimmed.is_empty() {
                return Err(InputError::invalid(format!(
                    "input.topics[{index}] must not be empty"
                )));
            }
            if trimmed.len() > MAX_TOPIC_BYTES {
                return Err(InputError::invalid(format!(
                    "input.topics[{index}] exceeds {MAX_TOPIC_BYTES} bytes"
                )));
            }
            if trimmed.starts_with('$') {
                return Err(InputError::invalid(format!(
                    "input.topics[{index}] must not start with '$'"
                )));
            }
            topics.push(trimmed.to_owned());
        }

        if raw.anchor_ids.len() > MAX_ANCHOR_IDS {
            return Err(InputError::invalid(format!(
                "input.anchor_ids may contain at most {MAX_ANCHOR_IDS} items"
            )));
        }
        let mut anchor_ids = Vec::with_capacity(raw.anchor_ids.len());
        for (index, anchor_id) in raw.anchor_ids.into_iter().enumerate() {
            let trimmed = anchor_id.trim();
            let parsed = Uuid::parse_str(trimmed).map_err(|_| {
                InputError::invalid(format!("input.anchor_ids[{index}] is not a valid UUIDv7"))
            })?;
            if parsed.get_version_num() != 7 {
                return Err(InputError::invalid(format!(
                    "input.anchor_ids[{index}] must be a UUIDv7"
                )));
            }
            anchor_ids.push(parsed.to_string());
        }

        let max_cards = match raw.max_cards {
            Some(0) => {
                return Err(InputError::invalid("input.max_cards must be at least 1"));
            }
            Some(limit) if limit > MAX_PROPOSED_CARDS => {
                return Err(InputError::invalid(format!(
                    "input.max_cards must be at most {MAX_PROPOSED_CARDS}"
                )));
            }
            Some(limit) => limit,
            None => MAX_PROPOSED_CARDS,
        };

        Ok(Self {
            resource_id,
            resource_title,
            content,
            subject,
            topics,
            anchor_ids,
            instructions,
            max_cards,
        })
    }
}

fn normalize_optional(
    value: Option<String>,
    field: &str,
    max_bytes: usize,
) -> Result<Option<String>, InputError> {
    let Some(text) = value else {
        return Ok(None);
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(InputError::invalid(format!(
            "input.{field} must not be empty when provided"
        )));
    }
    if trimmed.len() > max_bytes {
        return Err(InputError::invalid(format!(
            "input.{field} exceeds {max_bytes} bytes"
        )));
    }
    if matches!(field, "subject" | "resource_id") && trimmed.starts_with('$') {
        return Err(InputError::invalid(format!(
            "input.{field} must not start with '$'"
        )));
    }
    Ok(Some(trimmed.to_owned()))
}

fn resolve_content(
    inline: Option<String>,
    resource_path: Option<String>,
    resource_dir: Option<&Path>,
) -> Result<String, InputError> {
    match (inline, resource_path) {
        (Some(_), Some(_)) => Err(InputError::invalid(
            "provide either input.content or input.resource_path, not both",
        )),
        (None, None) => Err(InputError::invalid(
            "input must include non-empty content or resource_path",
        )),
        (Some(content), None) => {
            if content.trim().is_empty() {
                return Err(InputError::invalid("input.content must not be empty"));
            }
            if content.len() > MAX_RESOURCE_CONTENT_BYTES {
                return Err(InputError::invalid(format!(
                    "input.content exceeds {MAX_RESOURCE_CONTENT_BYTES} bytes"
                )));
            }
            Ok(content)
        }
        (None, Some(path_text)) => read_confined_resource(&path_text, resource_dir),
    }
}

fn read_confined_resource(
    path_text: &str,
    resource_dir: Option<&Path>,
) -> Result<String, InputError> {
    let Some(base_dir) = resource_dir else {
        return Err(InputError::denied(
            "input.resource_path requires --resource-dir to be configured in the plugin manifest",
        ));
    };
    let trimmed = path_text.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(InputError::invalid(
            "input.resource_path must be a valid path",
        ));
    }
    let canonical_base = std::fs::canonicalize(base_dir).map_err(|error| {
        InputError::denied(format!(
            "configured --resource-dir is not accessible: {error}"
        ))
    })?;
    let requested = PathBuf::from(trimmed);
    let candidate = if requested.is_absolute() {
        requested
    } else {
        canonical_base.join(requested)
    };
    let canonical_target = std::fs::canonicalize(&candidate).map_err(|error| {
        InputError::invalid(format!("cannot open resource_path {trimmed:?}: {error}"))
    })?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err(InputError::denied(
            "input.resource_path escapes the configured --resource-dir",
        ));
    }
    let metadata = std::fs::metadata(&canonical_target)
        .map_err(|error| InputError::invalid(format!("cannot stat resource_path: {error}")))?;
    if !metadata.is_file() {
        return Err(InputError::invalid(
            "input.resource_path must point to a regular file",
        ));
    }
    if metadata.len() as usize > MAX_RESOURCE_CONTENT_BYTES {
        return Err(InputError::invalid(format!(
            "resource file exceeds {MAX_RESOURCE_CONTENT_BYTES} bytes"
        )));
    }
    let text = std::fs::read_to_string(&canonical_target).map_err(|error| {
        InputError::invalid(format!("cannot read UTF-8 resource file: {error}"))
    })?;
    if text.trim().is_empty() {
        return Err(InputError::invalid("resource file is empty"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_inline_input() {
        let anchor = Uuid::now_v7().to_string();
        let resolved = ResolvedJobInput::parse(
            json!({
                "resource_id": "res-1",
                "resource_title": "Chapter 3",
                "content": "Action and reaction are equal and opposite.",
                "subject": "physics",
                "topics": ["mechanics"],
                "anchor_ids": [anchor],
                "instructions": "Draft concise cards.",
                "max_cards": 5
            }),
            None,
        )
        .expect("valid input");
        assert_eq!(resolved.subject.as_deref(), Some("physics"));
        assert_eq!(resolved.max_cards, 5);
        assert_eq!(resolved.anchor_ids.len(), 1);
    }

    #[test]
    fn rejects_missing_content_and_unknown_fields() {
        let missing = ResolvedJobInput::parse(json!({"subject": "physics"}), None)
            .expect_err("content required");
        assert_eq!(missing.code, "invalid_input");

        let unknown =
            ResolvedJobInput::parse(json!({"content": "hello", "command": "/bin/sh"}), None)
                .expect_err("unknown field rejected");
        assert_eq!(unknown.code, "invalid_input");
    }

    #[test]
    fn rejects_non_v7_anchor_id() {
        let v4 = Uuid::from_u128(0x1111_2222_3333_4444_8555_6666_7777_8888).to_string();
        let error = ResolvedJobInput::parse(json!({"content": "text", "anchor_ids": [v4]}), None)
            .expect_err("must be v7");
        assert_eq!(error.code, "invalid_input");
    }

    #[test]
    fn confines_resource_path_to_resource_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let inside_file = dir.path().join("notes.md");
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&inside_file, "Cell respiration produces ATP.").unwrap();
        std::fs::write(&outside_file, "secret").unwrap();

        let denied = ResolvedJobInput::parse(json!({"resource_path": "notes.md"}), None)
            .expect_err("requires --resource-dir");
        assert_eq!(denied.code, "resource_access_denied");

        let ok = ResolvedJobInput::parse(json!({"resource_path": "notes.md"}), Some(dir.path()))
            .expect("reads inside dir");
        assert!(ok.content.contains("ATP"));

        let escaped = ResolvedJobInput::parse(
            json!({"resource_path": outside_file.display().to_string()}),
            Some(dir.path()),
        )
        .expect_err("blocks escape");
        assert_eq!(escaped.code, "resource_access_denied");
    }
}
