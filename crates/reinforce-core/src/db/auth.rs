use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};

use crate::User;
use crate::db::study::insert_user;
use crate::primitives::{Error, Result, UserId};
use crate::timestamp::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }
}

impl FromStr for Role {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "owner" => Ok(Self::Owner),
            "member" => Ok(Self::Member),
            _ => Err(Error::InvalidValue("role", value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Credential {
    pub user_id: UserId,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionAuth {
    pub user_id: UserId,
    pub role: Role,
    pub expires_at: Timestamp,
}

pub fn get_setting(connection: &Connection, key: &str) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

pub fn set_setting(connection: &Connection, key: &str, value: &str) -> Result<()> {
    connection.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_setting(connection: &Connection, key: &str) -> Result<()> {
    connection.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
    Ok(())
}

pub fn user_by_id(connection: &Connection, id: &UserId) -> Result<Option<User>> {
    connection
        .query_row(
            "SELECT id, display_name, created_at FROM users WHERE id = ?1",
            params![id],
            |row| {
                Ok(User {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn create_user(connection: &Connection, display_name: &str, now: Timestamp) -> Result<UserId> {
    let id = UserId::generate();
    insert_user(
        connection,
        &User {
            id: id.clone(),
            display_name: display_name.to_owned(),
            created_at: now,
        },
    )?;
    Ok(id)
}

pub fn insert_credential(
    connection: &Connection,
    user_id: &UserId,
    username: &str,
    password_hash: &str,
    role: Role,
    now: Timestamp,
) -> Result<()> {
    connection.execute(
        "INSERT INTO credentials (user_id, username, password_hash, role, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, username, password_hash, role.as_str(), now],
    )?;
    Ok(())
}

pub fn credential_by_username(connection: &Connection, username: &str) -> Result<Option<Credential>> {
    connection
        .query_row(
            "SELECT user_id, username, password_hash, role FROM credentials WHERE username = ?1",
            params![username],
            |row| {
                Ok(Credential {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    role: Role::from_str(&row.get::<_, String>(3)?)
                        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error)))?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn owner_exists(connection: &Connection) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM credentials WHERE role = 'owner')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
        .map_err(Into::into)
}

pub fn insert_session(
    connection: &Connection,
    token_hash: &str,
    user_id: &UserId,
    created_at: Timestamp,
    expires_at: Timestamp,
) -> Result<()> {
    connection.execute(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![token_hash, user_id, created_at, expires_at],
    )?;
    Ok(())
}

pub fn session_by_token_hash(
    connection: &Connection,
    token_hash: &str,
    now: Timestamp,
) -> Result<Option<SessionAuth>> {
    connection
        .query_row(
            "SELECT s.user_id, c.role, s.expires_at
             FROM sessions s JOIN credentials c ON c.user_id = s.user_id
             WHERE s.token_hash = ?1 AND s.expires_at > ?2",
            params![token_hash, now],
            |row| {
                Ok(SessionAuth {
                    user_id: row.get(0)?,
                    role: Role::from_str(&row.get::<_, String>(1)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                    expires_at: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn delete_session(connection: &Connection, token_hash: &str) -> Result<()> {
    connection.execute("DELETE FROM sessions WHERE token_hash = ?1", params![token_hash])?;
    Ok(())
}

pub fn delete_expired_sessions(connection: &Connection, now: Timestamp) -> Result<()> {
    connection.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
    Ok(())
}

pub fn insert_invite(
    connection: &Connection,
    code_hash: &str,
    created_by: &UserId,
    created_at: Timestamp,
    expires_at: Timestamp,
) -> Result<()> {
    connection.execute(
        "INSERT INTO invites (code_hash, created_by, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![code_hash, created_by, created_at, expires_at],
    )?;
    Ok(())
}

/// Consumes a valid invite in one transaction so a code can never be used twice.
pub fn consume_invite(
    connection: &mut Connection,
    code_hash: &str,
    now: Timestamp,
) -> Result<Option<UserId>> {
    let transaction = connection.transaction()?;
    let created_by: Option<UserId> = transaction
        .query_row(
            "SELECT created_by FROM invites WHERE code_hash = ?1 AND expires_at > ?2",
            params![code_hash, now],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(created_by) = created_by {
        transaction.execute("DELETE FROM invites WHERE code_hash = ?1", params![code_hash])?;
        transaction.commit()?;
        Ok(Some(created_by))
    } else {
        Ok(None)
    }
}

/// Consumes a valid invite and creates the learner in one transaction, so a
/// failed registration cannot burn a one-time code.
pub fn register_with_invite(
    connection: &mut Connection,
    code_hash: &str,
    now: Timestamp,
    username: &str,
    password_hash: &str,
) -> Result<Option<UserId>> {
    let transaction = connection.transaction()?;
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM invites WHERE code_hash = ?1 AND expires_at > ?2)",
        params![code_hash, now],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if !valid {
        return Ok(None);
    }
    if credential_by_username(&transaction, username)?.is_some() {
        return Err(Error::InvalidValue(
            "username",
            "username is already taken".to_owned(),
        ));
    }
    let user_id = create_user(&transaction, username, now)?;
    insert_credential(
        &transaction,
        &user_id,
        username,
        password_hash,
        Role::Member,
        now,
    )?;
    transaction.execute("DELETE FROM invites WHERE code_hash = ?1", params![code_hash])?;
    transaction.commit()?;
    Ok(Some(user_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn invite_is_single_use_and_sessions_expire() {
        let mut database = Database::open_in_memory().unwrap();
        let connection = database.connection_mut();
        let now = Timestamp::now();
        let owner = create_user(connection, "owner", now).unwrap();
        insert_invite(connection, "hash", &owner, now, now.plus_days(1.0)).unwrap();
        assert_eq!(
            consume_invite(connection, "hash", now).unwrap(),
            Some(owner.clone())
        );
        assert_eq!(consume_invite(connection, "hash", now).unwrap(), None);

        insert_session(connection, "session", &owner, now, now.plus_days(1.0)).unwrap();
        assert!(session_by_token_hash(connection, "session", now)
            .unwrap()
            .is_some());
        delete_expired_sessions(connection, now.plus_days(2.0)).unwrap();
        assert!(session_by_token_hash(connection, "session", now.plus_days(2.0))
            .unwrap()
            .is_none());
    }
}
