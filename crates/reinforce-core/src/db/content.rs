use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::content::{
    Anchor, Edge, EdgeEndpoint, ExamSet, KnowledgeCard, Note, Proposal, ProposalTarget,
    QuestionCard, QuestionGroup, Resource, search_text_for_knowledge_card,
    search_text_for_question_card,
};
use crate::hash::ContentHash;
use crate::primitives::{
    AnchorKind, CardStatus, EndpointKind, Error, ExportPolicy, ProposalStatus, ProposalTargetKind,
    ResourceKind, Result, Sensitivity, Verification,
};
use crate::timestamp::Timestamp;
use crate::{
    AnchorId, EdgeId, ExamSetId, KnowledgeCardId, NoteId, ProposalId, QuestionCardId,
    QuestionGroupId, ResourceId,
};

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T> {
    Ok(serde_json::from_str(value)?)
}

/// Stores a resource, returning the id of the existing row when the same
/// sha256 was already ingested.
pub fn store_resource(connection: &Connection, resource: &Resource) -> Result<ResourceId> {
    if let Some(existing) = find_resource_by_hash(connection, &resource.sha256)? {
        connection.execute(
            "UPDATE resources SET kind = ?2, title = ?3, byte_size = ?4, page_count = ?5,
                    width = ?6, height = ?7, license = ?8, attribution_url = ?9,
                    attribution_notice = ?10, sensitivity = ?11, export_policy = ?12
             WHERE id = ?1",
            params![
                existing,
                resource.kind,
                resource.title,
                resource.byte_size,
                resource.page_count,
                resource.width,
                resource.height,
                resource.license,
                resource.attribution_url,
                resource.attribution_notice,
                resource.sensitivity,
                resource.export_policy,
            ],
        )?;
        return Ok(existing);
    }
    connection.execute(
        "INSERT INTO resources (
            id, kind, title, sha256, byte_size, page_count, width, height, license,
            attribution_url, attribution_notice, sensitivity, export_policy, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            resource.id,
            resource.kind,
            resource.title,
            resource.sha256,
            resource.byte_size,
            resource.page_count,
            resource.width,
            resource.height,
            resource.license,
            resource.attribution_url,
            resource.attribution_notice,
            resource.sensitivity,
            resource.export_policy,
            resource.created_at,
        ],
    )?;
    Ok(resource.id.clone())
}

/// Replaces a resource snapshot by id, including media dimensions.
pub fn upsert_resource(connection: &Connection, resource: &Resource) -> Result<()> {
    connection.execute(
        "INSERT INTO resources (
            id, kind, title, sha256, byte_size, page_count, width, height, license,
            attribution_url, attribution_notice, sensitivity, export_policy, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ON CONFLICT(id) DO UPDATE SET
            kind = excluded.kind, title = excluded.title, sha256 = excluded.sha256,
            byte_size = excluded.byte_size, page_count = excluded.page_count,
            width = excluded.width, height = excluded.height, license = excluded.license,
            attribution_url = excluded.attribution_url,
            attribution_notice = excluded.attribution_notice,
            sensitivity = excluded.sensitivity, export_policy = excluded.export_policy,
            created_at = excluded.created_at",
        params![
            resource.id,
            resource.kind,
            resource.title,
            resource.sha256,
            resource.byte_size,
            resource.page_count,
            resource.width,
            resource.height,
            resource.license,
            resource.attribution_url,
            resource.attribution_notice,
            resource.sensitivity,
            resource.export_policy,
            resource.created_at,
        ],
    )?;
    Ok(())
}

pub fn find_resource_by_hash(
    connection: &Connection,
    hash: &ContentHash,
) -> Result<Option<ResourceId>> {
    connection
        .query_row(
            "SELECT id FROM resources WHERE sha256 = ?1",
            params![hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

pub fn get_resource(connection: &Connection, id: &ResourceId) -> Result<Option<Resource>> {
    connection
        .query_row(
            "SELECT id, kind, title, sha256, byte_size, page_count, width, height, license,
                    attribution_url, attribution_notice, sensitivity, export_policy, created_at
             FROM resources WHERE id = ?1",
            params![id],
            |row| {
                Ok(Resource {
                    id: row.get(0)?,
                    kind: row.get::<_, ResourceKind>(1)?,
                    title: row.get(2)?,
                    sha256: row.get(3)?,
                    byte_size: row.get(4)?,
                    page_count: row.get(5)?,
                    width: row.get(6)?,
                    height: row.get(7)?,
                    license: row.get(8)?,
                    attribution_url: row.get(9)?,
                    attribution_notice: row.get(10)?,
                    sensitivity: row.get::<_, Sensitivity>(11)?,
                    export_policy: row.get::<_, ExportPolicy>(12)?,
                    created_at: row.get(13)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_anchor(connection: &Connection, anchor: &Anchor) -> Result<()> {
    let rect = anchor.rect;
    connection.execute(
        "INSERT INTO anchors (
            id, resource_id, anchor_kind, pdf_page, printed_page, legend_pdf_page,
            x, y, w, h, start_ms, end_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            anchor.id,
            anchor.resource_id,
            anchor.anchor_kind,
            anchor.pdf_page,
            anchor.printed_page,
            anchor.legend_pdf_page,
            rect.map(|value| value.x),
            rect.map(|value| value.y),
            rect.map(|value| value.w),
            rect.map(|value| value.h),
            anchor.media_span.map(|value| value.start_ms),
            anchor.media_span.map(|value| value.end_ms),
        ],
    )?;
    Ok(())
}

/// Replaces an anchor snapshot by id.
pub fn upsert_anchor(connection: &Connection, anchor: &Anchor) -> Result<()> {
    let rect = anchor.rect;
    connection.execute(
        "INSERT INTO anchors (
            id, resource_id, anchor_kind, pdf_page, printed_page, legend_pdf_page,
            x, y, w, h, start_ms, end_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(id) DO UPDATE SET
            resource_id = excluded.resource_id, anchor_kind = excluded.anchor_kind,
            pdf_page = excluded.pdf_page, printed_page = excluded.printed_page,
            legend_pdf_page = excluded.legend_pdf_page, x = excluded.x, y = excluded.y,
            w = excluded.w, h = excluded.h, start_ms = excluded.start_ms,
            end_ms = excluded.end_ms",
        params![
            anchor.id,
            anchor.resource_id,
            anchor.anchor_kind,
            anchor.pdf_page,
            anchor.printed_page,
            anchor.legend_pdf_page,
            rect.map(|value| value.x),
            rect.map(|value| value.y),
            rect.map(|value| value.w),
            rect.map(|value| value.h),
            anchor.media_span.map(|value| value.start_ms),
            anchor.media_span.map(|value| value.end_ms),
        ],
    )?;
    Ok(())
}

pub fn get_anchor(connection: &Connection, id: &AnchorId) -> Result<Option<Anchor>> {
    connection
        .query_row(
            "SELECT id, resource_id, anchor_kind, pdf_page, printed_page, legend_pdf_page,
                    x, y, w, h, start_ms, end_ms
             FROM anchors WHERE id = ?1",
            params![id],
            |row| {
                let x: Option<f64> = row.get(6)?;
                let y: Option<f64> = row.get(7)?;
                let w: Option<f64> = row.get(8)?;
                let h: Option<f64> = row.get(9)?;
                let start_ms: Option<i64> = row.get(10)?;
                let end_ms: Option<i64> = row.get(11)?;
                Ok(Anchor {
                    id: row.get(0)?,
                    resource_id: row.get(1)?,
                    anchor_kind: row.get::<_, AnchorKind>(2)?,
                    pdf_page: row.get(3)?,
                    printed_page: row.get(4)?,
                    legend_pdf_page: row.get(5)?,
                    rect: match (x, y, w, h) {
                        (Some(x), Some(y), Some(w), Some(h)) => {
                            Some(crate::content::NormalizedRect { x, y, w, h })
                        }
                        _ => None,
                    },
                    media_span: match (start_ms, end_ms) {
                        (Some(start_ms), Some(end_ms)) => {
                            Some(crate::content::MediaSpan { start_ms, end_ms })
                        }
                        _ => None,
                    },
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_knowledge_card(connection: &Connection, card: &KnowledgeCard) -> Result<()> {
    connection.execute(
        "INSERT INTO knowledge_cards (
            id, subject, topics_json, title, blocks_json, search_text, anchor_ids_json, status,
            verification, unsure_count, metadata_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            card.id,
            card.subject,
            to_json(&card.topics)?,
            card.title,
            to_json(&card.blocks)?,
            search_text_for_knowledge_card(card),
            to_json(&card.anchor_ids)?,
            card.status,
            card.verification,
            card.unsure_count,
            to_json(&card.metadata)?,
            card.created_at,
            card.updated_at,
        ],
    )?;
    Ok(())
}

/// Replaces a knowledge card snapshot by id and keeps FTS metadata in sync.
pub fn upsert_knowledge_card(connection: &Connection, card: &KnowledgeCard) -> Result<()> {
    connection.execute(
        "INSERT INTO knowledge_cards (
            id, subject, topics_json, title, blocks_json, search_text, anchor_ids_json, status,
            verification, unsure_count, metadata_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(id) DO UPDATE SET
            subject = excluded.subject, topics_json = excluded.topics_json,
            title = excluded.title, blocks_json = excluded.blocks_json,
            search_text = excluded.search_text, anchor_ids_json = excluded.anchor_ids_json,
            status = excluded.status, verification = excluded.verification,
            unsure_count = excluded.unsure_count, metadata_json = excluded.metadata_json,
            created_at = excluded.created_at, updated_at = excluded.updated_at",
        params![
            card.id,
            card.subject,
            to_json(&card.topics)?,
            card.title,
            to_json(&card.blocks)?,
            search_text_for_knowledge_card(card),
            to_json(&card.anchor_ids)?,
            card.status,
            card.verification,
            card.unsure_count,
            to_json(&card.metadata)?,
            card.created_at,
            card.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_knowledge_card(
    connection: &Connection,
    id: &KnowledgeCardId,
) -> Result<Option<KnowledgeCard>> {
    connection
        .query_row(
            "SELECT id, subject, topics_json, title, blocks_json, anchor_ids_json, status,
                    verification, unsure_count, metadata_json, created_at, updated_at
             FROM knowledge_cards WHERE id = ?1",
            params![id],
            |row| {
                Ok(KnowledgeCard {
                    id: row.get(0)?,
                    subject: row.get(1)?,
                    topics: from_json(&row.get::<_, String>(2)?)?,
                    title: row.get(3)?,
                    blocks: from_json(&row.get::<_, String>(4)?)?,
                    anchor_ids: from_json(&row.get::<_, String>(5)?)?,
                    status: row.get::<_, CardStatus>(6)?,
                    verification: row.get::<_, Verification>(7)?,
                    unsure_count: row.get::<_, i64>(8)? as u32,
                    metadata: from_json(&row.get::<_, String>(9)?)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_note(connection: &Connection, note: &Note) -> Result<()> {
    connection.execute(
        "INSERT INTO notes (
            id, subject, title, outline_json, view_settings_json, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            note.id,
            note.subject,
            note.title,
            to_json(&note.outline)?,
            to_json(&note.view_settings)?,
            note.status,
            note.created_at,
            note.updated_at,
        ],
    )?;
    Ok(())
}

pub fn upsert_note(connection: &Connection, note: &Note) -> Result<()> {
    connection.execute(
        "INSERT INTO notes (
            id, subject, title, outline_json, view_settings_json, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(id) DO UPDATE SET
            subject = excluded.subject, title = excluded.title,
            outline_json = excluded.outline_json, view_settings_json = excluded.view_settings_json,
            status = excluded.status, created_at = excluded.created_at,
            updated_at = excluded.updated_at",
        params![
            note.id,
            note.subject,
            note.title,
            to_json(&note.outline)?,
            to_json(&note.view_settings)?,
            note.status,
            note.created_at,
            note.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_note(connection: &Connection, id: &NoteId) -> Result<Option<Note>> {
    connection
        .query_row(
            "SELECT id, subject, title, outline_json, view_settings_json, status, created_at, updated_at
             FROM notes WHERE id = ?1",
            params![id],
            |row| {
                Ok(Note {
                    id: row.get(0)?,
                    subject: row.get(1)?,
                    title: row.get(2)?,
                    outline: from_json(&row.get::<_, String>(3)?)?,
                    view_settings: from_json(&row.get::<_, String>(4)?)?,
                    status: row.get::<_, CardStatus>(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_exam_set(connection: &Connection, exam_set: &ExamSet) -> Result<()> {
    connection.execute(
        "INSERT INTO exam_sets (
            id, subject, title, source_kind, exam_year, source_resource_id,
            expected_questions, metadata_json, is_complete
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            exam_set.id,
            exam_set.subject,
            exam_set.title,
            exam_set.source_kind,
            exam_set.exam_year,
            exam_set.source_resource_id,
            exam_set.expected_questions,
            to_json(&exam_set.metadata)?,
            i64::from(exam_set.is_complete),
        ],
    )?;
    Ok(())
}

pub fn upsert_exam_set(connection: &Connection, exam_set: &ExamSet) -> Result<()> {
    connection.execute(
        "INSERT INTO exam_sets (
            id, subject, title, source_kind, exam_year, source_resource_id,
            expected_questions, metadata_json, is_complete
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(id) DO UPDATE SET
            subject = excluded.subject, title = excluded.title,
            source_kind = excluded.source_kind, exam_year = excluded.exam_year,
            source_resource_id = excluded.source_resource_id,
            expected_questions = excluded.expected_questions,
            metadata_json = excluded.metadata_json, is_complete = excluded.is_complete",
        params![
            exam_set.id,
            exam_set.subject,
            exam_set.title,
            exam_set.source_kind,
            exam_set.exam_year,
            exam_set.source_resource_id,
            exam_set.expected_questions,
            to_json(&exam_set.metadata)?,
            i64::from(exam_set.is_complete),
        ],
    )?;
    Ok(())
}

pub fn get_exam_set(connection: &Connection, id: &ExamSetId) -> Result<Option<ExamSet>> {
    connection
        .query_row(
            "SELECT id, subject, title, source_kind, exam_year, source_resource_id,
                    expected_questions, metadata_json, is_complete
             FROM exam_sets WHERE id = ?1",
            params![id],
            |row| {
                Ok(ExamSet {
                    id: row.get(0)?,
                    subject: row.get(1)?,
                    title: row.get(2)?,
                    source_kind: row.get(3)?,
                    exam_year: row.get(4)?,
                    source_resource_id: row.get(5)?,
                    expected_questions: row.get::<_, Option<i64>>(6)?.map(|value| value as u32),
                    metadata: from_json(&row.get::<_, String>(7)?)?,
                    is_complete: row.get::<_, i64>(8)? != 0,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_question_group(connection: &Connection, group: &QuestionGroup) -> Result<()> {
    connection.execute(
        "INSERT INTO question_groups (id, exam_set_id, title, shared_stem_blocks_json)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            group.id,
            group.exam_set_id,
            group.title,
            to_json(&group.shared_stem_blocks)?,
        ],
    )?;
    Ok(())
}

pub fn upsert_question_group(connection: &Connection, group: &QuestionGroup) -> Result<()> {
    connection.execute(
        "INSERT INTO question_groups (id, exam_set_id, title, shared_stem_blocks_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
             exam_set_id = excluded.exam_set_id, title = excluded.title,
             shared_stem_blocks_json = excluded.shared_stem_blocks_json",
        params![
            group.id,
            group.exam_set_id,
            group.title,
            to_json(&group.shared_stem_blocks)?,
        ],
    )?;
    Ok(())
}

pub fn get_question_group(
    connection: &Connection,
    id: &QuestionGroupId,
) -> Result<Option<QuestionGroup>> {
    connection
        .query_row(
            "SELECT id, exam_set_id, title, shared_stem_blocks_json FROM question_groups WHERE id = ?1",
            params![id],
            |row| {
                Ok(QuestionGroup {
                    id: row.get(0)?,
                    exam_set_id: row.get(1)?,
                    title: row.get(2)?,
                    shared_stem_blocks: from_json(&row.get::<_, String>(3)?)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_question_card(connection: &Connection, card: &QuestionCard) -> Result<()> {
    connection.execute(
        "INSERT INTO question_cards (
            id, exam_set_id, question_group_id, question_no, subject, topics_json,
            stem_blocks_json, explanation_blocks_json, search_text, anchor_ids_json, question_type,
            grading_mode, options_json, answer_json, accepted_answers_json, answer_source_kind,
            answer_basis, duplicate_question_id, status, verification, unsure_count, metadata_json,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        params![
            card.id,
            card.exam_set_id,
            card.question_group_id,
            card.question_no,
            card.subject,
            to_json(&card.topics)?,
            to_json(&card.stem_blocks)?,
            to_json(&card.explanation_blocks)?,
            search_text_for_question_card(card),
            to_json(&card.anchor_ids)?,
            card.question_type,
            card.grading_mode,
            to_json(&card.options)?,
            to_json(&card.answer)?,
            to_json(&card.accepted_answers)?,
            card.answer_source_kind,
            card.answer_basis,
            card.duplicate_question_id,
            card.status,
            card.verification,
            card.unsure_count,
            to_json(&card.metadata)?,
            card.created_at,
            card.updated_at,
        ],
    )?;
    Ok(())
}

/// Replaces a question card snapshot by id and keeps FTS metadata in sync.
pub fn upsert_question_card(connection: &Connection, card: &QuestionCard) -> Result<()> {
    connection.execute(
        "INSERT INTO question_cards (
            id, exam_set_id, question_group_id, question_no, subject, topics_json,
            stem_blocks_json, explanation_blocks_json, search_text, anchor_ids_json, question_type,
            grading_mode, options_json, answer_json, accepted_answers_json, answer_source_kind,
            answer_basis, duplicate_question_id, status, verification, unsure_count, metadata_json,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
        ON CONFLICT(id) DO UPDATE SET
            exam_set_id = excluded.exam_set_id, question_group_id = excluded.question_group_id,
            question_no = excluded.question_no, subject = excluded.subject,
            topics_json = excluded.topics_json, stem_blocks_json = excluded.stem_blocks_json,
            explanation_blocks_json = excluded.explanation_blocks_json,
            search_text = excluded.search_text, anchor_ids_json = excluded.anchor_ids_json,
            question_type = excluded.question_type, grading_mode = excluded.grading_mode,
            options_json = excluded.options_json, answer_json = excluded.answer_json,
            accepted_answers_json = excluded.accepted_answers_json,
            answer_source_kind = excluded.answer_source_kind, answer_basis = excluded.answer_basis,
            duplicate_question_id = excluded.duplicate_question_id, status = excluded.status,
            verification = excluded.verification, unsure_count = excluded.unsure_count,
            metadata_json = excluded.metadata_json, created_at = excluded.created_at,
            updated_at = excluded.updated_at",
        params![
            card.id,
            card.exam_set_id,
            card.question_group_id,
            card.question_no,
            card.subject,
            to_json(&card.topics)?,
            to_json(&card.stem_blocks)?,
            to_json(&card.explanation_blocks)?,
            search_text_for_question_card(card),
            to_json(&card.anchor_ids)?,
            card.question_type,
            card.grading_mode,
            to_json(&card.options)?,
            to_json(&card.answer)?,
            to_json(&card.accepted_answers)?,
            card.answer_source_kind,
            card.answer_basis,
            card.duplicate_question_id,
            card.status,
            card.verification,
            card.unsure_count,
            to_json(&card.metadata)?,
            card.created_at,
            card.updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_question_card(
    connection: &Connection,
    id: &QuestionCardId,
) -> Result<Option<QuestionCard>> {
    connection
        .query_row(
            "SELECT id, exam_set_id, question_group_id, question_no, subject, topics_json,
                    stem_blocks_json, explanation_blocks_json, anchor_ids_json, question_type,
                    grading_mode, options_json, answer_json, accepted_answers_json, answer_source_kind,
                    answer_basis, duplicate_question_id, status, verification, unsure_count,
                    metadata_json, created_at, updated_at
             FROM question_cards WHERE id = ?1",
            params![id],
            |row| {
                Ok(QuestionCard {
                    id: row.get(0)?,
                    exam_set_id: row.get(1)?,
                    question_group_id: row.get(2)?,
                    question_no: row.get::<_, Option<i64>>(3)?.map(|value| value as u32),
                    subject: row.get(4)?,
                    topics: from_json(&row.get::<_, String>(5)?)?,
                    stem_blocks: from_json(&row.get::<_, String>(6)?)?,
                    explanation_blocks: from_json(&row.get::<_, String>(7)?)?,
                    anchor_ids: from_json(&row.get::<_, String>(8)?)?,
                    question_type: row.get(9)?,
                    grading_mode: row.get(10)?,
                    options: from_json(&row.get::<_, String>(11)?)?,
                    answer: from_json(&row.get::<_, String>(12)?)?,
                    accepted_answers: from_json(&row.get::<_, String>(13)?)?,
                    answer_source_kind: row.get(14)?,
                    answer_basis: row.get(15)?,
                    duplicate_question_id: row.get(16)?,
                    status: row.get::<_, CardStatus>(17)?,
                    verification: row.get::<_, Verification>(18)?,
                    unsure_count: row.get::<_, i64>(19)? as u32,
                    metadata: from_json(&row.get::<_, String>(20)?)?,
                    created_at: row.get(21)?,
                    updated_at: row.get(22)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_edge(connection: &Connection, edge: &Edge) -> Result<()> {
    connection.execute(
        "INSERT INTO edges (
            id, edge_type, from_kind, from_id, from_option_key, from_key,
            to_kind, to_id, to_option_key, to_key, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            edge.id,
            edge.edge_type,
            edge.from.kind(),
            edge.from.id(),
            edge.from.option_key(),
            edge.from.key(),
            edge.to.kind(),
            edge.to.id(),
            edge.to.option_key(),
            edge.to.key(),
            edge.created_at,
        ],
    )?;
    Ok(())
}

/// Replaces an edge snapshot by id.
pub fn upsert_edge(connection: &Connection, edge: &Edge) -> Result<()> {
    connection.execute(
        "INSERT INTO edges (
            id, edge_type, from_kind, from_id, from_option_key, from_key,
            to_kind, to_id, to_option_key, to_key, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(id) DO UPDATE SET
            edge_type = excluded.edge_type, from_kind = excluded.from_kind,
            from_id = excluded.from_id, from_option_key = excluded.from_option_key,
            from_key = excluded.from_key, to_kind = excluded.to_kind,
            to_id = excluded.to_id, to_option_key = excluded.to_option_key,
            to_key = excluded.to_key, created_at = excluded.created_at",
        params![
            edge.id,
            edge.edge_type,
            edge.from.kind(),
            edge.from.id(),
            edge.from.option_key(),
            edge.from.key(),
            edge.to.kind(),
            edge.to.id(),
            edge.to.option_key(),
            edge.to.key(),
            edge.created_at,
        ],
    )?;
    Ok(())
}

pub fn get_edge(connection: &Connection, id: &EdgeId) -> Result<Option<Edge>> {
    connection
        .query_row(
            "SELECT id, edge_type, from_kind, from_id, from_option_key,
                    to_kind, to_id, to_option_key, created_at
             FROM edges WHERE id = ?1",
            params![id],
            |row| {
                let from_kind: EndpointKind = row.get(2)?;
                let to_kind: EndpointKind = row.get(5)?;
                Ok(Edge {
                    id: row.get(0)?,
                    edge_type: row.get(1)?,
                    from: EdgeEndpoint::from_db_parts(
                        from_kind,
                        &row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?.as_deref(),
                    )?,
                    to: EdgeEndpoint::from_db_parts(
                        to_kind,
                        &row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?.as_deref(),
                    )?,
                    created_at: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn proposal_target(kind: ProposalTargetKind, id: &str) -> Result<ProposalTarget> {
    Ok(match kind {
        ProposalTargetKind::Resource => ProposalTarget::Resource(id.parse()?),
        ProposalTargetKind::KnowledgeCard => ProposalTarget::KnowledgeCard(id.parse()?),
        ProposalTargetKind::Note => ProposalTarget::Note(id.parse()?),
        ProposalTargetKind::ExamSet => ProposalTarget::ExamSet(id.parse()?),
        ProposalTargetKind::QuestionGroup => ProposalTarget::QuestionGroup(id.parse()?),
        ProposalTargetKind::QuestionCard => ProposalTarget::QuestionCard(id.parse()?),
        ProposalTargetKind::Edge => ProposalTarget::Edge(id.parse()?),
    })
}

pub fn insert_proposal(connection: &Connection, proposal: &Proposal) -> Result<()> {
    connection.execute(
        "INSERT INTO proposals (
            id, target_kind, target_id, payload_json, submitted_by, status,
            review_note, created_at, updated_at, sync_cursor
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            proposal.id,
            proposal.target.kind(),
            proposal.target.id(),
            to_json(&proposal.payload)?,
            proposal.submitted_by,
            proposal.status,
            proposal.review_note,
            proposal.created_at,
            proposal.updated_at,
            proposal.sync_cursor,
        ],
    )?;
    Ok(())
}

pub fn get_proposal(connection: &Connection, id: &ProposalId) -> Result<Option<Proposal>> {
    connection
        .query_row(
            "SELECT id, target_kind, target_id, payload_json, submitted_by, status,
                    review_note, created_at, updated_at, sync_cursor
             FROM proposals WHERE id = ?1",
            params![id],
            |row| {
                let kind: ProposalTargetKind = row.get(1)?;
                let target_id: String = row.get(2)?;
                Ok(Proposal {
                    id: row.get(0)?,
                    target: proposal_target(kind, &target_id)?,
                    payload: from_json(&row.get::<_, String>(3)?)?,
                    submitted_by: row.get(4)?,
                    status: row.get::<_, ProposalStatus>(5)?,
                    review_note: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get::<_, Timestamp>(8)?,
                    sync_cursor: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn list_proposals(connection: &Connection, status: ProposalStatus) -> Result<Vec<Proposal>> {
    let mut statement = connection.prepare(
        "SELECT id, target_kind, target_id, payload_json, submitted_by, status,
                review_note, created_at, updated_at, sync_cursor
         FROM proposals WHERE status = ?1 ORDER BY sync_cursor, id",
    )?;
    let rows = statement.query_map(params![status], |row| {
        let kind: ProposalTargetKind = row.get(1)?;
        let target_id: String = row.get(2)?;
        Ok(Proposal {
            id: row.get(0)?,
            target: proposal_target(kind, &target_id)?,
            payload: from_json(&row.get::<_, String>(3)?)?,
            submitted_by: row.get(4)?,
            status: row.get::<_, ProposalStatus>(5)?,
            review_note: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            sync_cursor: row.get(9)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Error::from)
}

/// Replaces proposal metadata while keeping its payload intact.
pub fn update_proposal(connection: &Connection, proposal: &Proposal) -> Result<()> {
    connection.execute(
        "UPDATE proposals SET status = ?2, review_note = ?3, updated_at = ?4
         WHERE id = ?1",
        params![
            proposal.id,
            proposal.status,
            proposal.review_note,
            proposal.updated_at,
        ],
    )?;
    Ok(())
}
