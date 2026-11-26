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
    pub status: CardStatus,
    pub verification: Verification,
    pub unsure_count: u32,
    pub metadata: Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
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
