use std::collections::{BTreeSet, HashMap};

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::content::{
    Anchor, Edge, ExamSet, KnowledgeCard, Note, Proposal, ProposalTarget, QuestionCard,
    QuestionGroup, Resource,
};
use crate::db::content as store;
use crate::hash::ContentHash;
use crate::primitives::{Error, Result, Verification};
use crate::validation::{
    Actor, Snapshot, Staged, ValidationError, ValidationReport, ValidationRule,
};
use crate::{
    AnchorId, EdgeId, ExamSetId, KnowledgeCardId, NoteId, QuestionCardId, QuestionGroupId,
    ResourceId,
};

pub const BATCH_FORMAT: &str = "reinforce_batch_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Resource,
    Anchor,
    KnowledgeCard,
    Note,
    ExamSet,
    QuestionGroup,
    QuestionCard,
    Edge,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Anchor => "anchor",
            Self::KnowledgeCard => "knowledge_card",
            Self::Note => "note",
            Self::ExamSet => "exam_set",
            Self::QuestionGroup => "question_group",
            Self::QuestionCard => "question_card",
            Self::Edge => "edge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchOperation {
    Resource {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    Anchor {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    KnowledgeCard {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    Note {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    ExamSet {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    QuestionGroup {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    QuestionCard {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
    Edge {
        #[serde(default)]
        temp_ref: Option<String>,
        data: Value,
    },
}

impl BatchOperation {
    pub fn new(kind: OperationKind, temp_ref: Option<String>, data: Value) -> Self {
        match kind {
            OperationKind::Resource => Self::Resource { temp_ref, data },
            OperationKind::Anchor => Self::Anchor { temp_ref, data },
            OperationKind::KnowledgeCard => Self::KnowledgeCard { temp_ref, data },
            OperationKind::Note => Self::Note { temp_ref, data },
            OperationKind::ExamSet => Self::ExamSet { temp_ref, data },
            OperationKind::QuestionGroup => Self::QuestionGroup { temp_ref, data },
            OperationKind::QuestionCard => Self::QuestionCard { temp_ref, data },
            OperationKind::Edge => Self::Edge { temp_ref, data },
        }
    }

    pub fn from_entity<T: Serialize>(
        kind: OperationKind,
        temp_ref: Option<String>,
        entity: &T,
    ) -> Result<Self> {
        Ok(Self::new(kind, temp_ref, serde_json::to_value(entity)?))
    }

    pub fn kind(&self) -> OperationKind {
        match self {
            Self::Resource { .. } => OperationKind::Resource,
            Self::Anchor { .. } => OperationKind::Anchor,
            Self::KnowledgeCard { .. } => OperationKind::KnowledgeCard,
            Self::Note { .. } => OperationKind::Note,
            Self::ExamSet { .. } => OperationKind::ExamSet,
            Self::QuestionGroup { .. } => OperationKind::QuestionGroup,
            Self::QuestionCard { .. } => OperationKind::QuestionCard,
            Self::Edge { .. } => OperationKind::Edge,
        }
    }

    pub fn temp_ref(&self) -> Option<&str> {
        match self {
            Self::Resource { temp_ref, .. }
            | Self::Anchor { temp_ref, .. }
            | Self::KnowledgeCard { temp_ref, .. }
            | Self::Note { temp_ref, .. }
            | Self::ExamSet { temp_ref, .. }
            | Self::QuestionGroup { temp_ref, .. }
            | Self::QuestionCard { temp_ref, .. }
            | Self::Edge { temp_ref, .. } => temp_ref.as_deref(),
        }
    }

    pub fn data(&self) -> &Value {
        match self {
            Self::Resource { data, .. }
            | Self::Anchor { data, .. }
            | Self::KnowledgeCard { data, .. }
            | Self::Note { data, .. }
            | Self::ExamSet { data, .. }
            | Self::QuestionGroup { data, .. }
            | Self::QuestionCard { data, .. }
            | Self::Edge { data, .. } => data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredOperation {
    pub op: OperationKind,
    pub temp_ref: Option<String>,
    pub id: String,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchPayload {
    pub format: String,
    pub actor: Actor,
    pub operations: Vec<StoredOperation>,
}

#[derive(Debug, Clone)]
struct PreparedOperation {
    index: usize,
    op: OperationKind,
    temp_ref: Option<String>,
    data: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedBatch {
    pub payload: BatchPayload,
    pub snapshot: Snapshot,
}

pub enum Preparation {
    Ready(Box<PreparedBatch>),
    Invalid(ValidationReport),
}

fn operation_error(
    index: usize,
    temp_ref: Option<String>,
    field_path: impl Into<String>,
    rule: ValidationRule,
    message: impl Into<String>,
) -> ValidationError {
    ValidationError {
        op_index: Some(index),
        temp_ref,
        field_path: field_path.into(),
        rule,
        message: message.into(),
    }
}

fn generate_id(connection: &Connection, op: OperationKind, data: &Value) -> Result<String> {
    if op == OperationKind::Resource {
        if let Some(hash_text) = data.get("sha256").and_then(Value::as_str) {
            if let Ok(hash) = hash_text.parse::<ContentHash>() {
                if let Some(existing) = store::find_resource_by_hash(connection, &hash)? {
                    return Ok(existing.as_str());
                }
            }
        }
    }
    Ok(match op {
        OperationKind::Resource => ResourceId::generate().as_str(),
        OperationKind::Anchor => AnchorId::generate().as_str(),
        OperationKind::KnowledgeCard => KnowledgeCardId::generate().as_str(),
        OperationKind::Note => NoteId::generate().as_str(),
        OperationKind::ExamSet => ExamSetId::generate().as_str(),
        OperationKind::QuestionGroup => QuestionGroupId::generate().as_str(),
        OperationKind::QuestionCard => QuestionCardId::generate().as_str(),
        OperationKind::Edge => EdgeId::generate().as_str(),
    })
}

fn is_uuid_v7(value: &str) -> bool {
    value.parse::<ResourceId>().is_ok()
}

fn resolve_refs(
    value: &mut Value,
    refs: &HashMap<String, String>,
    path: &str,
    index: usize,
    temp_ref: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Value::String(text) if text.starts_with('$') => match refs.get(text) {
            Some(id) => *text = id.clone(),
            None => errors.push(operation_error(
                index,
                temp_ref.map(str::to_owned),
                path,
                ValidationRule::TempRef,
                format!("unknown temp_ref {text}"),
            )),
        },
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                resolve_refs(
                    child,
                    refs,
                    &format!("{path}.{key}"),
                    index,
                    temp_ref,
                    errors,
                );
            }
        }
        Value::Array(items) => {
            for (position, child) in items.iter_mut().enumerate() {
                resolve_refs(
                    child,
                    refs,
                    &format!("{path}[{position}]"),
                    index,
                    temp_ref,
                    errors,
                );
            }
        }
        _ => {}
    }
}

fn decode<T>(
    data: &Value,
    index: usize,
    temp_ref: Option<String>,
    errors: &mut Vec<ValidationError>,
) -> Option<Staged<T>>
where
    T: DeserializeOwned,
{
    match serde_json::from_value::<T>(data.clone()) {
        Ok(value) => Some(Staged {
            op_index: index,
            temp_ref,
            value,
        }),
        Err(error) => {
            errors.push(operation_error(
                index,
                temp_ref,
                "data",
                ValidationRule::OperationShape,
                format!("cannot decode operation data: {error}"),
            ));
            None
        }
    }
}

/// Resolves temp_ref aliases to permanent UUIDv7 ids, coerces verification
/// for non-owners, and builds the replacement snapshot. Nothing is written.
pub fn prepare(
    connection: &Connection,
    actor: Actor,
    operations: Vec<BatchOperation>,
) -> Result<Preparation> {
    let mut errors = Vec::new();
    if operations.is_empty() {
        errors.push(ValidationError {
            op_index: None,
            temp_ref: None,
            field_path: "operations".to_owned(),
            rule: ValidationRule::OperationShape,
            message: "a batch must contain at least one operation".to_owned(),
        });
        return Ok(Preparation::Invalid(ValidationReport::new(errors)));
    }

    let mut seen_refs = BTreeSet::new();
    let mut prepared = Vec::with_capacity(operations.len());
    for (index, operation) in operations.into_iter().enumerate() {
        let temp_ref = operation.temp_ref().map(str::to_owned);
        if let Some(reference) = &temp_ref {
            if reference.len() < 2 || !reference.starts_with('$') {
                errors.push(operation_error(
                    index,
                    temp_ref.clone(),
                    "temp_ref",
                    ValidationRule::TempRef,
                    "temp_ref must start with $ and contain a name",
                ));
            } else if !seen_refs.insert(reference.clone()) {
                errors.push(operation_error(
                    index,
                    temp_ref.clone(),
                    "temp_ref",
                    ValidationRule::TempRef,
                    format!("duplicate temp_ref {reference}"),
                ));
            }
        }
        let data = operation.data().clone();
        if !data.is_object() {
            errors.push(operation_error(
                index,
                temp_ref.clone(),
                "data",
                ValidationRule::OperationShape,
                "operation data must be an object",
            ));
        }
        prepared.push(PreparedOperation {
            index,
            op: operation.kind(),
            temp_ref,
            data,
        });
    }
    if !errors.is_empty() {
        return Ok(Preparation::Invalid(ValidationReport::new(errors)));
    }

    let mut refs = HashMap::new();
    for item in &mut prepared {
        let explicit_id = item.data.get("id").and_then(Value::as_str);
        let id = match explicit_id {
            Some(value) if value.starts_with('$') => generate_id(connection, item.op, &item.data)?,
            Some(value) if is_uuid_v7(value) => value.to_owned(),
            Some(value) => {
                errors.push(operation_error(
                    item.index,
                    item.temp_ref.clone(),
                    "data.id",
                    ValidationRule::OperationShape,
                    format!("invalid UUIDv7 id {value}"),
                ));
                generate_id(connection, item.op, &item.data)?
            }
            None => generate_id(connection, item.op, &item.data)?,
        };
        item.data["id"] = Value::String(id.clone());
        if let Some(reference) = &item.temp_ref {
            refs.insert(reference.clone(), id);
        }
    }

    for item in &mut prepared {
        resolve_refs(
            &mut item.data,
            &refs,
            "data",
            item.index,
            item.temp_ref.as_deref(),
            &mut errors,
        );
    }
    if !errors.is_empty() {
        return Ok(Preparation::Invalid(ValidationReport::new(errors)));
    }

    let mut snapshot = Snapshot::new(actor.clone());
    let mut stored_operations = Vec::with_capacity(prepared.len());
    for item in &prepared {
        let id = item
            .data
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut data = item.data.clone();
        match item.op {
            OperationKind::Resource => {
                if let Some(entry) =
                    decode::<Resource>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    snapshot.push_resource(entry);
                }
            }
            OperationKind::Anchor => {
                if let Some(entry) =
                    decode::<Anchor>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    snapshot.push_anchor(entry);
                }
            }
            OperationKind::KnowledgeCard => {
                if let Some(mut entry) =
                    decode::<KnowledgeCard>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    if !actor.is_owner() {
                        entry.value.verification = Verification::Unchecked;
                    }
                    if let Ok(encoded) = serde_json::to_value(&entry.value) {
                        data = encoded;
                    }
                    snapshot.push_knowledge_card(entry);
                }
            }
            OperationKind::Note => {
                if let Some(entry) =
                    decode::<Note>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    snapshot.push_note(entry);
                }
            }
            OperationKind::ExamSet => {
                if let Some(entry) =
                    decode::<ExamSet>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    snapshot.push_exam_set(entry);
                }
            }
            OperationKind::QuestionGroup => {
                if let Some(entry) =
                    decode::<QuestionGroup>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    snapshot.push_question_group(entry);
                }
            }
            OperationKind::QuestionCard => {
                if let Some(mut entry) =
                    decode::<QuestionCard>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    if !actor.is_owner() {
                        entry.value.verification = Verification::Unchecked;
                    }
                    if let Ok(encoded) = serde_json::to_value(&entry.value) {
                        data = encoded;
                    }
                    snapshot.push_question_card(entry);
                }
            }
            OperationKind::Edge => {
                if let Some(entry) =
                    decode::<Edge>(&data, item.index, item.temp_ref.clone(), &mut errors)
                {
                    snapshot.push_edge(entry);
                }
            }
        }
        stored_operations.push(StoredOperation {
            op: item.op,
            temp_ref: item.temp_ref.clone(),
            id,
            data,
        });
    }
    if !errors.is_empty() {
        return Ok(Preparation::Invalid(ValidationReport::new(errors)));
    }

    Ok(Preparation::Ready(Box::new(PreparedBatch {
        payload: BatchPayload {
            format: BATCH_FORMAT.to_owned(),
            actor,
            operations: stored_operations,
        },
        snapshot,
    })))
}

pub fn batch_from_proposal(proposal: &Proposal) -> Result<BatchPayload> {
    let payload: BatchPayload = serde_json::from_value(proposal.payload.clone())?;
    if payload.format != BATCH_FORMAT {
        return Err(Error::InvalidValue(
            "proposal payload",
            "not a proposal batch".to_owned(),
        ));
    }
    Ok(payload)
}

pub fn snapshot_from_payload(payload: &BatchPayload) -> Result<Snapshot> {
    let mut snapshot = Snapshot::new(payload.actor.clone());
    let mut errors = Vec::new();
    for (index, operation) in payload.operations.iter().enumerate() {
        let temp_ref = operation.temp_ref.clone();
        match operation.op {
            OperationKind::Resource => {
                if let Some(entry) =
                    decode::<Resource>(&operation.data, index, temp_ref, &mut errors)
                {
                    snapshot.push_resource(entry);
                }
            }
            OperationKind::Anchor => {
                if let Some(entry) = decode::<Anchor>(&operation.data, index, temp_ref, &mut errors)
                {
                    snapshot.push_anchor(entry);
                }
            }
            OperationKind::KnowledgeCard => {
                if let Some(entry) =
                    decode::<KnowledgeCard>(&operation.data, index, temp_ref, &mut errors)
                {
                    snapshot.push_knowledge_card(entry);
                }
            }
            OperationKind::Note => {
                if let Some(entry) = decode::<Note>(&operation.data, index, temp_ref, &mut errors) {
                    snapshot.push_note(entry);
                }
            }
            OperationKind::ExamSet => {
                if let Some(entry) =
                    decode::<ExamSet>(&operation.data, index, temp_ref, &mut errors)
                {
                    snapshot.push_exam_set(entry);
                }
            }
            OperationKind::QuestionGroup => {
                if let Some(entry) =
                    decode::<QuestionGroup>(&operation.data, index, temp_ref, &mut errors)
                {
                    snapshot.push_question_group(entry);
                }
            }
            OperationKind::QuestionCard => {
                if let Some(entry) =
                    decode::<QuestionCard>(&operation.data, index, temp_ref, &mut errors)
                {
                    snapshot.push_question_card(entry);
                }
            }
            OperationKind::Edge => {
                if let Some(entry) = decode::<Edge>(&operation.data, index, temp_ref, &mut errors) {
                    snapshot.push_edge(entry);
                }
            }
        }
    }
    if let Some(error) = errors.first() {
        return Err(Error::InvalidValue(
            "proposal payload",
            format!("{}: {}", error.field_path, error.message),
        ));
    }
    Ok(snapshot)
}

pub fn batch_target(payload: &BatchPayload) -> Result<ProposalTarget> {
    let first = payload
        .operations
        .first()
        .ok_or_else(|| Error::InvalidValue("proposal batch", "no operations".to_owned()))?;
    Ok(match first.op {
        OperationKind::Resource => ProposalTarget::Resource(first.id.parse()?),
        OperationKind::Anchor => {
            let resource_id = first
                .data
                .get("resource_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::InvalidValue("anchor operation", "resource_id is missing".to_owned())
                })?;
            ProposalTarget::Resource(resource_id.parse()?)
        }
        OperationKind::KnowledgeCard => ProposalTarget::KnowledgeCard(first.id.parse()?),
        OperationKind::Note => ProposalTarget::Note(first.id.parse()?),
        OperationKind::ExamSet => ProposalTarget::ExamSet(first.id.parse()?),
        OperationKind::QuestionGroup => ProposalTarget::QuestionGroup(first.id.parse()?),
        OperationKind::QuestionCard => ProposalTarget::QuestionCard(first.id.parse()?),
        OperationKind::Edge => ProposalTarget::Edge(first.id.parse()?),
    })
}

pub fn submitted_by(actor: &Actor) -> String {
    actor.as_str()
}
