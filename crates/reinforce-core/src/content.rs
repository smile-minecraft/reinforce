use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hash::ContentHash;
use crate::primitives::{
    AnchorKind, AnswerSourceKind, CardStatus, DiagramSyntax, EdgeType, EndpointKind, ExportPolicy,
    GradingMode, KnowledgeCardId, ProposalStatus, ProposalTargetKind, QuestionCardId, QuestionType,
    ResourceId, ResourceKind, Sensitivity, SourceKind, Verification,
};
use crate::timestamp::Timestamp;
use crate::{AnchorId, EdgeId, ExamSetId, NoteId, ProposalId, QuestionGroupId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub kind: ResourceKind,
    pub title: String,
    pub sha256: ContentHash,
    pub byte_size: i64,
    pub page_count: Option<i64>,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
    pub license: Option<String>,
    pub attribution_url: Option<String>,
    pub attribution_notice: Option<String>,
    pub sensitivity: Sensitivity,
    pub export_policy: ExportPolicy,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaSpan {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub id: AnchorId,
    pub resource_id: ResourceId,
    pub anchor_kind: AnchorKind,
    pub pdf_page: Option<i64>,
    pub printed_page: Option<i64>,
    pub legend_pdf_page: Option<i64>,
    pub rect: Option<NormalizedRect>,
    pub media_span: Option<MediaSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcclusionMask {
    pub group_id: String,
    pub rect: NormalizedRect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramNodeBinding {
    pub node_id: String,
    pub card_id: KnowledgeCardId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub no: u32,
    pub x: f64,
    pub y: f64,
    pub label_x: f64,
    pub label_y: f64,
    pub term_primary: String,
    pub term_secondary: Option<String>,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub linked_card_id: Option<KnowledgeCardId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Markdown {
        text: String,
    },
    Math {
        latex: String,
        display: bool,
    },
    Diagram {
        source: String,
        syntax: DiagramSyntax,
        svg_cache: Option<String>,
        node_bindings: Vec<DiagramNodeBinding>,
    },
    ImageOcclusion {
        resource_id: ResourceId,
        masks: Vec<OcclusionMask>,
    },
    InteractivePins {
        resource_id: ResourceId,
        overlay_resource_id: Option<ResourceId>,
        pins: Vec<Pin>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeCard {
    pub id: KnowledgeCardId,
    pub subject: String,
    pub topics: Vec<String>,
    pub title: String,
    pub blocks: Vec<ContentBlock>,
    pub anchor_ids: Vec<AnchorId>,
    pub status: CardStatus,
    pub verification: Verification,
    pub unsure_count: u32,
    pub metadata: Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutlineNode {
    Section {
        heading: String,
        intro_markdown: Option<String>,
        children: Vec<OutlineNode>,
    },
    CardRef {
        card_id: KnowledgeCardId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub subject: String,
    pub title: String,
    pub outline: Vec<OutlineNode>,
    pub view_settings: Value,
    pub status: CardStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExamSet {
    pub id: ExamSetId,
    pub subject: String,
    pub title: String,
    pub source_kind: SourceKind,
    pub exam_year: Option<i32>,
    pub source_resource_id: Option<ResourceId>,
    pub expected_questions: Option<u32>,
    pub metadata: Value,
    pub is_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionGroup {
    pub id: QuestionGroupId,
    pub exam_set_id: Option<ExamSetId>,
    pub title: Option<String>,
    pub shared_stem_blocks: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub key: String,
    pub content: String,
    pub explanation: Option<String>,
    pub linked_card_ids: Vec<KnowledgeCardId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionCard {
    pub id: QuestionCardId,
    pub exam_set_id: Option<ExamSetId>,
    pub question_group_id: Option<QuestionGroupId>,
    pub question_no: Option<u32>,
    pub subject: String,
    pub topics: Vec<String>,
    pub stem_blocks: Vec<ContentBlock>,
    pub explanation_blocks: Vec<ContentBlock>,
    pub anchor_ids: Vec<AnchorId>,
    pub question_type: QuestionType,
    pub grading_mode: GradingMode,
    pub options: Vec<QuestionOption>,
    pub answer: Vec<String>,
    pub accepted_answers: Vec<Vec<String>>,
    pub answer_source_kind: AnswerSourceKind,
    pub answer_basis: Option<String>,
    #[serde(default)]
    pub duplicate_question_id: Option<QuestionCardId>,
    pub status: CardStatus,
    pub verification: Verification,
    pub unsure_count: u32,
    pub metadata: Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Joins searchable values with single spaces so JSON punctuation is never indexed.
fn join_search_text(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extracts readable content from typed blocks, never serde tags or field names.
pub fn search_text_for_blocks(blocks: &[ContentBlock]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Markdown { text } => parts.push(text),
            ContentBlock::Math { latex, .. } => parts.push(latex),
            ContentBlock::Diagram { source, .. } => parts.push(source),
            ContentBlock::ImageOcclusion { masks, .. } => {
                parts.extend(masks.iter().map(|mask| mask.group_id.as_str()));
            }
            ContentBlock::InteractivePins { pins, .. } => {
                for pin in pins {
                    parts.push(&pin.term_primary);
                    parts.extend(pin.term_secondary.as_deref());
                    parts.extend(pin.detail.as_deref());
                    parts.extend(pin.hint.as_deref());
                }
            }
        }
    }
    join_search_text(&parts)
}

pub fn search_text_for_knowledge_card(card: &KnowledgeCard) -> String {
    let block_text = search_text_for_blocks(&card.blocks);
    let mut parts: Vec<&str> = card.topics.iter().map(String::as_str).collect();
    parts.push(&block_text);
    join_search_text(&parts)
}

pub fn search_text_for_question_card(card: &QuestionCard) -> String {
    let stem_text = search_text_for_blocks(&card.stem_blocks);
    let explanation_text = search_text_for_blocks(&card.explanation_blocks);
    let mut parts: Vec<&str> = card.topics.iter().map(String::as_str).collect();
    parts.push(&stem_text);
    parts.push(&explanation_text);
    for option in &card.options {
        parts.push(&option.content);
        parts.extend(option.explanation.as_deref());
    }
    join_search_text(&parts)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalTarget {
    Resource(ResourceId),
    KnowledgeCard(KnowledgeCardId),
    Note(NoteId),
    ExamSet(ExamSetId),
    QuestionGroup(QuestionGroupId),
    QuestionCard(QuestionCardId),
    Edge(EdgeId),
}

impl ProposalTarget {
    pub fn kind(&self) -> ProposalTargetKind {
        match self {
            Self::Resource(_) => ProposalTargetKind::Resource,
            Self::KnowledgeCard(_) => ProposalTargetKind::KnowledgeCard,
            Self::Note(_) => ProposalTargetKind::Note,
            Self::ExamSet(_) => ProposalTargetKind::ExamSet,
            Self::QuestionGroup(_) => ProposalTargetKind::QuestionGroup,
            Self::QuestionCard(_) => ProposalTargetKind::QuestionCard,
            Self::Edge(_) => ProposalTargetKind::Edge,
        }
    }

    pub fn id(&self) -> String {
        match self {
            Self::Resource(id) => id.as_str(),
            Self::KnowledgeCard(id) => id.as_str(),
            Self::Note(id) => id.as_str(),
            Self::ExamSet(id) => id.as_str(),
            Self::QuestionGroup(id) => id.as_str(),
            Self::QuestionCard(id) => id.as_str(),
            Self::Edge(id) => id.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: ProposalId,
    pub target: ProposalTarget,
    pub payload: Value,
    pub submitted_by: String,
    pub status: ProposalStatus,
    pub review_note: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub sync_cursor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EdgeEndpoint {
    Card { id: KnowledgeCardId },
    Question { id: QuestionCardId },
    Option { id: QuestionCardId, key: String },
}

impl EdgeEndpoint {
    pub fn kind(&self) -> EndpointKind {
        match self {
            Self::Card { .. } => EndpointKind::Card,
            Self::Question { .. } => EndpointKind::Question,
            Self::Option { .. } => EndpointKind::Option,
        }
    }

    pub fn id(&self) -> String {
        match self {
            Self::Card { id } => id.as_str(),
            Self::Question { id } | Self::Option { id, .. } => id.as_str(),
        }
    }

    pub fn option_key(&self) -> Option<&str> {
        match self {
            Self::Option { key, .. } => Some(key),
            _ => None,
        }
    }

    pub fn key(&self) -> String {
        match self.option_key() {
            Some(key) => format!("{}:{}:{}", self.kind(), self.id(), key),
            None => format!("{}:{}", self.kind(), self.id()),
        }
    }

    pub fn from_db_parts(
        kind: EndpointKind,
        id: &str,
        option_key: Option<&str>,
    ) -> crate::Result<Self> {
        match (kind, option_key) {
            (EndpointKind::Card, None) => Ok(Self::Card { id: id.parse()? }),
            (EndpointKind::Question, None) => Ok(Self::Question { id: id.parse()? }),
            (EndpointKind::Option, Some(key)) => Ok(Self::Option {
                id: id.parse()?,
                key: key.to_owned(),
            }),
            (kind, option) => Err(crate::primitives::Error::InvalidValue(
                "edge endpoint",
                format!("{kind} with option key {option:?}"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub edge_type: EdgeType,
    pub from: EdgeEndpoint,
    pub to: EdgeEndpoint,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub endpoint: EdgeEndpoint,
    pub depth: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::DiagramSyntax;

    #[test]
    fn block_search_text_excludes_json_keys_and_type_names() {
        let blocks = vec![
            ContentBlock::Markdown {
                text: "粒線體運輸 mechanism".to_owned(),
            },
            ContentBlock::Math {
                latex: "\\alpha + \\beta".to_owned(),
                display: false,
            },
            ContentBlock::Diagram {
                source: "A --> B".to_owned(),
                syntax: DiagramSyntax::Mermaid,
                svg_cache: None,
                node_bindings: Vec::new(),
            },
            ContentBlock::ImageOcclusion {
                resource_id: ResourceId::generate(),
                masks: vec![OcclusionMask {
                    group_id: "外膜".to_owned(),
                    rect: NormalizedRect {
                        x: 0.0,
                        y: 0.0,
                        w: 0.1,
                        h: 0.1,
                    },
                }],
            },
            ContentBlock::InteractivePins {
                resource_id: ResourceId::generate(),
                overlay_resource_id: None,
                pins: vec![Pin {
                    no: 1,
                    x: 0.2,
                    y: 0.3,
                    label_x: 0.4,
                    label_y: 0.5,
                    term_primary: "鈉幫浦".to_owned(),
                    term_secondary: Some("Na/K ATPase".to_owned()),
                    detail: Some("消耗 ATP".to_owned()),
                    hint: Some("位於膜上".to_owned()),
                    linked_card_id: None,
                }],
            },
        ];

        let text = search_text_for_blocks(&blocks);
        for needle in [
            "粒線體運輸",
            "\\alpha",
            "A --> B",
            "外膜",
            "鈉幫浦",
            "Na/K ATPase",
            "消耗 ATP",
            "位於膜上",
        ] {
            assert!(text.contains(needle), "missing {needle} in {text}");
        }
        for absent in [
            "markdown",
            "math",
            "diagram",
            "image_occlusion",
            "interactive_pins",
            "resource_id",
            "svg_cache",
            "type",
        ] {
            assert!(
                !text.contains(absent),
                "unexpected JSON token {absent} in {text}"
            );
        }
    }
}
