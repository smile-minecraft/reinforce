use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use reinforce_core::auth::{
    Role, SessionAuth, delete_setting, get_setting, session_by_token_hash, set_setting,
};
use reinforce_core::{ContentHash, Timestamp, UserId};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;
use uuid::Uuid;

use crate::config::AuthMode;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "reinforce_session";
const SESSION_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Only SHA-256 hashes are persisted; the plaintext token is shown once.
const AGENT_TOKENS_SETTING: &str = "agent_token_hashes";

#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: UserId,
    pub role: Role,
    pub mode: AuthMode,
}

impl Principal {
    pub fn is_owner(&self) -> bool {
        self.role == Role::Owner
    }
}

#[derive(Debug, Clone)]
pub struct OptionalPrincipal(pub Option<Principal>);

impl FromRequestParts<AppState> for OptionalPrincipal {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(Principal::from_request_parts(parts, state).await.ok()))
    }
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.auth_mode == AuthMode::Single {
            return Ok(Self {
                user_id: state.owner_id.clone(),
                role: Role::Owner,
                mode: state.auth_mode,
            });
        }
        let token = cookie_value(parts, SESSION_COOKIE).ok_or_else(ApiError::unauthorized)?;
        let token_hash = hash_token(&token);
        let session: Option<SessionAuth> = state
            .with_db(move |database| {
                Ok(session_by_token_hash(
                    database.connection(),
                    &token_hash,
                    Timestamp::now(),
                )?)
            })
            .await?;
        let session = session.ok_or_else(ApiError::unauthorized)?;
        Ok(Self {
            user_id: session.user_id,
            role: session.role,
            mode: state.auth_mode,
        })
    }
}

pub fn hash_token(value: &str) -> String {
    ContentHash::from_bytes(value.as_bytes())
        .as_str()
        .to_owned()
}

pub fn random_token(byte_count: usize) -> String {
    let mut text = String::with_capacity(byte_count * 2);
    for _ in 0..byte_count {
        let byte: u8 = rand::random();
        use std::fmt::Write;
        write!(&mut text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAgentToken {
    id: String,
    name: String,
    hash: String,
    created_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StoredAgentTokenEntry {
    Named(StoredAgentToken),
    LegacyHash(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentTokenSummary {
    pub id: String,
    pub name: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatedAgentToken {
    pub id: String,
    pub name: String,
    pub token: String,
    pub created_at: Timestamp,
}

const DEFAULT_AGENT_TOKEN_NAME: &str = "mcp-http";

fn legacy_epoch() -> Timestamp {
    "1970-01-01T00:00:00.000000Z"
        .parse()
        .expect("static unix epoch timestamp is valid")
}

fn validate_agent_token_name(name: Option<&str>) -> ApiResult<String> {
    let Some(raw) = name else {
        return Ok(DEFAULT_AGENT_TOKEN_NAME.to_owned());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_AGENT_TOKEN_NAME.to_owned());
    }
    if trimmed.len() > 64
        || !trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ' ')
        })
    {
        return Err(ApiError::bad_request(
            "name",
            "must be 1-64 characters of ASCII letters, digits, spaces, '.', '_' or '-'",
        ));
    }
    Ok(trimmed.to_owned())
}

fn stored_agent_tokens(connection: &Connection) -> ApiResult<Vec<StoredAgentToken>> {
    let Some(value) = get_setting(connection, AGENT_TOKENS_SETTING)? else {
        return Ok(Vec::new());
    };
    let entries: Vec<StoredAgentTokenEntry> = serde_json::from_str(&value)
        .map_err(|error| ApiError::internal(format!("invalid agent token registry: {error}")))?;
    Ok(entries
        .into_iter()
        .map(|entry| match entry {
            StoredAgentTokenEntry::Named(record) => record,
            StoredAgentTokenEntry::LegacyHash(hash) => {
                let prefix = &hash[..hash.len().min(12)];
                StoredAgentToken {
                    id: format!("legacy-{prefix}"),
                    name: DEFAULT_AGENT_TOKEN_NAME.to_owned(),
                    hash,
                    created_at: legacy_epoch(),
                }
            }
        })
        .collect())
}

fn save_agent_tokens(connection: &Connection, tokens: &[StoredAgentToken]) -> ApiResult<()> {
    if tokens.is_empty() {
        delete_setting(connection, AGENT_TOKENS_SETTING)?;
    } else {
        set_setting(
            connection,
            AGENT_TOKENS_SETTING,
            &serde_json::to_string(tokens)?,
        )?;
    }
    Ok(())
}

/// Creates a named owner API token for remote MCP clients. Only its hash is stored.
pub fn create_named_agent_token(
    connection: &Connection,
    name: Option<&str>,
) -> ApiResult<CreatedAgentToken> {
    let name = validate_agent_token_name(name)?;
    let token = random_token(32);
    let id = Uuid::now_v7().to_string();
    let created_at = Timestamp::now();
    let mut records = stored_agent_tokens(connection)?;
    records.push(StoredAgentToken {
        id: id.clone(),
        name: name.clone(),
        hash: hash_token(&token),
        created_at,
    });
    save_agent_tokens(connection, &records)?;
    Ok(CreatedAgentToken {
        id,
        name,
        token,
        created_at,
    })
}

/// Creates an owner API token for remote MCP clients. Only its hash is stored.
pub fn create_agent_token(connection: &Connection) -> ApiResult<String> {
    Ok(create_named_agent_token(connection, None)?.token)
}

/// Lists configured agent tokens without exposing plaintext secrets or hashes.
pub fn list_agent_tokens(connection: &Connection) -> ApiResult<Vec<AgentTokenSummary>> {
    Ok(stored_agent_tokens(connection)?
        .into_iter()
        .map(|record| AgentTokenSummary {
            id: record.id,
            name: record.name,
            created_at: record.created_at,
        })
        .collect())
}

/// Removes a token by its plaintext value. A missing or unknown token is a
/// no-op so revocation is safe to retry.
pub fn revoke_agent_token(connection: &Connection, token: &str) -> ApiResult<()> {
    let hash = hash_token(token);
    let mut records = stored_agent_tokens(connection)?;
    records.retain(|record| record.hash != hash);
    save_agent_tokens(connection, &records)
}

/// Removes a token by its stored identifier. Returns whether a matching token
/// was removed.
pub fn revoke_agent_token_by_id(connection: &Connection, id: &str) -> ApiResult<bool> {
    let mut records = stored_agent_tokens(connection)?;
    let before = records.len();
    records.retain(|record| record.id != id);
    let removed = records.len() < before;
    if removed {
        save_agent_tokens(connection, &records)?;
    }
    Ok(removed)
}

pub fn resolve_agent_token(
    connection: &Connection,
    token: &str,
) -> ApiResult<Option<AgentTokenSummary>> {
    let hash = hash_token(token);
    Ok(stored_agent_tokens(connection)?
        .into_iter()
        .find(|record| record.hash == hash)
        .map(|record| AgentTokenSummary {
            id: record.id,
            name: record.name,
            created_at: record.created_at,
        }))
}

pub fn verify_agent_token(connection: &Connection, token: &str) -> ApiResult<bool> {
    Ok(resolve_agent_token(connection, token)?.is_some())
}

fn hash_password_sync(password: &str) -> ApiResult<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::internal(format!("cannot hash password: {error}")))
}

pub async fn hash_password(password: String) -> ApiResult<String> {
    spawn_blocking(move || hash_password_sync(&password))
        .await
        .map_err(|error| ApiError::internal(format!("password hashing task failed: {error}")))?
}

fn verify_password_sync(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub async fn verify_password(password: String, stored: String) -> bool {
    spawn_blocking(move || verify_password_sync(&password, &stored))
        .await
        .unwrap_or(false)
}

pub fn session_cookie(token: &str) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECONDS}"
    )
}

pub fn expired_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn session_ttl(now: Timestamp) -> Timestamp {
    now.plus_days(SESSION_TTL_SECONDS as f64 / 86_400.0)
}

fn cookie_value(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|entry| {
            let (key, value) = entry.trim().split_once('=')?;
            (key == name).then(|| value.trim().to_owned())
        })
}
