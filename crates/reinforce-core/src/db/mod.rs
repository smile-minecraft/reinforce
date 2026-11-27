use std::path::Path;

use rusqlite::{Connection, Transaction};

use crate::primitives::{Error, Result};

pub mod auth;
pub mod catalog;
pub mod content;
pub mod plugins;
pub mod quiz;
pub mod schema;
pub mod search;
pub mod study;
pub mod sync;

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let mut database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > i64::from(schema::SCHEMA_VERSION) {
            return Err(Error::UnsupportedSchemaVersion(
                u32::try_from(version).unwrap_or(u32::MAX),
                schema::SCHEMA_VERSION,
            ));
        }
        if version == i64::from(schema::SCHEMA_VERSION) {
            return Ok(());
        }

        let transaction = self.connection.transaction()?;
        transaction.execute_batch(schema::SCHEMA)?;
        transaction.pragma_update(None, "user_version", i64::from(schema::SCHEMA_VERSION))?;
        transaction.commit()?;
        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.connection.transaction()?)
    }

    pub fn schema_version(&self) -> Result<u32> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        Ok(u32::try_from(version).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_idempotent() {
        let mut database = Database::open_in_memory().unwrap();
        assert_eq!(database.schema_version().unwrap(), schema::SCHEMA_VERSION);
        database.migrate().unwrap();
        let names: Vec<String> = database
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        for table in [
            "resources",
            "anchors",
            "knowledge_cards",
            "notes",
            "exam_sets",
            "question_groups",
            "question_cards",
            "edges",
            "proposals",
            "users",
            "app_settings",
            "credentials",
            "sessions",
            "invites",
            "attempt_logs",
            "card_scheduling",
            "user_card_meta",
            "sync_state",
            "sync_attempt_uploads",
            "sync_meta_uploads",
            "plugin_jobs",
        ] {
            assert!(names.contains(&table.to_owned()), "missing table {table}");
        }
    }

    #[test]
    fn version_two_database_upgrades_to_three() {
        let mut database = Database::open_in_memory().unwrap();
        // A v2 store predates the plugin queue: same schema minus that table,
        // with the marker rolled back the way an older binary left it.
        database
            .connection()
            .execute_batch("DROP TABLE plugin_jobs; PRAGMA user_version = 2;")
            .unwrap();
        assert_eq!(database.schema_version().unwrap(), 2);

        database.migrate().unwrap();
        assert_eq!(database.schema_version().unwrap(), schema::SCHEMA_VERSION);

        let tables: Vec<String> = database
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        assert!(tables.contains(&"plugin_jobs".to_owned()));
        let indexes: Vec<String> = database
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        assert!(indexes.contains(&"plugin_jobs_queue".to_owned()));
        assert!(indexes.contains(&"plugin_jobs_plugin".to_owned()));

        // The upgraded store can immediately hold queue rows.
        let job = crate::plugin::PluginJob::queued(
            "fake",
            "ingest_pdf",
            serde_json::json!({"resource_id": "r1"}),
            1,
        );
        plugins::insert_job(database.connection(), &job).expect("insert after upgrade");
        assert!(
            plugins::get_job(database.connection(), &job.id)
                .expect("read")
                .is_some()
        );
    }
}
