use std::collections::{BTreeMap, BTreeSet};

use crate::primitives::{ExamSetId, Result};
use crate::validation::{
    Snapshot, StoreView, ValidationError, ValidationRule, exam_set_for,
    merged_question_cards_for_exam_set,
};

/// Where a count violation is reported: the staged exam set when the batch
/// carries it, otherwise the first staged question card that touches the set.
struct Origin {
    op_index: usize,
    temp_ref: Option<String>,
}

impl Origin {
    fn error(&self, field_path: impl Into<String>, message: impl Into<String>) -> ValidationError {
        ValidationError {
            op_index: Some(self.op_index),
            temp_ref: self.temp_ref.clone(),
            field_path: field_path.into(),
            rule: ValidationRule::CountConservation,
            message: message.into(),
        }
    }
}

/// Every exam set whose question list can change in this batch: staged sets,
/// sets that staged cards move into, and sets that staged cards move out of.
fn affected_exam_sets(
    snapshot: &Snapshot,
    view: &dyn StoreView,
) -> Result<BTreeMap<String, (ExamSetId, Origin)>> {
    let mut affected = BTreeMap::new();
    for entry in &snapshot.exam_sets {
        affected.insert(
            entry.value.id.as_str(),
            (
                entry.value.id.clone(),
                Origin {
                    op_index: entry.op_index,
                    temp_ref: entry.temp_ref.clone(),
                },
            ),
        );
    }
    for entry in &snapshot.question_cards {
        let previous = view
            .question_card(&entry.value.id)?
            .and_then(|card| card.exam_set_id);
        for id in entry.value.exam_set_id.iter().chain(previous.iter()) {
            affected.entry(id.as_str()).or_insert_with(|| {
                (
                    id.clone(),
                    Origin {
                        op_index: entry.op_index,
                        temp_ref: entry.temp_ref.clone(),
                    },
                )
            });
        }
    }
    Ok(affected)
}

pub fn validate(snapshot: &Snapshot, view: &dyn StoreView) -> Result<Vec<ValidationError>> {
    let mut errors = Vec::new();

    for (id, origin) in affected_exam_sets(snapshot, view)?.into_values() {
        let Some(exam_set) = exam_set_for(snapshot, view, &id)? else {
            continue;
        };
        let (Some(expected), true) = (exam_set.expected_questions, exam_set.is_complete) else {
            continue;
        };

        let cards = merged_question_cards_for_exam_set(snapshot, view, &exam_set.id)?;
        if cards.len() as u32 != expected {
            errors.push(origin.error(
                "expected_questions",
                format!(
                    "complete exam set {} expects {expected} questions but has {}",
                    exam_set.id,
                    cards.len()
                ),
            ));
        }

        let mut numbers: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for card in &cards {
            match card.question_no {
                Some(number) => numbers.entry(number).or_default().push(card.id.as_str()),
                None => errors.push(origin.error(
                    "question_cards.question_no",
                    format!("question {} has no question_no", card.id),
                )),
            }
        }

        for (number, ids) in &numbers {
            if ids.len() > 1 {
                errors.push(origin.error(
                    format!("question_cards.question_no[{number}]"),
                    format!("question_no {number} is used by multiple questions"),
                ));
            }
        }

        let wanted: BTreeSet<u32> = (1..=expected).collect();
        let present: BTreeSet<u32> = numbers.keys().copied().collect();
        let missing: Vec<u32> = wanted.difference(&present).copied().collect();
        let extra: Vec<u32> = present.difference(&wanted).copied().collect();
        if !missing.is_empty() {
            errors.push(origin.error(
                "expected_questions",
                format!(
                    "missing placeholder questions for question_no {:?}; use unanswered cards",
                    missing
                ),
            ));
        }
        if !extra.is_empty() {
            errors.push(origin.error(
                "question_cards.question_no",
                format!("question_no {:?} is outside 1..={expected}", extra),
            ));
        }
    }

    Ok(errors)
}
