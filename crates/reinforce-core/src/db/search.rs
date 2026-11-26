use rusqlite::{Connection, named_params};

use crate::content::{EdgeEndpoint, GraphNode};
use crate::primitives::{EndpointKind, Error, Result};
use crate::{KnowledgeCardId, QuestionCardId};

const MAX_GRAPH_DEPTH: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchHit {
    KnowledgeCard(KnowledgeCardId),
    QuestionCard(QuestionCardId),
}

fn fts_phrase(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

fn like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len());
    for character in query.chars() {
        match character {
            '%' | '_' | '\\' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    format!("%{escaped}%")
}

pub fn search_knowledge_cards(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeCardId>> {
    let limit = i64::try_from(limit.clamp(1, 1000)).unwrap_or(1000);
    let sql = if query.chars().count() >= 3 {
        "SELECT k.id FROM knowledge_cards_fts
         JOIN knowledge_cards k ON k.rowid = knowledge_cards_fts.rowid
         WHERE knowledge_cards_fts MATCH :query
         ORDER BY knowledge_cards_fts.rank LIMIT :limit"
    } else {
        "SELECT id FROM knowledge_cards
         WHERE title LIKE :query ESCAPE '\\' OR subject LIKE :query ESCAPE '\\'
            OR topics_json LIKE :query ESCAPE '\\' OR blocks_json LIKE :query ESCAPE '\\'
         ORDER BY id LIMIT :limit"
    };
    let parameter = if query.chars().count() >= 3 {
        fts_phrase(query)
    } else {
        like_pattern(query)
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        named_params! { ":query": parameter, ":limit": limit },
        |row| row.get(0),
    )?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

pub fn search_question_cards(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<QuestionCardId>> {
    let limit = i64::try_from(limit.clamp(1, 1000)).unwrap_or(1000);
    let sql = if query.chars().count() >= 3 {
        "SELECT q.id FROM question_cards_fts
         JOIN question_cards q ON q.rowid = question_cards_fts.rowid
         WHERE question_cards_fts MATCH :query
         ORDER BY question_cards_fts.rank LIMIT :limit"
    } else {
        "SELECT id FROM question_cards
         WHERE subject LIKE :query ESCAPE '\\' OR topics_json LIKE :query ESCAPE '\\'
            OR stem_blocks_json LIKE :query ESCAPE '\\'
            OR explanation_blocks_json LIKE :query ESCAPE '\\'
         ORDER BY id LIMIT :limit"
    };
    let parameter = if query.chars().count() >= 3 {
        fts_phrase(query)
    } else {
        like_pattern(query)
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        named_params! { ":query": parameter, ":limit": limit },
        |row| row.get(0),
    )?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}

pub fn search_cards(connection: &Connection, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let per_type = limit.clamp(1, 1000);
    let mut hits: Vec<SearchHit> = search_knowledge_cards(connection, query, per_type)?
        .into_iter()
        .map(SearchHit::KnowledgeCard)
        .collect();
    hits.extend(
        search_question_cards(connection, query, per_type)?
            .into_iter()
            .map(SearchHit::QuestionCard),
    );
    Ok(hits)
}

/// Walks typed edges in both directions. The recursive CTE keeps a visited
/// path and `max_depth` bounds expansion, so cycles terminate and the result
/// cannot grow without limit.
pub fn graph_neighbors(
    connection: &Connection,
    from: &EdgeEndpoint,
    max_depth: u32,
) -> Result<Vec<GraphNode>> {
    if max_depth > MAX_GRAPH_DEPTH {
        return Err(Error::GraphDepthTooLarge(max_depth, MAX_GRAPH_DEPTH));
    }

    let sql = r#"
        WITH RECURSIVE walk(node_key, next_kind, next_id, next_option_key, depth, path) AS (
            SELECT :start_key, NULL, NULL, NULL, 0, '|' || :start_key || '|'
            UNION ALL
            SELECT
                CASE WHEN e.from_key = w.node_key THEN e.to_key ELSE e.from_key END,
                CASE WHEN e.from_key = w.node_key THEN e.to_kind ELSE e.from_kind END,
                CASE WHEN e.from_key = w.node_key THEN e.to_id ELSE e.from_id END,
                CASE WHEN e.from_key = w.node_key THEN e.to_option_key ELSE e.from_option_key END,
                w.depth + 1,
                w.path || CASE WHEN e.from_key = w.node_key THEN e.to_key ELSE e.from_key END || '|'
            FROM walk w
            JOIN edges e ON e.from_key = w.node_key OR e.to_key = w.node_key
            WHERE w.depth < :max_depth
              AND instr(w.path, '|' ||
                  CASE WHEN e.from_key = w.node_key THEN e.to_key ELSE e.from_key END || '|') = 0
        )
        SELECT node_key, next_kind, next_id, next_option_key, MIN(depth) AS depth
        FROM walk
        WHERE depth > 0
        GROUP BY node_key, next_kind, next_id, next_option_key
        ORDER BY depth, node_key
    "#;

    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        named_params! { ":start_key": from.key(), ":max_depth": max_depth },
        |row| {
            let kind: EndpointKind = row.get(1)?;
            let id: String = row.get(2)?;
            let option_key: Option<String> = row.get(3)?;
            Ok(GraphNode {
                endpoint: EdgeEndpoint::from_db_parts(kind, &id, option_key.as_deref())?,
                depth: row.get::<_, i64>(4)? as u32,
            })
        },
    )?;
    rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
        .map_err(Into::into)
}
