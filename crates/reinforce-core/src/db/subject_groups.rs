//! Nested subject folders for the library view.
//!
//! A group is a named folder with an optional parent, a sort key and a set of
//! subject names. A drill may be scoped to a group, in which case every subject
//! of the group and of its descendants is included, so a folder never has to
//! repeat the subjects of its children.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::primitives::{Error, Result, SubjectGroupId};
use crate::timestamp::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectGroup {
    pub id: SubjectGroupId,
    pub parent_id: Option<SubjectGroupId>,
    pub name: String,
    pub sort_order: i32,
    pub subjects: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn validated_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidValue(
            "subject group name",
            "name must not be empty".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Trims, drops empty names, then deduplicates and sorts the remaining ones so
/// the stored set has one canonical order.
fn normalized_subjects(subjects: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = subjects
        .iter()
        .map(|subject| subject.trim())
        .filter(|subject| !subject.is_empty())
        .map(str::to_owned)
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

/// Reads every stored parent link; the value is `None` for a top-level group.
fn group_parent(
    connection: &Connection,
    id: &SubjectGroupId,
) -> Result<Option<Option<SubjectGroupId>>> {
    connection
        .query_row(
            "SELECT parent_id FROM subject_groups WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<SubjectGroupId>>(0),
        )
        .optional()
        .map_err(Into::into)
}

fn ensure_parent_usable(
    connection: &Connection,
    id: Option<&SubjectGroupId>,
    parent_id: &SubjectGroupId,
) -> Result<()> {
    let mut cursor = Some(parent_id.clone());
    let mut steps = 0_usize;
    while let Some(current) = cursor {
        if id.is_some_and(|id| id == &current) {
            return Err(Error::InvalidValue(
                "subject group parent",
                "a group cannot be its own parent or a descendant".to_owned(),
            ));
        }
        cursor = match group_parent(connection, &current)? {
            Some(parent) => parent,
            None => {
                return Err(Error::InvalidValue(
                    "subject group parent",
                    format!("{current} does not exist"),
                ));
            }
        };
        steps += 1;
        if steps > 4_096 {
            return Err(Error::InvalidValue(
                "subject group parent",
                "parent chain is too deep".to_owned(),
            ));
        }
    }
    Ok(())
}

fn subjects_by_group(connection: &Connection) -> Result<HashMap<String, Vec<String>>> {
    let mut statement = connection.prepare(
        "SELECT group_id, subject FROM subject_group_subjects ORDER BY group_id, subject",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut subjects: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let (group_id, subject) = row?;
        subjects.entry(group_id).or_default().push(subject);
    }
    Ok(subjects)
}

/// Lists every group ordered by `sort_order`, `name` and `id`.
pub fn list_subject_groups(connection: &Connection) -> Result<Vec<SubjectGroup>> {
    let mut statement = connection.prepare(
        "SELECT id, parent_id, name, sort_order, created_at, updated_at
         FROM subject_groups
         ORDER BY sort_order, name, id",
    )?;
    let mut subjects = subjects_by_group(connection)?;
    let rows = statement.query_map([], |row| {
        Ok(SubjectGroup {
            id: row.get(0)?,
            parent_id: row.get(1)?,
            name: row.get(2)?,
            sort_order: row.get(3)?,
            subjects: Vec::new(),
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    let mut groups = Vec::new();
    for row in rows {
        let mut group = row?;
        group.subjects = subjects.remove(&group.id.as_str()).unwrap_or_default();
        groups.push(group);
    }
    Ok(groups)
}

pub fn get_subject_group(
    connection: &Connection,
    id: &SubjectGroupId,
) -> Result<Option<SubjectGroup>> {
    let group = connection
        .query_row(
            "SELECT id, parent_id, name, sort_order, created_at, updated_at
             FROM subject_groups WHERE id = ?1",
            params![id],
            |row| {
                Ok(SubjectGroup {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    name: row.get(2)?,
                    sort_order: row.get(3)?,
                    subjects: Vec::new(),
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()?;
    let Some(mut group) = group else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT subject FROM subject_group_subjects WHERE group_id = ?1 ORDER BY subject",
    )?;
    group.subjects = statement
        .query_map(params![id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(Some(group))
}

fn replace_subjects(
    connection: &Connection,
    id: &SubjectGroupId,
    subjects: &[String],
) -> Result<()> {
    connection.execute(
        "DELETE FROM subject_group_subjects WHERE group_id = ?1",
        params![id],
    )?;
    for subject in subjects {
        connection.execute(
            "INSERT INTO subject_group_subjects (group_id, subject) VALUES (?1, ?2)",
            params![id, subject],
        )?;
    }
    Ok(())
}

fn with_savepoint<T>(
    connection: &Connection,
    name: &str,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    connection.execute_batch(&format!("SAVEPOINT {name}"))?;
    match work() {
        Ok(value) => {
            connection.execute_batch(&format!("RELEASE SAVEPOINT {name}"))?;
            Ok(value)
        }
        Err(error) => {
            let _ = connection.execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {name}; RELEASE SAVEPOINT {name}"
            ));
            Err(error)
        }
    }
}

pub fn create_subject_group(
    connection: &Connection,
    parent_id: Option<&SubjectGroupId>,
    name: &str,
    subjects: &[String],
    sort_order: i32,
    now: Timestamp,
) -> Result<SubjectGroup> {
    let name = validated_name(name)?;
    if let Some(parent_id) = parent_id {
        ensure_parent_usable(connection, None, parent_id)?;
    }
    let subjects = normalized_subjects(subjects);
    let group = SubjectGroup {
        id: SubjectGroupId::generate(),
        parent_id: parent_id.cloned(),
        name,
        sort_order,
        subjects,
        created_at: now,
        updated_at: now,
    };
    with_savepoint(connection, "create_subject_group", || {
        connection.execute(
            "INSERT INTO subject_groups (id, parent_id, name, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                group.id,
                group.parent_id,
                group.name,
                group.sort_order,
                group.created_at,
                group.updated_at,
            ],
        )?;
        replace_subjects(connection, &group.id, &group.subjects)?;
        Ok(())
    })?;
    Ok(group)
}

pub fn update_subject_group(
    connection: &Connection,
    id: &SubjectGroupId,
    parent_id: Option<&SubjectGroupId>,
    name: &str,
    subjects: &[String],
    sort_order: i32,
    now: Timestamp,
) -> Result<SubjectGroup> {
    let existing = get_subject_group(connection, id)?
        .ok_or_else(|| Error::InvalidValue("subject group id", format!("{id} does not exist")))?;
    let name = validated_name(name)?;
    if let Some(parent_id) = parent_id {
        ensure_parent_usable(connection, Some(id), parent_id)?;
    }
    let subjects = normalized_subjects(subjects);
    with_savepoint(connection, "update_subject_group", || {
        connection.execute(
            "UPDATE subject_groups
             SET parent_id = ?2, name = ?3, sort_order = ?4, updated_at = ?5
             WHERE id = ?1",
            params![id, parent_id, name, sort_order, now],
        )?;
        replace_subjects(connection, id, &subjects)?;
        Ok(())
    })?;
    Ok(SubjectGroup {
        id: id.clone(),
        parent_id: parent_id.cloned(),
        name,
        sort_order,
        subjects,
        created_at: existing.created_at,
        updated_at: now,
    })
}

pub fn delete_subject_group(connection: &Connection, id: &SubjectGroupId) -> Result<bool> {
    let deleted = connection.execute("DELETE FROM subject_groups WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

/// Collects the subjects of a group and of every descendant group, deduplicated
/// and sorted. A missing group is an invalid identifier, not an empty scope.
pub fn resolve_group_subjects(
    connection: &Connection,
    group_id: &SubjectGroupId,
) -> Result<Vec<String>> {
    if get_subject_group(connection, group_id)?.is_none() {
        return Err(Error::InvalidValue(
            "subject group id",
            format!("{group_id} does not exist"),
        ));
    }
    // `UNION` (not `UNION ALL`) keeps the walk finite even if a store ever held
    // a parent cycle that the update guard failed to prevent.
    let mut statement = connection.prepare(
        "WITH RECURSIVE tree(id) AS (
             SELECT id FROM subject_groups WHERE id = ?1
             UNION
             SELECT g.id FROM subject_groups g JOIN tree t ON g.parent_id = t.id
         )
         SELECT DISTINCT s.subject
         FROM subject_group_subjects s
         JOIN tree t ON t.id = s.group_id
         ORDER BY s.subject",
    )?;
    let rows = statement.query_map(params![group_id], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}
