use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reinforce_core::auth::{
    create_user, delete_setting, get_setting, owner_exists, set_setting, user_by_id,
};
use reinforce_core::{Database, QuizSession, Timestamp, UserId};
use serde_json::Value;
use tokio::task::spawn_blocking;
use uuid::Uuid;

use crate::auth::{hash_token, random_token};
use crate::config::{AuthMode, ServeArgs};
use crate::error::{ApiError, ApiResult};

const OWNER_SETTING: &str = "owner_user_id";
const SETUP_TOKEN_SETTING: &str = "owner_setup_token_hash";
pub const SESSION_IDLE_SECONDS: u64 = 30 * 60;

#[derive(Clone)]
pub struct Store {
    database: Arc<Mutex<Database>>,
}

impl Store {
    pub fn new(database: Database) -> Self {
        Self {
            database: Arc::new(Mutex::new(database)),
        }
    }

    /// Runs synchronous core code on a blocking thread. All database work in
    /// the server goes through this single seam so a pool can replace it later.
    pub async fn call<F, T>(&self, operation: F) -> ApiResult<T>
    where
        F: FnOnce(&mut Database) -> ApiResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let database = Arc::clone(&self.database);
        spawn_blocking(move || {
            let mut guard = database
                .lock()
                .map_err(|_| ApiError::internal("database lock is poisoned"))?;
            operation(&mut guard)
        })
        .await
        .map_err(|error| ApiError::internal(format!("blocking task failed: {error}")))?
    }
}

#[derive(Debug)]
pub struct SessionSlot {
    pub user_id: UserId,
    pub session: QuizSession,
    pub touched: Instant,
    pub persisted: bool,
    pub finish_result: Option<Value>,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub blobs: PathBuf,
    pub fonts: PathBuf,
    pub auth_mode: AuthMode,
    /// The `--host` value, accepted as a `Host` header in single-auth mode.
    pub bind_host: String,
    pub owner_id: UserId,
    pub sessions: Arc<Mutex<HashMap<String, SessionSlot>>>,
}

impl AppState {
    pub async fn open(config: &ServeArgs) -> ApiResult<Self> {
        if let Some(parent) = config.db.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        tokio::fs::create_dir_all(&config.blobs).await?;
        let database = Database::open(&config.db)?;
        let store = Store::new(database);

        let auth_mode = config.auth;
        let owner_id = store
            .call(move |database| {
                let connection = database.connection_mut();
                let mut owner = match get_setting(connection, OWNER_SETTING)? {
                    Some(value) => match value.parse::<UserId>() {
                        Ok(id) => id,
                        Err(_) => create_owner(connection)?,
                    },
                    None => create_owner(connection)?,
                };
                if user_by_id(connection, &owner)?.is_none() {
                    owner = create_owner(connection)?;
                }
                if auth_mode == AuthMode::Invite {
                    if owner_exists(connection)? {
                        delete_setting(connection, SETUP_TOKEN_SETTING)?;
                    } else {
                        let token = random_token(32);
                        set_setting(connection, SETUP_TOKEN_SETTING, &hash_token(&token))?;
                        println!("One-time owner setup token: {token}");
                    }
                }
                Ok(owner)
            })
            .await?;

        Ok(Self {
            store,
            blobs: config.blobs.clone(),
            fonts: config.fonts.clone(),
            auth_mode,
            bind_host: config.host.clone(),
            owner_id,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn with_db<F, T>(&self, operation: F) -> ApiResult<T>
    where
        F: FnOnce(&mut Database) -> ApiResult<T> + Send + 'static,
        T: Send + 'static,
    {
        self.store.call(operation).await
    }

    pub fn new_session_id() -> String {
        Uuid::now_v7().to_string()
    }

    pub fn cleanup_sessions(&self) {
        let mut sessions = match self.sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => return,
        };
        let idle = Duration::from_secs(SESSION_IDLE_SECONDS);
        sessions.retain(|_, slot| slot.touched.elapsed() < idle);
    }

    pub fn blob_path(&self, sha256: &str) -> PathBuf {
        let prefix = sha256.get(..2).unwrap_or("xx");
        Path::new(&self.blobs).join(prefix).join(sha256)
    }
}

fn create_owner(connection: &mut rusqlite::Connection) -> ApiResult<UserId> {
    let id = create_user(connection, "owner", Timestamp::now())?;
    set_setting(connection, OWNER_SETTING, &id.as_str())?;
    Ok(id)
}
