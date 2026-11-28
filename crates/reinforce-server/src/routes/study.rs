use std::sync::MutexGuard;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post};
use reinforce_core::db::{
    content as content_store, quiz as quiz_store, study as study_store,
    subject_groups as subject_groups_store,
};
use reinforce_core::primitives::{AttemptDirection, CardId, SessionMode};
use reinforce_core::quiz::{
    BuildOptions, QuizAnswer, QuizSession, SessionState, TickOutcome, build_daily_due,
    build_full_exam, build_practical_exam, build_subject_exam, build_topic_drill,
    build_weakness_drill,
};
use reinforce_core::{
    AttemptLog, CardScheduling, ContentBlock, ExamSetId, KnowledgeCardId, QuestionCard,
    QuestionCardId, QuestionGroupId, SubjectGroupId, Timestamp, UserId, WeaknessReport, weakness,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody, PathParam, QueryParams};
use crate::state::{AppState, SessionSlot};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/study/sessions", post(create_session))
        .route("/study/sessions/{id}", get(get_session))
        .route("/study/sessions/{id}/start", post(start_session))
        .route("/study/sessions/{id}/pause", post(pause_session))
        .route("/study/sessions/{id}/resume", post(resume_session))
        .route("/study/sessions/{id}/tick", post(tick_session))
        .route("/study/sessions/{id}/select", post(select_question))
        .route("/study/sessions/{id}/answers", post(submit_answer))
        .route("/study/sessions/{id}/finish", post(finish_session))
        .route("/study/sessions/{id}/score", get(get_score))
        .route("/study/attempts", get(list_attempts))
        .route("/study/scheduling", get(get_scheduling))
        .route("/study/weakness", get(weakness_report))
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    mode: SessionMode,
    #[serde(default)]
    exam_set_id: Option<ExamSetId>,
    #[serde(default)]
    knowledge_card_ids: Vec<KnowledgeCardId>,
    #[serde(default)]
    question_ids: Vec<QuestionCardId>,
    /// Scopes the session to every subject of this group and its descendants.
    #[serde(default)]
    subject_group_id: Option<SubjectGroupId>,
    /// Scopes the session to a single subject when no group is given.
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    options: SessionOptions,
}

#[derive(Debug, Deserialize)]
struct SubmitAnswerRequest {
    /// When present, answers that question index without advancing the session.
    #[serde(default)]
    index: Option<usize>,
    #[serde(flatten)]
    answer: QuizAnswer,
}

#[derive(Debug, Deserialize)]
struct SelectQuestionRequest {
    index: usize,
}

#[derive(Debug, Deserialize, Default)]
struct SessionOptions {
    question_limit: Option<usize>,
    new_card_limit: Option<usize>,
    total_time_secs: Option<u32>,
    seconds_per_question: Option<u16>,
    affects_scheduling: Option<bool>,
    pin_radius: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct AttemptsQuery {
    card_id: Option<CardId>,
    direction: Option<AttemptDirection>,
    user_id: Option<UserId>,
}

#[derive(Debug, Deserialize)]
struct SchedulingQuery {
    card_id: CardId,
    #[serde(default)]
    direction: Option<AttemptDirection>,
    #[serde(default)]
    user_id: Option<UserId>,
}

#[derive(Debug, Deserialize, Default)]
struct WeaknessQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    user_id: Option<UserId>,
}

fn session_options(request: &CreateSessionRequest) -> ApiResult<BuildOptions> {
    let mut options = BuildOptions::default();
    if let Some(limit) = request.options.question_limit {
        options.question_limit = limit.clamp(1, 1_000);
    }
    if let Some(limit) = request.options.new_card_limit {
        options.new_card_limit = limit.min(1_000);
    }
    if let Some(seconds) = request.options.total_time_secs {
        options.total_time_secs = Some(seconds);
    }
    if let Some(seconds) = request.options.seconds_per_question {
        options.seconds_per_question = seconds;
    }
    if let Some(affects) = request.options.affects_scheduling {
        options.affects_scheduling = affects;
    }
    if let Some(radius) = request.options.pin_radius {
        if !radius.is_finite() || !(0.0..=1.0).contains(&radius) {
            return Err(ApiError::bad_request(
                "pin_radius",
                "must be between 0 and 1",
            ));
        }
        options.pin_radius = radius;
    }
    Ok(options)
}

fn candidate_ids(
    connection: &Connection,
    request: &CreateSessionRequest,
    user_id: &UserId,
    now: Timestamp,
    options: &BuildOptions,
) -> ApiResult<Vec<QuestionCardId>> {
    let subjects = options.subjects.as_deref();
    let ids = match request.mode {
        SessionMode::DailyDue => quiz_store::due_questions_scoped(
            connection,
            user_id,
            now,
            options.new_card_limit,
            options.question_limit,
            subjects,
        )?
        .into_iter()
        .map(|due| due.id)
        .collect(),
        SessionMode::FullExam => match request.exam_set_id.as_ref() {
            Some(exam_set_id) => quiz_store::exam_question_ids_scoped(
                connection,
                exam_set_id,
                options.question_limit,
                subjects,
            )?,
            None => match subjects {
                Some([]) => Vec::new(),
                Some(subjects) => {
                    quiz_store::subject_question_ids(connection, subjects, options.question_limit)?
                }
                None => {
                    return Err(ApiError::bad_request(
                        "exam_set_id",
                        "required for a full exam session",
                    ));
                }
            },
        },
        SessionMode::TopicDrill => quiz_store::topic_question_ids_scoped(
            connection,
            &request.knowledge_card_ids,
            options.question_limit,
            subjects,
        )?,
        SessionMode::PracticalExam => {
            if request.question_ids.is_empty() {
                quiz_store::practical_question_ids_scoped(
                    connection,
                    options.question_limit,
                    subjects,
                )?
            } else {
                request.question_ids.clone()
            }
        }
        SessionMode::WeaknessDrill => {
            weakness::build_report_scoped(connection, user_id, options.question_limit, subjects)?
                .questions
                .into_iter()
                .map(|entry| entry.question_id)
                .collect()
        }
    };
    Ok(ids)
}

/// Resolves the optional `subject_group_id` / `subject` scope of a request.
fn requested_subjects(
    connection: &Connection,
    request: &CreateSessionRequest,
) -> ApiResult<Option<Vec<String>>> {
    if let Some(group_id) = request.subject_group_id.as_ref() {
        if subject_groups_store::get_subject_group(connection, group_id)?.is_none() {
            return Err(ApiError::not_found("subject group"));
        }
        return Ok(Some(subject_groups_store::resolve_group_subjects(
            connection, group_id,
        )?));
    }
    if let Some(subject) = request.subject.as_deref() {
        let subject = subject.trim();
        if subject.is_empty() {
            return Err(ApiError::bad_request("subject", "cannot be empty"));
        }
        return Ok(Some(vec![subject.to_owned()]));
    }
    Ok(None)
}

/// Pin grading derives the image aspect ratio from the stored resource, never
/// from a client-supplied value.
fn pin_aspect_ratio(connection: &Connection, ids: &[QuestionCardId]) -> ApiResult<Option<f64>> {
    for id in ids {
        let Some(card) = content_store::get_question_card(connection, id)? else {
            continue;
        };
        for block in card
            .stem_blocks
            .iter()
            .chain(card.explanation_blocks.iter())
        {
            let resource_id = match block {
                ContentBlock::InteractivePins { resource_id, .. }
                | ContentBlock::ImageOcclusion { resource_id, .. } => Some(resource_id),
                _ => None,
            };
            let Some(resource_id) = resource_id else {
                continue;
            };
            if let Some(resource) = content_store::get_resource(connection, resource_id)? {
                if let (Some(width), Some(height)) = (resource.width, resource.height) {
                    if width > 0 && height > 0 {
                        return Ok(Some(width as f64 / height as f64));
                    }
                }
            }
        }
    }
    Ok(None)
}

fn build_session(
    connection: &Connection,
    request: &CreateSessionRequest,
    user_id: &UserId,
    now: Timestamp,
    mut options: BuildOptions,
) -> ApiResult<QuizSession> {
    if let Some(subjects) = requested_subjects(connection, request)? {
        options.subjects = Some(subjects);
    }
    let ids = candidate_ids(connection, request, user_id, now, &options)?;
    options.pin_aspect_ratio = pin_aspect_ratio(connection, &ids)?;
    let session = match request.mode {
        SessionMode::DailyDue => build_daily_due(connection, user_id, now, &options)?,
        SessionMode::FullExam => match request.exam_set_id.as_ref() {
            Some(exam_set_id) => build_full_exam(connection, exam_set_id, &options)?,
            None => {
                let subjects = options.subjects.clone().ok_or_else(|| {
                    ApiError::bad_request("exam_set_id", "required for a full exam session")
                })?;
                build_subject_exam(connection, &subjects, &options)?
            }
        },
        SessionMode::TopicDrill => {
            build_topic_drill(connection, &request.knowledge_card_ids, &options)?
        }
        SessionMode::PracticalExam => {
            build_practical_exam(connection, &request.question_ids, &options)?
        }
        SessionMode::WeaknessDrill => build_weakness_drill(connection, user_id, &options)?,
    };
    Ok(session)
}

fn sessions(
    state: &AppState,
) -> ApiResult<MutexGuard<'_, std::collections::HashMap<String, SessionSlot>>> {
    state
        .sessions
        .lock()
        .map_err(|_| ApiError::internal("session store lock is poisoned"))
}

fn slot<'a>(
    guard: &'a mut std::collections::HashMap<String, SessionSlot>,
    id: &str,
    user_id: &UserId,
) -> ApiResult<&'a mut SessionSlot> {
    let slot = guard
        .get_mut(id)
        .ok_or_else(|| ApiError::not_found("quiz session"))?;
    if slot.user_id != *user_id {
        return Err(ApiError::forbidden(
            "this quiz session belongs to another user",
        ));
    }
    slot.touched = Instant::now();
    Ok(slot)
}

fn public_question(question: &QuestionCard) -> Value {
    let mut value = json!(question);
    if let Some(object) = value.as_object_mut() {
        object.remove("answer");
        object.remove("accepted_answers");
        object.remove("answer_basis");
        object.remove("explanation_blocks");
        if let Some(options) = object
            .get_mut("options")
            .and_then(serde_json::Value::as_array_mut)
        {
            for option in options {
                if let Some(option) = option.as_object_mut() {
                    option.remove("explanation");
                }
            }
        }
    }
    value
}

fn normalize_error_tags(tags: Vec<String>) -> ApiResult<Vec<String>> {
    if tags.len() > 10 {
        return Err(ApiError::bad_request(
            "error_tags",
            "at most 10 tags are allowed",
        ));
    }
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            return Err(ApiError::bad_request(
                "error_tags",
                "each tag must contain at least one character",
            ));
        }
        if tag.chars().count() > 40 {
            return Err(ApiError::bad_request(
                "error_tags",
                "each tag must be at most 40 characters",
            ));
        }
        if !normalized.iter().any(|existing| existing == tag) {
            normalized.push(tag.to_owned());
        }
    }
    Ok(normalized)
}

fn current_question(session: &QuizSession) -> Value {
    match session.state() {
        SessionState::InProgress { index }
        | SessionState::Paused { index }
        | SessionState::Expired { index } => session
            .questions()
            .get(index)
            .map(public_question)
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn check_scope(requested: Option<&UserId>, principal: &Principal) -> ApiResult<()> {
    if requested.is_some_and(|id| id != &principal.user_id) {
        return Err(ApiError::forbidden("a study store is private to its owner"));
    }
    Ok(())
}

async fn create_session(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(request): JsonBody<CreateSessionRequest>,
) -> ApiResult<Json<Value>> {
    let options = session_options(&request)?;
    let user_id = principal.user_id.clone();
    let now = Timestamp::now();
    let mut session = state
        .with_db(move |database| {
            build_session(database.connection(), &request, &user_id, now, options)
        })
        .await?;
    session.start(now);
    let id = AppState::new_session_id();
    let question_count = session.questions().len();
    let session_state = session.state();
    state.cleanup_sessions();
    let mut sessions = sessions(&state)?;
    sessions.insert(
        id.clone(),
        SessionSlot {
            user_id: principal.user_id,
            session,
            touched: Instant::now(),
            persisted: false,
            finish_result: None,
        },
    );
    Ok(Json(json!({
        "session_id": id,
        "state": session_state,
        "question_count": question_count,
    })))
}

async fn get_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    state.cleanup_sessions();
    let (
        mode,
        session_state,
        question,
        question_count,
        questions,
        answers,
        graded,
        group_ids,
        exam_ids,
    ) = {
        let mut sessions = sessions(&state)?;
        let slot = slot(&mut sessions, &id, &principal.user_id)?;
        let session = &slot.session;
        // A question whose grading is already known can be revealed in full;
        // everything else uses the answer-free projection.
        let questions = session
            .questions()
            .iter()
            .enumerate()
            .map(|(index, question)| {
                if session.graded_for(index).is_some() {
                    json!(question)
                } else {
                    public_question(question)
                }
            })
            .collect::<Vec<_>>();
        let mut group_ids: Vec<QuestionGroupId> = Vec::new();
        let mut exam_ids: Vec<ExamSetId> = Vec::new();
        for question in session.questions() {
            if let Some(group_id) = question.question_group_id.clone() {
                if !group_ids.contains(&group_id) {
                    group_ids.push(group_id);
                }
            }
            if let Some(exam_id) = question.exam_set_id.clone() {
                if !exam_ids.contains(&exam_id) {
                    exam_ids.push(exam_id);
                }
            }
        }
        (
            session.mode(),
            session.state(),
            current_question(session),
            session.questions().len(),
            questions,
            session.answers().to_vec(),
            session.graded().to_vec(),
            group_ids,
            exam_ids,
        )
    };
    let (question_groups, exam_sets) = state
        .with_db(move |database| {
            let connection = database.connection();
            let mut groups = serde_json::Map::new();
            for group_id in group_ids {
                if let Some(group) = content_store::get_question_group(connection, &group_id)? {
                    groups.insert(group_id.as_str(), json!(group));
                }
            }
            let mut sets = serde_json::Map::new();
            for exam_id in exam_ids {
                if let Some(set) = content_store::get_exam_set(connection, &exam_id)? {
                    sets.insert(
                        exam_id.as_str(),
                        json!({
                            "id": set.id,
                            "title": set.title,
                            "exam_year": set.exam_year,
                            "subject": set.subject,
                        }),
                    );
                }
            }
            Ok((Value::Object(groups), Value::Object(sets)))
        })
        .await?;
    Ok(Json(json!({
        "session_id": id,
        "mode": mode,
        "state": session_state,
        "question": question,
        "question_count": question_count,
        "questions": questions,
        "question_groups": question_groups,
        "exam_sets": exam_sets,
        "answers": answers,
        "graded": graded,
    })))
}

async fn start_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    slot.session.start(Timestamp::now());
    Ok(Json(json!({ "state": slot.session.state() })))
}

async fn pause_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    slot.session.pause(Timestamp::now())?;
    Ok(Json(json!({ "state": slot.session.state() })))
}

async fn resume_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    slot.session.resume(Timestamp::now())?;
    Ok(Json(json!({ "state": slot.session.state() })))
}

async fn tick_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    let outcome = slot.session.tick(Timestamp::now())?;
    let mut value = serde_json::to_value(&outcome)?;
    if let TickOutcome::QuestionTimedOut { index, .. } = &outcome {
        if let Some(question) = slot.session.questions().get(*index) {
            value["revealed_question"] = json!(question);
        }
    }
    Ok(Json(value))
}

async fn select_question(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
    JsonBody(request): JsonBody<SelectQuestionRequest>,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    slot.session.select_index(request.index, Timestamp::now())?;
    Ok(Json(json!({
        "state": slot.session.state(),
        "question": current_question(&slot.session),
    })))
}

async fn submit_answer(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
    JsonBody(request): JsonBody<SubmitAnswerRequest>,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    let mut answer = request.answer;
    answer.error_tags = normalize_error_tags(answer.error_tags)?;
    let now = Timestamp::now();
    let (answered_index, grading, revealed_question) = match request.index {
        Some(index) => {
            let grading = slot.session.submit_answer_at(index, answer, now)?;
            let revealed = if grading.is_some() {
                slot.session
                    .questions()
                    .get(index)
                    .map(|question| json!(question))
            } else {
                None
            };
            (Some(index), grading, revealed)
        }
        None => {
            let answered_index = match slot.session.state() {
                SessionState::InProgress { index } => Some(index),
                _ => None,
            };
            let grading = slot.session.submit_answer(answer, now)?;
            let revealed = if grading.is_some() {
                answered_index
                    .and_then(|index| slot.session.questions().get(index))
                    .map(|question| json!(question))
            } else {
                None
            };
            (answered_index, grading, revealed)
        }
    };
    Ok(Json(json!({
        "state": slot.session.state(),
        "index": answered_index,
        "grading": grading,
        "revealed_question": revealed_question,
    })))
}

async fn finish_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let now = Timestamp::now();
    let (user_id, session, score, graded, revealed_questions) = {
        let mut sessions = sessions(&state)?;
        let slot = slot(&mut sessions, &id, &principal.user_id)?;
        if let Some(result) = slot.finish_result.clone() {
            return Ok(Json(result));
        }
        if slot.persisted {
            return Err(ApiError::conflict("the session is being finalized"));
        }
        slot.session.finish(now)?;
        // Score before marking the slot persisted so a failure here leaves
        // the session retryable instead of stuck behind the 409 guard.
        let score = slot.session.score()?;
        let graded = slot.session.graded().to_vec();
        let revealed_questions = slot.session.questions().to_vec();
        slot.persisted = true;
        (
            principal.user_id.clone(),
            slot.session.clone(),
            score,
            graded,
            revealed_questions,
        )
    };
    let written = state
        .with_db(move |database| {
            let transaction = database.transaction()?;
            let cursor = study_store::next_user_attempt_cursor(&transaction, &user_id)?;
            let attempts = session.attempts(&user_id, cursor);
            for attempt in &attempts {
                study_store::append_attempt(&transaction, attempt)?;
                if attempt.affects_scheduling {
                    let scheduling: CardScheduling = study_store::recompute_for_card(
                        &transaction,
                        &attempt.user_id,
                        &attempt.card_id,
                        attempt.direction,
                        0.9,
                    )?;
                    study_store::save_scheduling(&transaction, &scheduling)?;
                }
            }
            transaction.commit()?;
            Ok(attempts.len())
        })
        .await;
    let written = match written {
        Ok(written) => written,
        Err(error) => {
            if let Ok(mut sessions) = sessions(&state) {
                if let Ok(slot) = slot(&mut sessions, &id, &principal.user_id) {
                    slot.persisted = false;
                }
            }
            return Err(error);
        }
    };
    let result = json!({
        "attempts_written": written,
        "score": score,
        "graded": graded,
        "revealed_questions": revealed_questions,
    });
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    slot.finish_result = Some(result.clone());
    Ok(Json(result))
}

async fn get_score(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    Ok(Json(json!({
        "score": slot.session.score()?,
        "graded_count": slot.session.graded().len(),
    })))
}

async fn list_attempts(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<AttemptsQuery>,
    principal: Principal,
) -> ApiResult<Json<Vec<AttemptLog>>> {
    check_scope(query.user_id.as_ref(), &principal)?;
    let user_id = principal.user_id;
    let attempts = state
        .with_db(move |database| {
            let connection = database.connection();
            match (query.card_id, query.direction) {
                (Some(card_id), Some(direction)) => {
                    study_store::list_attempts(connection, &user_id, &card_id, direction)
                        .map_err(Into::into)
                }
                (Some(card_id), None) => study_store::list_attempts(
                    connection,
                    &user_id,
                    &card_id,
                    AttemptDirection::Default,
                )
                .map_err(Into::into),
                _ => quiz_store::user_attempts(connection, &user_id).map_err(Into::into),
            }
        })
        .await?;
    Ok(Json(attempts))
}

async fn get_scheduling(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<SchedulingQuery>,
    principal: Principal,
) -> ApiResult<Json<Option<CardScheduling>>> {
    check_scope(query.user_id.as_ref(), &principal)?;
    let user_id = principal.user_id;
    let scheduling = state
        .with_db(move |database| {
            study_store::get_scheduling(
                database.connection(),
                &user_id,
                &query.card_id,
                query.direction.unwrap_or(AttemptDirection::Default),
            )
            .map_err(Into::into)
        })
        .await?;
    Ok(Json(scheduling))
}

async fn weakness_report(
    State(state): State<AppState>,
    QueryParams(query): QueryParams<WeaknessQuery>,
    principal: Principal,
) -> ApiResult<Json<WeaknessReport>> {
    check_scope(query.user_id.as_ref(), &principal)?;
    let user_id = principal.user_id;
    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    let report = state
        .with_db(move |database| {
            weakness::build_report(database.connection(), &user_id, limit).map_err(Into::into)
        })
        .await?;
    Ok(Json(report))
}
