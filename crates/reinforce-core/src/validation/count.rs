use std::collections::{BTreeMap, BTreeSet};

use crate::primitives::Result;
use crate::validation::{
    Snapshot, StoreView, ValidationError, ValidationRule, merged_question_cards_for_exam_set,
};

pub fn validate(snapshot: &Snapshot, view: &dyn StoreView) -> Result<Vec<ValidationError>> {
    let mut errors = Vec::new();

    for entry in &snapshot.exam_sets {
        let exam_set = &entry.value;
        let (Some(expected), true) = (exam_set.expected_questions, exam_set.is_complete) else {
            continue;
        };

        let cards = merged_question_cards_for_exam_set(snapshot, view, &exam_set.id)?;
        if cards.len() as u32 != expected {
            errors.push(entry.error(
                ValidationRule::CountConservation,
                "expected_questions",
                format!(
                    "complete exam set expects {expected} questions but has {}",
                    cards.len()
                ),
            ));
        }

        let mut numbers: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for card in &cards {
            match card.question_no {
                Some(number) => numbers.entry(number).or_default().push(card.id.as_str()),
                None => errors.push(ValidationError {
                    op_index: Some(entry.op_index),
                    temp_ref: entry.temp_ref.clone(),
                    field_path: "question_cards.question_no".to_owned(),
                    rule: ValidationRule::CountConservation,
                    message: format!("question {} has no question_no", card.id),
                }),
            }
        }

        for (number, ids) in &numbers {
            if ids.len() > 1 {
                errors.push(ValidationError {
                    op_index: Some(entry.op_index),
                    temp_ref: entry.temp_ref.clone(),
                    field_path: format!("question_cards.question_no[{number}]"),
                    rule: ValidationRule::CountConservation,
                    message: format!("question_no {number} is used by multiple questions"),
                });
            }
        }

        let wanted: BTreeSet<u32> = (1..=expected).collect();
        let present: BTreeSet<u32> = numbers.keys().copied().collect();
        let missing: Vec<u32> = wanted.difference(&present).copied().collect();
        let extra: Vec<u32> = present.difference(&wanted).copied().collect();
        if !missing.is_empty() {
            errors.push(entry.error(
                ValidationRule::CountConservation,
                "expected_questions",
                format!(
                    "missing placeholder questions for question_no {:?}; use unanswered cards",
                    missing
                ),
            ));
        }
        if !extra.is_empty() {
            errors.push(entry.error(
                ValidationRule::CountConservation,
                "question_cards.question_no",
                format!("question_no {:?} is outside 1..={expected}", extra),
            ));
        }
    }

    Ok(errors)
}
