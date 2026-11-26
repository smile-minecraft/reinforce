use std::collections::HashSet;

use rusqlite::{Connection, named_params, params_from_iter};

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
            OR search_text LIKE :query ESCAPE '\\'
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
         WHERE subject LIKE :query ESCAPE '\\'
            OR search_text LIKE :query ESCAPE '\\'
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

/// Walks typed edges in both directions, one breadth-first SQL query per depth.
/// Visited keys are tracked in Rust so each node is returned once at its
/// shortest depth, without enumerating every simple path in a dense graph.
pub fn graph_neighbors(
    connection: &Connection,
    from: &EdgeEndpoint,
    max_depth: u32,
) -> Result<Vec<GraphNode>> {
    if max_depth > MAX_GRAPH_DEPTH {
        return Err(Error::GraphDepthTooLarge(max_depth, MAX_GRAPH_DEPTH));
    }

    let start_key = from.key();
    let mut visited: HashSet<String> = HashSet::from([start_key.clone()]);
    let mut frontier = vec![start_key];
    let mut nodes: Vec<GraphNode> = Vec::new();

    for depth in 1..=max_depth {
        if frontier.is_empty() {
            break;
        }
        let placeholders = frontier
            .iter()
            .map(|_| "(?)")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "WITH frontier(key) AS (VALUES {placeholders})
             SELECT DISTINCT
                 CASE WHEN e.from_key = f.key THEN e.to_key ELSE e.from_key END,
                 CASE WHEN e.from_key = f.key THEN e.to_kind ELSE e.from_kind END,
                 CASE WHEN e.from_key = f.key THEN e.to_id ELSE e.from_id END,
                 CASE WHEN e.from_key = f.key THEN e.to_option_key ELSE e.from_option_key END
             FROM edges e
             JOIN frontier f ON e.from_key = f.key OR e.to_key = f.key"
        );

        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params_from_iter(frontier.iter().map(String::as_str)),
            |row| {
                let key: String = row.get(0)?;
                let kind: EndpointKind = row.get(1)?;
                let id: String = row.get(2)?;
                let option_key: Option<String> = row.get(3)?;
                Ok((
                    key,
                    GraphNode {
                        endpoint: EdgeEndpoint::from_db_parts(kind, &id, option_key.as_deref())?,
                        depth,
                    },
                ))
            },
        )?;
        let discovered = rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

        let mut next_frontier: Vec<String> = Vec::new();
        for (key, node) in discovered {
            if visited.insert(key.clone()) {
                nodes.push(node);
                next_frontier.push(key);
            }
        }
        frontier = next_frontier;
    }

    nodes.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.endpoint.key().cmp(&right.endpoint.key()))
    });
    Ok(nodes)
}
