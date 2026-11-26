use axum::extract::FromRequestParts;
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use reinforce_core::auth::{Role, SessionAuth, session_by_token_hash};
use reinforce_core::{ContentHash, Timestamp, UserId};

use crate::config::AuthMode;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "reinforce_session";
const SESSION_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

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

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
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
    ContentHash::from_bytes(value.as_bytes()).as_str().to_owned()
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

pub fn hash_password(password: &str) -> ApiResult<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::internal(format!("cannot hash password: {error}")))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
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
