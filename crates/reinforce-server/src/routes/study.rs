use std::sync::MutexGuard;
use std::time::Instant;

use axum::Json;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Router;
use reinforce_core::db::{content as content_store, quiz as quiz_store, study as study_store};
use reinforce_core::primitives::{AttemptDirection, CardId, SessionMode};
use reinforce_core::quiz::{
    BuildOptions, QuizAnswer, QuizSession, SessionState, TickOutcome, build_daily_due,
    build_full_exam, build_practical_exam, build_topic_drill, build_weakness_drill,
};
use reinforce_core::{
    AttemptLog, CardScheduling, ContentBlock, ExamSetId, KnowledgeCardId, QuestionCard,
    QuestionCardId, Timestamp, UserId, WeaknessReport, weakness,
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
        .route("/study/sessions/:id", get(get_session))
        .route("/study/sessions/:id/start", post(start_session))
        .route("/study/sessions/:id/pause", post(pause_session))
        .route("/study/sessions/:id/resume", post(resume_session))
        .route("/study/sessions/:id/tick", post(tick_session))
        .route("/study/sessions/:id/answers", post(submit_answer))
        .route("/study/sessions/:id/finish", post(finish_session))
        .route("/study/sessions/:id/score", get(get_score))
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
    #[serde(default)]
    options: SessionOptions,
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
            return Err(ApiError::bad_request("pin_radius", "must be between 0 and 1"));
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
    let ids = match request.mode {
        SessionMode::DailyDue => quiz_store::due_questions(
            connection,
            user_id,
            now,
            options.new_card_limit,
            options.question_limit,
        )?
        .into_iter()
        .map(|due| due.id)
        .collect(),
        SessionMode::FullExam => {
            let exam_set_id = request.exam_set_id.as_ref().ok_or_else(|| {
                ApiError::bad_request("exam_set_id", "required for a full exam session")
            })?;
            quiz_store::exam_question_ids(connection, exam_set_id, options.question_limit)?
        }
        SessionMode::TopicDrill => quiz_store::topic_question_ids(
            connection,
            &request.knowledge_card_ids,
            options.question_limit,
        )?,
        SessionMode::PracticalExam => {
            if request.question_ids.is_empty() {
                quiz_store::practical_question_ids(connection, options.question_limit)?
            } else {
                request.question_ids.clone()
            }
        }
        SessionMode::WeaknessDrill => weakness::build_report(connection, user_id, options.question_limit)?
            .questions
            .into_iter()
            .map(|entry| entry.question_id)
            .collect(),
    };
    Ok(ids)
}

/// Pin grading derives the image aspect ratio from the stored resource, never
/// from a client-supplied value.
fn pin_aspect_ratio(connection: &Connection, ids: &[QuestionCardId]) -> ApiResult<Option<f64>> {
    for id in ids {
        let Some(card) = content_store::get_question_card(connection, id)? else {
            continue;
        };
        for block in card.stem_blocks.iter().chain(card.explanation_blocks.iter()) {
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
    let ids = candidate_ids(connection, request, user_id, now, &options)?;
    options.pin_aspect_ratio = pin_aspect_ratio(connection, &ids)?;
    let session = match request.mode {
        SessionMode::DailyDue => build_daily_due(connection, user_id, now, &options)?,
        SessionMode::FullExam => {
            let exam_set_id = request.exam_set_id.as_ref().ok_or_else(|| {
                ApiError::bad_request("exam_set_id", "required for a full exam session")
            })?;
            build_full_exam(connection, exam_set_id, &options)?
        }
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

fn sessions(state: &AppState) -> ApiResult<MutexGuard<'_, std::collections::HashMap<String, SessionSlot>>> {
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
        return Err(ApiError::forbidden("this quiz session belongs to another user"));
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
    }
    value
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
        .with_db(move |database| build_session(database.connection(), &request, &user_id, now, options))
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
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    Ok(Json(json!({
        "session_id": id,
        "mode": slot.session.mode(),
        "state": slot.session.state(),
        "question": current_question(&slot.session),
        "question_count": slot.session.questions().len(),
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
) -> ApiResult<Json<TickOutcome>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    let outcome = slot.session.tick(Timestamp::now())?;
    Ok(Json(outcome))
}

async fn submit_answer(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
    JsonBody(answer): JsonBody<QuizAnswer>,
) -> ApiResult<Json<Value>> {
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    let grading = slot.session.submit_answer(answer, Timestamp::now())?;
    Ok(Json(json!({
        "state": slot.session.state(),
        "grading": grading,
    })))
}

async fn finish_session(
    State(state): State<AppState>,
    PathParam(id): PathParam<String>,
    principal: Principal,
) -> ApiResult<Json<Value>> {
    let cursor = state
        .with_db(move |database| Ok(study_store::next_attempt_cursor(database.connection())?))
        .await?;
    let now = Timestamp::now();
    let attempts: Vec<AttemptLog> = {
        let mut sessions = sessions(&state)?;
        let slot = slot(&mut sessions, &id, &principal.user_id)?;
        slot.session.finish(now)?;
        slot.session.attempts(&principal.user_id, cursor)
    };
    let written = attempts.len();
    state
        .with_db(move |database| {
            let transaction = database.transaction()?;
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
            Ok(())
        })
        .await?;
    let mut sessions = sessions(&state)?;
    let slot = slot(&mut sessions, &id, &principal.user_id)?;
    Ok(Json(json!({
        "attempts_written": written,
        "score": slot.session.score()?,
    })))
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
                (Some(card_id), Some(direction)) => study_store::list_attempts(
                    connection,
                    &user_id,
                    &card_id,
                    direction,
                )
                .map_err(Into::into),
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
