//! Offline catalogue and graph reads.
//!
//! Native clients browse the same SQLite store the server owns, so these
//! methods answer the library questions (subjects, notes, exam sets, resolved
//! outlines) and the typed-edge traversal topic drills need, without any HTTP
//! call. Every type here is FFI-only: `reinforce-core` stays UniFFI-free.

use std::collections::HashMap;

use reinforce_core::content::{
    EdgeEndpoint as CoreEdgeEndpoint, GraphNode as CoreGraphNode, OutlineNode as CoreOutlineNode,
};
use reinforce_core::db::{
    catalog as catalog_store, quiz as quiz_store, search as search_store,
    subject_groups as subject_groups_store,
};
use reinforce_core::primitives::CardStatus;
use reinforce_core::{ExamSetId, KnowledgeCard, KnowledgeCardId, NoteId, SubjectGroupId};

use crate::database::LocalDatabase;
use crate::error::{FfiError, guard, locked, parse_id, usize_field};
use crate::types::{
    CardStatus as FfiCardStatus, ExamSet, KnowledgeCard as FfiKnowledgeCard, SubjectGroup,
};
use crate::types::{Note, QuestionCard, QuestionGroup, Verification};

/// Published-content totals for one subject.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SubjectSummary {
    pub subject: String,
    /// Published notes of this subject.
    pub note_count: u32,
    /// Published knowledge cards of this subject.
    pub knowledge_card_count: u32,
    /// Exam sets of this subject; exam sets carry no status of their own.
    pub exam_set_count: u32,
    /// Published question cards of this subject.
    pub question_card_count: u32,
}

impl From<catalog_store::SubjectSummary> for SubjectSummary {
    fn from(value: catalog_store::SubjectSummary) -> Self {
        Self {
            subject: value.subject,
            note_count: value.note_count,
            knowledge_card_count: value.knowledge_card_count,
            exam_set_count: value.exam_set_count,
            question_card_count: value.question_card_count,
        }
    }
}

/// An exam set with the size of its question bank.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ExamSetSummary {
    pub exam_set: ExamSet,
    pub question_count: u32,
    /// Question cards of the set that learners may drill.
    pub published_question_count: u32,
}

impl From<catalog_store::ExamSetSummary> for ExamSetSummary {
    fn from(value: catalog_store::ExamSetSummary) -> Self {
        Self {
            exam_set: value.exam_set.into(),
            question_count: value.question_count,
            published_question_count: value.published_question_count,
        }
    }
}

/// The fields a library row needs from a knowledge card.
#[derive(Debug, Clone, uniffi::Record)]
pub struct KnowledgeCardSummary {
    pub id: String,
    pub subject: String,
    pub topics: Vec<String>,
    pub title: String,
    pub status: FfiCardStatus,
    pub verification: Verification,
}

impl From<KnowledgeCard> for KnowledgeCardSummary {
    fn from(value: KnowledgeCard) -> Self {
        Self {
            id: value.id.as_str(),
            subject: value.subject,
            topics: value.topics,
            title: value.title,
            status: value.status.into(),
            verification: value.verification.into(),
        }
    }
}

/// One node of a note outline with its card references resolved.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum NoteOutlineNode {
    Section {
        heading: String,
        intro_markdown: Option<String>,
        children: Vec<NoteOutlineNode>,
    },
    Card {
        card: KnowledgeCardSummary,
    },
}

/// A graph endpoint, matching the server's `card`, `question` and `option`
/// kinds.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum GraphEndpoint {
    Card { id: String },
    Question { id: String },
    Option { question_id: String, key: String },
}

impl GraphEndpoint {
    fn into_core(self) -> Result<CoreEdgeEndpoint, FfiError> {
        Ok(match self {
            Self::Card { id } => CoreEdgeEndpoint::Card {
                id: parse_id("knowledge card id", &id)?,
            },
            Self::Question { id } => CoreEdgeEndpoint::Question {
                id: parse_id("question card id", &id)?,
            },
            Self::Option { question_id, key } => {
                if key.trim().is_empty() {
                    return Err(FfiError::invalid("option key", "key must not be empty"));
                }
                CoreEdgeEndpoint::Option {
                    id: parse_id("question card id", &question_id)?,
                    key,
                }
            }
        })
    }
}

impl From<CoreEdgeEndpoint> for GraphEndpoint {
    fn from(value: CoreEdgeEndpoint) -> Self {
        match value {
            CoreEdgeEndpoint::Card { id } => Self::Card { id: id.as_str() },
            CoreEdgeEndpoint::Question { id } => Self::Question { id: id.as_str() },
            CoreEdgeEndpoint::Option { id, key } => Self::Option {
                question_id: id.as_str(),
                key,
            },
        }
    }
}

/// A neighbour reached through typed edges, with its shortest distance.
#[derive(Debug, Clone, uniffi::Record)]
pub struct GraphNeighbor {
    pub endpoint: GraphEndpoint,
    pub depth: u32,
}

impl From<CoreGraphNode> for GraphNeighbor {
    fn from(value: CoreGraphNode) -> Self {
        Self {
            endpoint: value.endpoint.into(),
            depth: value.depth,
        }
    }
}

/// Resolves an outline tree against the cards that survived the import.
fn resolve_outline(
    nodes: &[CoreOutlineNode],
    cards: &HashMap<KnowledgeCardId, KnowledgeCardSummary>,
) -> Result<Vec<NoteOutlineNode>, FfiError> {
    let mut resolved = Vec::with_capacity(nodes.len());
    for node in nodes {
        resolved.push(match node {
            CoreOutlineNode::Section {
                heading,
                intro_markdown,
                children,
            } => NoteOutlineNode::Section {
                heading: heading.clone(),
                intro_markdown: intro_markdown.clone(),
                children: resolve_outline(children, cards)?,
            },
            CoreOutlineNode::CardRef { card_id } => NoteOutlineNode::Card {
                card: cards
                    .get(card_id)
                    .cloned()
                    .ok_or_else(|| FfiError::Internal {
                        message: format!("outline card {card_id} vanished during resolution"),
                    })?,
            },
        });
    }
    Ok(resolved)
}

#[allow(unsafe_code)]
#[uniffi::export]
impl LocalDatabase {
    /// Lists every subject with its published content counts.
    pub fn subjects(&self) -> Result<Vec<SubjectSummary>, FfiError> {
        guard("list subjects", || {
            let database = locked("database", &self.inner)?;
            Ok(catalog_store::list_subjects(database.connection())?
                .into_iter()
                .map(SubjectSummary::from)
                .collect())
        })
    }

    /// Lists published notes, optionally narrowed to one subject.
    pub fn notes(&self, subject: Option<String>, limit: u32) -> Result<Vec<Note>, FfiError> {
        guard("list notes", || {
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(catalog_store::list_notes(
                database.connection(),
                subject.as_deref(),
                Some(CardStatus::Published),
                limit,
            )?
            .into_iter()
            .map(Note::from)
            .collect())
        })
    }

    /// Lists published knowledge cards, optionally narrowed to one subject.
    pub fn knowledge_cards(
        &self,
        subject: Option<String>,
        limit: u32,
    ) -> Result<Vec<FfiKnowledgeCard>, FfiError> {
        guard("list knowledge cards", || {
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(catalog_store::list_knowledge_cards(
                database.connection(),
                subject.as_deref(),
                Some(CardStatus::Published),
                limit,
            )?
            .into_iter()
            .map(FfiKnowledgeCard::from)
            .collect())
        })
    }

    /// Lists exam sets with their question counts, optionally for one subject.
    pub fn exam_sets(
        &self,
        subject: Option<String>,
        limit: u32,
    ) -> Result<Vec<ExamSetSummary>, FfiError> {
        guard("list exam sets", || {
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(
                catalog_store::list_exam_sets(database.connection(), subject.as_deref(), limit)?
                    .into_iter()
                    .map(ExamSetSummary::from)
                    .collect(),
            )
        })
    }

    /// Resolves a note outline into sections and card summaries.
    ///
    /// Card references keep their first-appearance order and are resolved even
    /// when the card is not published, because the note is the unit being read.
    /// A reference without a stored card is a data error and fails with
    /// `NotFound`.
    pub fn note_outline(&self, note_id: String) -> Result<Vec<NoteOutlineNode>, FfiError> {
        guard("read note outline", || {
            let note_id: NoteId = parse_id("note id", &note_id)?;
            let database = locked("database", &self.inner)?;
            let outline = catalog_store::note_outline(database.connection(), &note_id)?
                .ok_or_else(|| FfiError::missing("note does not exist"))?;
            if let Some(missing) = outline.missing_card_ids.first() {
                return Err(FfiError::missing(format!(
                    "note outline references missing knowledge card {missing}"
                )));
            }
            let cards: HashMap<KnowledgeCardId, KnowledgeCardSummary> = outline
                .cards
                .into_iter()
                .map(|card| (card.id.clone(), KnowledgeCardSummary::from(card)))
                .collect();
            resolve_outline(&outline.note.outline, &cards)
        })
    }

    /// Lists the question groups of one exam set.
    pub fn question_groups(
        &self,
        exam_set_id: String,
        limit: u32,
    ) -> Result<Vec<QuestionGroup>, FfiError> {
        guard("list question groups", || {
            let exam_set_id: ExamSetId = parse_id("exam set id", &exam_set_id)?;
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(
                catalog_store::list_question_groups(database.connection(), &exam_set_id, limit)?
                    .into_iter()
                    .map(QuestionGroup::from)
                    .collect(),
            )
        })
    }

    /// Lists the question cards of one exam set in question-number order.
    pub fn question_cards(
        &self,
        exam_set_id: String,
        limit: u32,
    ) -> Result<Vec<QuestionCard>, FfiError> {
        guard("list question cards", || {
            let exam_set_id: ExamSetId = parse_id("exam set id", &exam_set_id)?;
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(catalog_store::list_question_cards(
                database.connection(),
                &exam_set_id,
                None,
                limit,
            )?
            .into_iter()
            .map(QuestionCard::from)
            .collect())
        })
    }

    /// Lists nested subject folders with their directly assigned subjects.
    pub fn subject_groups(&self) -> Result<Vec<SubjectGroup>, FfiError> {
        guard("list subject groups", || {
            let database = locked("database", &self.inner)?;
            Ok(
                subject_groups_store::list_subject_groups(database.connection())?
                    .into_iter()
                    .map(SubjectGroup::from)
                    .collect(),
            )
        })
    }

    /// Every subject of one group and of its descendants, deduplicated and
    /// sorted; a missing group is an invalid input rather than an empty scope.
    pub fn resolve_group_subjects(&self, group_id: String) -> Result<Vec<String>, FfiError> {
        guard("resolve subject group subjects", || {
            let group_id: SubjectGroupId = parse_id("subject group id", &group_id)?;
            let database = locked("database", &self.inner)?;
            Ok(subject_groups_store::resolve_group_subjects(
                database.connection(),
                &group_id,
            )?)
        })
    }

    /// Question ids linked to the given knowledge cards through `tests` edges,
    /// in id order and limited the same way the server route limits them.
    pub fn topic_question_ids(
        &self,
        card_ids: Vec<String>,
        limit: u32,
    ) -> Result<Vec<String>, FfiError> {
        guard("list topic question ids", || {
            let mut ids = Vec::with_capacity(card_ids.len());
            for id in &card_ids {
                ids.push(parse_id::<KnowledgeCardId>("knowledge card id", id)?);
            }
            let limit = usize_field("limit", limit)?;
            let database = locked("database", &self.inner)?;
            Ok(
                quiz_store::topic_question_ids(database.connection(), &ids, limit)?
                    .into_iter()
                    .map(|id| id.as_str())
                    .collect(),
            )
        })
    }

    /// Walks typed edges from one endpoint.
    ///
    /// This mirrors `/api/graph/{kind}/{id}/neighbors`: every edge type is
    /// followed in both directions, each node is returned once at its shortest
    /// depth, and `depth` must be between 1 and 16. The local store belongs to
    /// the device owner, so unlike the HTTP route there is no visibility
    /// filtering and an unknown endpoint yields an empty list.
    pub fn neighbors(
        &self,
        endpoint: GraphEndpoint,
        depth: u32,
    ) -> Result<Vec<GraphNeighbor>, FfiError> {
        guard("read graph neighbors", || {
            if depth == 0 || depth > search_store::MAX_GRAPH_DEPTH {
                return Err(FfiError::invalid(
                    "depth",
                    format!("must be between 1 and {}", search_store::MAX_GRAPH_DEPTH),
                ));
            }
            let endpoint = endpoint.into_core()?;
            let database = locked("database", &self.inner)?;
            Ok(
                search_store::graph_neighbors(database.connection(), &endpoint, depth)?
                    .into_iter()
                    .map(GraphNeighbor::from)
                    .collect(),
            )
        })
    }
}
