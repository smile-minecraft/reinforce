use std::collections::HashMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{content as store, quiz as quiz_store};
use crate::primitives::{Error, Result};
use crate::quiz::grade::unexpected_keys;
use crate::study::{AttemptLog, is_voided_attempt};
use crate::{KnowledgeCardId, QuestionCard, QuestionCardId, UserId};

const REFERENCE_DURATION_MS: f64 = 30_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeWeakness {
    pub card_id: KnowledgeCardId,
    pub attempts: u32,
    pub error_rate: f64,
    pub hesitant_rate: f64,
    pub average_duration_ms: f64,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionWeakness {
    pub question_id: QuestionCardId,
    pub attempts: u32,
    pub error_rate: f64,
    pub hesitant_rate: f64,
    pub average_duration_ms: f64,
    pub score: f64,
    pub linked_card_ids: Vec<KnowledgeCardId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfusablePair {
    pub question_id: QuestionCardId,
    pub wrong_option: String,
    pub correct_option: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeaknessReport {
    pub knowledge_cards: Vec<KnowledgeWeakness>,
    pub questions: Vec<QuestionWeakness>,
    pub confusable_pairs: Vec<ConfusablePair>,
}

impl WeaknessReport {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    attempts: u32,
    wrong: u32,
    hesitant: u32,
    duration_sum_ms: u64,
}

impl Stats {
    fn observe(&mut self, attempt: &AttemptLog) {
        self.attempts += 1;
        if !attempt.is_correct {
            self.wrong += 1;
        }
        if attempt.confidence != crate::Confidence::Certain {
            self.hesitant += 1;
        }
        self.duration_sum_ms = self
            .duration_sum_ms
            .saturating_add(u64::try_from(attempt.duration_ms.max(0)).unwrap_or(u64::MAX));
    }

    fn rates(&self) -> (f64, f64, f64, f64) {
        if self.attempts == 0 {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let total = f64::from(self.attempts);
        let error_rate = f64::from(self.wrong) / total;
        let hesitant_rate = f64::from(self.hesitant) / total;
        let average_duration_ms = self.duration_sum_ms as f64 / total;
        let duration_weight = (average_duration_ms / REFERENCE_DURATION_MS).clamp(0.0, 1.0);
        let score = error_rate * 0.5 + hesitant_rate * 0.25 + duration_weight * 0.25;
        (error_rate, hesitant_rate, average_duration_ms, score)
    }
}

fn submitted_keys(payload: &Value) -> Vec<String> {
    let Some(answer) = payload.get("answer") else {
        return Vec::new();
    };
    match answer.get("kind").and_then(Value::as_str) {
        Some("keys") => answer
            .get("values")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        Some("text") => answer
            .get("value")
            .or_else(|| answer.get("values"))
            .and_then(Value::as_str)
            .map(|value| vec![value.to_owned()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub fn build_report(
    connection: &Connection,
    user_id: &UserId,
    limit: usize,
) -> Result<WeaknessReport> {
    if limit == 0 {
        return Err(Error::InvalidValue(
            "report limit",
            "must be greater than zero".to_owned(),
        ));
    }
    let attempts = quiz_store::user_attempts(connection, user_id)?;
    let mut questions: HashMap<QuestionCardId, QuestionCard> = HashMap::new();
    for attempt in &attempts {
        let Ok(id) = attempt.card_id.as_str().parse::<QuestionCardId>() else {
            continue;
        };
        if let Some(question) = store::get_question_card(connection, &id)? {
            questions.insert(id, question);
        }
    }
    let question_ids: Vec<QuestionCardId> = questions.keys().cloned().collect();
    let links = quiz_store::tests_links(connection, &question_ids)?;

    let mut question_stats: HashMap<QuestionCardId, Stats> = HashMap::new();
    let mut knowledge_stats: HashMap<KnowledgeCardId, Stats> = HashMap::new();
    let mut pairs: HashMap<(QuestionCardId, String, String), u32> = HashMap::new();

    for attempt in &attempts {
        if is_voided_attempt(attempt) {
            continue;
        }
        let Ok(question_id) = attempt.card_id.as_str().parse::<QuestionCardId>() else {
            continue;
        };
        if let Some(question) = questions.get(&question_id) {
            question_stats
                .entry(question_id.clone())
                .or_default()
                .observe(attempt);
            for card_id in links.get(&question_id).into_iter().flatten() {
                knowledge_stats
                    .entry(card_id.clone())
                    .or_default()
                    .observe(attempt);
            }
            if !attempt.is_correct {
                let wrong = unexpected_keys(
                    &submitted_keys(&attempt.submitted_payload),
                    &question.answer,
                );
                if let (Some(wrong_option), Some(correct_option)) =
                    (wrong.first(), question.answer.first())
                {
                    let key = (
                        question_id.clone(),
                        wrong_option.clone(),
                        correct_option.clone(),
                    );
                    *pairs.entry(key).or_insert(0) += 1;
                }
            }
        } else if let Ok(card_id) = attempt.card_id.as_str().parse::<KnowledgeCardId>() {
            knowledge_stats.entry(card_id).or_default().observe(attempt);
        }
    }

    let mut knowledge_cards: Vec<KnowledgeWeakness> = knowledge_stats
        .into_iter()
        .map(|(card_id, stats)| {
            let (error_rate, hesitant_rate, average_duration_ms, score) = stats.rates();
            KnowledgeWeakness {
                card_id,
                attempts: stats.attempts,
                error_rate,
                hesitant_rate,
                average_duration_ms,
                score,
            }
        })
        .collect();
    knowledge_cards.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.card_id.as_str().cmp(&right.card_id.as_str()))
    });
    knowledge_cards.truncate(limit);

    let mut question_weaknesses: Vec<QuestionWeakness> = question_stats
        .into_iter()
        .map(|(question_id, stats)| {
            let (error_rate, hesitant_rate, average_duration_ms, score) = stats.rates();
            QuestionWeakness {
                question_id: question_id.clone(),
                attempts: stats.attempts,
                error_rate,
                hesitant_rate,
                average_duration_ms,
                score,
                linked_card_ids: links.get(&question_id).cloned().unwrap_or_default(),
            }
        })
        .collect();
    question_weaknesses.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.question_id.as_str().cmp(&right.question_id.as_str()))
    });
    question_weaknesses.truncate(limit);

    let mut confusable_pairs: Vec<ConfusablePair> = pairs
        .into_iter()
        .map(
            |((question_id, wrong_option, correct_option), count)| ConfusablePair {
                question_id,
                wrong_option,
                correct_option,
                count,
            },
        )
        .collect();
    confusable_pairs.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.question_id.as_str().cmp(&right.question_id.as_str()))
            .then_with(|| left.wrong_option.cmp(&right.wrong_option))
    });
    confusable_pairs.truncate(limit);

    Ok(WeaknessReport {
        knowledge_cards,
        questions: question_weaknesses,
        confusable_pairs,
    })
}
