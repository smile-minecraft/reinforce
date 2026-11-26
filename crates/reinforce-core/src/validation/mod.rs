use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::content::{
    Anchor, Edge, ExamSet, KnowledgeCard, Note, QuestionCard, QuestionGroup, Resource,
};
use crate::db::content as store;
use crate::primitives::{
    AnchorId, ExamSetId, KnowledgeCardId, QuestionCardId, QuestionGroupId, ResourceId, Result,
};

pub mod actor;
pub mod answer;
pub mod count;
pub mod grading;
pub mod original;
pub mod reference;

pub use actor::Actor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRule {
    AnswerProvenance,
    OriginalAntiFabrication,
    TypeGradingConsistency,
    CountConservation,
    ReferentialIntegrity,
    VerificationLock,
    TempRef,
    OperationShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    pub op_index: Option<usize>,
    pub temp_ref: Option<String>,
    pub field_path: String,
    pub rule: ValidationRule,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
}

impl ValidationReport {
    pub fn new(errors: Vec<ValidationError>) -> Self {
        Self { errors }
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Staged<T> {
    pub op_index: usize,
    pub temp_ref: Option<String>,
    pub value: T,
}

impl<T> Staged<T> {
    pub fn error(
        &self,
        rule: ValidationRule,
        field_path: impl Into<String>,
        message: impl Into<String>,
    ) -> ValidationError {
        ValidationError {
            op_index: Some(self.op_index),
            temp_ref: self.temp_ref.clone(),
            field_path: field_path.into(),
            rule,
            message: message.into(),
        }
    }
}

/// Full replacement snapshot: every staged entity is written as a whole on
/// approval, never merged field by field.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub actor: Actor,
    pub resources: Vec<Staged<Resource>>,
    pub anchors: Vec<Staged<Anchor>>,
    pub knowledge_cards: Vec<Staged<KnowledgeCard>>,
    pub notes: Vec<Staged<Note>>,
    pub exam_sets: Vec<Staged<ExamSet>>,
    pub question_groups: Vec<Staged<QuestionGroup>>,
    pub question_cards: Vec<Staged<QuestionCard>>,
    pub edges: Vec<Staged<Edge>>,
}

impl Snapshot {
    pub fn new(actor: Actor) -> Self {
        Self {
            actor,
            resources: Vec::new(),
            anchors: Vec::new(),
            knowledge_cards: Vec::new(),
            notes: Vec::new(),
            exam_sets: Vec::new(),
            question_groups: Vec::new(),
            question_cards: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
            && self.anchors.is_empty()
            && self.knowledge_cards.is_empty()
            && self.notes.is_empty()
            && self.exam_sets.is_empty()
            && self.question_groups.is_empty()
            && self.question_cards.is_empty()
            && self.edges.is_empty()
    }

    pub fn push_resource(&mut self, entry: Staged<Resource>) {
        self.resources.push(entry);
    }

    pub fn push_anchor(&mut self, entry: Staged<Anchor>) {
        self.anchors.push(entry);
    }

    pub fn push_knowledge_card(&mut self, entry: Staged<KnowledgeCard>) {
        self.knowledge_cards.push(entry);
    }

    pub fn push_note(&mut self, entry: Staged<Note>) {
        self.notes.push(entry);
    }

    pub fn push_exam_set(&mut self, entry: Staged<ExamSet>) {
        self.exam_sets.push(entry);
    }

    pub fn push_question_group(&mut self, entry: Staged<QuestionGroup>) {
        self.question_groups.push(entry);
    }

    pub fn push_question_card(&mut self, entry: Staged<QuestionCard>) {
        self.question_cards.push(entry);
    }

    pub fn push_edge(&mut self, entry: Staged<Edge>) {
        self.edges.push(entry);
    }
}

/// Read-only boundary used by validation rules. Staged entities always win
/// over persisted rows because approval replaces the whole entity.
pub trait StoreView {
    fn resource(&self, id: &ResourceId) -> Result<Option<Resource>>;
    fn anchor(&self, id: &AnchorId) -> Result<Option<Anchor>>;
    fn knowledge_card(&self, id: &KnowledgeCardId) -> Result<Option<KnowledgeCard>>;
    fn note(&self, id: &crate::NoteId) -> Result<Option<Note>>;
    fn exam_set(&self, id: &ExamSetId) -> Result<Option<ExamSet>>;
    fn question_group(&self, id: &QuestionGroupId) -> Result<Option<QuestionGroup>>;
    fn question_card(&self, id: &QuestionCardId) -> Result<Option<QuestionCard>>;
    fn question_cards_for_exam_set(&self, id: &ExamSetId) -> Result<Vec<QuestionCard>>;
}

impl StoreView for Connection {
    fn resource(&self, id: &ResourceId) -> Result<Option<Resource>> {
        store::get_resource(self, id)
    }

    fn anchor(&self, id: &AnchorId) -> Result<Option<Anchor>> {
        store::get_anchor(self, id)
    }

    fn knowledge_card(&self, id: &KnowledgeCardId) -> Result<Option<KnowledgeCard>> {
        store::get_knowledge_card(self, id)
    }

    fn note(&self, id: &crate::NoteId) -> Result<Option<Note>> {
        store::get_note(self, id)
    }

    fn exam_set(&self, id: &ExamSetId) -> Result<Option<ExamSet>> {
        store::get_exam_set(self, id)
    }

    fn question_group(&self, id: &QuestionGroupId) -> Result<Option<QuestionGroup>> {
        store::get_question_group(self, id)
    }

    fn question_card(&self, id: &QuestionCardId) -> Result<Option<QuestionCard>> {
        store::get_question_card(self, id)
    }

    fn question_cards_for_exam_set(&self, id: &ExamSetId) -> Result<Vec<QuestionCard>> {
        let mut statement =
            self.prepare("SELECT id FROM question_cards WHERE exam_set_id = ?1 ORDER BY id")?;
        let ids = statement
            .query_map([id], |row| row.get::<_, QuestionCardId>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .filter_map(|id| store::get_question_card(self, &id).transpose())
            .collect()
    }
}

pub(crate) fn staged_resource<'a>(snapshot: &'a Snapshot, id: &ResourceId) -> Option<&'a Resource> {
    snapshot
        .resources
        .iter()
        .find(|entry| entry.value.id == *id)
        .map(|entry| &entry.value)
}

pub(crate) fn resource_exists(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &ResourceId,
) -> Result<bool> {
    Ok(staged_resource(snapshot, id).is_some() || view.resource(id)?.is_some())
}

/// Returns (width, height). A staged resource overrides persisted dimensions
/// even when its own dimensions are absent, because approval replaces it.
pub(crate) fn resource_dimensions(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &ResourceId,
) -> Result<Option<(i64, i64)>> {
    let resource = match staged_resource(snapshot, id) {
        Some(resource) => resource.clone(),
        None => match view.resource(id)? {
            Some(resource) => resource,
            None => return Ok(None),
        },
    };
    Ok(resource.width.zip(resource.height))
}

pub(crate) fn anchor_exists(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &AnchorId,
) -> Result<bool> {
    Ok(snapshot.anchors.iter().any(|entry| entry.value.id == *id) || view.anchor(id)?.is_some())
}

pub(crate) fn knowledge_card_exists(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &KnowledgeCardId,
) -> Result<bool> {
    Ok(snapshot
        .knowledge_cards
        .iter()
        .any(|entry| entry.value.id == *id)
        || view.knowledge_card(id)?.is_some())
}

pub(crate) fn question_card_exists(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &QuestionCardId,
) -> Result<bool> {
    Ok(snapshot
        .question_cards
        .iter()
        .any(|entry| entry.value.id == *id)
        || view.question_card(id)?.is_some())
}

pub(crate) fn question_card_for(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &QuestionCardId,
) -> Result<Option<QuestionCard>> {
    if let Some(entry) = snapshot
        .question_cards
        .iter()
        .find(|entry| entry.value.id == *id)
    {
        return Ok(Some(entry.value.clone()));
    }
    view.question_card(id)
}

pub(crate) fn exam_set_for(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &ExamSetId,
) -> Result<Option<ExamSet>> {
    if let Some(entry) = snapshot
        .exam_sets
        .iter()
        .find(|entry| entry.value.id == *id)
    {
        return Ok(Some(entry.value.clone()));
    }
    view.exam_set(id)
}

pub(crate) fn merged_question_cards_for_exam_set(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    id: &ExamSetId,
) -> Result<Vec<QuestionCard>> {
    let mut cards: std::collections::BTreeMap<String, QuestionCard> = view
        .question_cards_for_exam_set(id)?
        .into_iter()
        .map(|card| (card.id.as_str(), card))
        .collect();
    for entry in &snapshot.question_cards {
        if entry.value.exam_set_id.as_ref() == Some(id) {
            cards.insert(entry.value.id.as_str(), entry.value.clone());
        } else {
            cards.remove(&entry.value.id.as_str());
        }
    }
    Ok(cards.into_values().collect())
}

/// Runs every hard gate and returns all violations instead of stopping at the
/// first one, so a submitting agent can fix the whole batch in one round trip.
pub fn validate_snapshot(snapshot: &Snapshot, view: &dyn StoreView) -> Result<ValidationReport> {
    let mut errors = Vec::new();
    errors.extend(answer::validate(snapshot, view)?);
    errors.extend(original::validate(snapshot, view)?);
    errors.extend(grading::validate(snapshot)?);
    errors.extend(count::validate(snapshot, view)?);
    errors.extend(reference::validate(snapshot, view)?);
    Ok(ValidationReport::new(errors))
}
