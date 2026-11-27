//! Prompt construction, strict validation of the agent's JSON proposal, and
//! conversion into `reinforce_plugin` protocol v1 [`Operation`]s.

use std::collections::HashSet;

use reinforce_plugin_protocol::{Operation, OperationKind};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::input::ResolvedJobInput;

/// Maximum UTF-8 byte length accepted for the agent's JSON proposal text.
pub const MAX_PROPOSAL_BYTES: usize = 512 * 1024;
/// Maximum number of cards accepted from a single ACP run.
pub const MAX_PROPOSED_CARDS: usize = 50;
/// Maximum number of content blocks per card section.
pub const MAX_BLOCKS_PER_CARD: usize = 32;
/// Maximum byte length of a single block payload (`text` or `latex`).
pub const MAX_BLOCK_TEXT_BYTES: usize = 32 * 1024;
/// Maximum byte length of a knowledge card title.
pub const MAX_TITLE_BYTES: usize = 256;
/// Maximum byte length of a card subject.
pub const MAX_SUBJECT_BYTES: usize = 128;
/// Maximum number of topics per card.
pub const MAX_TOPICS_PER_CARD: usize = 32;
/// Maximum byte length of a single topic string.
pub const MAX_TOPIC_BYTES: usize = 128;
/// Maximum number of options on a choice question card.
pub const MAX_OPTIONS_PER_QUESTION: usize = 16;
/// Maximum byte length of an option key.
pub const MAX_OPTION_KEY_BYTES: usize = 32;
/// Maximum byte length of an option's text or explanation.
pub const MAX_OPTION_CONTENT_BYTES: usize = 4 * 1024;
/// Maximum byte length of an answer basis explanation.
pub const MAX_ANSWER_BASIS_BYTES: usize = 4 * 1024;

const TIMESTAMP_FORMAT: &[time::format_description::FormatItem<'_>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6]Z");

/// Structured validation failure returned when the agent emits invalid output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProposalError {
    pub code: &'static str,
    pub message: String,
}

impl ProposalError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_proposal",
            message: message.into(),
        }
    }
}

/// Top-level JSON contract expected from the ACP agent.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProposal {
    cards: Vec<ProposedCard>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProposedCard {
    KnowledgeCard(ProposedKnowledgeCard),
    QuestionCard(ProposedQuestionCard),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposedKnowledgeCard {
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    title: String,
    blocks: Vec<ProposedBlock>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposedQuestionCard {
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    question_type: ProposedQuestionType,
    stem_blocks: Vec<ProposedBlock>,
    #[serde(default)]
    explanation_blocks: Vec<ProposedBlock>,
    #[serde(default)]
    options: Vec<ProposedOption>,
    #[serde(default)]
    answer: Vec<String>,
    #[serde(default)]
    answer_basis: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProposedQuestionType {
    SingleChoice,
    MultipleChoice,
    ShortAnswer,
    Vocab,
}

impl ProposedQuestionType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SingleChoice => "single_choice",
            Self::MultipleChoice => "multiple_choice",
            Self::ShortAnswer => "short_answer",
            Self::Vocab => "vocab",
        }
    }

    const fn is_choice(self) -> bool {
        matches!(self, Self::SingleChoice | Self::MultipleChoice)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposedOption {
    key: String,
    content: String,
    #[serde(default)]
    explanation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ProposedBlock {
    Markdown {
        text: String,
    },
    Math {
        latex: String,
        #[serde(default)]
        display: bool,
    },
}

/// Builds the ACP prompt asking the external agent to return strictly formatted
/// card proposal JSON for `input`.
pub fn build_prompt(input: &ResolvedJobInput, capability: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "You are drafting study card proposals for Reinforce.\n\
         Do NOT read files, write files, or run tools/commands. All source material is provided below.\n\
         Return ONLY a single JSON object matching this exact schema (no prose before or after):\n\
         {\n\
           \"cards\": [\n\
             {\n\
               \"kind\": \"knowledge_card\",\n\
               \"subject\": \"<non-empty subject>\",\n\
               \"topics\": [\"<topic>\"],\n\
               \"title\": \"<concise title>\",\n\
               \"blocks\": [\n\
                 {\"type\": \"markdown\", \"text\": \"<markdown body>\"},\n\
                 {\"type\": \"math\", \"latex\": \"<raw latex without $ delimiters>\", \"display\": true}\n\
               ]\n\
             },\n\
             {\n\
               \"kind\": \"question_card\",\n\
               \"subject\": \"<non-empty subject>\",\n\
               \"topics\": [\"<topic>\"],\n\
               \"question_type\": \"single_choice\" | \"multiple_choice\" | \"short_answer\" | \"vocab\",\n\
               \"stem_blocks\": [{\"type\": \"markdown\", \"text\": \"<question stem>\"}],\n\
               \"explanation_blocks\": [{\"type\": \"markdown\", \"text\": \"<explanation>\"}],\n\
               \"options\": [{\"key\": \"A\", \"content\": \"<choice>\", \"explanation\": \"<optional>\"}],\n\
               \"answer\": [\"A\"],\n\
               \"answer_basis\": \"<why the answer follows from the material>\"\n\
             }\n\
           ]\n\
         }\n\
         Rules:\n\
         - Provide between 1 and ",
    );
    prompt.push_str(&input.max_cards.to_string());
    prompt.push_str(
        " cards.\n\
         - Do not include unknown fields.\n\
         - Do not start any string field with '$'. For math formulas, use {\"type\": \"math\", \"latex\": \"...\"} without surrounding '$' signs.\n\
         - Choice questions (single_choice / multiple_choice) must have at least 2 unique options, non-empty answer keys matching options, and a non-empty answer_basis.\n\
         - single_choice must have exactly 1 answer key.\n\
         - short_answer and vocab questions must have empty options (or omit options) and, if answered, a non-empty answer_basis.\n",
    );

    prompt.push_str("\nJob context:\n");
    prompt.push_str(&format!("- capability: {capability}\n"));
    if let Some(subject) = &input.subject {
        prompt.push_str(&format!("- default_subject: {subject}\n"));
    }
    if !input.topics.is_empty() {
        prompt.push_str(&format!(
            "- suggested_topics: {}\n",
            input.topics.join(", ")
        ));
    }
    if let Some(resource_id) = &input.resource_id {
        prompt.push_str(&format!("- resource_id: {resource_id}\n"));
    }
    if let Some(title) = &input.resource_title {
        prompt.push_str(&format!("- resource_title: {title}\n"));
    }
    if let Some(instructions) = &input.instructions {
        prompt.push_str(&format!("- instructions: {instructions}\n"));
    }

    prompt.push_str("\nSource resource content:\n<<<RESOURCE_CONTENT>>>\n");
    prompt.push_str(&input.content);
    prompt.push_str("\n<<<END_RESOURCE_CONTENT>>>\n");
    prompt
}

/// Parses, strictly validates, and converts raw ACP assistant text into
/// `reinforce_plugin` protocol v1 operations.
pub fn parse_and_convert(
    raw_text: &str,
    input: &ResolvedJobInput,
) -> Result<Vec<Operation>, ProposalError> {
    if raw_text.len() > MAX_PROPOSAL_BYTES {
        return Err(ProposalError::invalid(format!(
            "agent proposal exceeds maximum size of {MAX_PROPOSAL_BYTES} bytes"
        )));
    }
    let json_text = unwrap_single_json_fence(raw_text)?;
    let proposal: AgentProposal = serde_json::from_str(json_text).map_err(|error| {
        ProposalError::invalid(format!("agent output is not valid proposal JSON: {error}"))
    })?;

    if proposal.cards.is_empty() {
        return Err(ProposalError::invalid(
            "agent proposal must contain at least one card",
        ));
    }
    if proposal.cards.len() > input.max_cards {
        return Err(ProposalError::invalid(format!(
            "agent proposed {} cards, exceeding the limit of {}",
            proposal.cards.len(),
            input.max_cards
        )));
    }

    let now = now_timestamp();
    let base_metadata = build_metadata(input);
    let mut operations = Vec::with_capacity(proposal.cards.len());

    for (index, card) in proposal.cards.into_iter().enumerate() {
        let temp_ref = format!("$card_{}", index + 1);
        let operation = match card {
            ProposedCard::KnowledgeCard(card) => {
                convert_knowledge_card(index, temp_ref, card, input, &base_metadata, &now)?
            }
            ProposedCard::QuestionCard(card) => {
                convert_question_card(index, temp_ref, card, input, &base_metadata, &now)?
            }
        };
        operations.push(operation);
    }

    Ok(operations)
}

fn unwrap_single_json_fence(raw: &str) -> Result<&str, ProposalError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ProposalError::invalid("agent returned empty output"));
    }
    if let Some(after_open) = trimmed.strip_prefix("```") {
        let Some(first_newline) = after_open.find('\n') else {
            return Err(ProposalError::invalid(
                "agent returned an incomplete fenced code block",
            ));
        };
        let info = after_open[..first_newline].trim();
        if !info.is_empty() && !info.eq_ignore_ascii_case("json") {
            return Err(ProposalError::invalid(format!(
                "unexpected fenced code block language {info:?}; expected json"
            )));
        }
        let body = &after_open[first_newline + 1..];
        let Some(inner) = body.strip_suffix("```") else {
            return Err(ProposalError::invalid(
                "agent returned an unterminated fenced code block",
            ));
        };
        let inner = inner.trim();
        if inner.contains("```") {
            return Err(ProposalError::invalid(
                "agent output must contain a single JSON object without extra fenced blocks",
            ));
        }
        Ok(inner)
    } else {
        Ok(trimmed)
    }
}

fn convert_knowledge_card(
    index: usize,
    temp_ref: String,
    card: ProposedKnowledgeCard,
    input: &ResolvedJobInput,
    metadata: &Value,
    timestamp: &str,
) -> Result<Operation, ProposalError> {
    let prefix = format!("cards[{index}]");
    let subject = resolve_subject(card.subject, input, &prefix)?;
    let topics = resolve_topics(card.topics, input, &prefix)?;
    let title = validate_text(&card.title, &format!("{prefix}.title"), MAX_TITLE_BYTES)?;
    let blocks = validate_blocks(card.blocks, &format!("{prefix}.blocks"), true)?;

    let data = json!({
        "id": temp_ref,
        "subject": subject,
        "topics": topics,
        "title": title,
        "blocks": blocks,
        "anchor_ids": input.anchor_ids,
        "status": "draft",
        "verification": "unchecked",
        "unsure_count": 0,
        "metadata": metadata,
        "created_at": timestamp,
        "updated_at": timestamp,
    });

    Ok(Operation {
        op: OperationKind::KnowledgeCard,
        temp_ref: Some(temp_ref),
        data,
    })
}

fn convert_question_card(
    index: usize,
    temp_ref: String,
    card: ProposedQuestionCard,
    input: &ResolvedJobInput,
    metadata: &Value,
    timestamp: &str,
) -> Result<Operation, ProposalError> {
    let prefix = format!("cards[{index}]");
    let subject = resolve_subject(card.subject, input, &prefix)?;
    let topics = resolve_topics(card.topics, input, &prefix)?;
    let stem_blocks = validate_blocks(card.stem_blocks, &format!("{prefix}.stem_blocks"), true)?;
    let explanation_blocks = validate_blocks(
        card.explanation_blocks,
        &format!("{prefix}.explanation_blocks"),
        false,
    )?;

    if card.options.len() > MAX_OPTIONS_PER_QUESTION {
        return Err(ProposalError::invalid(format!(
            "{prefix}.options exceeds {MAX_OPTIONS_PER_QUESTION} items"
        )));
    }
    if card.question_type.is_choice() && card.options.len() < 2 {
        return Err(ProposalError::invalid(format!(
            "{prefix}.options must contain at least 2 options for {}",
            card.question_type.as_str()
        )));
    }
    if !card.question_type.is_choice() && !card.options.is_empty() {
        return Err(ProposalError::invalid(format!(
            "{prefix}.options must be empty for {}",
            card.question_type.as_str()
        )));
    }

    let mut option_keys = HashSet::new();
    let mut encoded_options = Vec::with_capacity(card.options.len());
    for (opt_index, option) in card.options.into_iter().enumerate() {
        let opt_prefix = format!("{prefix}.options[{opt_index}]");
        let key = validate_text(
            &option.key,
            &format!("{opt_prefix}.key"),
            MAX_OPTION_KEY_BYTES,
        )?;
        if !option_keys.insert(key.clone()) {
            return Err(ProposalError::invalid(format!(
                "{opt_prefix}.key duplicates option key {key:?}"
            )));
        }
        let content = validate_text(
            &option.content,
            &format!("{opt_prefix}.content"),
            MAX_OPTION_CONTENT_BYTES,
        )?;
        let explanation = match option.explanation {
            Some(text) if !text.trim().is_empty() => Some(validate_text(
                &text,
                &format!("{opt_prefix}.explanation"),
                MAX_OPTION_CONTENT_BYTES,
            )?),
            _ => None,
        };
        encoded_options.push(json!({
            "key": key,
            "content": content,
            "explanation": explanation,
            "linked_card_ids": [],
        }));
    }

    let mut answers = Vec::with_capacity(card.answer.len());
    let mut seen_answers = HashSet::new();
    for (ans_index, answer) in card.answer.into_iter().enumerate() {
        let cleaned = validate_text(
            &answer,
            &format!("{prefix}.answer[{ans_index}]"),
            MAX_OPTION_CONTENT_BYTES,
        )?;
        if !seen_answers.insert(cleaned.clone()) {
            return Err(ProposalError::invalid(format!(
                "{prefix}.answer[{ans_index}] duplicates answer {cleaned:?}"
            )));
        }
        if card.question_type.is_choice() && !option_keys.contains(&cleaned) {
            return Err(ProposalError::invalid(format!(
                "{prefix}.answer[{ans_index}] references undeclared option {cleaned:?}"
            )));
        }
        answers.push(cleaned);
    }

    if card.question_type == ProposedQuestionType::SingleChoice && answers.len() != 1 {
        return Err(ProposalError::invalid(format!(
            "{prefix}.answer must contain exactly 1 option key for single_choice"
        )));
    }
    if card.question_type == ProposedQuestionType::MultipleChoice && answers.is_empty() {
        return Err(ProposalError::invalid(format!(
            "{prefix}.answer must contain at least 1 option key for multiple_choice"
        )));
    }

    let (answer_source_kind, answer_basis) = if answers.is_empty() {
        ("unanswered", None)
    } else {
        let raw_basis = card.answer_basis.ok_or_else(|| {
            ProposalError::invalid(format!(
                "{prefix}.answer_basis is required when an answer is provided"
            ))
        })?;
        let basis = validate_text(
            &raw_basis,
            &format!("{prefix}.answer_basis"),
            MAX_ANSWER_BASIS_BYTES,
        )?;
        ("original_authored", Some(basis))
    };

    let data = json!({
        "id": temp_ref,
        "exam_set_id": Value::Null,
        "question_group_id": Value::Null,
        "question_no": Value::Null,
        "subject": subject,
        "topics": topics,
        "stem_blocks": stem_blocks,
        "explanation_blocks": explanation_blocks,
        "anchor_ids": input.anchor_ids,
        "question_type": card.question_type.as_str(),
        "grading_mode": "exact",
        "options": encoded_options,
        "answer": answers,
        "accepted_answers": [],
        "answer_source_kind": answer_source_kind,
        "answer_basis": answer_basis,
        "duplicate_question_id": Value::Null,
        "status": "draft",
        "verification": "unchecked",
        "unsure_count": 0,
        "metadata": metadata,
        "created_at": timestamp,
        "updated_at": timestamp,
    });

    Ok(Operation {
        op: OperationKind::QuestionCard,
        temp_ref: Some(temp_ref),
        data,
    })
}

fn resolve_subject(
    card_subject: Option<String>,
    input: &ResolvedJobInput,
    prefix: &str,
) -> Result<String, ProposalError> {
    let raw = card_subject
        .or_else(|| input.subject.clone())
        .ok_or_else(|| {
            ProposalError::invalid(format!(
                "{prefix}.subject is required when input.subject is not set"
            ))
        })?;
    validate_text(&raw, &format!("{prefix}.subject"), MAX_SUBJECT_BYTES)
}

fn resolve_topics(
    card_topics: Vec<String>,
    input: &ResolvedJobInput,
    prefix: &str,
) -> Result<Vec<String>, ProposalError> {
    let source = if card_topics.is_empty() {
        input.topics.clone()
    } else {
        card_topics
    };
    if source.len() > MAX_TOPICS_PER_CARD {
        return Err(ProposalError::invalid(format!(
            "{prefix}.topics exceeds {MAX_TOPICS_PER_CARD} items"
        )));
    }
    let mut topics = Vec::with_capacity(source.len());
    for (idx, topic) in source.into_iter().enumerate() {
        topics.push(validate_text(
            &topic,
            &format!("{prefix}.topics[{idx}]"),
            MAX_TOPIC_BYTES,
        )?);
    }
    Ok(topics)
}

fn validate_blocks(
    blocks: Vec<ProposedBlock>,
    field_path: &str,
    require_non_empty: bool,
) -> Result<Vec<Value>, ProposalError> {
    if require_non_empty && blocks.is_empty() {
        return Err(ProposalError::invalid(format!(
            "{field_path} must contain at least one block"
        )));
    }
    if blocks.len() > MAX_BLOCKS_PER_CARD {
        return Err(ProposalError::invalid(format!(
            "{field_path} exceeds {MAX_BLOCKS_PER_CARD} blocks"
        )));
    }
    let mut encoded = Vec::with_capacity(blocks.len());
    for (idx, block) in blocks.into_iter().enumerate() {
        match block {
            ProposedBlock::Markdown { text } => {
                let cleaned = validate_text(
                    &text,
                    &format!("{field_path}[{idx}].text"),
                    MAX_BLOCK_TEXT_BYTES,
                )?;
                encoded.push(json!({
                    "type": "markdown",
                    "text": cleaned,
                }));
            }
            ProposedBlock::Math { latex, display } => {
                let cleaned = validate_text(
                    &latex,
                    &format!("{field_path}[{idx}].latex"),
                    MAX_BLOCK_TEXT_BYTES,
                )?;
                encoded.push(json!({
                    "type": "math",
                    "latex": cleaned,
                    "display": display,
                }));
            }
        }
    }
    Ok(encoded)
}

fn validate_text(value: &str, field_path: &str, max_bytes: usize) -> Result<String, ProposalError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ProposalError::invalid(format!(
            "{field_path} must not be empty"
        )));
    }
    if trimmed.len() > max_bytes {
        return Err(ProposalError::invalid(format!(
            "{field_path} exceeds {max_bytes} bytes"
        )));
    }
    if trimmed.starts_with('$') {
        return Err(ProposalError::invalid(format!(
            "{field_path} must not start with '$' (reserved for batch temp_ref aliases)"
        )));
    }
    Ok(trimmed.to_owned())
}

fn build_metadata(input: &ResolvedJobInput) -> Value {
    let mut map = Map::new();
    map.insert("source_plugin".to_owned(), Value::String("acp".to_owned()));
    if let Some(resource_id) = &input.resource_id {
        map.insert(
            "source_resource_id".to_owned(),
            Value::String(resource_id.clone()),
        );
    }
    Value::Object(map)
}

fn now_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    let truncated = now
        .replace_nanosecond(now.nanosecond() / 1_000 * 1_000)
        .unwrap_or(now);
    truncated
        .format(TIMESTAMP_FORMAT)
        .expect("UTC timestamp is always representable")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> ResolvedJobInput {
        ResolvedJobInput::parse(
            json!({
                "resource_id": "res-1",
                "content": "Newton's third law states action equals reaction.",
                "subject": "physics",
                "topics": ["mechanics"],
                "max_cards": 10
            }),
            None,
        )
        .expect("sample input")
    }

    #[test]
    fn converts_valid_knowledge_and_question_cards() {
        let raw = r#"```json
{
  "cards": [
    {
      "kind": "knowledge_card",
      "title": "Newton's Third Law",
      "blocks": [
        {"type": "markdown", "text": "Every action has an equal and opposite reaction."},
        {"type": "math", "latex": "\\vec{F}_{AB} = -\\vec{F}_{BA}", "display": true}
      ]
    },
    {
      "kind": "question_card",
      "question_type": "single_choice",
      "stem_blocks": [{"type": "markdown", "text": "Forces in a third-law pair act on:"}],
      "explanation_blocks": [{"type": "markdown", "text": "They act on two different bodies."}],
      "options": [
        {"key": "A", "content": "The same object"},
        {"key": "B", "content": "Two different interacting objects"}
      ],
      "answer": ["B"],
      "answer_basis": "Action-reaction pairs always act on distinct interacting bodies."
    }
  ]
}
```"#;
        let ops = parse_and_convert(raw, &sample_input()).expect("valid proposal");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].op, OperationKind::KnowledgeCard);
        assert_eq!(ops[0].temp_ref.as_deref(), Some("$card_1"));
        assert_eq!(ops[0].data["subject"], "physics");
        assert_eq!(ops[0].data["verification"], "unchecked");
        assert_eq!(ops[1].op, OperationKind::QuestionCard);
        assert_eq!(ops[1].temp_ref.as_deref(), Some("$card_2"));
        assert_eq!(ops[1].data["answer_source_kind"], "original_authored");
    }

    #[test]
    fn rejects_malformed_empty_unknown_and_inconsistent_proposals() {
        let input = sample_input();

        assert!(parse_and_convert("not json", &input).is_err());
        assert!(parse_and_convert(r#"{"cards":[]}"#, &input).is_err());
        assert!(
            parse_and_convert(
                r#"{"cards":[{"kind":"knowledge_card","title":"T","blocks":[{"type":"markdown","text":"x"}],"extra":1}]}"#,
                &input
            )
            .is_err()
        );
        // Single choice with undeclared answer key.
        assert!(
            parse_and_convert(
                r#"{"cards":[{"kind":"question_card","question_type":"single_choice","stem_blocks":[{"type":"markdown","text":"Q"}],"options":[{"key":"A","content":"a"},{"key":"B","content":"b"}],"answer":["C"],"answer_basis":"because"}]}"#,
                &input
            )
            .is_err()
        );
        // Leading '$' in text rejected before hitting host batch temp_ref resolver.
        assert!(
            parse_and_convert(
                r#"{"cards":[{"kind":"knowledge_card","title":"$bad","blocks":[{"type":"markdown","text":"ok"}]}]}"#,
                &input
            )
            .is_err()
        );
    }
}
