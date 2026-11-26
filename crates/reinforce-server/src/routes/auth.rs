use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::routing::{get, post};
use axum::{Router, response::IntoResponse};
use reinforce_core::Timestamp;
use reinforce_core::auth::{
    credential_by_username, delete_session, get_setting, insert_credential, insert_invite,
    insert_session, owner_exists, register_with_invite,
};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{
    OptionalPrincipal, Principal, SESSION_COOKIE, expired_cookie, hash_password, hash_token,
    random_token, session_cookie, session_ttl, verify_password,
};
use crate::config::AuthMode;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::state::AppState;

const SETUP_TOKEN_SETTING: &str = "owner_setup_token_hash";
const DEFAULT_INVITE_SECONDS: i64 = 24 * 60 * 60;
const MAX_INVITE_SECONDS: i64 = 30 * 24 * 60 * 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/owner/setup", post(setup_owner))
        .route("/auth/invites", post(create_invite))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(current_session))
}

#[derive(Deserialize)]
struct SetupOwner {
    token: String,
    #[serde(default = "default_owner_name")]
    username: String,
    password: String,
}

fn default_owner_name() -> String {
    "owner".to_owned()
}

#[derive(Deserialize)]
struct CreateInvite {
    #[serde(default)]
    expires_in_secs: Option<i64>,
}

#[derive(Deserialize)]
struct Register {
    invite_code: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

async fn status(
    State(state): State<AppState>,
    OptionalPrincipal(principal): OptionalPrincipal,
) -> ApiResult<Json<serde_json::Value>> {
    let invite_mode = state.auth_mode == AuthMode::Invite;
    let needs_owner_setup = state
        .with_db(move |database| {
            if !invite_mode {
                return Ok(false);
            }
            Ok(!owner_exists(database.connection())?)
        })
        .await?;
    Ok(Json(json!({
        "auth_mode": state.auth_mode.as_str(),
        "needs_owner_setup": needs_owner_setup,
        "authenticated": principal.is_some(),
        "role": principal.as_ref().map(|value| value.role.as_str()),
    })))
}

async fn current_session(principal: Principal) -> Json<serde_json::Value> {
    Json(json!({
        "user_id": principal.user_id,
        "role": principal.role.as_str(),
    }))
}

async fn setup_owner(
    State(state): State<AppState>,
    JsonBody(request): JsonBody<SetupOwner>,
) -> ApiResult<Json<serde_json::Value>> {
    if state.auth_mode != AuthMode::Invite {
        return Err(ApiError::forbidden(
            "owner setup is only available in invite auth mode",
        ));
    }
    validate_credentials(&request.username, &request.password)?;
    let stored_hash = hash_password(request.password.clone()).await?;
    let submitted_hash = hash_token(&request.token);
    let username = request.username;
    let owner_id = state.owner_id.clone();
    let user_id = state
        .with_db(move |database| {
            let transaction = database.transaction()?;
            if owner_exists(&transaction)? {
                return Err(ApiError::conflict("owner is already configured"));
            }
            let expected = get_setting(&transaction, SETUP_TOKEN_SETTING)?
                .ok_or_else(ApiError::unauthorized)?;
            if expected != submitted_hash {
                return Err(ApiError::unauthorized());
            }
            let now = Timestamp::now();
            insert_credential(
                &transaction,
                &owner_id,
                &username,
                &stored_hash,
                reinforce_core::auth::Role::Owner,
                now,
            )?;
            reinforce_core::auth::delete_setting(&transaction, SETUP_TOKEN_SETTING)?;
            transaction.commit()?;
            Ok(owner_id)
        })
        .await?;
    Ok(Json(json!({ "user_id": user_id })))
}

async fn create_invite(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(request): JsonBody<CreateInvite>,
) -> ApiResult<Json<serde_json::Value>> {
    require_owner(&principal)?;
    let seconds = request.expires_in_secs.unwrap_or(DEFAULT_INVITE_SECONDS);
    if !(60..=MAX_INVITE_SECONDS).contains(&seconds) {
        return Err(ApiError::bad_request(
            "expires_in_secs",
            "must be between 60 and 2592000 seconds",
        ));
    }
    let code = random_token(18);
    let code_hash = hash_token(&code);
    let owner_id = principal.user_id.clone();
    let expires_at = state
        .with_db(move |database| {
            let now = Timestamp::now();
            insert_invite(
                database.connection(),
                &code_hash,
                &owner_id,
                now,
                now.plus_days(seconds as f64 / 86_400.0),
            )?;
            Ok(Timestamp::now().plus_days(seconds as f64 / 86_400.0))
        })
        .await?;
    Ok(Json(json!({ "code": code, "expires_at": expires_at })))
}

async fn register(
    State(state): State<AppState>,
    JsonBody(request): JsonBody<Register>,
) -> ApiResult<Json<serde_json::Value>> {
    if state.auth_mode != AuthMode::Invite {
        return Err(ApiError::forbidden(
            "registration is only available in invite auth mode",
        ));
    }
    validate_credentials(&request.username, &request.password)?;
    if request.invite_code.trim().is_empty() {
        return Err(ApiError::bad_request("invite_code", "cannot be empty"));
    }
    let stored_hash = hash_password(request.password.clone()).await?;
    let code_hash = hash_token(&request.invite_code);
    let username = request.username;
    let user_id = state
        .with_db(move |database| {
            register_with_invite(
                database.connection_mut(),
                &code_hash,
                Timestamp::now(),
                &username,
                &stored_hash,
            )?
            .ok_or_else(|| ApiError::bad_request("invite_code", "invalid or expired invite"))
        })
        .await?;
    Ok(Json(json!({ "user_id": user_id })))
}

async fn login(
    State(state): State<AppState>,
    JsonBody(request): JsonBody<Login>,
) -> ApiResult<impl IntoResponse> {
    if state.auth_mode != AuthMode::Invite {
        return Err(ApiError::forbidden(
            "login is only available in invite auth mode",
        ));
    }
    let username = request.username.clone();
    let credential = state
        .with_db(move |database| Ok(credential_by_username(database.connection(), &username)?))
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    if !verify_password(request.password.clone(), credential.password_hash.clone()).await {
        return Err(ApiError::unauthorized());
    }
    let token = random_token(32);
    let token_hash = hash_token(&token);
    let user_id = credential.user_id;
    let session_user_id = user_id.clone();
    state
        .with_db(move |database| {
            let now = Timestamp::now();
            insert_session(
                database.connection(),
                &token_hash,
                &session_user_id,
                now,
                session_ttl(now),
            )?;
            Ok(())
        })
        .await?;
    Ok((
        [(header::SET_COOKIE, session_cookie(&token))],
        Json(json!({ "user_id": user_id, "role": credential.role.as_str() })),
    ))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(token) = cookie_from_headers(&headers) {
        let token_hash = hash_token(&token);
        state
            .with_db(move |database| {
                delete_session(database.connection(), &token_hash)?;
                Ok(())
            })
            .await?;
    }
    Ok((
        [(header::SET_COOKIE, expired_cookie())],
        Json(json!({ "ok": true })),
    ))
}

fn require_owner(principal: &Principal) -> ApiResult<()> {
    if principal.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden("owner access is required"))
    }
}

fn validate_credentials(username: &str, password: &str) -> ApiResult<()> {
    if !(3..=64).contains(&username.len())
        || !username.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err(ApiError::bad_request(
            "username",
            "must be 3-64 characters of letters, digits, '_' or '-'",
        ));
    }
    if password.len() < 8 || password.len() > 256 {
        return Err(ApiError::bad_request(
            "password",
            "must be between 8 and 256 bytes",
        ));
    }
    Ok(())
}

fn cookie_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|entry| {
            let (key, value) = entry.trim().split_once('=')?;
            (key == SESSION_COOKIE).then(|| value.trim().to_owned())
        })
}
